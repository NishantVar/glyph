//! Name-resolution table for go-to-definition.
//!
//! See `design/glyph-lsp.md` §4.4. The compiler already knows, at analyze
//! time, which `text`/`block`/`export block` declaration each identifier
//! reference resolves to — it just throws that information away after running
//! its diagnostic checks. This module replays the same matching logic over
//! the AST and exposes the result as a flat [`Resolution`] list.
//!
//! The list is the contract the LSP's `textDocument/definition` handler
//! consumes: given a cursor byte-offset, find the smallest [`Resolution`]
//! whose `use_span` contains it, then return the `def_span` (and `def_file`)
//! to the editor.
//!
//! M2 scope: same-file resolutions only. The `def_file` field is always
//! populated with the analyzing file's own path (`file_label`) for non-stdlib
//! kinds. Cross-file resolutions land in M3 — see the design doc.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::ast::{
    self, ContextEntry, Decl, FlowStmt, ReturnExpr, SourceFile,
};
use crate::span::Span;

/// A resolved name reference: where the name was used, and where it was
/// declared.
///
/// `use_span` covers the identifier token at the use-site (e.g., the bytes of
/// `validate_plan` in `validate_plan()`). `def_span` covers the declaration
/// — currently the entire decl span (which starts at the keyword like
/// `block` / `text`); the editor positions the cursor at `def_span.start`,
/// which lands on the declaration keyword.
///
/// `def_file` is populated with the analyzing file's path for same-file
/// resolutions. For [`ResolutionKind::Stdlib`] it is left empty — the LSP
/// returns `null` for stdlib jumps per design §10.D.
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

/// Build the resolution table for one parsed file. Same-file references only
/// — see module docs.
///
/// The walk is purely structural and does not emit diagnostics; the caller is
/// expected to have already run [`crate::analyze::analyze_with_diagnostics`]
/// and to keep its output. Unresolvable names produce no entry — the LSP
/// returns `null` for those (see design §7).
pub fn collect_same_file_resolutions(file: &SourceFile, file_path: &PathBuf) -> Vec<Resolution> {
    // Build name → def_span maps from the file's declarations. These mirror
    // the `text_names` / `block_names` checks in `analyze.rs`; the only
    // difference is we keep the decl's full span (rather than discarding it
    // after the membership test).
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

    let mut out: Vec<Resolution> = Vec::new();

    // Walk every use-site in the file.
    for decl in &file.decls {
        match decl {
            Decl::Skill(spanned) => {
                let skill = &spanned.node;
                walk_flow(
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
                walk_flow(
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
                // span as a Stdlib resolution. Cross-file imports (M3) would
                // record `Resolution { kind: Import, def_file: ..., ... }`.
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

fn walk_flow(
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
                walk_flow(then_body, file_path, text_defs, block_defs, export_block_defs, stdlib_names, out);
                for elif in elif_branches {
                    walk_flow(&elif.body, file_path, text_defs, block_defs, export_block_defs, stdlib_names, out);
                }
                if let Some(eb) = else_body {
                    walk_flow(eb, file_path, text_defs, block_defs, export_block_defs, stdlib_names, out);
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
    target: &crate::span::Spanned<String>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use crate::span::LineIndex;

    fn parse_file(source: &str) -> SourceFile {
        let (file, _) = parse::parse(source, 0).expect("parse");
        file
    }

    #[test]
    fn resolves_block_call_target() {
        let src = r#"skill main()
    description: "main."
    flow:
        validate_plan()

block validate_plan()
    "Check the plan."
"#;
        let file = parse_file(src);
        let res = collect_same_file_resolutions(&file, &PathBuf::from("test.glyph.md"));
        let block_res = res.iter().find(|r| r.kind == ResolutionKind::Block);
        assert!(block_res.is_some(), "expected a Block resolution, got: {:?}", res);
        let r = block_res.unwrap();
        // use_span should cover `validate_plan` at the call-site (under flow:).
        let use_text = &src[r.use_span.start as usize..r.use_span.end as usize];
        assert_eq!(use_text, "validate_plan");
        // def_span starts at the `block` keyword.
        let def_text = &src[r.def_span.start as usize..r.def_span.start as usize + 5];
        assert_eq!(def_text, "block");
    }

    #[test]
    fn resolves_text_constraint() {
        let src = r#"skill main()
    description: "main."
    require accuracy
    flow:
        "Do something."

text accuracy = "Be accurate."
"#;
        let file = parse_file(src);
        let res = collect_same_file_resolutions(&file, &PathBuf::from("t.glyph.md"));
        let text_res = res.iter().find(|r| r.kind == ResolutionKind::Text);
        assert!(text_res.is_some(), "expected a Text resolution, got: {:?}", res);
        let r = text_res.unwrap();
        let use_text = &src[r.use_span.start as usize..r.use_span.end as usize];
        assert_eq!(use_text, "accuracy");
    }

    #[test]
    fn unresolved_call_produces_no_resolution() {
        let src = r#"skill main()
    description: "main."
    flow:
        no_such_block()
"#;
        let file = parse_file(src);
        let res = collect_same_file_resolutions(&file, &PathBuf::from("t.glyph.md"));
        assert!(
            !res.iter().any(|r| r.kind == ResolutionKind::Block),
            "unresolved call should produce no Block resolution, got: {:?}",
            res
        );
    }

    #[test]
    fn stdlib_call_marked_stdlib() {
        let src = r#"import "@glyph/std" { subagent }

skill main()
    description: "main."
    flow:
        subagent()
"#;
        let file = parse_file(src);
        let _ = LineIndex::new(src);
        let res = collect_same_file_resolutions(&file, &PathBuf::from("t.glyph.md"));
        // Two stdlib resolutions expected: the import-line `subagent` token
        // and the call-site `subagent` token.
        let stdlib_count = res.iter().filter(|r| r.kind == ResolutionKind::Stdlib).count();
        assert_eq!(stdlib_count, 2, "expected 2 Stdlib resolutions, got: {:?}", res);
    }
}
