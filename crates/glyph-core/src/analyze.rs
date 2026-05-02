//! Phase 2 (Analyze) — name and effect resolution.
//!
//! Slice 4 wires two parameter-related rules:
//!
//! - `G::analyze::unknown-param-slot` — error. A `{name}` slot inside an
//!   instruction-bearing string (the walking-skeleton subset = inline `flow:`
//!   strings) refers to an identifier that is not a declared header parameter
//!   on the enclosing skill.
//! - `G::analyze::missing-param-default` — error. An `export block` declares a
//!   parameter without a default. Skill parameters without defaults are legal
//!   (runtime-required); only `export block` parameters require defaults per
//!   `design/language-surface.md` §3.10.
//!
//! Both diagnostics fire from the parsed AST, before lowering, so they surface
//! through `glyph check` as well as `glyph compile`.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use crate::ast::{self, BlockDecl, ContextEntry, Decl, FlowStmt, ReturnExpr, SourceFile};
use crate::diagnostic::{Classification, DiagBag, Diagnostic, SourceSpan};
use crate::slot::scan_slots;
use crate::span::{LineIndex, Span, Spanned};

// ---------------------------------------------------------------------------
// Name-resolution table for go-to-definition (LSP M2).
//
// See `design/glyph-lsp.md` §4.4. The compiler already knows, at analyze
// time, which `text`/`block`/`export block` declaration each identifier
// reference resolves to — it just throws that information away after running
// its diagnostic checks. The types and `analyze_with_resolutions` entry point
// below replay the same matching logic over the AST and expose the result
// as a flat [`Resolution`] list.
//
// The list is the contract the LSP's `textDocument/definition` handler
// consumes: given a cursor byte-offset, find the smallest [`Resolution`]
// whose `use_span` contains it, then return the `def_span` (and `def_file`)
// to the editor.
// ---------------------------------------------------------------------------

/// A resolved name reference: where the name was used, and where it was
/// declared.
///
/// `use_span` covers the identifier token at the use-site (e.g., the bytes of
/// `validate_plan` in `validate_plan()`). `def_span` covers the declaration
/// — currently the entire decl span (which starts at the keyword like
/// `block` / `text`); the editor positions the cursor at `def_span.start`,
/// which lands on the declaration keyword.
///
/// `def_file` is the path of the file the declaration lives in. For same-file
/// resolutions it equals the analyzing file's own path. For cross-file
/// (imported) resolutions it points at the imported file. For
/// [`ResolutionKind::Stdlib`] it is left empty — the LSP returns `null` for
/// stdlib jumps per design §10.D.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub use_span: Span,
    pub def_span: Span,
    pub def_file: PathBuf,
    pub kind: ResolutionKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionKind {
    Skill,
    Block,
    ExportBlock,
    /// Includes both private `text` and `export text` declarations.
    Text,
    /// `{name}` slot resolving to a header parameter of the enclosing decl.
    Param,
    /// The name token of an `import "<path>" { name }` clause itself.
    Import,
    /// `@glyph/std` member (`subagent`, `send`). The LSP returns `null` for
    /// these — they have no `.glyph.md` source to jump to.
    Stdlib,
}

/// Backwards-compatible Phase-2 entry point — returns the AST unchanged.
///
/// Kept so existing callers (and the structural shape of `lib.rs::compile_source`)
/// continue to compile while slice-4 routes go through
/// [`analyze_with_diagnostics`].
pub fn analyze(file: SourceFile) -> SourceFile {
    file
}

/// Run Phase 2 with diagnostic emission.
///
/// Pushes any structured diagnostics onto `bag` and returns the AST unchanged.
/// `file_label` and `line_index` follow the same contract as the parser entry
/// point (`design/diagnostics.md` §Span Semantics).
pub fn analyze_with_diagnostics(
    file: SourceFile,
    file_id: u32,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    enable_effects: bool,
) -> SourceFile {
    // Collect text declaration names for bare-name detection in flow.
    let text_names: HashSet<&str> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Text(t) => Some(t.node.name.as_str()),
            _ => None,
        })
        .collect();

    // Collect block declaration names for call resolution.
    let block_names: HashSet<&str> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Block(b) => Some(b.node.name.as_str()),
            _ => None,
        })
        .collect();

    // Collect block declarations for effect inference.
    let block_decls: HashMap<&str, &BlockDecl> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Block(b) => Some((b.node.name.as_str(), &b.node)),
            _ => None,
        })
        .collect();

    // Collect private (non-exported) names for closure checking.
    let private_names: HashSet<&str> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Text(t) if !t.node.exported => Some(t.node.name.as_str()),
            Decl::Block(b) => Some(b.node.name.as_str()),
            _ => None,
        })
        .collect();

    for decl in &file.decls {
        match decl {
            Decl::Skill(spanned) => {
                analyze_skill(spanned, file_id, file_label, line_index, bag, &text_names, &block_names, &block_decls, &HashMap::new(), enable_effects)
            }
            Decl::ExportBlock(spanned) => {
                analyze_export_block(spanned, file_label, line_index, bag, &private_names);
            }
            Decl::Block(_) => {}
            Decl::Text(_) => {}
            Decl::Import(_) => {}
        }
    }

    // G::analyze::name-collision — duplicate export names.
    {
        let mut seen_exports: HashMap<&str, Span> = HashMap::new();
        for decl in &file.decls {
            let (name, span) = match decl {
                Decl::ExportBlock(b) => (b.node.name.as_str(), b.span),
                Decl::Text(t) if t.node.exported => (t.node.name.as_str(), t.span),
                _ => continue,
            };
            if let Some(_prev_span) = seen_exports.get(name) {
                bag.push(
                    Diagnostic::error(
                        "G::analyze::name-collision",
                        format!("duplicate export name `{}`", name),
                        SourceSpan::from_byte_span(file_label, span, line_index),
                    ),
                    span,
                );
            } else {
                seen_exports.insert(name, span);
            }
        }
    }

    // Library detection: file with zero skills.
    let has_skill = file.decls.iter().any(|d| matches!(d, Decl::Skill(_)));
    if !has_skill {
        let has_export = file.decls.iter().any(|d| {
            matches!(d, Decl::ExportBlock(_))
                || matches!(d, Decl::Text(t) if t.node.exported)
        });
        if !has_export {
            let span = crate::span::Span::new(file_id, 0, 0);
            bag.push(
                Diagnostic::error(
                    "G::analyze::no-exports-in-library",
                    "file has no `skill` and no `export` declarations",
                    SourceSpan::from_byte_span(file_label, span, line_index),
                ),
                span,
            );
        }
    }

    file
}

/// Run Phase 2 like [`analyze_with_diagnostics`], but additionally return a
/// flat list of every resolved reference covering same-file targets.
///
/// This is the entry point the LSP uses for `textDocument/definition` (M2).
/// The diagnostics emitted are identical to those of
/// [`analyze_with_diagnostics`] — this function is purely additive: it walks
/// the AST a second time to build the [`Resolution`] table.
///
/// `file_path` is recorded as the `def_file` for every same-file resolution.
/// Cross-file resolutions (i.e., for names brought in via `import`) are
/// produced separately by [`record_cross_file_import_resolutions`], called
/// from `lib::check_*_with_resolutions` once each imported file has been
/// parsed.
pub fn analyze_with_resolutions(
    file: SourceFile,
    file_id: u32,
    file_label: &str,
    file_path: &PathBuf,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    enable_effects: bool,
) -> (SourceFile, Vec<Resolution>) {
    let file = analyze_with_diagnostics(file, file_id, file_label, line_index, bag, enable_effects);
    let resolutions = collect_same_file_resolutions(&file, file_path);
    (file, resolutions)
}

/// Build the resolution table for one parsed file. Same-file references only
/// — cross-file resolutions are added downstream once the importer has
/// parsed each dependency.
///
/// The walk is purely structural and does not emit diagnostics; the caller
/// is expected to have already run [`analyze_with_diagnostics`] (or to
/// invoke [`analyze_with_resolutions`] which does both in one call).
/// Unresolvable names produce no entry — the LSP returns `null` for those
/// (see design §7).
pub fn collect_same_file_resolutions(file: &SourceFile, file_path: &PathBuf) -> Vec<Resolution> {
    // Build name → def_span maps from the file's declarations. These mirror
    // the `text_names` / `block_names` checks above; the only difference is
    // we keep the decl's full span (rather than discarding it after the
    // membership test).
    let mut text_defs: HashMap<&str, Span> = HashMap::new();
    let mut block_defs: HashMap<&str, Span> = HashMap::new();
    let mut export_block_defs: HashMap<&str, Span> = HashMap::new();
    let mut skill_defs: HashMap<&str, Span> = HashMap::new();
    // Stdlib names brought into scope by `import "@glyph/std" { ... }`.
    let mut stdlib_names: HashMap<String, Span> = HashMap::new();

    for decl in &file.decls {
        match decl {
            Decl::Text(t) => {
                text_defs.insert(t.node.name.as_str(), t.span);
            }
            Decl::Block(b) => {
                block_defs.insert(b.node.name.as_str(), b.span);
            }
            Decl::ExportBlock(b) => {
                export_block_defs.insert(b.node.name.as_str(), b.span);
            }
            Decl::Skill(s) => {
                skill_defs.insert(s.node.name.as_str(), s.span);
            }
            Decl::Import(imp) => {
                if imp.node.path == "@glyph/std" {
                    if let ast::ImportKind::Selective(names) = &imp.node.kind {
                        for imp_name in names {
                            if imp_name.name.node == "subagent"
                                || imp_name.name.node == "send"
                            {
                                let local = imp_name
                                    .alias
                                    .clone()
                                    .unwrap_or_else(|| imp_name.name.node.clone());
                                stdlib_names.insert(local, imp_name.name.span);
                            }
                        }
                    }
                }
            }
        }
    }
    // Avoid "unused but populated" warning — `skill_defs` is reserved for a
    // future ResolutionKind::Skill use-case (e.g. `applies()` self-references).
    let _ = &skill_defs;

    let mut out: Vec<Resolution> = Vec::new();

    // Walk every use-site in the file.
    for decl in &file.decls {
        match decl {
            Decl::Skill(spanned) => {
                let skill = &spanned.node;
                walk_flow_for_resolutions(
                    &skill.flow,
                    file_path,
                    &text_defs,
                    &block_defs,
                    &export_block_defs,
                    &stdlib_names,
                    &mut out,
                );
                for marker in &skill.body_constraints {
                    record_text_use(&marker.name.node, marker.name.span, &text_defs, file_path, &mut out);
                }
                for entry in skill.body_context.iter().chain(skill.context_section.iter()) {
                    if let ContextEntry::NameRef(name) = entry {
                        record_text_use(&name.node, name.span, &text_defs, file_path, &mut out);
                    }
                }
                for bare in &skill.body_bare_names {
                    record_text_use(&bare.node, bare.span, &text_defs, file_path, &mut out);
                }
            }
            Decl::Block(spanned) => {
                walk_flow_for_resolutions(
                    &spanned.node.flow,
                    file_path,
                    &text_defs,
                    &block_defs,
                    &export_block_defs,
                    &stdlib_names,
                    &mut out,
                );
            }
            Decl::ExportBlock(_) => {
                // Slice 4 captured only the header shape for export blocks
                // (no flow recorded in the AST). Once §13 ships full
                // export-block lowering, walk its flow here too.
            }
            Decl::Text(_) => {}
            Decl::Import(imp) => {
                // For `@glyph/std` selective imports, record the import name
                // span as a Stdlib resolution. Cross-file imports are
                // recorded by `record_cross_file_import_resolutions`, which
                // is invoked from `lib::check_source_with_resolutions` once
                // the dependency files have been resolved + parsed.
                if imp.node.path == "@glyph/std" {
                    if let ast::ImportKind::Selective(names) = &imp.node.kind {
                        for imp_name in names {
                            if imp_name.name.node == "subagent"
                                || imp_name.name.node == "send"
                            {
                                out.push(Resolution {
                                    use_span: imp_name.name.span,
                                    def_span: Span::new(0, 0, 0),
                                    def_file: PathBuf::new(),
                                    kind: ResolutionKind::Stdlib,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    out
}

/// Per-imported-name target descriptor used when wiring cross-file
/// resolutions. The LSP needs to know which file the def lives in and where
/// inside that file.
#[derive(Clone, Debug)]
pub struct ImportTarget {
    /// Local name as visible to the importer (i.e., the alias if one was
    /// given, otherwise the original name).
    pub local_name: String,
    /// Path of the file the def lives in.
    pub def_file: PathBuf,
    /// Span of the declaration in the def file.
    pub def_span: Span,
    /// Kind of the def (Text / Block / ExportBlock).
    pub kind: ResolutionKind,
}

/// Walk every use-site in `file` and record cross-file resolutions for every
/// reference whose name matches one of `targets`. Targets are keyed by the
/// importer's local-name view (alias-resolved).
///
/// This is the cross-file complement to [`collect_same_file_resolutions`].
/// The caller assembles `targets` by walking the file's `import` decls and
/// looking up each imported name in the corresponding dependency file.
pub fn collect_cross_file_resolutions(
    file: &SourceFile,
    targets: &HashMap<String, ImportTarget>,
) -> Vec<Resolution> {
    if targets.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<Resolution> = Vec::new();

    for decl in &file.decls {
        match decl {
            Decl::Skill(spanned) => {
                let skill = &spanned.node;
                walk_flow_for_cross_file(&skill.flow, targets, &mut out);
                for marker in &skill.body_constraints {
                    record_cross_file_text_use(&marker.name, targets, &mut out);
                }
                for entry in skill.body_context.iter().chain(skill.context_section.iter()) {
                    if let ContextEntry::NameRef(name) = entry {
                        record_cross_file_text_use(name, targets, &mut out);
                    }
                }
                for bare in &skill.body_bare_names {
                    record_cross_file_text_use(bare, targets, &mut out);
                }
            }
            Decl::Block(spanned) => {
                walk_flow_for_cross_file(&spanned.node.flow, targets, &mut out);
            }
            Decl::ExportBlock(_) | Decl::Text(_) => {}
            Decl::Import(imp) => {
                // The selective-import name token itself jumps to the
                // declaration in the dependency file. (Stdlib imports are
                // handled by `collect_same_file_resolutions`.)
                if imp.node.path.starts_with("@glyph/") {
                    continue;
                }
                if let ast::ImportKind::Selective(names) = &imp.node.kind {
                    for imp_name in names {
                        let local = imp_name
                            .alias
                            .clone()
                            .unwrap_or_else(|| imp_name.name.node.clone());
                        if let Some(t) = targets.get(&local) {
                            out.push(Resolution {
                                use_span: imp_name.name.span,
                                def_span: t.def_span,
                                def_file: t.def_file.clone(),
                                kind: ResolutionKind::Import,
                            });
                        }
                    }
                }
            }
        }
    }

    out
}

fn record_text_use(
    name: &str,
    use_span: Span,
    text_defs: &HashMap<&str, Span>,
    file_path: &PathBuf,
    out: &mut Vec<Resolution>,
) {
    if let Some(def_span) = text_defs.get(name) {
        out.push(Resolution {
            use_span,
            def_span: *def_span,
            def_file: file_path.clone(),
            kind: ResolutionKind::Text,
        });
    }
}

fn walk_flow_for_resolutions(
    stmts: &[FlowStmt],
    file_path: &PathBuf,
    text_defs: &HashMap<&str, Span>,
    block_defs: &HashMap<&str, Span>,
    export_block_defs: &HashMap<&str, Span>,
    stdlib_names: &HashMap<String, Span>,
    out: &mut Vec<Resolution>,
) {
    for stmt in stmts {
        match stmt {
            FlowStmt::Call { target, .. } => {
                record_call_target(target, file_path, block_defs, export_block_defs, stdlib_names, out);
            }
            FlowStmt::ConstraintMarker(marker) => {
                record_text_use(&marker.name.node, marker.name.span, text_defs, file_path, out);
            }
            FlowStmt::ContextMarker(entry) => {
                if let ContextEntry::NameRef(name) = entry {
                    record_text_use(&name.node, name.span, text_defs, file_path, out);
                }
            }
            FlowStmt::BareName(name) => {
                record_text_use(&name.node, name.span, text_defs, file_path, out);
            }
            FlowStmt::Return(ReturnExpr::Call { target, .. }) => {
                record_call_target(target, file_path, block_defs, export_block_defs, stdlib_names, out);
            }
            FlowStmt::Return(ReturnExpr::Name(name)) => {
                record_text_use(&name.node, name.span, text_defs, file_path, out);
            }
            FlowStmt::Branch { then_body, elif_branches, else_body, .. } => {
                walk_flow_for_resolutions(then_body, file_path, text_defs, block_defs, export_block_defs, stdlib_names, out);
                for elif in elif_branches {
                    walk_flow_for_resolutions(&elif.body, file_path, text_defs, block_defs, export_block_defs, stdlib_names, out);
                }
                if let Some(eb) = else_body {
                    walk_flow_for_resolutions(eb, file_path, text_defs, block_defs, export_block_defs, stdlib_names, out);
                }
            }
            FlowStmt::InlineString(_) | FlowStmt::Return(_) => {
                // InlineString: `{param}` slot resolution happens in the LSP
                // handler via a source-text scan + scan_slots, since we
                // don't carry slot spans in the AST. Bare `return` / inline
                // string return have no name to resolve.
            }
        }
    }
}

fn record_call_target(
    target: &Spanned<String>,
    file_path: &PathBuf,
    block_defs: &HashMap<&str, Span>,
    export_block_defs: &HashMap<&str, Span>,
    stdlib_names: &HashMap<String, Span>,
    out: &mut Vec<Resolution>,
) {
    if let Some(def_span) = block_defs.get(target.node.as_str()) {
        out.push(Resolution {
            use_span: target.span,
            def_span: *def_span,
            def_file: file_path.clone(),
            kind: ResolutionKind::Block,
        });
    } else if let Some(def_span) = export_block_defs.get(target.node.as_str()) {
        out.push(Resolution {
            use_span: target.span,
            def_span: *def_span,
            def_file: file_path.clone(),
            kind: ResolutionKind::ExportBlock,
        });
    } else if stdlib_names.contains_key(&target.node) {
        out.push(Resolution {
            use_span: target.span,
            def_span: Span::new(0, 0, 0),
            def_file: PathBuf::new(),
            kind: ResolutionKind::Stdlib,
        });
    }
}

fn walk_flow_for_cross_file(
    stmts: &[FlowStmt],
    targets: &HashMap<String, ImportTarget>,
    out: &mut Vec<Resolution>,
) {
    for stmt in stmts {
        match stmt {
            FlowStmt::Call { target, .. } => {
                record_cross_file_call(target, targets, out);
            }
            FlowStmt::ConstraintMarker(marker) => {
                record_cross_file_text_use(&marker.name, targets, out);
            }
            FlowStmt::ContextMarker(entry) => {
                if let ContextEntry::NameRef(name) = entry {
                    record_cross_file_text_use(name, targets, out);
                }
            }
            FlowStmt::BareName(name) => {
                record_cross_file_text_use(name, targets, out);
            }
            FlowStmt::Return(ReturnExpr::Call { target, .. }) => {
                record_cross_file_call(target, targets, out);
            }
            FlowStmt::Return(ReturnExpr::Name(name)) => {
                record_cross_file_text_use(name, targets, out);
            }
            FlowStmt::Branch { then_body, elif_branches, else_body, .. } => {
                walk_flow_for_cross_file(then_body, targets, out);
                for elif in elif_branches {
                    walk_flow_for_cross_file(&elif.body, targets, out);
                }
                if let Some(eb) = else_body {
                    walk_flow_for_cross_file(eb, targets, out);
                }
            }
            FlowStmt::InlineString(_) | FlowStmt::Return(_) => {}
        }
    }
}

fn record_cross_file_text_use(
    name: &Spanned<String>,
    targets: &HashMap<String, ImportTarget>,
    out: &mut Vec<Resolution>,
) {
    if let Some(t) = targets.get(&name.node) {
        if matches!(t.kind, ResolutionKind::Text) {
            out.push(Resolution {
                use_span: name.span,
                def_span: t.def_span,
                def_file: t.def_file.clone(),
                kind: ResolutionKind::Text,
            });
        }
    }
}

fn record_cross_file_call(
    target: &Spanned<String>,
    targets: &HashMap<String, ImportTarget>,
    out: &mut Vec<Resolution>,
) {
    if let Some(t) = targets.get(&target.node) {
        // Imported callable — Block or ExportBlock.
        if matches!(t.kind, ResolutionKind::Block | ResolutionKind::ExportBlock) {
            out.push(Resolution {
                use_span: target.span,
                def_span: t.def_span,
                def_file: t.def_file.clone(),
                kind: t.kind,
            });
        }
    }
}

/// Run Phase 2 with import-augmented name sets.
///
/// Like `analyze_with_diagnostics` but also considers imported texts and blocks
/// when resolving names. Tracks which imported names are actually used via
/// `used_import_names`.
pub fn analyze_with_imports(
    file: &SourceFile,
    file_id: u32,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    imported_texts: &HashSet<String>,
    imported_blocks: &HashSet<String>,
    used_import_names: &mut HashSet<String>,
    imported_block_descriptions: &HashMap<String, String>,
    enable_effects: bool,
) -> SourceFile {
    // Collect local text declaration names.
    let local_text_names: HashSet<&str> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Text(t) => Some(t.node.name.as_str()),
            _ => None,
        })
        .collect();

    // Collect local block declaration names.
    let local_block_names: HashSet<&str> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Block(b) => Some(b.node.name.as_str()),
            _ => None,
        })
        .collect();

    // Combined sets including imports.
    let mut text_names: HashSet<&str> = local_text_names;
    let imported_text_refs: Vec<String> = imported_texts.iter().cloned().collect();
    for t in &imported_text_refs {
        text_names.insert(t.as_str());
    }

    let mut block_names: HashSet<&str> = local_block_names;
    let imported_block_refs: Vec<String> = imported_blocks.iter().cloned().collect();
    for b in &imported_block_refs {
        block_names.insert(b.as_str());
    }

    // Collect block declarations for effect inference (local only).
    let block_decls: HashMap<&str, &crate::ast::BlockDecl> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Block(b) => Some((b.node.name.as_str(), &b.node)),
            _ => None,
        })
        .collect();

    // Collect private (non-exported) names for closure checking.
    let private_names: HashSet<&str> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Text(t) if !t.node.exported => Some(t.node.name.as_str()),
            Decl::Block(b) => Some(b.node.name.as_str()),
            _ => None,
        })
        .collect();

    for decl in &file.decls {
        match decl {
            Decl::Skill(spanned) => {
                analyze_skill_with_usage_tracking(
                    spanned, file_id, file_label, line_index, bag,
                    &text_names, &block_names, &block_decls,
                    imported_texts, imported_blocks, used_import_names,
                    imported_block_descriptions,
                    enable_effects,
                );
            }
            Decl::ExportBlock(spanned) => {
                analyze_export_block(spanned, file_label, line_index, bag, &private_names);
            }
            Decl::Block(_) | Decl::Text(_) | Decl::Import(_) => {}
        }
    }

    // G::analyze::name-collision — duplicate export names.
    {
        let mut seen_exports: HashMap<&str, Span> = HashMap::new();
        for decl in &file.decls {
            let (name, span) = match decl {
                Decl::ExportBlock(b) => (b.node.name.as_str(), b.span),
                Decl::Text(t) if t.node.exported => (t.node.name.as_str(), t.span),
                _ => continue,
            };
            if let Some(_prev_span) = seen_exports.get(name) {
                bag.push(
                    Diagnostic::error(
                        "G::analyze::name-collision",
                        format!("duplicate export name `{}`", name),
                        SourceSpan::from_byte_span(file_label, span, line_index),
                    ),
                    span,
                );
            } else {
                seen_exports.insert(name, span);
            }
        }
    }

    // Library detection: file with zero skills.
    let has_skill = file.decls.iter().any(|d| matches!(d, Decl::Skill(_)));
    if !has_skill {
        let has_export = file.decls.iter().any(|d| {
            matches!(d, Decl::ExportBlock(_))
                || matches!(d, Decl::Text(t) if t.node.exported)
        });
        if !has_export {
            let span = crate::span::Span::new(file_id, 0, 0);
            bag.push(
                Diagnostic::error(
                    "G::analyze::no-exports-in-library",
                    "file has no `skill` and no `export` declarations",
                    SourceSpan::from_byte_span(file_label, span, line_index),
                ),
                span,
            );
        }
    }

    file.clone()
}

/// Like `analyze_skill` but also tracks which imported names are used.
fn analyze_skill_with_usage_tracking(
    spanned: &Spanned<crate::ast::Skill>,
    file_id: u32,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    text_names: &HashSet<&str>,
    block_names: &HashSet<&str>,
    block_decls: &HashMap<&str, &crate::ast::BlockDecl>,
    imported_texts: &HashSet<String>,
    imported_blocks: &HashSet<String>,
    used_import_names: &mut HashSet<String>,
    imported_block_descriptions: &HashMap<String, String>,
    enable_effects: bool,
) {
    // Run the normal analysis.
    analyze_skill(spanned, file_id, file_label, line_index, bag, text_names, block_names, block_decls, imported_block_descriptions, enable_effects);

    // Track usage: walk flow/constraints/context to see which imported names are referenced.
    let skill = &spanned.node;

    // Check constraint markers.
    for marker in &skill.body_constraints {
        if imported_texts.contains(&marker.name.node) {
            used_import_names.insert(marker.name.node.clone());
        }
    }

    // Check context entries.
    for entry in &skill.body_context {
        if let crate::ast::ContextEntry::NameRef(name) = entry {
            if imported_texts.contains(&name.node) {
                used_import_names.insert(name.node.clone());
            }
        }
    }
    for entry in &skill.context_section {
        if let crate::ast::ContextEntry::NameRef(name) = entry {
            if imported_texts.contains(&name.node) {
                used_import_names.insert(name.node.clone());
            }
        }
    }

    // Check flow statements.
    track_flow_usage(&skill.flow, imported_texts, imported_blocks, used_import_names);
}

fn track_flow_usage(
    flow: &[crate::ast::FlowStmt],
    imported_texts: &HashSet<String>,
    imported_blocks: &HashSet<String>,
    used: &mut HashSet<String>,
) {
    for stmt in flow {
        match stmt {
            crate::ast::FlowStmt::Call { target, .. } => {
                if imported_blocks.contains(&target.node) {
                    used.insert(target.node.clone());
                }
            }
            crate::ast::FlowStmt::ConstraintMarker(marker) => {
                if imported_texts.contains(&marker.name.node) {
                    used.insert(marker.name.node.clone());
                }
            }
            crate::ast::FlowStmt::ContextMarker(entry) => {
                if let crate::ast::ContextEntry::NameRef(name) = entry {
                    if imported_texts.contains(&name.node) {
                        used.insert(name.node.clone());
                    }
                }
            }
            crate::ast::FlowStmt::Branch { then_body, elif_branches, else_body, .. } => {
                track_flow_usage(then_body, imported_texts, imported_blocks, used);
                for elif in elif_branches {
                    track_flow_usage(&elif.body, imported_texts, imported_blocks, used);
                }
                if let Some(eb) = else_body {
                    track_flow_usage(eb, imported_texts, imported_blocks, used);
                }
            }
            _ => {}
        }
    }
}

fn analyze_skill(
    spanned: &Spanned<crate::ast::Skill>,
    file_id: u32,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    text_names: &HashSet<&str>,
    block_names: &HashSet<&str>,
    block_decls: &HashMap<&str, &BlockDecl>,
    imported_block_descriptions: &HashMap<String, String>,
    enable_effects: bool,
) {
    let skill = &spanned.node;
    let declared: HashSet<&str> = skill.params.iter().map(|p| p.name.as_str()).collect();

    // Walking-skeleton subset: `flow:` inline strings are the only
    // instruction-bearing strings the parser captures with their source span
    // available. Other instruction-bearing positions (constraint prose,
    // generated block bodies) are added when those constructs lower in later
    // slices. The AST currently keeps only the cooked text for a flow inline
    // string, not its source span — so we cannot pinpoint a slot inside it
    // back to the original source. Until the AST grows per-statement spans we
    // attribute slot diagnostics to the enclosing skill header span; this is
    // synthetic-fallback option (3) per `design/diagnostics.md` §Span
    // Semantics. The IDs and messages remain accurate.
    for stmt in &skill.flow {
        match stmt {
            FlowStmt::InlineString(text) => {
                for slot in scan_slots(text) {
                    if !declared.contains(slot.name.as_str()) {
                        let span = spanned.span;
                        bag.push(
                            Diagnostic::error(
                                "G::analyze::unknown-param-slot",
                                format!(
                                    "`{{{}}}` is not a declared parameter of `{}`",
                                    slot.name, skill.name
                                ),
                                SourceSpan::from_byte_span(file_label, span, line_index),
                            ),
                            span,
                        );
                        let _ = file_id;
                    }
                }
            }
            FlowStmt::BareName(name) => {
                // A bare name in flow: without a keyword prefix is a compile error.
                // Per spec: `G::analyze::text-in-flow` (repairable — Repair adds
                // parens and materializes a `generated block`).
                let span = spanned.span;
                bag.push(
                    crate::diagnostic::Diagnostic {
                        id: "G::analyze::text-in-flow".into(),
                        classification: crate::diagnostic::Classification::Repairable,
                        message: format!(
                            "bare name `{}` in `flow:` is not a valid statement; add a keyword prefix (`require`/`avoid`/`must`/`context`) or parentheses for a call",
                            name.node
                        ),
                        span: SourceSpan::from_byte_span(file_label, span, line_index),
                        related: Vec::new(),
                        hints: vec![
                            "if this is a block call, add `()` after the name; if it is a constraint or context, add the appropriate keyword prefix".into(),
                        ],
                    },
                    span,
                );
            }
            FlowStmt::Call { target, .. } => {
                // Check that the call target resolves to a declared block.
                if !block_names.contains(target.node.as_str()) {
                    // Check if this is a stdlib name used without import.
                    if is_stdlib_block_name(&target.node) {
                        let span = spanned.span;
                        bag.push(
                            crate::diagnostic::Diagnostic {
                                id: "G::analyze::stdlib-missing-import".into(),
                                classification: crate::diagnostic::Classification::Repairable,
                                message: format!(
                                    "`{}` is a standard library block; add `import \"@glyph/std\" {{ {} }}`",
                                    target.node, target.node
                                ),
                                span: SourceSpan::from_byte_span(file_label, span, line_index),
                                related: Vec::new(),
                                hints: vec![
                                    format!("add `import \"@glyph/std\" {{ {} }}` at the top of the file", target.node),
                                ],
                            },
                            span,
                        );
                    } else {
                        let span = spanned.span;
                        bag.push(
                            crate::diagnostic::Diagnostic {
                                id: "G::analyze::undefined-call".into(),
                                classification: crate::diagnostic::Classification::Repairable,
                                message: format!(
                                    "call to `{}()` but no `block {}` is declared in this file",
                                    target.node, target.node
                                ),
                                span: SourceSpan::from_byte_span(file_label, span, line_index),
                                related: Vec::new(),
                                hints: vec![
                                    format!("declare `block {}()` or check the name for typos", target.node),
                                ],
                            },
                            span,
                        );
                    }
                }
            }
            FlowStmt::ConstraintMarker(marker) => {
                // Check that the constraint name resolves to a text declaration.
                if !text_names.contains(marker.name.node.as_str()) {
                    let span = spanned.span;
                    bag.push(
                        Diagnostic::error(
                            "G::analyze::undefined-name",
                            format!(
                                "`{}` is not a declared `text` in this file",
                                marker.name.node
                            ),
                            SourceSpan::from_byte_span(file_label, span, line_index),
                        ),
                        span,
                    );
                }
            }
            FlowStmt::ContextMarker(entry) => {
                check_context_entry_name(entry, text_names, spanned.span, file_label, line_index, bag);
            }
            FlowStmt::Return(_) => {
                // Return statements are validated structurally by the parser
                // (check_return_rules). No analyze-phase checks needed.
            }
            FlowStmt::Branch { condition, then_body, elif_branches, else_body } => {
                // Check for nested branches.
                check_nested_branches(then_body, spanned.span, file_label, line_index, bag);
                for elif in elif_branches {
                    check_nested_branches(&elif.body, spanned.span, file_label, line_index, bag);
                }
                if let Some(eb) = else_body {
                    check_nested_branches(eb, spanned.span, file_label, line_index, bag);
                }
                // Check applies() calls in condition.
                check_applies_in_condition(
                    condition, spanned.span, file_id, file_label, line_index, bag,
                    &text_names, &block_names, &block_decls, imported_block_descriptions,
                );
                // Check elif conditions too.
                for elif in elif_branches {
                    check_applies_in_condition(
                        &elif.condition, spanned.span, file_id, file_label, line_index, bag,
                        &text_names, &block_names, &block_decls, imported_block_descriptions,
                    );
                }
                // Check flow statements inside branch bodies for name resolution.
                check_branch_body_names(then_body, spanned.span, file_label, line_index, bag, &text_names, &block_names);
                for elif in elif_branches {
                    check_branch_body_names(&elif.body, spanned.span, file_label, line_index, bag, &text_names, &block_names);
                }
                if let Some(eb) = else_body {
                    check_branch_body_names(eb, spanned.span, file_label, line_index, bag, &text_names, &block_names);
                }
            }
        }
    }

    // Check body-level constraint name refs.
    for marker in &skill.body_constraints {
        if !text_names.contains(marker.name.node.as_str()) {
            let span = spanned.span;
            bag.push(
                Diagnostic::error(
                    "G::analyze::undefined-name",
                    format!(
                        "`{}` is not a declared `text` in this file",
                        marker.name.node
                    ),
                    SourceSpan::from_byte_span(file_label, span, line_index),
                ),
                span,
            );
        }
    }

    // Check body-level context name refs.
    for entry in &skill.body_context {
        check_context_entry_name(entry, text_names, spanned.span, file_label, line_index, bag);
    }

    // Check context: section name refs.
    for entry in &skill.context_section {
        check_context_entry_name(entry, text_names, spanned.span, file_label, line_index, bag);
    }

    // Check body-level bare names against text declarations.
    // A bare text name at body level (no keyword prefix) is ambiguous — the
    // compiler doesn't know if the author meant constraint, context, or step.
    for name in &skill.body_bare_names {
        if text_names.contains(name.node.as_str()) {
            let span = spanned.span;
            bag.push(
                crate::diagnostic::Diagnostic {
                    id: "G::analyze::ambiguous-role".into(),
                    classification: crate::diagnostic::Classification::Repairable,
                    message: format!(
                        "bare name `{}` at body level is ambiguous — add a keyword prefix (`require`/`avoid`/`must`/`context`) to clarify intent",
                        name.node
                    ),
                    span: SourceSpan::from_byte_span(file_label, span, line_index),
                    related: Vec::new(),
                    hints: vec![
                        "use `require <name>` for a constraint, `context <name>` for context, or move it into `flow:` for a step".into(),
                    ],
                },
                span,
            );
        }
    }

    // G::analyze::empty-skill-body — skill with no description, no flow, no
    // constraints, no effects. A skill must have at least one of flow (with
    // statements) or constraints (with markers) to be projectable.
    // When enable_effects is off, effects are not considered as content.
    let effects_count_as_content = enable_effects && !skill.effects.is_empty();
    if skill.description.is_none()
        && skill.flow.is_empty()
        && skill.body_constraints.is_empty()
        && !effects_count_as_content
        && skill.body_context.is_empty()
        && skill.context_section.is_empty()
    {
        let span = spanned.span;
        bag.push(
            Diagnostic::error(
                "G::analyze::empty-skill-body",
                format!(
                    "`skill {}` has no `description:`, `flow:`, `constraints:`, or `effects:` — nothing to project",
                    skill.name
                ),
                SourceSpan::from_byte_span(file_label, span, line_index),
            ),
            span,
        );
        return; // No point checking further if the skill is empty.
    }

    // Check missing description — repairable (Phase 3 Repair generates one).
    if skill.description.is_none() {
        let span = spanned.span;
        bag.push(
            crate::diagnostic::Diagnostic {
                id: "G::analyze::missing-description".into(),
                classification: crate::diagnostic::Classification::Repairable,
                message: format!("`skill {}` has no `description:` sub-section", skill.name),
                span: SourceSpan::from_byte_span(file_label, span, line_index),
                related: Vec::new(),
                hints: vec![
                    "add a `description:` sub-section, or let `glyph fmt` generate one".into(),
                ],
            },
            span,
        );
    }

    // --- Effect inference and validation ---
    // Skip entirely when effects are disabled.
    if !enable_effects {
        return;
    }
    // Infer effects by walking the call graph (local-transitive for same-file blocks).
    let inferred = infer_effects_for_skill(skill, block_decls);

    let declared_set: BTreeSet<&str> = skill.effects.iter().map(|s| s.as_str()).collect();

    // Skip validation if `effects: none` was declared (author assertion of no effects).
    let has_effects_declaration = !skill.effects.is_empty();
    let declared_none = skill.effects.iter().any(|e| e == "none");

    if has_effects_declaration && declared_none {
        // `effects: none` is an author assertion of zero effects.
        // If the call graph infers any effects, that's under-declared.
        if !inferred.is_empty() {
            let span = spanned.span;
            bag.push(
                Diagnostic::error(
                    "G::analyze::effects-under-declared",
                    format!(
                        "`effects: none` declared but call graph infers: {}",
                        inferred.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                    ),
                    SourceSpan::from_byte_span(file_label, span, line_index),
                ),
                span,
            );
        }
    } else if has_effects_declaration && !declared_none {
        // Check under-declared: inferred effects not in declared set.
        let missing: BTreeSet<&str> = inferred.iter().map(|s| s.as_str()).filter(|e| !declared_set.contains(e)).collect();
        if !missing.is_empty() {
            let span = spanned.span;
            bag.push(
                Diagnostic::error(
                    "G::analyze::effects-under-declared",
                    format!(
                        "declared effects are missing inferred effects: {}",
                        missing.iter().copied().collect::<Vec<_>>().join(", ")
                    ),
                    SourceSpan::from_byte_span(file_label, span, line_index),
                ),
                span,
            );
        }

        // Check over-declared: declared effects not in inferred set.
        let extra: BTreeSet<&str> = declared_set.iter().filter(|e| !inferred.contains(**e)).copied().collect();
        if !extra.is_empty() {
            let span = spanned.span;
            bag.push(
                Diagnostic {
                    id: "G::analyze::effects-over-declared".into(),
                    classification: Classification::Warning,
                    message: format!(
                        "declared effects not inferred from call graph: {}",
                        extra.iter().copied().collect::<Vec<_>>().join(", ")
                    ),
                    span: SourceSpan::from_byte_span(file_label, span, line_index),
                    related: Vec::new(),
                    hints: vec![
                        "remove unused effects or verify they are needed".into(),
                    ],
                },
                span,
            );
        }
    } else if !has_effects_declaration && !inferred.is_empty() {
        // No `effects:` declared and inferred set is non-empty → repairable.
        let span = spanned.span;
        bag.push(
            Diagnostic {
                id: "G::analyze::missing-effects".into(),
                classification: Classification::Repairable,
                message: format!(
                    "`skill {}` has no `effects:` declaration; inferred: {}",
                    skill.name,
                    inferred.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                ),
                span: SourceSpan::from_byte_span(file_label, span, line_index),
                related: Vec::new(),
                hints: vec![
                    "add `effects:` or let `glyph fmt` (Phase 3a) auto-add inferred effects".into(),
                ],
            },
            span,
        );
    }
}

/// Infer effects for a skill by walking its call graph transitively.
///
/// Returns the union of all effects declared on blocks reachable from
/// the skill's flow via call expressions.
fn infer_effects_for_skill(
    skill: &crate::ast::Skill,
    block_decls: &HashMap<&str, &BlockDecl>,
) -> BTreeSet<String> {
    let mut inferred = BTreeSet::new();
    let mut visited: HashSet<String> = HashSet::new();

    // Collect all call targets from the skill's flow.
    let mut worklist: Vec<String> = skill
        .flow
        .iter()
        .filter_map(|stmt| match stmt {
            FlowStmt::Call { target, .. } => Some(target.node.clone()),
            _ => None,
        })
        .collect();

    while let Some(target) = worklist.pop() {
        if !visited.insert(target.clone()) {
            continue; // already visited
        }
        if let Some(block) = block_decls.get(target.as_str()) {
            // Add this block's declared effects.
            for eff in &block.effects {
                if eff != "none" {
                    inferred.insert(eff.clone());
                }
            }
            // Add transitive calls from this block.
            for stmt in &block.flow {
                if let FlowStmt::Call { target: inner, .. } = stmt {
                    worklist.push(inner.node.clone());
                }
            }
        } else if let Some(effects) = stdlib_block_effects(&target) {
            // Stdlib block: add its known effect signature.
            for eff in effects {
                inferred.insert((*eff).to_string());
            }
        }
    }

    inferred
}

fn check_context_entry_name(
    entry: &ContextEntry,
    text_names: &HashSet<&str>,
    span: crate::span::Span,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    if let ContextEntry::NameRef(name) = entry {
        if !text_names.contains(name.node.as_str()) {
            bag.push(
                Diagnostic::error(
                    "G::analyze::undefined-name",
                    format!("`{}` is not a declared `text` in this file", name.node),
                    SourceSpan::from_byte_span(file_label, span, line_index),
                ),
                span,
            );
        }
    }
}

/// Check for nested branches — a Branch inside another Branch's body.
fn check_nested_branches(
    body: &[FlowStmt],
    span: crate::span::Span,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    for stmt in body {
        if let FlowStmt::Branch { .. } = stmt {
            bag.push(
                Diagnostic {
                    id: "G::analyze::nested-branch".into(),
                    classification: Classification::Repairable,
                    message: "nested `if`/`elif`/`else` inside a branch body; only one level of branching is supported in compiled output".into(),
                    span: SourceSpan::from_byte_span(file_label, span, line_index),
                    related: Vec::new(),
                    hints: vec![
                        "extract the inner branch into a separate `block` declaration".into(),
                    ],
                },
                span,
            );
        }
    }
}

/// Check applies() calls in a branch condition string.
/// Validates: applies-on-non-block, applies-on-undescribed-block.
fn check_applies_in_condition(
    condition: &str,
    span: crate::span::Span,
    _file_id: u32,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    text_names: &HashSet<&str>,
    block_names: &HashSet<&str>,
    block_decls: &HashMap<&str, &BlockDecl>,
    imported_block_descriptions: &HashMap<String, String>,
) {
    // Find all `NAME.applies()` patterns in the condition.
    // Simple string scanning — condition is a reconstructed string.
    let applies_suffix = ".applies()";
    let mut search_from = 0;
    while let Some(pos) = condition[search_from..].find(applies_suffix) {
        let abs_pos = search_from + pos;
        // Extract the receiver name (word before the dot).
        let receiver = &condition[..abs_pos];
        let receiver_name = receiver.rsplit(|c: char| !c.is_alphanumeric() && c != '_').next().unwrap_or("");
        if !receiver_name.is_empty() {
            if text_names.contains(receiver_name) {
                // Receiver is a text declaration — not a block.
                bag.push(
                    Diagnostic::error(
                        "G::analyze::applies-on-non-block",
                        format!("`{}.applies()` — receiver `{}` is a `text` declaration, not a `block`", receiver_name, receiver_name),
                        SourceSpan::from_byte_span(file_label, span, line_index),
                    ),
                    span,
                );
            } else if block_names.contains(receiver_name) {
                // Check if the block has a description.
                if let Some(block) = block_decls.get(receiver_name) {
                    if block.description.is_none() {
                        bag.push(
                            Diagnostic {
                                id: "G::analyze::applies-on-undescribed-block".into(),
                                classification: Classification::Repairable,
                                message: format!(
                                    "`{}.applies()` but `block {}` has no `description:` sub-section",
                                    receiver_name, receiver_name
                                ),
                                span: SourceSpan::from_byte_span(file_label, span, line_index),
                                related: Vec::new(),
                                hints: vec![
                                    format!("add `description:` to `block {}`", receiver_name),
                                ],
                            },
                            span,
                        );
                    }
                } else if !imported_block_descriptions.contains_key(receiver_name) {
                    // Block is known by name but not in block_decls — imported
                    // block without accessible declaration. Treat as hard error
                    // per ir-and-semantics.md §Block Trigger Predicate: imported
                    // export blocks without description are not repairable
                    // (Repair is single-file).
                    bag.push(
                        Diagnostic::error(
                            "G::analyze::applies-on-undescribed-block",
                            format!(
                                "`{}.applies()` but imported block `{}` has no accessible `description:`; add `description:` in the source file",
                                receiver_name, receiver_name
                            ),
                            SourceSpan::from_byte_span(file_label, span, line_index),
                        ),
                        span,
                    );
                }
            } else {
                // Not a block, not a text — unknown name or parameter.
                bag.push(
                    Diagnostic::error(
                        "G::analyze::applies-on-non-block",
                        format!("`{}.applies()` — receiver `{}` does not resolve to a `block`", receiver_name, receiver_name),
                        SourceSpan::from_byte_span(file_label, span, line_index),
                    ),
                    span,
                );
            }
        }
        search_from = abs_pos + applies_suffix.len();
    }
}

/// Check flow statements inside branch bodies for name resolution.
fn check_branch_body_names(
    body: &[FlowStmt],
    span: crate::span::Span,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    text_names: &HashSet<&str>,
    block_names: &HashSet<&str>,
) {
    for stmt in body {
        match stmt {
            FlowStmt::Call { target, .. } => {
                if !block_names.contains(target.node.as_str()) {
                    if is_stdlib_block_name(&target.node) {
                        bag.push(
                            Diagnostic {
                                id: "G::analyze::stdlib-missing-import".into(),
                                classification: Classification::Repairable,
                                message: format!(
                                    "`{}` is a standard library block; add `import \"@glyph/std\" {{ {} }}`",
                                    target.node, target.node
                                ),
                                span: SourceSpan::from_byte_span(file_label, span, line_index),
                                related: Vec::new(),
                                hints: vec![
                                    format!("add `import \"@glyph/std\" {{ {} }}` at the top of the file", target.node),
                                ],
                            },
                            span,
                        );
                    } else {
                        bag.push(
                            Diagnostic {
                                id: "G::analyze::undefined-call".into(),
                                classification: Classification::Repairable,
                                message: format!(
                                    "call to `{}()` but no `block {}` is declared in this file",
                                    target.node, target.node
                                ),
                                span: SourceSpan::from_byte_span(file_label, span, line_index),
                                related: Vec::new(),
                                hints: vec![
                                    format!("declare `block {}()` or check the name for typos", target.node),
                                ],
                            },
                            span,
                        );
                    }
                }
            }
            FlowStmt::ConstraintMarker(marker) => {
                if !text_names.contains(marker.name.node.as_str()) {
                    bag.push(
                        Diagnostic::error(
                            "G::analyze::undefined-name",
                            format!("`{}` is not a declared `text` in this file", marker.name.node),
                            SourceSpan::from_byte_span(file_label, span, line_index),
                        ),
                        span,
                    );
                }
            }
            FlowStmt::ContextMarker(entry) => {
                check_context_entry_name(entry, text_names, span, file_label, line_index, bag);
            }
            _ => {}
        }
    }
}

/// Check if a name is a stdlib block (author-importable from `@glyph/std`).
fn is_stdlib_block_name(name: &str) -> bool {
    name == "subagent" || name == "send"
}

/// Return the effect signature for a stdlib block, if it is one.
pub fn stdlib_block_effects(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "subagent" => Some(&["spawns_agent"]),
        "send" => Some(&["spawns_agent"]),
        _ => None,
    }
}

/// Emit `G::analyze::nominal-mismatch` for a type name mismatch at a call boundary.
///
/// In the full type system, this fires when a call passes a value whose nominal
/// type doesn't match the callee's parameter type annotation. The MVP grammar
/// does not yet have type annotations, so this is a placeholder that fires when
/// explicitly invoked by the compiler infrastructure once type annotations land.
pub fn emit_nominal_mismatch(
    actual_type: &str,
    expected_type: &str,
    context_name: &str,
    span: Span,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    bag.push(
        Diagnostic::error(
            "G::analyze::nominal-mismatch",
            format!(
                "type mismatch at call boundary for `{}`: expected `{}`, got `{}`",
                context_name, expected_type, actual_type
            ),
            SourceSpan::from_byte_span(file_label, span, line_index),
        ),
        span,
    );
}

/// Emit `G::analyze::lossy-coercion` for a lossy numeric conversion.
///
/// Fires when a float value is passed where an integer is expected, or similar
/// lossy conversions. The MVP grammar does not yet support numeric literals or
/// type annotations, so this is a placeholder that fires when explicitly invoked.
pub fn emit_lossy_coercion(
    from_type: &str,
    to_type: &str,
    context_name: &str,
    span: Span,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    bag.push(
        Diagnostic::error(
            "G::analyze::lossy-coercion",
            format!(
                "lossy coercion for `{}`: `{}` cannot be losslessly converted to `{}`",
                context_name, from_type, to_type
            ),
            SourceSpan::from_byte_span(file_label, span, line_index),
        ),
        span,
    );
}

fn analyze_export_block(
    spanned: &crate::span::Spanned<crate::ast::ExportBlockDecl>,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    private_names: &HashSet<&str>,
) {
    let decl = &spanned.node;
    for p in &decl.params {
        if p.default.is_none() {
            let span: Span = p.span;
            bag.push(
                Diagnostic::error(
                    "G::analyze::missing-param-default",
                    format!(
                        "`export block` parameter `{}` requires a default value",
                        p.name
                    ),
                    SourceSpan::from_byte_span(file_label, span, line_index),
                ),
                span,
            );
        }
    }
    // G::analyze::missing-return — export block must have an explicit return.
    if !decl.has_return {
        let span = spanned.span;
        bag.push(
            Diagnostic {
                id: "G::analyze::missing-return".into(),
                classification: Classification::Repairable,
                message: format!(
                    "`export block {}` requires an explicit `return` statement",
                    decl.name
                ),
                span: SourceSpan::from_byte_span(file_label, span, line_index),
                related: Vec::new(),
                hints: vec![
                    "add a `return` statement at the end of the `flow:` section".into(),
                ],
            },
            span,
        );
    }

    // G::analyze::closure-violation — export block must not reference private names.
    let param_names: HashSet<&str> = decl.params.iter().map(|p| p.name.as_str()).collect();
    for body_ref in &decl.body_refs {
        if private_names.contains(body_ref.as_str()) && !param_names.contains(body_ref.as_str()) {
            let span = spanned.span;
            bag.push(
                Diagnostic::error(
                    "G::analyze::closure-violation",
                    format!(
                        "`export block {}` references private name `{}` which is not visible to importers",
                        decl.name, body_ref
                    ),
                    SourceSpan::from_byte_span(file_label, span, line_index),
                ),
                span,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imported_block_without_description_fires_error() {
        // AC6: When a block name is in block_names but not in block_decls
        // (simulating an imported block), applies-on-undescribed-block fires
        // as a hard error (not repairable).
        let mut bag = DiagBag::new();
        let source = "imported_block.applies()";
        let line_index = LineIndex::new(source);
        let span = Span::new(0, 0, source.len() as u32);
        let text_names: HashSet<&str> = HashSet::new();
        let mut block_names: HashSet<&str> = HashSet::new();
        block_names.insert("imported_block");
        let block_decls: HashMap<&str, &BlockDecl> = HashMap::new(); // not in decls = imported

        check_applies_in_condition(
            source,
            span,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &text_names,
            &block_names,
            &block_decls,
            &HashMap::new(),
        );

        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::applies-on-undescribed-block"),
            "expected applies-on-undescribed-block for imported block, got: {:?}",
            ids
        );
        // Should be a hard error, not repairable.
        let diag = bag.iter().find(|d| d.id == "G::analyze::applies-on-undescribed-block").unwrap();
        assert_eq!(
            diag.classification,
            Classification::Error,
            "imported block applies-on-undescribed-block should be Error, not Repairable"
        );
    }

    #[test]
    fn nominal_mismatch_fires() {
        let mut bag = DiagBag::new();
        let source = "test";
        let line_index = LineIndex::new(source);
        let span = Span::new(0, 0, source.len() as u32);

        emit_nominal_mismatch("Report", "TestResult", "my_call", span, "test.glyph.md", &line_index, &mut bag);

        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"G::analyze::nominal-mismatch"), "ids: {:?}", ids);
        let diag = bag.iter().find(|d| d.id == "G::analyze::nominal-mismatch").unwrap();
        assert_eq!(diag.classification, Classification::Error);
        assert!(diag.message.contains("Report"));
        assert!(diag.message.contains("TestResult"));
    }

    #[test]
    fn analyze_with_diagnostics_receives_enable_effects() {
        // Verify that analyze_with_diagnostics accepts the enable_effects flag
        // and that when false, effect inference is skipped.
        use crate::ast::{BlockDecl, Decl, FlowStmt, Skill, SourceFile};
        use crate::span::Spanned;

        // Build a source file with a block that has effects and a skill that
        // calls it without declaring effects.
        let block = Spanned {
            node: BlockDecl {
                name: "writer".to_string(),
                params: Vec::new(),
                flow: vec![FlowStmt::InlineString("Write files.".to_string())],
                description: None,
                effects: vec!["writes_files".to_string()],
            },
            span: Span::new(0, 0, 10),
        };
        let skill = Spanned {
            node: Skill {
                name: "main".to_string(),
                params: Vec::new(),
                description: Some("Main skill.".to_string()),
                flow: vec![FlowStmt::Call {
                    target: Spanned::new("writer".to_string(), Span::new(0, 0, 6)),
                    args: Vec::new(),
                    site_modifier: None,
                }],
                flow_present: true,
                body_constraints: Vec::new(),
                body_context: Vec::new(),
                body_bare_names: Vec::new(),
                effects: Vec::new(),
                context_section: Vec::new(),
            },
            span: Span::new(0, 0, 10),
        };
        let file = SourceFile {
            decls: vec![Decl::Block(block), Decl::Skill(skill)],
        };
        let source = "dummy source";
        let li = LineIndex::new(source);

        // With enable_effects=true, missing-effects should fire.
        let mut bag_on = DiagBag::new();
        analyze_with_diagnostics(file.clone(), 0, "test.glyph.md", &li, &mut bag_on, true);
        let ids_on: Vec<&str> = bag_on.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids_on.contains(&"G::analyze::missing-effects"),
            "with effects on, expected missing-effects, got: {:?}",
            ids_on
        );

        // With enable_effects=false, no effect diagnostics should fire.
        let mut bag_off = DiagBag::new();
        analyze_with_diagnostics(file, 0, "test.glyph.md", &li, &mut bag_off, false);
        let ids_off: Vec<&str> = bag_off.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids_off.iter().any(|id| id.starts_with("G::analyze::effects") || *id == "G::analyze::missing-effects"),
            "with effects off, no effect diagnostics should fire, got: {:?}",
            ids_off
        );
    }

    #[test]
    fn lossy_coercion_fires() {
        let mut bag = DiagBag::new();
        let source = "test";
        let line_index = LineIndex::new(source);
        let span = Span::new(0, 0, source.len() as u32);

        emit_lossy_coercion("float", "int", "my_param", span, "test.glyph.md", &line_index, &mut bag);

        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"G::analyze::lossy-coercion"), "ids: {:?}", ids);
        let diag = bag.iter().find(|d| d.id == "G::analyze::lossy-coercion").unwrap();
        assert_eq!(diag.classification, Classification::Error);
        assert!(diag.message.contains("float"));
        assert!(diag.message.contains("int"));
    }

    // -----------------------------------------------------------------
    // Resolution table tests (LSP M2 — design §4.4)
    // -----------------------------------------------------------------

    fn parse_for_resolutions(source: &str) -> SourceFile {
        let (file, _) = crate::parse::parse(source, 0).expect("parse");
        file
    }

    #[test]
    fn analyze_with_resolutions_records_block_call_target() {
        let src = r#"skill main()
    description: "main."
    flow:
        validate_plan()

block validate_plan()
    "Check the plan."
"#;
        let file = parse_for_resolutions(src);
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let path = PathBuf::from("test.glyph.md");
        let (_file, res) = analyze_with_resolutions(file, 0, "test.glyph.md", &path, &line_index, &mut bag, false);
        let block_res = res.iter().find(|r| r.kind == ResolutionKind::Block);
        assert!(block_res.is_some(), "expected a Block resolution, got: {:?}", res);
        let r = block_res.unwrap();
        let use_text = &src[r.use_span.start as usize..r.use_span.end as usize];
        assert_eq!(use_text, "validate_plan");
        let def_text = &src[r.def_span.start as usize..r.def_span.start as usize + 5];
        assert_eq!(def_text, "block");
        assert_eq!(r.def_file, path);
    }

    #[test]
    fn analyze_with_resolutions_records_text_constraint() {
        let src = r#"skill main()
    description: "main."
    require accuracy
    flow:
        "Do something."

text accuracy = "Be accurate."
"#;
        let file = parse_for_resolutions(src);
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let path = PathBuf::from("t.glyph.md");
        let (_, res) = analyze_with_resolutions(file, 0, "t.glyph.md", &path, &line_index, &mut bag, false);
        let text_res = res.iter().find(|r| r.kind == ResolutionKind::Text);
        assert!(text_res.is_some(), "expected a Text resolution, got: {:?}", res);
        let r = text_res.unwrap();
        let use_text = &src[r.use_span.start as usize..r.use_span.end as usize];
        assert_eq!(use_text, "accuracy");
    }

    #[test]
    fn analyze_with_resolutions_unresolved_call_no_resolution() {
        let src = r#"skill main()
    description: "main."
    flow:
        no_such_block()
"#;
        let file = parse_for_resolutions(src);
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let path = PathBuf::from("t.glyph.md");
        let (_, res) = analyze_with_resolutions(file, 0, "t.glyph.md", &path, &line_index, &mut bag, false);
        assert!(
            !res.iter().any(|r| r.kind == ResolutionKind::Block),
            "unresolved call should produce no Block resolution, got: {:?}",
            res
        );
    }

    #[test]
    fn analyze_with_resolutions_stdlib_call_marked_stdlib() {
        let src = r#"import "@glyph/std" { subagent }

skill main()
    description: "main."
    flow:
        subagent()
"#;
        let file = parse_for_resolutions(src);
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let path = PathBuf::from("t.glyph.md");
        let (_, res) = analyze_with_resolutions(file, 0, "t.glyph.md", &path, &line_index, &mut bag, false);
        let stdlib_count = res.iter().filter(|r| r.kind == ResolutionKind::Stdlib).count();
        assert_eq!(stdlib_count, 2, "expected 2 Stdlib resolutions, got: {:?}", res);
    }

    #[test]
    fn collect_cross_file_resolutions_records_imported_block_call() {
        // Importer references an imported block by its local name.
        let src = r#"import "./repo_tools.glyph.md" { inspect_repo }

skill main()
    description: "main."
    flow:
        inspect_repo()
"#;
        let file = parse_for_resolutions(src);

        // Build a target table mirroring what `lib::check_source_with_resolutions`
        // would produce after parsing the dependency.
        let mut targets: HashMap<String, ImportTarget> = HashMap::new();
        let dep_path = PathBuf::from("/tmp/repo_tools.glyph.md");
        targets.insert(
            "inspect_repo".to_string(),
            ImportTarget {
                local_name: "inspect_repo".to_string(),
                def_file: dep_path.clone(),
                def_span: Span::new(0, 0, 64),
                kind: ResolutionKind::ExportBlock,
            },
        );

        let res = collect_cross_file_resolutions(&file, &targets);
        // Two cross-file resolutions: the import-line name token + the call.
        assert_eq!(res.len(), 2, "expected 2 cross-file resolutions, got: {:?}", res);
        // Both should point at the dep file.
        for r in &res {
            assert_eq!(r.def_file, dep_path);
        }
        let import_kind_count = res.iter().filter(|r| r.kind == ResolutionKind::Import).count();
        let block_kind_count = res
            .iter()
            .filter(|r| matches!(r.kind, ResolutionKind::Block | ResolutionKind::ExportBlock))
            .count();
        assert_eq!(import_kind_count, 1, "expected 1 Import-kind resolution");
        assert_eq!(block_kind_count, 1, "expected 1 Block/ExportBlock-kind resolution");
    }
}
