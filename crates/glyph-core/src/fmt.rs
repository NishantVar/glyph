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

    // Collect the 0-based line indices of top-level import lines (indent 0).
    let mut import_line_idx: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !line.starts_with(' ') && !line.starts_with('\t') && line.trim().starts_with("import ") {
            import_line_idx.push(i);
        }
    }

    // Build groups keyed by import path, preserving first-seen order.
    let mut groups: HashMap<String, Group> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    let mut import_seq = 0usize;
    for decl in &file.decls {
        if let Decl::Import(imp) = decl {
            if import_seq >= import_line_idx.len() {
                break;
            }
            let line_idx = import_line_idx[import_seq];
            import_seq += 1;

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
            format!(r#"import "{}" as {}"#, path, g.whole_module_alias.as_deref().unwrap_or(""))
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

/// Stratum 2: AST-level rewrites (dispatcher).
///
/// First runs `collapse_duplicate_imports`. If any imports were collapsed the
/// source is re-parsed and signals are recomputed before forwarding to
/// `ast_rewrite_inner`, because the earlier AST is now stale.
fn ast_rewrite(
    source: &str,
    file: &crate::ast::SourceFile,
    signals: &crate::analyze::FmtSignals,
    enable_effects: bool,
) -> String {
    let collapsed = collapse_duplicate_imports(source, file);
    if collapsed != source {
        let line_index = crate::span::LineIndex::new(&collapsed);
        let mut bag = crate::diagnostic::DiagBag::new();
        if let Some(reparsed) = crate::parse::parse_with_diagnostics_opts(
            &collapsed, 0, "<fmt>", &line_index, &mut bag, enable_effects,
        ) {
            let new_signals = crate::analyze::fmt_signals(&reparsed);
            return ast_rewrite_inner(&collapsed, &reparsed, &new_signals, enable_effects);
        }
        return collapsed;
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
    _signals: &crate::analyze::FmtSignals,
    _enable_effects: bool,
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
fn rewrite_decl_body(body_lines: &[&str], ast_decl: Option<&crate::ast::Decl>) -> String {
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
        sections.push(Section {
            kind: kind,
            lines: current_lines,
        });
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
    fn fmt_collapse_two_whole_module_imports_same_path() {
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
        let src = r#"import "./a.glyph.md" { foo }
import "./b.glyph.md" { bar }

skill main()
    description: "Main."
    flow:
        foo()
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
}
