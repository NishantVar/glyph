//! Phase 3a — deterministic source rewrites (`glyph fmt`).
//!
//! Two strata:
//! 1. Pre-Parse text-level: tab → 4-space, mixed-indentation fix.
//! 2. Post-Parse AST-level: constraint hoisting, context hoisting,
//!    section reorder to canonical layout.

use crate::diagnostic::DiagBag;
use crate::parse;
use crate::span::LineIndex;

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
///
/// `enable_effects` gates the parser: when `false`, any `effects:` sub-section
/// in the source produces a `G::parse::effects-disabled` parse error and the
/// formatter falls back to the pre-parse stratum only (no AST rewrite). When
/// `true`, the parser accepts `effects:` and the AST stratum reorders sections
/// canonically (placing `effects:` between `description:` and `context:`).
pub fn fmt_source(source: &str, enable_effects: bool) -> FmtResult {
    let mut bag = DiagBag::new();

    // Stratum 1: pre-parse text-level rewrites.
    let after_preparse = preparse_rewrite(source);
    // Issue #82 chunk 3: strip legacy `-> None` return-type annotations
    // from declaration headers so the parser never sees them. The parser
    // would otherwise emit `G::parse::none-as-return-type` (Repairable) and
    // drop the slot anyway; doing the rewrite at the text layer means
    // `ast_rewrite`'s verbatim header copy emits the cleaned-up form.
    let after_preparse = strip_legacy_none_return_types(&after_preparse);

    // Try to parse for stratum 2.
    let line_index = LineIndex::new(&after_preparse);
    let parsed = parse::parse_with_diagnostics_opts(&after_preparse, 0, "<fmt>", &line_index, &mut bag, enable_effects);

    match parsed {
        Some(file) => {
            // Stratum 2: AST-level rewrites.
            let signals = crate::analyze::fmt_signals(&file);
            let after_ast = ast_rewrite(&after_preparse, &file, &signals, enable_effects);
            let changed = after_ast != source;
            FmtResult {
                output: after_ast,
                changed,
                diagnostics: bag,
            }
        }
        None => {
            // Parse failed — emit only pre-parse fixes.
            let changed = after_preparse != source;
            FmtResult {
                output: after_preparse,
                changed,
                diagnostics: bag,
            }
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

/// Collapse duplicate `import` declarations that share the same path.
///
/// - Two selective imports for the same path → merged into one (union of names,
///   first-occurrence-wins order, deduped).
/// - A whole-module import supersedes any selective imports for the same path.
/// - Returns the source unchanged (same `String` value) when nothing to collapse.
fn collapse_duplicate_imports(source: &str, file: &crate::ast::SourceFile) -> String {
    use std::collections::{HashMap, HashSet};
    use crate::ast::{Decl, ImportKind};

    struct Group {
        first_line_idx: usize,
        is_whole_module: bool,
        whole_module_alias: Option<String>,
        /// (name, alias) pairs, deduped in first-occurrence order.
        selective_names: Vec<(String, Option<String>)>,
        line_indices: Vec<usize>,
    }

    let lines: Vec<&str> = source.lines().collect();
    let line_index = crate::span::LineIndex::new(source);

    // Build groups keyed by import path, preserving first-seen order.
    let mut groups: HashMap<String, Group> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for decl in &file.decls {
        if let Decl::Import(imp) = decl {
            // Derive the 0-based line index directly from the span byte offset.
            let (line_1based, _col) = line_index.line_col(imp.span.start);
            let line_idx = (line_1based - 1) as usize;

            let entry = groups.entry(imp.node.path.clone()).or_insert_with(|| {
                order.push(imp.node.path.clone());
                Group {
                    first_line_idx: line_idx,
                    is_whole_module: false,
                    whole_module_alias: None,
                    selective_names: Vec::new(),
                    line_indices: Vec::new(),
                }
            });
            entry.line_indices.push(line_idx);

            match &imp.node.kind {
                ImportKind::Selective(names) => {
                    for n in names {
                        let key = (n.name.node.clone(), n.alias.clone());
                        if !entry.selective_names.iter().any(|e| e == &key) {
                            entry.selective_names.push(key);
                        }
                    }
                }
                ImportKind::WholeModule { alias } => {
                    entry.is_whole_module = true;
                    entry.whole_module_alias = Some(alias.clone());
                }
            }
        }
    }

    // Nothing to do if every path appears exactly once.
    if !groups.values().any(|g| g.line_indices.len() > 1) {
        return source.to_string();
    }

    // Compute which lines to drop and which to replace.
    let mut to_drop: HashSet<usize> = HashSet::new();
    let mut replacements: HashMap<usize, String> = HashMap::new();

    for path in &order {
        let g = &groups[path];
        if g.line_indices.len() <= 1 {
            continue;
        }
        // All occurrences after the first are dropped.
        for &idx in g.line_indices.iter().skip(1) {
            to_drop.insert(idx);
        }
        // Build the merged import line.
        let merged = if g.is_whole_module {
            format!(r#"import "{}" as {}"#, path, g.whole_module_alias.as_deref().expect("WholeModule branch always sets alias"))
        } else {
            let names = g
                .selective_names
                .iter()
                .map(|(n, a)| match a {
                    Some(alias) => format!("{} as {}", n, alias),
                    None => n.clone(),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(r#"import "{}" {{ {} }}"#, path, names)
        };
        replacements.insert(g.first_line_idx, merged);
    }

    // Reconstruct the source, skipping dropped lines and substituting replacements.
    let mut out = String::with_capacity(source.len());
    for (i, line) in lines.iter().enumerate() {
        if to_drop.contains(&i) {
            continue;
        }
        if let Some(repl) = replacements.get(&i) {
            out.push_str(repl);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    // Preserve original trailing-newline behaviour.
    if !source.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Drop selective import names that are never referenced; if all names in a
/// selective list are unused, drop the entire import line. Whole-module imports
/// whose alias is never referenced are dropped entirely.
///
/// Returns the source unchanged (same `String` value) when nothing to drop.
fn remove_unused_imports(
    source: &str,
    file: &crate::ast::SourceFile,
    signals: &crate::analyze::FmtSignals,
) -> String {
    use crate::ast::{Decl, ImportKind};
    use std::collections::{HashMap, HashSet};

    let lines: Vec<&str> = source.lines().collect();
    let line_index = crate::span::LineIndex::new(source);

    let mut to_drop: HashSet<usize> = HashSet::new();
    let mut replacements: HashMap<usize, String> = HashMap::new();

    for decl in &file.decls {
        let Decl::Import(imp) = decl else { continue };
        // Derive the 0-based line index directly from the span byte offset
        // (same pattern as collapse_duplicate_imports).
        let (line_1based, _col) = line_index.line_col(imp.span.start);
        let line_idx = (line_1based - 1) as usize;

        match &imp.node.kind {
            ImportKind::Selective(names) => {
                let kept: Vec<_> = names
                    .iter()
                    .filter(|n| {
                        let local = n.alias.as_deref().unwrap_or(&n.name.node);
                        signals.referenced_names.contains(local)
                    })
                    .collect();
                if kept.is_empty() {
                    to_drop.insert(line_idx);
                } else if kept.len() < names.len() {
                    let names_str = kept
                        .iter()
                        .map(|n| match &n.alias {
                            Some(a) => format!("{} as {}", n.name.node, a),
                            None => n.name.node.clone(),
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    replacements.insert(
                        line_idx,
                        format!(r#"import "{}" {{ {} }}"#, imp.node.path, names_str),
                    );
                }
            }
            ImportKind::WholeModule { alias } => {
                if !signals.referenced_names.contains(alias) {
                    to_drop.insert(line_idx);
                }
            }
        }
    }

    if to_drop.is_empty() && replacements.is_empty() {
        return source.to_string();
    }

    let mut out = String::with_capacity(source.len());
    for (i, line) in lines.iter().enumerate() {
        if to_drop.contains(&i) {
            continue;
        }
        if let Some(repl) = replacements.get(&i) {
            out.push_str(repl);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    // Preserve original trailing-newline behaviour.
    if !source.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Append any unresolved stdlib names to the existing `@glyph/std` selective
/// import, or insert a new one if none is present.
fn auto_import_stdlib(
    source: &str,
    file: &crate::ast::SourceFile,
    signals: &crate::analyze::FmtSignals,
) -> String {
    use crate::ast::{Decl, ImportKind};

    let mut to_import: Vec<String> = signals
        .unresolved_names
        .iter()
        .filter(|n| crate::analyze::is_stdlib_block_name(n))
        .cloned()
        .collect();
    to_import.sort();
    if to_import.is_empty() {
        return source.to_string();
    }

    let lines: Vec<&str> = source.lines().collect();
    let line_index = crate::span::LineIndex::new(source);

    // Find existing @glyph/std selective import + collect ALL import line
    // indices for "insert after last".
    let mut existing_idx: Option<usize> = None;
    let mut existing_names: Vec<String> = Vec::new();
    let mut all_import_line_indices: Vec<usize> = Vec::new();

    for decl in &file.decls {
        if let Decl::Import(imp) = decl {
            let (line_1based, _col) = line_index.line_col(imp.span.start);
            let line_idx = (line_1based - 1) as usize;
            all_import_line_indices.push(line_idx);
            if existing_idx.is_none() && imp.node.path == "@glyph/std" {
                if let ImportKind::Selective(names) = &imp.node.kind {
                    existing_idx = Some(line_idx);
                    for n in names {
                        existing_names.push(match &n.alias {
                            Some(a) => format!("{} as {}", n.name.node, a),
                            None => n.name.node.clone(),
                        });
                    }
                }
            }
        }
    }

    let mut out = String::with_capacity(source.len() + 64);

    if let Some(idx) = existing_idx {
        let mut all = existing_names.clone();
        for n in &to_import {
            if !all.contains(n) {
                all.push(n.clone());
            }
        }
        let new_line = format!(r#"import "@glyph/std" {{ {} }}"#, all.join(", "));
        for (i, line) in lines.iter().enumerate() {
            if i == idx {
                out.push_str(&new_line);
            } else {
                out.push_str(line);
            }
            out.push('\n');
        }
    } else {
        let new_line = format!(r#"import "@glyph/std" {{ {} }}"#, to_import.join(", "));
        if let Some(&last_import) = all_import_line_indices.last() {
            // Insert after the last existing import.
            for (i, line) in lines.iter().enumerate() {
                out.push_str(line);
                out.push('\n');
                if i == last_import {
                    out.push_str(&new_line);
                    out.push('\n');
                }
            }
        } else {
            // No imports at all: prepend with a blank-line separator.
            out.push_str(&new_line);
            out.push('\n');
            out.push('\n');
            for line in &lines {
                out.push_str(line);
                out.push('\n');
            }
        }
    }

    if !source.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Re-parse `source` and run `f` on the fresh AST/signals. If parsing fails,
/// returns `source` unchanged.
fn reparse_and_run<F>(source: String, enable_effects: bool, f: F) -> String
where
    F: FnOnce(&str, &crate::ast::SourceFile, &crate::analyze::FmtSignals) -> String,
{
    let line_index = crate::span::LineIndex::new(&source);
    let mut bag = crate::diagnostic::DiagBag::new();
    match crate::parse::parse_with_diagnostics_opts(
        &source, 0, "<fmt>", &line_index, &mut bag, enable_effects,
    ) {
        Some(file) => {
            let signals = crate::analyze::fmt_signals(&file);
            f(&source, &file, &signals)
        }
        None => source,
    }
}

/// Stratum 2: AST-level rewrites (dispatcher).
///
/// Runs file-level passes in sequence:
/// 1. `collapse_duplicate_imports` — merge duplicate import lines.
/// 2. `remove_unused_imports` — drop names/lines that are never referenced.
/// 3. `auto_import_stdlib` — insert/extend `@glyph/std` for unresolved stdlib names.
///
/// After each pass that changes the source, the file is re-parsed and signals
/// are recomputed before the next pass uses the AST. After all file-level
/// passes, if any changed occurred, `ast_rewrite_inner` is called with the
/// fresh AST/signals. If any re-parse fails, the latest valid source is
/// returned unchanged (no crash, no regression).
fn ast_rewrite(
    source: &str,
    file: &crate::ast::SourceFile,
    signals: &crate::analyze::FmtSignals,
    enable_effects: bool,
) -> String {
    // Pass 1: collapse duplicate imports — uses original AST/signals.
    let after_collapse = collapse_duplicate_imports(source, file);

    // Pass 2: remove unused imports — needs fresh AST/signals if Pass 1 changed source.
    let after_unused = if after_collapse != source {
        reparse_and_run(after_collapse, enable_effects, remove_unused_imports)
    } else {
        remove_unused_imports(source, file, signals)
    };

    // Pass 3: auto-import stdlib — needs fresh AST/signals if Pass 2 changed source.
    let after_stdlib = if after_unused != source {
        reparse_and_run(after_unused, enable_effects, auto_import_stdlib)
    } else {
        auto_import_stdlib(source, file, signals)
    };

    // Final: re-parse and run inner per-decl rewrites.
    if after_stdlib != source {
        let line_index = crate::span::LineIndex::new(&after_stdlib);
        let mut bag = crate::diagnostic::DiagBag::new();
        if let Some(re) = crate::parse::parse_with_diagnostics_opts(
            &after_stdlib, 0, "<fmt>", &line_index, &mut bag, enable_effects,
        ) {
            let new_signals = crate::analyze::fmt_signals(&re);
            return ast_rewrite_inner(&after_stdlib, &re, &new_signals, enable_effects);
        }
        return after_stdlib;
    }

    ast_rewrite_inner(source, file, signals, enable_effects)
}

/// Stratum 2: AST-level rewrites (inner).
///
/// Operates by identifying declaration boundaries in the source text, then
/// reconstructing each declaration body in canonical sub-section order with
/// hoisted constraints and context.
fn ast_rewrite_inner(
    source: &str,
    file: &crate::ast::SourceFile,
    signals: &crate::analyze::FmtSignals,
    enable_effects: bool,
) -> String {
    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        return source.to_string();
    }

    // Find declaration header lines (indent 0, starts with skill/block/export/const/import).
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
                || trimmed.starts_with("export const ")
                || trimmed.starts_with("const ")
                || trimmed.starts_with("generated ")
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

    // For simple declarations (const, import), just pass through.
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
        if header.starts_with("const ")
            || header.starts_with("export const ")
            || header.starts_with("import ")
            || header.starts_with("generated ")
        {
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
        let rewritten = rewrite_decl_body(&body_lines, ast_decl, signals, enable_effects);
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

/// Build a synthesized `effects:` sub-section line.
fn synthesize_effects_section(effects: &[String], indent: &str) -> String {
    let mut s = String::new();
    s.push_str(indent);
    s.push_str("effects: ");
    s.push_str(&effects.join(", "));
    s.push('\n');
    s
}

/// Rewrite a declaration body (lines at indent >= 1) in canonical order.
fn rewrite_decl_body(
    body_lines: &[&str],
    ast_decl: Option<&crate::ast::Decl>,
    signals: &crate::analyze::FmtSignals,
    enable_effects: bool,
) -> String {
    let placeholder_target = placeholder_string_return_target(ast_decl);

    // Parse lines into sections.
    let mut sections: Vec<Section> = Vec::new();
    let mut current_kind: Option<SectionKind> = None;
    let mut current_lines: Vec<String> = Vec::new();
    let mut in_flow_block = false;

    // Constraint and context markers found at body level or flow top level that
    // should be hoisted.
    let mut hoisted_constraints: Vec<String> = Vec::new();
    let mut hoisted_context: Vec<String> = Vec::new();

    for raw_line in body_lines {
        let line_owned = placeholder_target
            .as_ref()
            .and_then(|repair| rewrite_placeholder_return_line(raw_line, repair))
            .unwrap_or_else(|| (*raw_line).to_string());
        let line = line_owned.as_str();
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
                    sections.push(Section {
                        kind: prev_kind,
                        lines: std::mem::take(&mut current_lines),
                    });
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
                // Rewrite bare unresolved names in flow to `name()`.
                let rewritten_line = rewrite_bare_name_in_flow_line(line, signals)
                    .unwrap_or_else(|| line.to_string());
                current_lines.push(rewritten_line);
            } else {
                current_lines.push(line.to_string());
            }
        } else {
            // Line at body level that's not a recognized section header.
            // Could be a bare name or something else — pass through.
            current_lines.push(line.to_string());
            current_kind = Some(SectionKind::BlankOrOther);
        }
    }

    // Flush last section.
    if let Some(kind) = current_kind {
        sections.push(Section {
            kind: kind,
            lines: current_lines,
        });
    }

    // Compute the decl name for effects lookup (Skill and Block only).
    let decl_name: Option<&str> = ast_decl.and_then(|d| match d {
        crate::ast::Decl::Skill(s) => Some(s.node.name.as_str()),
        crate::ast::Decl::Block(b) => Some(b.node.name.as_str()),
        _ => None,
    });

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
        // Issue #109 chunk 4 — gather ALL sections of this kind so duplicate
        // sub-sections under the same declaration get merged into a single
        // canonical block (instead of being silently dropped, which was the
        // pre-#109 behavior of `sections.iter().find(...)`).
        let matching: Vec<&Section> = sections.iter().filter(|s| &s.kind == target_kind).collect();
        let has_section = !matching.is_empty();

        match target_kind {
            SectionKind::Effects => {
                if has_section {
                    emit_merged_sections(&mut out, &matching);
                } else if enable_effects {
                    if let Some(name) = decl_name {
                        if let Some(effects) = signals.inferred_effects.get(name) {
                            if !effects.is_empty() {
                                out.push_str(&synthesize_effects_section(effects, "    "));
                            }
                        }
                    }
                }
            }
            SectionKind::Context => {
                if !hoisted_context.is_empty() || has_section {
                    if has_section {
                        // Existing context: section(s) — emit merged form,
                        // then append hoisted entries.
                        emit_merged_sections(&mut out, &matching);
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
                if !hoisted_constraints.is_empty() || has_section {
                    if has_section {
                        emit_merged_sections(&mut out, &matching);
                        for marker in &hoisted_constraints {
                            out.push_str("        ");
                            out.push_str(marker);
                            out.push('\n');
                        }
                    } else {
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
                if has_section {
                    emit_merged_sections(&mut out, &matching);
                }
            }
        }
    }

    // Emit any "other" sections (blank/unknown) that didn't match canonical kinds.
    for sec in &sections {
        if !canonical_order.contains(&sec.kind)
            && sec.kind != SectionKind::BodyConstraintMarker
            && sec.kind != SectionKind::BodyContextMarker
        {
            for line in &sec.lines {
                out.push_str(line);
                out.push('\n');
            }
        }
    }

    out
}

/// If `line` is a bare identifier in a flow section that is unresolved, return
/// the rewritten form `indent + name + "()"`. Returns `None` if the line is
/// not a bare identifier or the name is locally bound (not in `unresolved_names`).
fn rewrite_bare_name_in_flow_line(line: &str, signals: &crate::analyze::FmtSignals) -> Option<String> {
    let indent_len = line.len() - line.trim_start().len();
    let indent = &line[..indent_len];
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Must be a pure bare identifier — letters/digits/underscore only.
    if !trimmed.chars().all(|c| c == '_' || c.is_ascii_alphanumeric()) {
        return None;
    }
    let first = trimmed.chars().next()?;
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return None;
    }
    if signals.unresolved_names.contains(trimmed) {
        return Some(format!("{}{}()", indent, trimmed));
    }
    None
}

/// Issue #109 chunk 4 — merge multiple sections of the same kind into one
/// canonical block.
///
/// `matching` must be non-empty and must list the sections in source order
/// (which it is, because the caller iterates `sections` in source order via
/// `filter`).
///
/// Emission rules (per `design/repair.md` §4.11):
/// - The first section is emitted verbatim (header line + body lines), so
///   the canonical sub-section header keeps its original formatting,
///   indentation, and any trailing comment.
/// - For each subsequent section, the `<kind>:` header line is dropped —
///   but if it carried a trailing comment (rule b), that comment is
///   surfaced as a whole-line comment on its own line before the appended
///   body, indented to match the dropped header.
/// - Body lines (everything after the header) are appended verbatim. Whole-
///   line comments inside the body (rule a) and comments at the boundary
///   that landed in the previous section's accumulator (rule c) ride along
///   automatically because they are already in `lines`.
///
/// The single-line forms (`description: "..."` and `effects: a, b`) need
/// special handling: their "body" lives on the header line itself, and the
/// parser doesn't admit a multi-line form, so duplicates can only be
/// reconciled by concatenating the inline contents into one canonical
/// header. Multi-line forms (`context:`, `constraints:`, `flow:`) splice
/// body lines verbatim.
fn emit_merged_sections(out: &mut String, matching: &[&Section]) {
    if matching.is_empty() {
        return;
    }
    match matching[0].kind {
        SectionKind::Description => emit_merged_descriptions(out, matching),
        SectionKind::Effects => emit_merged_effects(out, matching),
        _ => emit_merged_multiline(out, matching),
    }
}

/// Multi-line merge: appropriate for `context:`, `constraints:`, `flow:`.
/// Mixed short/long context (`context: "x"` followed by another `context:`
/// with indented entries) is supported by lifting the short form's inline
/// string into an indented body line under the canonical header.
fn emit_merged_multiline(out: &mut String, matching: &[&Section]) {
    let mut iter = matching.iter();
    if let Some(first) = iter.next() {
        // Anchor emission. The standard case (single section, or anchor
        // already in long-form) is verbatim. The corner case (anchor in
        // SHORT form `<kind>: "x"` AND at least one duplicate) requires
        // normalization: a short-form header followed by appended body
        // lines (from the duplicate) is invalid Glyph — short-form is
        // exclusive. So when both conditions hold, we lift the anchor's
        // inline content into a body line under a bare `<kind>:` header,
        // preserving any trailing comment on the original short-form
        // header. Long-form duplicates then splice cleanly underneath.
        let header_line = first.lines.first().map(|s| s.as_str()).unwrap_or("");
        let anchor_inline_raw = inline_content_after_colon(header_line);
        let anchor_inline_payload = anchor_inline_raw
            .map(|c| strip_trailing_comment(c).trim().to_string())
            .filter(|s| !s.is_empty());
        let anchor_is_short_form = anchor_inline_payload.is_some();
        let has_duplicates = matching.len() > 1;

        if anchor_is_short_form && has_duplicates {
            let indent = leading_whitespace_of(header_line);
            if let Some(colon_pos) = header_line.find(':') {
                // Bare `<indent><kind>:` header. If the original short-form
                // header had a trailing comment, hoist it onto the bare
                // header line so no source-author comment is dropped.
                out.push_str(&header_line[..=colon_pos]);
                if let Some(comment) = trailing_comment_after_keyword(header_line) {
                    out.push_str("  ");
                    out.push_str(&comment);
                }
                out.push('\n');
                // Lift inline content into an indent-2 body line under the
                // canonical bare header.
                if let Some(payload) = anchor_inline_payload {
                    out.push_str(indent);
                    out.push_str("    ");
                    out.push_str(&payload);
                    out.push('\n');
                }
                // Anchor-side body lines after the header (rare for true
                // short-form, but preserve them if present).
                for line in first.lines.iter().skip(1) {
                    out.push_str(line);
                    out.push('\n');
                }
            } else {
                // Pathological — no colon in header. Fall back to verbatim.
                for line in &first.lines {
                    out.push_str(line);
                    out.push('\n');
                }
            }
        } else {
            // Single section, or anchor already in long-form, or anchor's
            // inline content is empty (e.g. trailing-only comment) — emit
            // verbatim, which is the canonical form.
            for line in &first.lines {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    for section in iter {
        // Drop the header line, preserve any trailing comment on it as a
        // whole-line comment at the boundary (rule b).
        if let Some(header_line) = section.lines.first() {
            if let Some(comment) = trailing_comment_after_keyword(header_line) {
                let indent = leading_whitespace_of(header_line);
                out.push_str(indent);
                out.push_str(&comment);
                out.push('\n');
            }
            // If this duplicate used the short inline form (e.g.
            // `context: "x"` instead of `context:` + indented body), lift
            // the inline content into a body line at indent 2 so the merged
            // form stays valid syntax.
            if let Some(inline) = inline_content_after_colon(header_line) {
                let trimmed_inline = strip_trailing_comment(inline);
                let trimmed_inline = trimmed_inline.trim();
                if !trimmed_inline.is_empty() {
                    out.push_str("        ");
                    out.push_str(trimmed_inline);
                    out.push('\n');
                }
            }
        }
        for line in section.lines.iter().skip(1) {
            out.push_str(line);
            out.push('\n');
        }
    }
}

/// Description merge: the parser only accepts a single-line form, so
/// duplicates collapse by concatenating their inline strings into one
/// canonical `description: "<a> <b> ..."` line. Trailing comments from
/// removed lines are emitted as whole-line comments at the original indent
/// before the merged line.
///
/// When there is only one occurrence (no merge needed), the section is
/// emitted verbatim. This is required for correctness — the merge path
/// rebuilds the string literal via `unwrap_string_literal` + concatenation,
/// which does NOT round-trip escapes (`\"`, `\\`) and would corrupt them on
/// every fmt run. Comment-preservation is also handled by verbatim emission
/// (rule b lifts comments off DROPPED headers; the single-section case has
/// no dropped header, so the original line is the canonical form).
fn emit_merged_descriptions(out: &mut String, matching: &[&Section]) {
    if matching.len() <= 1 {
        if let Some(section) = matching.first() {
            for line in &section.lines {
                out.push_str(line);
                out.push('\n');
            }
        }
        return;
    }
    let mut bodies: Vec<String> = Vec::new();
    let indent = matching
        .first()
        .and_then(|s| s.lines.first())
        .map(|l| leading_whitespace_of(l).to_string())
        .unwrap_or_else(|| "    ".to_string());
    let mut header_indent_for_first: Option<String> = None;
    let mut comments: Vec<(String, String)> = Vec::new(); // (indent, comment)
    for (idx, section) in matching.iter().enumerate() {
        if let Some(line) = section.lines.first() {
            let line_indent = leading_whitespace_of(line).to_string();
            if idx == 0 {
                header_indent_for_first = Some(line_indent.clone());
            } else if let Some(comment) = trailing_comment_after_keyword(line) {
                comments.push((line_indent, comment));
            }
            if let Some(content) = inline_content_after_colon(line) {
                let payload = strip_trailing_comment(content);
                if let Some(inner) = unwrap_string_literal(payload.trim()) {
                    // Issue #109 codex pass-2 finding 6: decode escape
                    // sequences before merging so the round-trip
                    // `decode → concat → re-escape` is lossless.
                    bodies.push(unescape_string_literal_inner(&inner));
                }
            }
        }
        // Issue #109 codex pass-2 finding 7: a `description:` is a
        // single-line section, so anything in `lines[1..]` is non-content
        // (whole-line `// comment` accumulated into the preceding section
        // by `rewrite_decl_body`). Lift those whole-line comments into
        // the boundary so they aren't silently dropped on merge.
        for extra in section.lines.iter().skip(1) {
            let trimmed = extra.trim_start();
            if trimmed.starts_with("//") {
                let cindent = leading_whitespace_of(extra).to_string();
                comments.push((cindent, trimmed.to_string()));
            }
        }
    }
    // Emit any trailing comments from removed headers BEFORE the canonical
    // line (rule b — boundary).
    for (cindent, ctext) in &comments {
        out.push_str(cindent);
        out.push_str(ctext);
        out.push('\n');
    }
    let header_indent = header_indent_for_first.unwrap_or(indent);
    // Issue #109 codex pass-3 finding 10: `design/repair.md` §4.11.4 says
    // multi-line `description:` merges concatenate "with a single blank
    // line between bodies"; the design is silent on the inline-string
    // form, so we default to a single `\n` embedded inside the merged
    // literal. `escape_string_literal` re-encodes that `\n` back to the
    // two-character `\n` source escape so the merged inline literal stays
    // single-line in the source file.
    let merged = bodies.join("\n");
    out.push_str(&header_indent);
    out.push_str("description: \"");
    out.push_str(&escape_string_literal(&merged));
    out.push_str("\"\n");
}

/// Effects merge: the parser only accepts a single-line short form
/// (comma-separated idents), so duplicates collapse by concatenating their
/// effect lists into one canonical `effects: a, b, c, ...` line.
///
/// When there is only one occurrence (no merge needed), the section is
/// emitted verbatim — the merge path rebuilds the line from
/// `effects_acc.join(", ")`, which drops any trailing comment on the
/// original header. Verbatim emission preserves the comment and any
/// non-canonical whitespace the user wrote.
fn emit_merged_effects(out: &mut String, matching: &[&Section]) {
    if matching.len() <= 1 {
        if let Some(section) = matching.first() {
            for line in &section.lines {
                out.push_str(line);
                out.push('\n');
            }
        }
        return;
    }
    let mut effects_acc: Vec<String> = Vec::new();
    let mut header_indent: Option<String> = None;
    let mut comments: Vec<(String, String)> = Vec::new();
    for (idx, section) in matching.iter().enumerate() {
        if let Some(line) = section.lines.first() {
            let line_indent = leading_whitespace_of(line).to_string();
            if idx == 0 {
                header_indent = Some(line_indent.clone());
            } else if let Some(comment) = trailing_comment_after_keyword(line) {
                comments.push((line_indent, comment));
            }
            if let Some(content) = inline_content_after_colon(line) {
                let payload = strip_trailing_comment(content);
                for tok in payload.split(',') {
                    let t = tok.trim();
                    if !t.is_empty() {
                        effects_acc.push(t.to_string());
                    }
                }
            }
        }
        // Issue #109 codex pass-2 finding 7: lift any whole-line `//`
        // comments out of `lines[1..]` so the boundary is preserved.
        for extra in section.lines.iter().skip(1) {
            let trimmed = extra.trim_start();
            if trimmed.starts_with("//") {
                let cindent = leading_whitespace_of(extra).to_string();
                comments.push((cindent, trimmed.to_string()));
            }
        }
    }
    for (cindent, ctext) in &comments {
        out.push_str(cindent);
        out.push_str(ctext);
        out.push('\n');
    }
    let header_indent = header_indent.unwrap_or_else(|| "    ".to_string());
    out.push_str(&header_indent);
    out.push_str("effects: ");
    out.push_str(&effects_acc.join(", "));
    out.push('\n');
}

/// Return the slice after the first `:` on a section header line, or `None`
/// if the line is just `<indent><kind>:` (no inline content). Returned
/// slice is the raw content (may begin with whitespace and may include a
/// trailing comment) — callers further parse it.
fn inline_content_after_colon(line: &str) -> Option<&str> {
    let colon = line.find(':')?;
    let after = &line[colon + 1..];
    if after.trim().is_empty() {
        return None;
    }
    Some(after)
}

/// Strip a trailing `// ...` line comment from a string slice (string-literal
/// aware). Returns the slice up to (not including) the comment marker.
fn strip_trailing_comment(s: &str) -> &str {
    let mut in_string = false;
    let mut prev: char = ' ';
    let bytes = s.as_bytes();
    for (i, ch) in s.char_indices() {
        if in_string {
            if ch == '"' && prev != '\\' {
                in_string = false;
            }
        } else if ch == '"' {
            in_string = true;
        } else if ch == '/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            return &s[..i];
        }
        prev = ch;
    }
    s
}

/// Trailing-comment extractor for the case where the comment sits on a
/// `<kind>:` header line — this includes lines like `    constraints:  // foo`
/// and `    description: "x"  // bar`. String-literal aware, same as
/// [`strip_trailing_comment`]. Returns the comment slice trimmed of leading
/// whitespace, including the `//` marker.
fn trailing_comment_after_keyword(line: &str) -> Option<String> {
    let mut in_string = false;
    let mut prev: char = ' ';
    let bytes = line.as_bytes();
    for (i, ch) in line.char_indices() {
        if in_string {
            if ch == '"' && prev != '\\' {
                in_string = false;
            }
        } else if ch == '"' {
            in_string = true;
        } else if ch == '/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            return Some(line[i..].trim().to_string());
        }
        prev = ch;
    }
    None
}


/// Strip the surrounding `"..."` from a string literal token slice. Returns
/// `None` if the slice isn't a quoted string.
fn unwrap_string_literal(s: &str) -> Option<String> {
    let inner = s.strip_prefix('"').and_then(|x| x.strip_suffix('"'))?;
    Some(inner.to_string())
}

/// Decode the raw inner-source of a Glyph string literal, mirroring
/// `tokenize.rs`'s "minimal escape handling: \" \\ \n \t" so a fmt-time
/// round trip (decode → concat → re-escape via `escape_string_literal`)
/// is byte-equal to what the tokenizer would produce. Issue #109 codex
/// pass-2 finding 6: without this, the multi-section `description:` /
/// `effects:` merge double-escaped `\"` and `\\` because
/// `unwrap_string_literal` strips the outer quotes but leaves the
/// inner escape sequences as raw `\X` byte pairs.
///
/// Unknown escapes (`\X` for X not in `"`, `\`, `n`, `t`) are preserved
/// verbatim — same fallback as the tokenizer at `tokenize.rs` §"unknown
/// escapes preserve the literal `\X` source bytes".
fn unescape_string_literal_inner(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let bytes = inner.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'"' => {
                    out.push('"');
                    i += 2;
                    continue;
                }
                b'\\' => {
                    out.push('\\');
                    i += 2;
                    continue;
                }
                b'n' => {
                    out.push('\n');
                    i += 2;
                    continue;
                }
                b't' => {
                    out.push('\t');
                    i += 2;
                    continue;
                }
                _ => {
                    // Unknown escape: preserve literal backslash + char
                    // bytes (matches tokenizer fallback). Push the `\` and
                    // let the next iteration push the following byte.
                    out.push('\\');
                    i += 1;
                    continue;
                }
            }
        }
        // ASCII fast path; otherwise reconstruct the full UTF-8 char.
        if b.is_ascii() {
            out.push(b as char);
            i += 1;
        } else {
            // Find the end of this UTF-8 codepoint.
            let cont_len = if b & 0xE0 == 0xC0 {
                2
            } else if b & 0xF0 == 0xE0 {
                3
            } else if b & 0xF8 == 0xF0 {
                4
            } else {
                1
            };
            let end = (i + cont_len).min(bytes.len());
            // Safe: the source is a valid `&str` so the byte range is a
            // valid UTF-8 codepoint boundary.
            out.push_str(&inner[i..end]);
            i = end;
        }
    }
    out
}

/// Re-escape a description payload before re-emitting it as a Glyph string
/// literal.
///
/// `"` and `\` always need escaping. Control characters in a single
/// description body would already have failed Chunk-2 parsing because the
/// parser tokenizes string literals before we ever see them — but the
/// post-merge string handed in here can contain a real `\n` or `\t`,
/// because issue #109 codex pass-3 finding 10 chose `\n` as the
/// inline-form description merge separator. Re-encoding those control
/// characters back to their two-character source escapes keeps the
/// merged inline literal single-line and re-parseable, mirroring the
/// tokenizer's escape vocabulary in `crate::tokenize` (`\"`, `\\`, `\n`,
/// `\t`).
fn escape_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

/// Return the leading-whitespace prefix of `line` as a borrowed slice.
fn leading_whitespace_of(line: &str) -> &str {
    let n = line.len() - line.trim_start().len();
    &line[..n]
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

enum PlaceholderRepair {
    Identifier(String),
    Description(String),
}

fn placeholder_string_return_target(ast_decl: Option<&crate::ast::Decl>) -> Option<PlaceholderRepair> {
    let decl = ast_decl?;
    match decl {
        crate::ast::Decl::Skill(s) if is_domain_return_type(s.node.return_type.as_ref()) => {
            flow_placeholder_target(&s.node.flow)
        }
        crate::ast::Decl::Block(b) if is_domain_return_type(b.node.return_type.as_ref()) => {
            flow_placeholder_target(&b.node.flow)
        }
        crate::ast::Decl::ExportBlock(b) if is_domain_return_type(b.node.return_type.as_ref()) => {
            return_expr_placeholder_target(b.node.terminal_return.as_ref())
        }
        _ => None,
    }
}

fn flow_placeholder_target(flow: &[crate::ast::FlowStmt]) -> Option<PlaceholderRepair> {
    flow.iter().rev().find_map(|stmt| match stmt {
        crate::ast::FlowStmt::Return(expr) => return_expr_placeholder_target(Some(expr)),
        _ => None,
    })
}

fn return_expr_placeholder_target(expr: Option<&crate::ast::ReturnExpr>) -> Option<PlaceholderRepair> {
    let Some(crate::ast::ReturnExpr::Inline(s)) = expr else {
        return None;
    };
    if let Some(id) = placeholder_identifier(s) {
        return Some(PlaceholderRepair::Identifier(id.to_string()));
    }
    if let Some(desc) = placeholder_description(s) {
        return Some(PlaceholderRepair::Description(desc.to_string()));
    }
    None
}

fn placeholder_identifier(s: &str) -> Option<&str> {
    let inner = s.strip_prefix('<')?.strip_suffix('>')?;
    if inner.is_empty() {
        return None;
    }
    let mut chars = inner.chars();
    let first = chars.next()?;
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return None;
    }
    if chars.all(|c| c == '_' || c.is_ascii_alphanumeric()) {
        Some(inner)
    } else {
        None
    }
}

fn placeholder_description(s: &str) -> Option<&str> {
    // Mirrors `analyze::placeholder_description` — must reject the same set of
    // contents so `glyph check` (which fires the diagnostic) and `glyph fmt`
    // (which performs the rewrite) stay in sync. See the analyze.rs copy for
    // the rationale (round-trip safety after tokenizer-level escape decoding).
    if placeholder_identifier(s).is_some() {
        return None;
    }
    let inner = s.strip_prefix('<')?.strip_suffix('>')?;
    if inner.is_empty() {
        return None;
    }
    if inner.contains(|c: char| c == '"' || c == '\\' || c == '\n' || c == '\t' || c == '\r') {
        return None;
    }
    Some(inner)
}

fn is_domain_return_type(rt: Option<&crate::span::Spanned<String>>) -> bool {
    let Some(rt) = rt else {
        return false;
    };
    crate::type_position::validate_type_position(&rt.node).is_ok()
        && !is_builtin_type_name(&rt.node)
}

fn is_builtin_type_name(s: &str) -> bool {
    const CANONICAL_BUILTINS: &[&str] = &["string", "int", "float", "bool", "none", "agent"];
    let canonical = crate::domain_registry::canonicalize_identifier(s);
    CANONICAL_BUILTINS.contains(&canonical.as_str())
}

fn rewrite_placeholder_return_line(line: &str, repair: &PlaceholderRepair) -> Option<String> {
    let trimmed = line.trim();
    let indent_len = line.len() - line.trim_start().len();
    let indent = &line[..indent_len];
    match repair {
        PlaceholderRepair::Identifier(target) => {
            let expected = format!("return \"<{target}>\"");
            if trimmed != expected {
                return None;
            }
            Some(format!("{}return <{}>", indent, target))
        }
        PlaceholderRepair::Description(desc) => {
            let expected = format!("return \"<{desc}>\"");
            if trimmed != expected {
                return None;
            }
            Some(format!("{}return <\"{}\">", indent, desc))
        }
    }
}

/// Strip legacy `-> None` (case-insensitive) return-type annotations from
/// declaration headers. Issue #82 dropped the `-> None` annotation in favor
/// of an omitted `->`; this text-level pass rewrites legacy sources during
/// `glyph fmt` so they conform to the new surface.
///
/// Applies only to lines at indent 0 that begin with a declaration keyword
/// (`skill `, `block `, `export block `, `generated block `). Body lines and
/// non-declaration top-level lines are excluded by construction, so the
/// `none` value-position keyword (`return none`, `effects: none`, …) is
/// untouched.
///
/// Detection mirrors `parse::Parser::try_parse_return_type`'s ident-boundary
/// check: locate `->`, skip interior whitespace, read an ident, and match
/// case-insensitively against `none`. Matching ident is stripped along with
/// the preceding whitespace and `->`, then the line's trailing whitespace is
/// trimmed so `skill foo()  ->  None  ` becomes `skill foo()`.
///
/// Idempotent: once stripped, no `-> none` remains, so a second pass is a
/// no-op.
fn strip_legacy_none_return_types(source: &str) -> String {
    let lines: Vec<&str> = source.split('\n').collect();
    let mut out = String::with_capacity(source.len());
    for (idx, line) in lines.iter().enumerate() {
        if is_decl_header_line(line) {
            out.push_str(&strip_none_return_from_line(line));
        } else {
            out.push_str(line);
        }
        if idx + 1 < lines.len() {
            out.push('\n');
        }
    }
    out
}

/// True iff `line` is a declaration header line (indent 0, declaration
/// keyword prefix). Used to scope `strip_legacy_none_return_types` to
/// headers only.
fn is_decl_header_line(line: &str) -> bool {
    if line.starts_with(' ') || line.starts_with('\t') {
        return false;
    }
    line.starts_with("skill ")
        || line.starts_with("block ")
        || line.starts_with("export block ")
        || line.starts_with("generated block ")
}

/// Strip a trailing `-> None` (case-insensitive) annotation from a single
/// declaration header line. If no match is found, returns the line
/// unchanged. Trailing whitespace is trimmed on a successful strip so the
/// result has no dangling space.
///
/// The match is restricted to the **return-type slot** — i.e., the substring
/// strictly after the rightmost `)` on the line (the parameter-list close).
/// This prevents a `-> None` substring inside a string-default parameter
/// (e.g. `block foo(msg = "a -> None")`) from being silently stripped.
/// Per the header grammar, only whitespace may sit between the param-close
/// and the return-type `->`, and only whitespace may follow the type ident
/// — both are enforced so we leave malformed or overdecorated lines alone.
fn strip_none_return_from_line(line: &str) -> String {
    let bytes = line.as_bytes();
    // Locate the parameter-list close. If the line has no `)` at all, it's
    // not a well-formed declaration header — leave it alone.
    let close = match bytes.iter().rposition(|&b| b == b')') {
        Some(p) => p,
        None => return line.to_string(),
    };
    // Examine only the post-`)` substring.
    let mut j = close + 1;
    // Whitespace before `->`.
    while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
        j += 1;
    }
    // Need a literal `->` next. If anything else (or end-of-line), bail.
    if j + 1 >= bytes.len() || bytes[j] != b'-' || bytes[j + 1] != b'>' {
        return line.to_string();
    }
    let arrow_start = j;
    j += 2;
    // Whitespace after `->`.
    while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
        j += 1;
    }
    // Read the type ident.
    let ident_start = j;
    while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
        j += 1;
    }
    let ident_end = j;
    if ident_end == ident_start || !line[ident_start..ident_end].eq_ignore_ascii_case("none") {
        return line.to_string();
    }
    // Per the header grammar, nothing but trailing whitespace may follow
    // the return-type ident. If anything else appears, leave the line alone.
    if bytes[ident_end..].iter().any(|&b| b != b' ' && b != b'\t') {
        return line.to_string();
    }
    // Match found. Strip from the run of whitespace immediately preceding
    // `->` through end-of-line; the prefix already ends at `)` with no
    // trailing whitespace, so a final `trim_end` is defensive (handles a
    // stray `\r` on Windows line endings).
    let mut strip_start = arrow_start;
    while strip_start > 0 && (bytes[strip_start - 1] == b' ' || bytes[strip_start - 1] == b'\t') {
        strip_start -= 1;
    }
    line[..strip_start].trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Issue #82 chunk 3: G::parse::none-as-return-type repair ---

    #[test]
    fn strip_none_return_skill_basic() {
        let src = "skill foo() -> None\n    flow:\n        \"x\"\n";
        let out = strip_legacy_none_return_types(src);
        assert_eq!(out, "skill foo()\n    flow:\n        \"x\"\n");
    }

    #[test]
    fn strip_none_return_lowercase() {
        let src = "skill foo() -> none\n    flow:\n        \"x\"\n";
        let out = strip_legacy_none_return_types(src);
        assert_eq!(out, "skill foo()\n    flow:\n        \"x\"\n");
    }

    #[test]
    fn strip_none_return_uppercase() {
        let src = "skill foo() -> NONE\n    flow:\n        \"x\"\n";
        let out = strip_legacy_none_return_types(src);
        assert_eq!(out, "skill foo()\n    flow:\n        \"x\"\n");
    }

    #[test]
    fn strip_none_return_extra_interior_spaces() {
        let src = "skill foo()  ->  None\n    flow:\n        \"x\"\n";
        let out = strip_legacy_none_return_types(src);
        assert_eq!(out, "skill foo()\n    flow:\n        \"x\"\n");
    }

    #[test]
    fn strip_none_return_trailing_whitespace() {
        let src = "skill foo() -> None  \n    flow:\n        \"x\"\n";
        let out = strip_legacy_none_return_types(src);
        assert_eq!(out, "skill foo()\n    flow:\n        \"x\"\n");
    }

    #[test]
    fn strip_none_return_block() {
        let src = "block helper() -> None\n    description: \"d\"\n";
        let out = strip_legacy_none_return_types(src);
        assert_eq!(out, "block helper()\n    description: \"d\"\n");
    }

    #[test]
    fn strip_none_return_export_block() {
        let src = "export block widget() -> None\n    flow:\n        \"x\"\n";
        let out = strip_legacy_none_return_types(src);
        assert_eq!(out, "export block widget()\n    flow:\n        \"x\"\n");
    }

    #[test]
    fn strip_none_return_generated_block() {
        // Defensive: design says `generated block` headers don't admit `->`,
        // but a legacy file that mistakenly has one should still get cleaned.
        let src = "generated block reword() -> None\n    description: \"d\"\n";
        let out = strip_legacy_none_return_types(src);
        assert_eq!(out, "generated block reword()\n    description: \"d\"\n");
    }

    #[test]
    fn strip_none_return_preserves_valid_arrow_type() {
        // A valid `-> SomeType` header must survive untouched.
        let src = "skill foo() -> SomeType\n    flow:\n        \"x\"\n";
        let out = strip_legacy_none_return_types(src);
        assert_eq!(out, src, "valid `-> SomeType` must not be touched");
    }

    #[test]
    fn strip_none_return_does_not_match_none_prefix() {
        // Ident-boundary: `-> nonexistent` must not match `none`.
        let src = "skill foo() -> nonexistent\n    flow:\n        \"x\"\n";
        let out = strip_legacy_none_return_types(src);
        assert_eq!(out, src, "`-> nonexistent` must not be matched as `none`");
    }

    #[test]
    fn strip_none_return_does_not_touch_body_return_none() {
        // The `none` value-keyword in the body must survive.
        let src = "skill foo()\n    flow:\n        return none\n";
        let out = strip_legacy_none_return_types(src);
        assert_eq!(out, src, "body `return none` must be untouched");
    }

    #[test]
    fn strip_none_return_does_not_touch_body_arrow_none() {
        // A body line that happens to contain `-> None` (e.g. inside a
        // string literal) must survive — only header lines are scanned.
        let src = "skill foo()\n    flow:\n        \"a -> None marker\"\n";
        let out = strip_legacy_none_return_types(src);
        assert_eq!(out, src, "body line with `-> None` text must be untouched");
    }

    #[test]
    fn strip_none_return_multi_decl_only_legacy_stripped() {
        // Mixed file: legacy `-> None` stripped; valid `-> Path` preserved.
        let src = "\
skill cleanup() -> None
    flow:
        \"clean up\"

export block compute(scope = \".\") -> Path
    flow:
        \"compute\"
        return scope
";
        let out = strip_legacy_none_return_types(src);
        let expected = "\
skill cleanup()
    flow:
        \"clean up\"

export block compute(scope = \".\") -> Path
    flow:
        \"compute\"
        return scope
";
        assert_eq!(out, expected);
    }

    #[test]
    fn strip_none_return_idempotent() {
        // Running the strip a second time must be a no-op.
        let src = "skill foo() -> None\n    flow:\n        \"x\"\n";
        let once = strip_legacy_none_return_types(src);
        let twice = strip_legacy_none_return_types(&once);
        assert_eq!(once, twice, "strip must be idempotent");
        // And on already-clean source, the strip must be a no-op.
        let clean = "skill foo()\n    flow:\n        \"x\"\n";
        let out = strip_legacy_none_return_types(clean);
        assert_eq!(out, clean, "already-clean source must be unchanged");
    }

    #[test]
    fn strip_none_return_no_trailing_newline() {
        // Source without a trailing newline must round-trip cleanly.
        let src = "skill foo() -> None";
        let out = strip_legacy_none_return_types(src);
        assert_eq!(out, "skill foo()");
    }

    #[test]
    fn fmt_source_strips_legacy_none_return() {
        // End-to-end: `fmt_source` produces a cleaned-up output and
        // `changed: true` when the only difference is `-> None`.
        let src = "skill foo() -> None\n    flow:\n        \"x\"\n";
        let result = fmt_source(src, false);
        assert!(result.changed, "fmt should report changed=true");
        assert!(
            !result.output.contains("-> None"),
            "no `-> None` should remain, got:\n{}",
            result.output
        );
        assert!(
            result.output.contains("skill foo()"),
            "stripped header should be present, got:\n{}",
            result.output
        );
    }

    #[test]
    fn fmt_source_rewrites_placeholder_string_return_to_output_target() {
        let src = "\
skill current() -> BranchName
    description: \"Return the current branch.\"
    flow:
        return \"<current_branch>\"
";
        let result = fmt_source(src, false);
        assert!(result.changed, "fmt should rewrite the placeholder string");
        assert!(
            result.output.contains("        return <current_branch>\n"),
            "expected output target return after fmt, got:\n{}",
            result.output
        );
        assert!(
            !result.output.contains("return \"<current_branch>\""),
            "placeholder string return should be gone, got:\n{}",
            result.output
        );
    }

    #[test]
    fn fmt_source_rewrites_descriptive_placeholder_string_return_to_output_target() {
        let src = "\
skill diagnose() -> Confirmation
    description: \"Diagnose the issue.\"
    flow:
        return \"<root cause and severity>\"
";
        let result = fmt_source(src, false);
        assert!(result.changed, "fmt should rewrite the descriptive placeholder string");
        assert!(
            result.output.contains("        return <\"root cause and severity\">\n"),
            "expected descriptive output target return after fmt, got:\n{}",
            result.output
        );
        assert!(
            !result.output.contains("return \"<root cause and severity>\""),
            "placeholder string return should be gone, got:\n{}",
            result.output
        );
    }

    #[test]
    fn fmt_source_leaves_placeholder_string_return_with_inner_quotes_unrewritten() {
        // "<\"foo\">" contains literal quotes inside the angle brackets;
        // rewriting it would produce invalid syntax, so fmt must leave it as-is.
        let src = "\
skill diagnose() -> Confirmation
    description: \"Diagnose the issue.\"
    flow:
        return \"<\\\"foo\\\">\"
";
        let result = fmt_source(src, false);
        assert_eq!(result.output, src, "line with inner-quoted placeholder must be left unrewritten");
        assert!(!result.changed, "fmt must not mark source as changed");
    }

    #[test]
    fn fmt_source_leaves_placeholder_string_return_with_inner_escapes_unrewritten() {
        // The tokenizer decodes `\n` to a literal newline inside the string,
        // so the AST-level content no longer matches the source spelling. The
        // descriptive guard must reject anything containing chars that require
        // source-level escaping; otherwise fmt would silently fail to rewrite
        // (decoded form != source form when reconstructing the line).
        let cases: &[(&str, &str)] = &[
            ("newline",   "skill d() -> Confirmation\n    flow:\n        return \"<root cause\\nseverity>\"\n"),
            ("tab",       "skill d() -> Confirmation\n    flow:\n        return \"<root\\tcause>\"\n"),
            ("cr",        "skill d() -> Confirmation\n    flow:\n        return \"<root\\rcause>\"\n"),
            ("backslash", "skill d() -> Confirmation\n    flow:\n        return \"<path\\\\to\\\\foo>\"\n"),
        ];
        for (label, src) in cases {
            let result = fmt_source(src, false);
            assert_eq!(
                result.output, *src,
                "[{label}] line with escape-requiring inner placeholder must be left unrewritten"
            );
            assert!(!result.changed, "[{label}] fmt must not mark source as changed");
        }
    }

    #[test]
    fn fmt_source_preserves_placeholder_string_return_without_domain_type() {
        let src = "\
skill current()
    description: \"Return the current branch.\"
    flow:
        return \"<current_branch>\"
";
        let result = fmt_source(src, false);
        assert_eq!(result.output, src);
        assert!(!result.changed);
    }

    // --- Codex pass 1 P1: strip restricted to the return-type slot ---
    // The strip helper must NOT corrupt a string-default parameter that
    // happens to contain the substring `-> None`.

    #[test]
    fn strip_preserves_string_default_containing_arrow_none() {
        // `block foo(msg = "a -> None")` has NO trailing return-type
        // annotation; the `-> None` is part of the string default. The
        // strip must leave the line untouched.
        let src = "block foo(msg = \"a -> None\")\n    flow:\n        \"x\"\n";
        let out = strip_legacy_none_return_types(src);
        assert_eq!(
            out, src,
            "string-default containing `-> None` must be preserved untouched, got:\n{}",
            out
        );
    }

    #[test]
    fn strip_preserves_string_default_containing_arrow_none_lowercase() {
        // Lowercase variant inside a string default — same protection
        // (the strip is case-insensitive on the type ident, so the
        // pre-fix bug would corrupt this too).
        let src = "skill foo(default = \"x -> none y\")\n    flow:\n        \"x\"\n";
        let out = strip_legacy_none_return_types(src);
        assert_eq!(
            out, src,
            "string-default containing `-> none` must be preserved untouched, got:\n{}",
            out
        );
    }

    #[test]
    fn strip_trailing_none_with_string_default_preserved() {
        // Both conditions in one line: a string default that does NOT
        // contain `-> None` PLUS a real trailing `-> None` annotation.
        // The trailing annotation must be stripped; the parameter list
        // (including its string default) must survive intact.
        let src = "block bar(p = \"ignore\") -> None\n    flow:\n        \"x\"\n";
        let out = strip_legacy_none_return_types(src);
        assert_eq!(
            out, "block bar(p = \"ignore\")\n    flow:\n        \"x\"\n",
            "trailing `-> None` must be stripped while `(p = \"ignore\")` is preserved, got:\n{}",
            out
        );
    }

    #[test]
    fn fmt_collapse_two_selective_imports_drops_exact_duplicate() {
        let src = r#"import "@glyph/std" { send }
import "@glyph/std" { send }

skill main()
    description: "Main."
    flow:
        send("hi")
"#;
        let result = fmt_source(src, true);
        let expected = r#"import "@glyph/std" { send }

skill main()
    description: "Main."
    effects: spawns_agent
    flow:
        send("hi")
"#;
        assert_eq!(result.output, expected);
        assert!(result.changed);
    }

    #[test]
    fn fmt_collapse_two_selective_imports_unions_selectors() {
        let src = r#"import "@glyph/std" { send }
import "@glyph/std" { subagent }

skill main()
    description: "Main."
    flow:
        send("hi")
        subagent("x")
"#;
        let result = fmt_source(src, true);
        assert!(result.output.contains(r#"import "@glyph/std" { send, subagent }"#));
        assert_eq!(result.output.matches(r#"import "@glyph/std""#).count(), 1);
        assert!(result.changed);
    }

    #[test]
    fn fmt_collapse_imports_no_op_when_paths_differ() {
        // Both imports are used, so unused-removal won't touch them.
        // Collapse only fires when two imports share the same path.
        let src = r#"import "./a.glyph.md" { foo }
import "./b.glyph.md" { bar }

skill main()
    description: "Main."
    flow:
        foo()
        bar()
"#;
        let result = fmt_source(src, true);
        assert_eq!(result.output, src);
        assert!(!result.changed);
    }

    #[test]
    fn fmt_collapse_imports_idempotent() {
        let src = r#"import "@glyph/std" { send }
import "@glyph/std" { subagent }

skill main()
    description: "Main."
    flow:
        send("x")
        subagent("y")
"#;
        let once = fmt_source(src, true).output;
        let twice = fmt_source(&once, true).output;
        assert_eq!(once, twice, "fmt should be idempotent");
    }

    #[test]
    fn fmt_collapse_two_whole_module_imports_same_path() {
        // Reference `Std` in the flow so unused-removal keeps the collapsed import.
        let src = r#"import "@glyph/std" as Std
import "@glyph/std" as Std

skill main()
    description: "Main."
    flow:
        Std("x")
"#;
        let result = fmt_source(src, true);
        assert_eq!(result.output.matches(r#"import "@glyph/std""#).count(), 1);
        assert!(result.output.contains(r#"import "@glyph/std" as Std"#));
        assert!(result.changed);
    }

    #[test]
    fn fmt_collapse_whole_module_supersedes_selective() {
        // Reference `Std` so unused-removal keeps the collapsed whole-module import.
        let src = r#"import "@glyph/std" { send }
import "@glyph/std" as Std

skill main()
    description: "Main."
    flow:
        Std("hi")
"#;
        let result = fmt_source(src, true);
        // Whole-module form wins; selective form is replaced.
        assert_eq!(result.output.matches(r#"import "@glyph/std""#).count(), 1);
        assert!(result.output.contains(r#"import "@glyph/std" as Std"#));
        assert!(!result.output.contains(r#"import "@glyph/std" { send }"#));
        assert!(result.changed);
    }

    #[test]
    fn fmt_remove_unused_selective_name_keeps_used_one() {
        let src = r#"import "@glyph/std" { send, subagent }

skill main()
    description: "Main."
    flow:
        send("hi")
"#;
        let result = fmt_source(src, true);
        assert!(result.output.contains(r#"import "@glyph/std" { send }"#),
            "expected only `send` to remain, got: {}", result.output);
        assert!(!result.output.contains("subagent"));
        assert!(result.changed);
    }

    #[test]
    fn fmt_remove_unused_drops_entire_line_when_all_unused() {
        let src = r#"import "@glyph/std" { send, subagent }

skill main()
    description: "Main."
    flow:
        return "<done>"
"#;
        let result = fmt_source(src, true);
        assert!(!result.output.contains("import"),
            "expected import line dropped, got: {}", result.output);
        assert!(result.changed);
    }

    #[test]
    fn fmt_remove_unused_no_op_when_all_used() {
        let src = r#"import "@glyph/std" { send, subagent }

skill main()
    description: "Main."
    flow:
        send("x")
        subagent("y")
"#;
        let result = fmt_source(src, true);
        // Both names are used (no import change), but effects are auto-inserted.
        let expected = r#"import "@glyph/std" { send, subagent }

skill main()
    description: "Main."
    effects: spawns_agent
    flow:
        send("x")
        subagent("y")
"#;
        assert_eq!(result.output, expected);
        assert!(result.changed);
    }

    #[test]
    fn fmt_remove_unused_idempotent() {
        let src = r#"import "@glyph/std" { send, subagent }

skill main()
    description: "Main."
    flow:
        send("x")
"#;
        let once = fmt_source(src, true).output;
        let twice = fmt_source(&once, true).output;
        assert_eq!(once, twice);
    }

    #[test]
    fn fmt_remove_unused_keeps_aliased_selective_when_alias_used() {
        let src = r#"import "@glyph/std" { send as S, subagent as Sub }

skill main()
    description: "Main."
    flow:
        S("hi")
"#;
        let result = fmt_source(src, true);
        // Aliased name `S` is referenced; raw name `subagent` (alias `Sub`) is not.
        assert!(result.output.contains(r#"import "@glyph/std" { send as S }"#),
            "expected only `send as S` to remain, got: {}", result.output);
        assert!(!result.output.contains("Sub"));
        assert!(!result.output.contains("subagent"));
        assert!(result.changed);
    }

    // --- Task 3: stdlib auto-import ---

    #[test]
    fn fmt_auto_import_stdlib_inserts_new_import_when_absent() {
        let src = r#"skill main()
    description: "Main."
    flow:
        send("hi")
"#;
        let result = fmt_source(src, true);
        assert!(result.output.starts_with(r#"import "@glyph/std" { send }"#),
            "expected stdlib import inserted at top, got: {}", result.output);
        assert!(result.changed);
    }

    #[test]
    fn fmt_auto_import_stdlib_appends_to_existing() {
        let src = r#"import "@glyph/std" { send }

skill main()
    description: "Main."
    flow:
        send("x")
        subagent("y")
"#;
        let result = fmt_source(src, true);
        assert!(result.output.contains(r#"import "@glyph/std" { send, subagent }"#),
            "expected subagent appended, got: {}", result.output);
        assert!(result.changed);
    }

    #[test]
    fn fmt_auto_import_no_op_when_user_shadowed() {
        let src = r#"const subagent = "user-defined"

skill main()
    description: "Main."
    flow:
        send_value(subagent)
"#;
        let result = fmt_source(src, true);
        assert!(!result.output.contains("@glyph/std"),
            "should not auto-import when name is locally bound");
    }

    #[test]
    fn fmt_auto_import_no_op_when_name_not_in_registry() {
        let src = r#"skill main()
    description: "Main."
    flow:
        zorp("bogus")
"#;
        let result = fmt_source(src, true);
        assert!(!result.output.contains("@glyph/std"));
        assert!(!result.changed);
    }

    #[test]
    fn fmt_auto_import_idempotent() {
        let src = r#"skill main()
    description: "Main."
    flow:
        send("x")
"#;
        let once = fmt_source(src, true).output;
        let twice = fmt_source(&once, true).output;
        assert_eq!(once, twice);
    }

    #[test]
    fn fmt_auto_import_appends_preserves_existing_order() {
        let src = r#"import "@glyph/std" { subagent }

skill main()
    description: "Main."
    flow:
        send("hi")
        subagent("x")
"#;
        let result = fmt_source(src, true);
        // User authored `subagent` first; new `send` must be appended at the end,
        // not alphabetically reordered before `subagent`.
        assert!(result.output.contains(r#"import "@glyph/std" { subagent, send }"#),
            "expected appended order, got: {}", result.output);
        assert!(!result.output.contains(r#"{ send, subagent }"#),
            "must not reorder existing names alphabetically");
        assert!(result.changed);
    }

    #[test]
    fn fmt_auto_import_load_stdlib_name() {
        let src = r#"skill main()
    description: "Main."
    flow:
        load("config.txt")
"#;
        let result = fmt_source(src, true);
        assert!(result.output.contains(r#"import "@glyph/std" { load }"#),
            "expected `load` auto-imported, got: {}", result.output);
        assert!(result.changed);
    }

    // --- Task 4: #111 Const-in-flow parens-add ---

    #[test]
    fn fmt_const_in_flow_adds_parens_to_unresolved_bare_name() {
        let src = r#"skill main()
    description: "Main."
    flow:
        helper
"#;
        let result = fmt_source(src, true);
        assert!(result.output.contains("helper()"),
            "expected `helper` rewritten to `helper()`, got: {}", result.output);
        assert!(result.changed);
    }

    #[test]
    fn fmt_const_in_flow_no_op_when_resolves_to_local_const() {
        let src = r#"const helper = "x"

skill main()
    description: "Main."
    flow:
        helper
"#;
        let result = fmt_source(src, true);
        assert!(!result.output.contains("helper()"));
    }

    #[test]
    fn fmt_const_in_flow_no_op_when_resolves_to_local_block() {
        let src = r#"block helper() -> Report
    description: "Helper."
    flow:
        return "<x>"

skill main()
    description: "Main."
    flow:
        helper
"#;
        let result = fmt_source(src, true);
        // The block declaration's HEADER `block helper() -> Report` contains `helper()` but not `helper()\n` directly
        // (it ends with `Report\n`). The flow-body line `        helper` is what we're checking is NOT rewritten.
        assert!(!result.output.contains("helper()\n"),
            "should not auto-paren when name resolves locally");
    }

    #[test]
    fn fmt_const_in_flow_idempotent() {
        let src = r#"skill main()
    description: "Main."
    flow:
        helper
"#;
        let once = fmt_source(src, true).output;
        let twice = fmt_source(&once, true).output;
        assert_eq!(once, twice);
    }

    #[test]
    fn fmt_placeholder_return_no_rewrite_when_inner_contains_quote() {
        // Conservative behavior per design spec: descriptive form refuses
        // to rewrite when inner contains `"`, `\`, `\n`, `\t`, `\r`.
        let src = r#"export block report() -> Report
    description: "Report."
    return "<has \"quote\" inside>"
"#;
        let result = fmt_source(src, true);
        // Should leave the line alone — the diagnostic remains, no malformed output.
        assert!(result.output.contains(r#"return "<has \"quote\" inside>""#));
    }

    // --- Task 5: #112 Effects auto-insert ---

    #[test]
    fn fmt_effects_auto_insert_adds_inferred_effects() {
        let src = r#"import "@glyph/std" { send }

skill main()
    description: "Main."
    flow:
        send("hi")
"#;
        let result = fmt_source(src, true);
        assert!(result.output.contains("effects: spawns_agent"),
            "expected inferred effects inserted, got: {}", result.output);
        assert!(result.changed);
    }

    #[test]
    fn fmt_effects_auto_insert_no_op_when_user_declared() {
        let src = r#"import "@glyph/std" { send }

skill main()
    description: "Main."
    effects: none
    flow:
        send("hi")
"#;
        let result = fmt_source(src, true);
        assert!(result.output.contains("effects: none"));
        assert!(!result.output.contains("effects: spawns_agent"));
    }

    #[test]
    fn fmt_effects_auto_insert_no_op_when_inferred_empty() {
        let src = r#"skill main()
    description: "Main."
    flow:
        return "<done>"
"#;
        let result = fmt_source(src, true);
        assert!(!result.output.contains("effects:"));
    }

    #[test]
    fn fmt_effects_auto_insert_no_op_when_enable_effects_false() {
        let src = r#"import "@glyph/std" { send }

skill main()
    description: "Main."
    flow:
        send("hi")
"#;
        let result = fmt_source(src, false);
        assert!(!result.output.contains("effects:"));
    }

    #[test]
    fn fmt_effects_auto_insert_idempotent() {
        let src = r#"import "@glyph/std" { send }

skill main()
    description: "Main."
    flow:
        send("hi")
"#;
        let once = fmt_source(src, true).output;
        let twice = fmt_source(&once, true).output;
        assert_eq!(once, twice);
    }

    // ---------------------------------------------------------------------
    // Issue #109 chunk 4 — fmt merges duplicate sub-sections.
    //
    // After Chunks 1-3, the parser recovers a duplicate sub-section into
    // `extra_subsections` (emitting `G::parse::duplicate-subsection`,
    // Repairable) and Analyze gates Lower with
    // `G::analyze::unmerged-duplicate-subsection` (Error). `glyph fmt` is
    // the merger: when it encounters duplicate `<kind>:` headers under one
    // declaration, it keeps the first header verbatim, appends the bodies
    // of subsequent occurrences in source order, and removes the second-
    // and-beyond header lines (preserving any trailing comments on those
    // headers per `design/repair.md` §4.11 rule b). After fmt runs and the
    // output is re-parsed, `extra_subsections` is empty and the parse-tier
    // diagnostic does not refire — the recovery loop converges.
    // ---------------------------------------------------------------------

    /// Test 1 — two `constraints:` sub-sections under one `skill` merge into
    /// a single `constraints:` whose body carries both originals' markers in
    /// source order. `changed == true`.
    #[test]
    fn fmt_merges_two_constraints_sections_in_skill() {
        let src = "\
skill the_skill()
    constraints:
        require accuracy
    constraints:
        avoid stale_references
    flow:
        \"do work\"
";
        let result = fmt_source(src, false);
        assert!(result.changed, "fmt should report changed=true");

        // Exactly one `constraints:` header line in the output.
        let header_lines = result
            .output
            .lines()
            .filter(|l| l.trim() == "constraints:")
            .count();
        assert_eq!(
            header_lines, 1,
            "expected exactly one `constraints:` header in output, got:\n{}",
            result.output
        );

        // Both markers present, and the first body's marker comes before the
        // second's (source-order preservation).
        let req_idx = result.output.find("require accuracy").unwrap_or_else(|| {
            panic!("first body's marker missing from output:\n{}", result.output)
        });
        let avd_idx = result
            .output
            .find("avoid stale_references")
            .unwrap_or_else(|| {
                panic!("second body's marker missing from output:\n{}", result.output)
            });
        assert!(
            req_idx < avd_idx,
            "first body's marker must precede second body's marker in source order; got:\n{}",
            result.output
        );
    }

    /// Test 2a — two `description:` sub-sections merge.
    #[test]
    fn fmt_merges_two_descriptions_in_skill() {
        let src = "\
skill the_skill()
    description: \"First.\"
    description: \"Second.\"
    flow:
        \"do work\"
";
        let result = fmt_source(src, false);
        assert!(result.changed, "fmt should report changed=true");

        let header_lines = result
            .output
            .lines()
            .filter(|l| l.trim_start().starts_with("description:"))
            .count();
        assert_eq!(
            header_lines, 1,
            "expected exactly one `description:` header in output, got:\n{}",
            result.output
        );

        let first_idx = result.output.find("First.").expect("first body lost");
        let second_idx = result.output.find("Second.").expect("second body lost");
        assert!(
            first_idx < second_idx,
            "source order must be preserved; got:\n{}",
            result.output
        );
    }

    /// Test 2b — two `context:` sub-sections merge.
    #[test]
    fn fmt_merges_two_contexts_in_skill() {
        let src = "\
skill the_skill()
    context:
        \"first context entry\"
    context:
        \"second context entry\"
    flow:
        \"do work\"
";
        let result = fmt_source(src, false);
        assert!(result.changed, "fmt should report changed=true");

        // After merge there is exactly one `context:` header (counted as a
        // line whose trimmed content equals `context:` — body-level
        // `context <name>` markers don't match this).
        let header_lines = result
            .output
            .lines()
            .filter(|l| l.trim() == "context:")
            .count();
        assert_eq!(
            header_lines, 1,
            "expected exactly one `context:` header, got:\n{}",
            result.output
        );

        let first_idx = result
            .output
            .find("first context entry")
            .expect("first body lost");
        let second_idx = result
            .output
            .find("second context entry")
            .expect("second body lost");
        assert!(
            first_idx < second_idx,
            "source order must be preserved; got:\n{}",
            result.output
        );
    }

    /// Test 2c — two `flow:` sub-sections merge.
    #[test]
    fn fmt_merges_two_flows_in_skill() {
        let src = "\
skill the_skill()
    flow:
        \"step from first flow\"
    flow:
        \"step from second flow\"
";
        let result = fmt_source(src, false);
        assert!(result.changed, "fmt should report changed=true");

        let header_lines = result
            .output
            .lines()
            .filter(|l| l.trim() == "flow:")
            .count();
        assert_eq!(
            header_lines, 1,
            "expected exactly one `flow:` header, got:\n{}",
            result.output
        );

        let first_idx = result
            .output
            .find("step from first flow")
            .expect("first body lost");
        let second_idx = result
            .output
            .find("step from second flow")
            .expect("second body lost");
        assert!(
            first_idx < second_idx,
            "source order must be preserved; got:\n{}",
            result.output
        );
    }

    /// Test 5 — idempotence: running `fmt_source` twice yields the same
    /// output as running it once, and the second run reports `changed=false`.
    /// This is the classic fixpoint property — fmt's job is to drive the
    /// agent-repair loop to convergence; a non-idempotent merge would loop.
    #[test]
    fn fmt_merge_is_idempotent() {
        let src = "\
skill the_skill()
    constraints:
        require accuracy
    constraints:
        avoid stale_references
    flow:
        \"do work\"
";
        let first = fmt_source(src, false);
        assert!(first.changed, "first fmt run must report changed=true");
        let second = fmt_source(&first.output, false);
        assert!(
            !second.changed,
            "second fmt run must report changed=false (idempotence)"
        );
        assert_eq!(
            second.output, first.output,
            "second-run output must equal first-run output byte-for-byte"
        );
    }

    /// Test 6 — rule (a): a whole-line comment inside the body of a
    /// duplicate sub-section survives the merge, in its relative position
    /// inside the appended body. Body lines are verbatim — only the
    /// header line of the duplicate is dropped.
    #[test]
    fn fmt_preserves_comment_inside_duplicate_body() {
        let src = "\
skill the_skill()
    constraints:
        require accuracy
    constraints:
        // author note: tightening below
        avoid stale_references
    flow:
        \"do work\"
";
        let result = fmt_source(src, false);
        assert!(result.changed, "fmt should report changed=true");

        let header_lines = result
            .output
            .lines()
            .filter(|l| l.trim() == "constraints:")
            .count();
        assert_eq!(
            header_lines, 1,
            "expected exactly one `constraints:` header, got:\n{}",
            result.output
        );

        let comment_idx = result
            .output
            .find("// author note: tightening below")
            .expect("body comment lost");
        let avoid_idx = result
            .output
            .find("avoid stale_references")
            .expect("second body lost");
        let require_idx = result
            .output
            .find("require accuracy")
            .expect("first body lost");
        assert!(
            require_idx < comment_idx && comment_idx < avoid_idx,
            "comment must remain immediately above its original successor (between bodies):\n{}",
            result.output
        );
    }

    /// Test 7 — rule (b): a trailing `//` comment on the second
    /// `<kind>:` header (which gets dropped by the merge) is preserved as
    /// a whole-line comment at the boundary, indented to match the dropped
    /// header. The rule says: "no source-author comment vanishes."
    #[test]
    fn fmt_preserves_trailing_comment_on_removed_header() {
        let src = "\
skill the_skill()
    constraints:
        require accuracy
    constraints:  // extras for second pass
        avoid stale_references
    flow:
        \"do work\"
";
        let result = fmt_source(src, false);
        assert!(result.changed, "fmt should report changed=true");

        let header_lines = result
            .output
            .lines()
            .filter(|l| l.trim() == "constraints:")
            .count();
        assert_eq!(
            header_lines, 1,
            "expected exactly one `constraints:` header (the trailing comment must be lifted off the dropped header):\n{}",
            result.output
        );

        let comment_idx = result
            .output
            .find("// extras for second pass")
            .expect("trailing comment from removed header lost");
        let require_idx = result
            .output
            .find("require accuracy")
            .expect("first body lost");
        let avoid_idx = result
            .output
            .find("avoid stale_references")
            .expect("second body lost");
        assert!(
            require_idx < comment_idx && comment_idx < avoid_idx,
            "trailing comment from removed header must land between the two bodies:\n{}",
            result.output
        );
    }

    /// Test 8 — rule (c): a whole-line comment that sits between the end
    /// of the first body and the second `<kind>:` header is captured into
    /// the first section's accumulator (because the line-grouper appends
    /// it to whatever section is currently open). After the merge it
    /// emerges in source-order — i.e. between the two original bodies.
    #[test]
    fn fmt_preserves_comment_between_bodies() {
        let src = "\
skill the_skill()
    constraints:
        require accuracy
        // boundary note: more below
    constraints:
        avoid stale_references
    flow:
        \"do work\"
";
        let result = fmt_source(src, false);
        assert!(result.changed, "fmt should report changed=true");

        let header_lines = result
            .output
            .lines()
            .filter(|l| l.trim() == "constraints:")
            .count();
        assert_eq!(
            header_lines, 1,
            "expected exactly one `constraints:` header, got:\n{}",
            result.output
        );

        let comment_idx = result
            .output
            .find("// boundary note: more below")
            .expect("between-bodies comment lost");
        let require_idx = result
            .output
            .find("require accuracy")
            .expect("first body lost");
        let avoid_idx = result
            .output
            .find("avoid stale_references")
            .expect("second body lost");
        assert!(
            require_idx < comment_idx && comment_idx < avoid_idx,
            "between-bodies comment must remain between the two bodies in source order:\n{}",
            result.output
        );
    }

    /// Test 9 — multiple sub-section kinds duplicated within one
    /// declaration: `constraints:` AND `flow:` each appear twice in the
    /// same skill. Both pairs must merge independently — the merger gathers
    /// per-kind, so cross-kind interference must not happen.
    #[test]
    fn fmt_merges_multiple_duplicate_kinds_in_one_decl() {
        let src = "\
skill the_skill()
    constraints:
        require accuracy
    constraints:
        avoid stale_references
    flow:
        \"step from first flow\"
    flow:
        \"step from second flow\"
";
        let result = fmt_source(src, false);
        assert!(result.changed, "fmt should report changed=true");

        let constraints_headers = result
            .output
            .lines()
            .filter(|l| l.trim() == "constraints:")
            .count();
        assert_eq!(
            constraints_headers, 1,
            "expected exactly one `constraints:` header, got:\n{}",
            result.output
        );

        let flow_headers = result
            .output
            .lines()
            .filter(|l| l.trim() == "flow:")
            .count();
        assert_eq!(
            flow_headers, 1,
            "expected exactly one `flow:` header, got:\n{}",
            result.output
        );

        // All four bodies present.
        for marker in [
            "require accuracy",
            "avoid stale_references",
            "step from first flow",
            "step from second flow",
        ] {
            assert!(
                result.output.contains(marker),
                "marker {:?} missing from output:\n{}",
                marker,
                result.output
            );
        }
    }

    /// Test 10 — convergence: after fmt, re-parsing the output must
    /// yield zero `extra_subsections` and zero `G::parse::duplicate-subsection`
    /// diagnostics. This is the contract that lets the agent-repair loop
    /// terminate: fmt's output must be a fixed point relative to the
    /// duplicate-subsection diagnostic.
    #[test]
    fn fmt_output_reparses_without_duplicate_subsection_diagnostic() {
        let src = "\
skill the_skill()
    constraints:
        require accuracy
    constraints:
        avoid stale_references
    flow:
        \"do work\"
";
        let result = fmt_source(src, false);
        assert!(result.changed, "fmt should report changed=true");

        // Re-parse the output through the same parser fmt itself uses.
        let mut bag = DiagBag::new();
        let line_index = LineIndex::new(&result.output);
        let reparsed = parse::parse_with_diagnostics_opts(
            &result.output,
            0,
            "<reparse>",
            &line_index,
            &mut bag,
            false,
        )
        .expect("fmt output must re-parse to Some(file)");

        // No duplicate-subsection diagnostic should fire on the merged form.
        let dup_diags: Vec<&crate::diagnostic::Diagnostic> = bag
            .iter()
            .filter(|d| d.id == "G::parse::duplicate-subsection")
            .collect();
        assert!(
            dup_diags.is_empty(),
            "expected zero duplicate-subsection diagnostics on reparse, got {}:\n{}",
            dup_diags.len(),
            result.output
        );

        // Every skill / block / export-block decl must have empty
        // `extra_subsections` (the AST-level signal of unmerged duplicates).
        for decl in &reparsed.decls {
            match decl {
                crate::ast::Decl::Skill(s) => {
                    assert!(
                        s.node.extra_subsections.is_empty(),
                        "skill {:?} still has extra_subsections after fmt:\n{}",
                        s.node.name,
                        result.output
                    );
                }
                crate::ast::Decl::Block(b) => {
                    assert!(
                        b.node.extra_subsections.is_empty(),
                        "block {:?} still has extra_subsections after fmt:\n{}",
                        b.node.name,
                        result.output
                    );
                }
                crate::ast::Decl::ExportBlock(e) => {
                    assert!(
                        e.node.extra_subsections.is_empty(),
                        "export block {:?} still has extra_subsections after fmt:\n{}",
                        e.node.name,
                        result.output
                    );
                }
                _ => {}
            }
        }
    }

    // --- Issue #109 chunk 5: integration smoke ---
    //
    // The unit-level `fmt_output_reparses_without_duplicate_subsection_diagnostic`
    // test (chunk 4) verified the parse-tier contract. These two tests close
    // the loop end-to-end: the same source must surface BOTH the parse-tier
    // (`G::parse::duplicate-subsection`) and the analyze-tier
    // (`G::analyze::unmerged-duplicate-subsection`) diagnostics through the
    // public `check_source` API; after `fmt_source`, the same `check_source`
    // call must surface NEITHER.
    //
    // Together these tests pin the agent-repair-loop contract: a duplicate
    // sub-section is recoverable (parse keeps the AST), the analyzer flags it
    // as a hard error so the user sees something is wrong, and `glyph fmt`
    // is the canonical fixer that drives both diagnostics to zero.

    /// Test A — pre-fmt: a source with a duplicate `constraints:` sub-section
    /// surfaces both the parse-tier and the analyze-tier diagnostics through
    /// the public `check_source` API.
    #[test]
    fn duplicate_subsection_pre_fmt_surfaces_both_tiers() {
        let src = "\
skill the_skill()
    constraints:
        require accuracy
    constraints:
        avoid stale_references
    flow:
        \"do work\"
";
        let bag = crate::check_source(src, 0, "<test>");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::parse::duplicate-subsection"),
            "expected `G::parse::duplicate-subsection` in pre-fmt bag, got: {:?}",
            ids
        );
        assert!(
            ids.contains(&"G::analyze::unmerged-duplicate-subsection"),
            "expected `G::analyze::unmerged-duplicate-subsection` in pre-fmt bag, got: {:?}",
            ids
        );
    }

    /// Test B — post-fmt: running `fmt_source` and re-checking yields neither
    /// diagnostic. Other diagnostics are tolerated (we don't pin bag-empty
    /// here; just that the two duplicate-subsection IDs are absent).
    /// Also pins that the fmt run actually did work (`changed == true`) — a
    /// silent no-op would let this test pass spuriously.
    #[test]
    fn duplicate_subsection_post_fmt_clears_both_tiers() {
        let src = "\
skill the_skill()
    constraints:
        require accuracy
    constraints:
        avoid stale_references
    flow:
        \"do work\"
";
        let result = fmt_source(src, false);
        assert!(
            result.changed,
            "fmt must report changed=true on a duplicate-subsection input"
        );
        let bag = crate::check_source(&result.output, 0, "<test>");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::parse::duplicate-subsection"),
            "post-fmt bag must not contain `G::parse::duplicate-subsection`; got: {:?}\noutput:\n{}",
            ids,
            result.output
        );
        assert!(
            !ids.contains(&"G::analyze::unmerged-duplicate-subsection"),
            "post-fmt bag must not contain `G::analyze::unmerged-duplicate-subsection`; got: {:?}\noutput:\n{}",
            ids,
            result.output
        );
    }

    // --- Issue #109 codex-pass-1 fixes ---

    /// Codex finding 2 — single `description:` with escaped chars must NOT
    /// be re-emitted (the merge helper double-escapes existing `\"` and `\\`
    /// because `unwrap_string_literal` strips quotes but not escapes). The
    /// fix is to early-return verbatim when there's only one section to emit.
    /// Property: `fmt_source` is a no-op (`changed=false`, byte-equal output)
    /// on a single description containing escaped characters.
    #[test]
    fn fmt_does_not_double_escape_single_description_with_escapes() {
        let src = "\
skill the_skill()
    description: \"He said \\\"hi\\\"\"
    flow:
        \"do work\"
";
        let result = fmt_source(src, false);
        assert!(
            !result.changed,
            "fmt should be a no-op on a single description with escapes; got changed=true with output:\n{}",
            result.output
        );
        assert_eq!(
            result.output, src,
            "single description with escapes must round-trip byte-for-byte; got:\n{}",
            result.output
        );
    }

    /// Codex finding 3a — single `description:` with a trailing `// note`
    /// comment must preserve the comment. Pre-fix, the merge helper only
    /// lifted trailing comments off DUPLICATE headers (idx > 0), so on a
    /// single section the comment was silently dropped. Fixed by the same
    /// early-return-verbatim that fixes Finding 2 (the original line carries
    /// the comment).
    #[test]
    fn fmt_preserves_trailing_comment_on_single_description() {
        let src = "\
skill the_skill()
    description: \"x\"  // important note
    flow:
        \"do work\"
";
        let result = fmt_source(src, false);
        assert!(
            !result.changed,
            "fmt should be a no-op on a single description with a trailing comment; got changed=true with output:\n{}",
            result.output
        );
        assert!(
            result.output.contains("// important note"),
            "trailing comment dropped from single description; output:\n{}",
            result.output
        );
    }

    /// Codex finding 3b — single `effects:` with a trailing `// note`
    /// comment must preserve the comment, same as 3a but for the effects
    /// kind. Pre-fix, `emit_merged_effects` rebuilt the line from
    /// `effects_acc.join(", ")` and dropped the trailing comment.
    #[test]
    fn fmt_preserves_trailing_comment_on_single_effects() {
        let src = "\
skill the_skill()
    effects: reads_files  // important note
    flow:
        \"do work\"
";
        let result = fmt_source(src, true);
        assert!(
            !result.changed,
            "fmt should be a no-op on a single effects line with a trailing comment; got changed=true with output:\n{}",
            result.output
        );
        assert!(
            result.output.contains("// important note"),
            "trailing comment dropped from single effects line; output:\n{}",
            result.output
        );
    }

    /// Codex finding 1 — anchor `context:` is short-form (`context: "x"`)
    /// and the duplicate is long-form (`context:` + indented body lines).
    /// Pre-fix, the merge emitted the anchor verbatim then appended the
    /// duplicate's body lines underneath, producing:
    ///     context: "first"
    ///         some entry
    ///         another entry
    /// — which the parser rejects (short-form is exclusive). Fix: when the
    /// anchor is short-form and any subsequent duplicate exists, normalize
    /// the anchor to long-form (bare `context:` header + inline content as
    /// indent-2 body line) before appending duplicate bodies.
    ///
    /// The acceptance shape is "output re-parses cleanly through the public
    /// `check_source` API with no `G::parse::*` errors on the merged kind"
    /// — so we re-parse the fmt output and assert no parse failure on
    /// `context:`.
    #[test]
    fn fmt_normalizes_anchor_short_context_with_long_duplicate() {
        let src = "\
skill the_skill()
    context: \"first inline\"
    context:
        \"long form entry\"
    flow:
        \"do work\"
";
        let result = fmt_source(src, false);
        assert!(result.changed, "fmt should report changed=true");

        // Re-parse the output: it must be valid Glyph (the duplicate-
        // subsection diagnostic is fine; the output must NOT trigger any
        // structural parse errors on the merged `context:` block).
        let mut bag = DiagBag::new();
        let line_index = LineIndex::new(&result.output);
        let reparsed = parse::parse_with_diagnostics_opts(
            &result.output,
            0,
            "<reparse>",
            &line_index,
            &mut bag,
            false,
        );
        assert!(
            reparsed.is_some(),
            "fmt output failed to re-parse:\n{}\nbag:\n{:?}",
            result.output,
            bag.iter().map(|d| (&d.id, &d.message)).collect::<Vec<_>>()
        );
        // No duplicate-subsection diagnostic on the merged form (the merge
        // succeeded — i.e., the AST does not contain unmerged extras).
        let dup_count = bag
            .iter()
            .filter(|d| d.id == "G::parse::duplicate-subsection")
            .count();
        assert_eq!(
            dup_count, 0,
            "merged output still has duplicate-subsection diagnostic; output:\n{}",
            result.output
        );
        // The merged `context:` must be in valid long-form: a bare `context:`
        // header at indent 1 followed by indented body lines that include
        // both the lifted anchor's inline string and the duplicate's body.
        assert!(
            result.output.contains("first inline"),
            "anchor's inline content lost in merge:\n{}",
            result.output
        );
        assert!(
            result.output.contains("long form entry"),
            "duplicate's body content lost in merge:\n{}",
            result.output
        );
        // There must be exactly one `context:` header (possibly with inline
        // content stripped — the canonical form is bare `context:`).
        let context_headers = result
            .output
            .lines()
            .filter(|l| l.trim() == "context:")
            .count();
        assert_eq!(
            context_headers, 1,
            "expected exactly one bare `context:` header after merge; output:\n{}",
            result.output
        );
    }

    // --- Issue #109 codex-pass-2 findings 6 & 7 ---

    /// Codex finding 6 — multi-section `description:` merge must NOT
    /// double-escape `\"` and `\\`. Pre-fix, `unwrap_string_literal` strips
    /// quotes but leaves escape sequences as raw backslash + char; then
    /// `escape_string_literal` re-escapes the backslashes, yielding e.g.
    /// `\\\"` from `\"`. The fix is to mirror the tokenizer's escape
    /// handling (`\"` → `"`, `\\` → `\`, `\n` → newline, `\t` → tab) when
    /// extracting the inner payload, so the merge round-trip is lossless.
    ///
    /// Acceptance pins the semantic round-trip: re-parse the merged output
    /// and assert the AST `description` value equals the concatenation of
    /// the two original (already-decoded) bodies. This catches the actual
    /// mangling — checking source bytes alone would miss it because the
    /// post-merge source still parses, just to a wrong value.
    #[test]
    fn fmt_merges_two_descriptions_with_escapes_without_double_escape() {
        let src = "\
skill the_skill()
    description: \"He said \\\"hi\\\"\"
    description: \"and \\\\done\"
    flow:
        \"do work\"
";
        let result = fmt_source(src, false);
        assert!(result.changed, "fmt should merge duplicates");

        // Re-parse the merged output and pull the description AST value.
        let mut bag = DiagBag::new();
        let line_index = LineIndex::new(&result.output);
        let reparsed = parse::parse_with_diagnostics_opts(
            &result.output,
            0,
            "<reparse>",
            &line_index,
            &mut bag,
            false,
        )
        .unwrap_or_else(|| {
            panic!(
                "fmt output failed to re-parse:\n{}\nbag:\n{:?}",
                result.output,
                bag.iter().map(|d| (&d.id, &d.message)).collect::<Vec<_>>()
            )
        });
        let dup_count = bag
            .iter()
            .filter(|d| d.id == "G::parse::duplicate-subsection")
            .count();
        assert_eq!(dup_count, 0);

        // Decoded bodies (what the parser already produced for the two
        // duplicates): `He said "hi"` and `and \done`. After merging they
        // collapse into `He said "hi"\nand \done` — joined by a `\n`
        // separator (codex pass-3 finding 10; design silent on inline-form
        // join, default per planner is `\n`).
        let merged_value = reparsed
            .decls
            .iter()
            .find_map(|d| match d {
                crate::ast::Decl::Skill(s) => s.node.description.clone(),
                _ => None,
            })
            .expect("merged skill must have a description");
        assert_eq!(
            merged_value,
            "He said \"hi\"\nand \\done",
            "description merge double-escaped or otherwise mangled content; got `{}`",
            merged_value
        );

        // Idempotence: a second pass is a no-op.
        let second = fmt_source(&result.output, false);
        assert!(
            !second.changed,
            "fmt is not idempotent on description merge with escapes"
        );
        assert_eq!(second.output, result.output);
    }

    /// Codex pass-3 finding 10 — inline-form `description:` merge separator.
    ///
    /// `design/repair.md` §4.11.4 specifies a "single blank line between
    /// bodies" for the multi-line bare form, and is silent on the
    /// inline-string form. Per the planner default, the inline-string
    /// merge concatenates two bodies with a single `\n` (LF) embedded
    /// inside the merged literal — so the merged source looks like
    /// `description: "first.\nsecond."`. The semantic value (after
    /// re-parsing the merged source) is `"first.\nsecond."`.
    #[test]
    fn fmt_merges_two_inline_descriptions_with_newline_separator() {
        let src = "\
skill the_skill()
    description: \"first.\"
    description: \"second.\"
    flow:
        \"do work\"
";
        let result = fmt_source(src, false);
        assert!(result.changed, "fmt should merge duplicates");

        // Re-parse the merged output and inspect the AST description value.
        let mut bag = DiagBag::new();
        let line_index = LineIndex::new(&result.output);
        let reparsed = parse::parse_with_diagnostics_opts(
            &result.output,
            0,
            "<reparse>",
            &line_index,
            &mut bag,
            false,
        )
        .unwrap_or_else(|| {
            panic!(
                "fmt output failed to re-parse:\n{}\nbag:\n{:?}",
                result.output,
                bag.iter().map(|d| (&d.id, &d.message)).collect::<Vec<_>>()
            )
        });
        let dup_count = bag
            .iter()
            .filter(|d| d.id == "G::parse::duplicate-subsection")
            .count();
        assert_eq!(dup_count, 0, "merge must collapse to a single description:");

        let merged_value = reparsed
            .decls
            .iter()
            .find_map(|d| match d {
                crate::ast::Decl::Skill(s) => s.node.description.clone(),
                _ => None,
            })
            .expect("merged skill must have a description");
        assert_eq!(
            merged_value, "first.\nsecond.",
            "inline-form merge must use a `\\n` separator (single newline embedded inside the merged literal); got `{:?}`",
            merged_value
        );

        // Idempotence: a second pass is a no-op.
        let second = fmt_source(&result.output, false);
        assert!(
            !second.changed,
            "fmt is not idempotent on inline-form description merge"
        );
        assert_eq!(second.output, result.output);
    }

    /// Codex finding 7 — when a duplicate inline (single-line) section is
    /// merged, any whole-line `// comment` that appears between the anchor
    /// and the duplicate header (or between the dup and following content)
    /// must be preserved at the boundary. Pre-fix, the merge helpers only
    /// captured trailing comments on the duplicate header line itself
    /// (`trailing_comment_after_keyword`); whole-line comment-only lines
    /// inside `section.lines[1..]` were silently dropped because the helper
    /// rebuilds the canonical line and never re-emits the comment lines.
    /// Acceptance: the comment line appears verbatim in the output near
    /// the merged section.
    #[test]
    fn fmt_preserves_boundary_comment_between_two_descriptions() {
        let src = "\
skill the_skill()
    description: \"first.\"
    // boundary note
    description: \"second.\"
    flow:
        \"do work\"
";
        let result = fmt_source(src, false);
        assert!(result.changed, "fmt should merge duplicates");
        assert!(
            result.output.contains("// boundary note"),
            "boundary `//` comment dropped during description merge; output:\n{}",
            result.output
        );
    }

    /// Codex finding 7 — same shape but for `effects:`. A whole-line
    /// `// comment` between two duplicate `effects:` headers must be
    /// preserved.
    #[test]
    fn fmt_preserves_boundary_comment_between_two_effects() {
        let src = "\
skill the_skill()
    effects: reads_files
    // boundary note
    effects: writes_files
    flow:
        \"do work\"
";
        let result = fmt_source(src, true);
        assert!(result.changed, "fmt should merge duplicates");
        assert!(
            result.output.contains("// boundary note"),
            "boundary `//` comment dropped during effects merge; output:\n{}",
            result.output
        );
    }

    /// Test 4 — no-op: a source with no duplicate sub-sections passes
    /// through unchanged. `changed == false`. The fmt's other rewrites
    /// (canonical reorder, hoisting) must not be triggered by this input.
    #[test]
    fn fmt_no_op_when_no_duplicates() {
        let src = "\
skill the_skill()
    description: \"A skill that does work.\"
    context:
        \"some context\"
    constraints:
        require accuracy
    flow:
        \"do work\"
";
        let result = fmt_source(src, false);
        assert!(
            !result.changed,
            "fmt should report changed=false on input without duplicates; got changed=true with output:\n{}",
            result.output
        );
        assert_eq!(
            result.output, src,
            "output should equal input byte-for-byte"
        );
    }

    /// Test 3 — triple `constraints:` sub-sections all merge into one.
    /// Pins source-order across more than two duplicates: the bodies must
    /// appear in their original 1→2→3 order.
    #[test]
    fn fmt_merges_three_constraints_sections_in_skill() {
        let src = "\
skill the_skill()
    constraints:
        require accuracy
    constraints:
        avoid stale_references
    constraints:
        must verify
    flow:
        \"do work\"
";
        let result = fmt_source(src, false);
        assert!(result.changed, "fmt should report changed=true");

        let header_lines = result
            .output
            .lines()
            .filter(|l| l.trim() == "constraints:")
            .count();
        assert_eq!(
            header_lines, 1,
            "expected exactly one `constraints:` header in output, got:\n{}",
            result.output
        );

        let one = result.output.find("require accuracy").expect("first lost");
        let two = result
            .output
            .find("avoid stale_references")
            .expect("second lost");
        let three = result.output.find("must verify").expect("third lost");
        assert!(
            one < two && two < three,
            "all three bodies must appear in source order; got:\n{}",
            result.output
        );
    }

    /// Test 2d — two `effects:` sub-sections merge.
    #[test]
    fn fmt_merges_two_effects_in_skill() {
        let src = "\
skill the_skill()
    effects: writes_files
    effects: reads_files
    flow:
        \"do work\"
";
        // Effects are gated by the parser flag — pass `true` so the source
        // parses and ast_rewrite gets a chance to merge.
        let result = fmt_source(src, true);
        assert!(result.changed, "fmt should report changed=true");

        let header_lines = result
            .output
            .lines()
            .filter(|l| l.trim_start().starts_with("effects:"))
            .count();
        assert_eq!(
            header_lines, 1,
            "expected exactly one `effects:` header, got:\n{}",
            result.output
        );

        let first_idx = result
            .output
            .find("writes_files")
            .expect("first body lost");
        let second_idx = result
            .output
            .find("reads_files")
            .expect("second body lost");
        assert!(
            first_idx < second_idx,
            "source order must be preserved; got:\n{}",
            result.output
        );
    }
}
