//! Phase 3a — deterministic source rewrites (`glyph fmt`).
//!
//! Two strata:
//! 1. Pre-Parse text-level: tab → 4-space, mixed-indentation fix.
//! 2. Post-Parse AST-level: constraint hoisting, context hoisting,
//!    section reorder to canonical layout.

use crate::parse;
use crate::span::LineIndex;
use crate::diagnostic::DiagBag;

/// Result of formatting a source string.
pub struct FmtResult {
    /// The formatted source text.
    pub output: String,
    /// Whether the output differs from the input.
    pub changed: bool,
    /// If Phase 1 failed after pre-parse fixes, contains the parse diagnostics.
    pub diagnostics: DiagBag,
}

/// Format a Glyph source string. Returns the formatted output and metadata.
pub fn fmt_source(source: &str) -> FmtResult {
    let mut bag = DiagBag::new();

    // Stratum 1: pre-parse text-level rewrites.
    let after_preparse = preparse_rewrite(source);

    // Try to parse for stratum 2.
    let line_index = LineIndex::new(&after_preparse);
    let parsed = parse::parse_with_diagnostics(
        &after_preparse,
        0,
        "<fmt>",
        &line_index,
        &mut bag,
    );

    match parsed {
        Some(file) => {
            // Stratum 2: AST-level rewrites.
            let after_ast = ast_rewrite(&after_preparse, &file);
            let changed = after_ast != source;
            FmtResult { output: after_ast, changed, diagnostics: bag }
        }
        None => {
            // Parse failed — emit only pre-parse fixes.
            let changed = after_preparse != source;
            FmtResult { output: after_preparse, changed, diagnostics: bag }
        }
    }
}

/// Stratum 1: text-level rewrites. Converts tabs to 4 spaces.
fn preparse_rewrite(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    for line in source.split('\n') {
        // Count leading whitespace and replace tabs.
        let mut col = 0;
        let mut content_start = 0;
        for (i, ch) in line.char_indices() {
            match ch {
                '\t' => {
                    // Tab → advance to next 4-space boundary.
                    let next = (col / 4 + 1) * 4;
                    col = next;
                    content_start = i + 1;
                }
                ' ' => {
                    col += 1;
                    content_start = i + 1;
                }
                _ => break,
            }
        }
        // Emit `col` spaces then the rest of the line.
        for _ in 0..col {
            out.push(' ');
        }
        out.push_str(&line[content_start..]);
        out.push('\n');
    }
    // `split('\n')` on a string that ends with '\n' produces an extra empty item.
    // We added one extra '\n' for that empty item — remove it if source didn't
    // end with double newline.
    if !source.is_empty() && out.len() > source.len() {
        // More precisely: source.split('\n') has N+1 items if source ends with \n,
        // and we added N+1 newlines. The original had N newlines. Pop the extra.
        if source.ends_with('\n') && !source.ends_with("\n\n") {
            // Actually let's be more careful. The split produces one empty trailing
            // element for a trailing \n. We loop N+1 times and add N+1 newlines.
            // Original has N newlines. So we have one extra.
            out.pop(); // remove trailing \n
        }
    }
    // If source doesn't end with \n, we still added a \n for the last segment.
    if !source.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Stratum 2: AST-level rewrites.
///
/// Operates by identifying declaration boundaries in the source text, then
/// reconstructing each declaration body in canonical sub-section order with
/// hoisted constraints and context.
fn ast_rewrite(source: &str, file: &crate::ast::SourceFile) -> String {
    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        return source.to_string();
    }

    // Find declaration header lines (indent 0, starts with skill/block/export/text/import).
    let mut decl_ranges: Vec<(usize, usize)> = Vec::new(); // (start_line, end_line exclusive)
    let mut decl_starts: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // A line at indent 0 that starts a declaration keyword.
        if !line.starts_with(' ') && !line.starts_with('\t') {
            if trimmed.starts_with("skill ")
                || trimmed.starts_with("block ")
                || trimmed.starts_with("export block ")
                || trimmed.starts_with("export text ")
                || trimmed.starts_with("text ")
                || trimmed.starts_with("import ")
            {
                decl_starts.push(i);
            }
        }
    }

    // Compute ranges.
    for (idx, &start) in decl_starts.iter().enumerate() {
        let end = if idx + 1 < decl_starts.len() {
            // Find the end: scan backwards from next decl start to skip blank lines.
            decl_starts[idx + 1]
        } else {
            lines.len()
        };
        decl_ranges.push((start, end));
    }

    // For simple declarations (text, import), just pass through.
    // For skill/block/export block, do the rewrite.
    let mut out = String::new();
    let mut last_end = 0;

    for (decl_idx, &(start, end)) in decl_ranges.iter().enumerate() {
        // Emit any lines before this declaration (blank lines between decls).
        for i in last_end..start {
            out.push_str(lines[i]);
            out.push('\n');
        }
        last_end = end;

        let header = lines[start].trim();
        if header.starts_with("text ") || header.starts_with("export text ") || header.starts_with("import ") {
            // Pass through unchanged.
            for i in start..end {
                out.push_str(lines[i]);
                out.push('\n');
            }
            continue;
        }

        // This is a skill, block, or export block declaration.
        // Find the matching AST decl to know what sections exist.
        let ast_decl = file.decls.get(decl_idx);

        // Rewrite the declaration body in canonical order.
        out.push_str(lines[start]);
        out.push('\n');

        // Parse body lines into sections.
        let body_lines: Vec<&str> = (start + 1..end).map(|i| lines[i]).collect();
        let rewritten = rewrite_decl_body(&body_lines, ast_decl);
        out.push_str(&rewritten);
    }

    // Emit any trailing lines after the last declaration.
    for i in last_end..lines.len() {
        out.push_str(lines[i]);
        out.push('\n');
    }

    // Preserve original trailing newline behavior.
    if source.ends_with('\n') && !out.ends_with('\n') {
        out.push('\n');
    }
    if !source.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }

    out
}

/// Identifies which "section" a body line belongs to.
#[derive(Debug, Clone, PartialEq)]
enum SectionKind {
    Description,
    Effects,
    Context,
    Constraints,
    Flow,
    BodyConstraintMarker,
    BodyContextMarker,
    BlankOrOther,
}

/// A group of lines belonging to one section.
#[derive(Debug, Clone)]
struct Section {
    kind: SectionKind,
    lines: Vec<String>,
}

/// Rewrite a declaration body (lines at indent >= 1) in canonical order.
fn rewrite_decl_body(body_lines: &[&str], _ast_decl: Option<&crate::ast::Decl>) -> String {
    // Parse lines into sections.
    let mut sections: Vec<Section> = Vec::new();
    let mut current_kind: Option<SectionKind> = None;
    let mut current_lines: Vec<String> = Vec::new();
    let mut in_flow_block = false;

    // Constraint and context markers found at body level or flow top level that
    // should be hoisted.
    let mut hoisted_constraints: Vec<String> = Vec::new();
    let mut hoisted_context: Vec<String> = Vec::new();

    for line in body_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // Blank line — accumulate with current section or skip.
            if let Some(_) = &current_kind {
                current_lines.push(line.to_string());
            }
            continue;
        }

        // Determine indent level (in spaces).
        let indent = line.len() - line.trim_start().len();
        let indent_level = indent / 4;

        // Section headers at indent level 1 (4 spaces).
        if indent_level == 1 {
            let kw = trimmed;
            let new_kind = if kw.starts_with("description:") {
                Some(SectionKind::Description)
            } else if kw.starts_with("effects:") {
                Some(SectionKind::Effects)
            } else if kw == "context:" || kw.starts_with("context:") {
                Some(SectionKind::Context)
            } else if kw == "constraints:" || kw.starts_with("constraints:") {
                Some(SectionKind::Constraints)
            } else if kw == "flow:" || kw.starts_with("flow:") {
                Some(SectionKind::Flow)
            } else if is_constraint_marker(trimmed) {
                Some(SectionKind::BodyConstraintMarker)
            } else if is_context_marker(trimmed) {
                Some(SectionKind::BodyContextMarker)
            } else {
                None
            };

            if let Some(kind) = new_kind {
                // Flush previous section.
                if let Some(prev_kind) = current_kind.take() {
                    sections.push(Section { kind: prev_kind, lines: std::mem::take(&mut current_lines) });
                }

                match kind {
                    SectionKind::BodyConstraintMarker => {
                        // Hoist: extract the marker text.
                        hoisted_constraints.push(trimmed.to_string());
                        continue;
                    }
                    SectionKind::BodyContextMarker => {
                        // Hoist: extract the context entry.
                        let entry = trimmed.strip_prefix("context ").unwrap_or(trimmed);
                        hoisted_context.push(entry.to_string());
                        continue;
                    }
                    _ => {
                        current_kind = Some(kind);
                        current_lines.push(line.to_string());
                    }
                }
                continue;
            }
        }

        // Lines inside flow: check for top-level constraint/context markers.
        if in_flow_block && indent_level == 2 {
            if is_constraint_marker(trimmed) {
                hoisted_constraints.push(trimmed.to_string());
                continue;
            }
            if is_context_marker(trimmed) {
                let entry = trimmed.strip_prefix("context ").unwrap_or(trimmed);
                hoisted_context.push(entry.to_string());
                continue;
            }
        }

        // Continue accumulating in current section.
        if current_kind.is_some() {
            if matches!(current_kind, Some(SectionKind::Flow)) {
                in_flow_block = true;
            }
            current_lines.push(line.to_string());
        } else {
            // Line at body level that's not a recognized section header.
            // Could be a bare name or something else — pass through.
            current_lines.push(line.to_string());
            current_kind = Some(SectionKind::BlankOrOther);
        }
    }

    // Flush last section.
    if let Some(kind) = current_kind {
        sections.push(Section { kind: kind, lines: current_lines });
    }

    // Now reconstruct in canonical order: description, effects, context, constraints, flow.
    let canonical_order = [
        SectionKind::Description,
        SectionKind::Effects,
        SectionKind::Context,
        SectionKind::Constraints,
        SectionKind::Flow,
    ];

    let mut out = String::new();

    for target_kind in &canonical_order {
        // Find existing section of this kind.
        let section = sections.iter().find(|s| &s.kind == target_kind);

        match target_kind {
            SectionKind::Context => {
                if !hoisted_context.is_empty() || section.is_some() {
                    // Build context section.
                    if let Some(sec) = section {
                        // Existing context: section — append hoisted entries.
                        for line in &sec.lines {
                            out.push_str(line);
                            out.push('\n');
                        }
                        // Add hoisted entries at indent 2.
                        for entry in &hoisted_context {
                            out.push_str("        ");
                            out.push_str(entry);
                            out.push('\n');
                        }
                    } else {
                        // Create new context: section.
                        out.push_str("    context:\n");
                        for entry in &hoisted_context {
                            out.push_str("        ");
                            out.push_str(entry);
                            out.push('\n');
                        }
                    }
                }
            }
            SectionKind::Constraints => {
                if !hoisted_constraints.is_empty() || section.is_some() {
                    if let Some(sec) = section {
                        // Existing constraints: section — append hoisted entries.
                        for line in &sec.lines {
                            out.push_str(line);
                            out.push('\n');
                        }
                        for marker in &hoisted_constraints {
                            out.push_str("        ");
                            out.push_str(marker);
                            out.push('\n');
                        }
                    } else {
                        // Create new constraints: section.
                        out.push_str("    constraints:\n");
                        for marker in &hoisted_constraints {
                            out.push_str("        ");
                            out.push_str(marker);
                            out.push('\n');
                        }
                    }
                }
            }
            _ => {
                if let Some(sec) = section {
                    for line in &sec.lines {
                        out.push_str(line);
                        out.push('\n');
                    }
                }
            }
        }
    }

    // Emit any "other" sections (blank/unknown) that didn't match canonical kinds.
    for sec in &sections {
        if !canonical_order.contains(&sec.kind) && sec.kind != SectionKind::BodyConstraintMarker && sec.kind != SectionKind::BodyContextMarker {
            for line in &sec.lines {
                out.push_str(line);
                out.push('\n');
            }
        }
    }

    out
}

fn is_constraint_marker(trimmed: &str) -> bool {
    trimmed.starts_with("require ")
        || trimmed.starts_with("avoid ")
        || trimmed.starts_with("must avoid ")
        || trimmed.starts_with("must ")
}

fn is_context_marker(trimmed: &str) -> bool {
    trimmed.starts_with("context ") && !trimmed.starts_with("context:")
}
