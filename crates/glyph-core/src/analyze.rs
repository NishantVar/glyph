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

use std::collections::HashSet;

use crate::ast::{ContextEntry, Decl, FlowStmt, SourceFile};
use crate::diagnostic::{DiagBag, Diagnostic, SourceSpan};
use crate::slot::scan_slots;
use crate::span::{LineIndex, Span, Spanned};

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

    for decl in &file.decls {
        match decl {
            Decl::Skill(spanned) => {
                analyze_skill(spanned, file_id, file_label, line_index, bag, &text_names, &block_names)
            }
            Decl::ExportBlock(spanned) => {
                analyze_export_block(spanned, file_label, line_index, bag);
            }
            Decl::Block(_) => {}
            Decl::Text(_) => {}
        }
    }
    file
}

fn analyze_skill(
    spanned: &Spanned<crate::ast::Skill>,
    file_id: u32,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    text_names: &HashSet<&str>,
    block_names: &HashSet<&str>,
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
                            name
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
                if !block_names.contains(target.as_str()) {
                    let span = spanned.span;
                    bag.push(
                        crate::diagnostic::Diagnostic {
                            id: "G::analyze::undefined-call".into(),
                            classification: crate::diagnostic::Classification::Repairable,
                            message: format!(
                                "call to `{}()` but no `block {}` is declared in this file",
                                target, target
                            ),
                            span: SourceSpan::from_byte_span(file_label, span, line_index),
                            related: Vec::new(),
                            hints: vec![
                                format!("declare `block {}()` or check the name for typos", target),
                            ],
                        },
                        span,
                    );
                }
            }
            FlowStmt::ConstraintMarker(marker) => {
                // Check that the constraint name resolves to a text declaration.
                if !text_names.contains(marker.name.as_str()) {
                    let span = spanned.span;
                    bag.push(
                        Diagnostic::error(
                            "G::analyze::undefined-name",
                            format!(
                                "`{}` is not a declared `text` in this file",
                                marker.name
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
        }
    }

    // Check body-level constraint name refs.
    for marker in &skill.body_constraints {
        if !text_names.contains(marker.name.as_str()) {
            let span = spanned.span;
            bag.push(
                Diagnostic::error(
                    "G::analyze::undefined-name",
                    format!(
                        "`{}` is not a declared `text` in this file",
                        marker.name
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
        if text_names.contains(name.as_str()) {
            let span = spanned.span;
            bag.push(
                crate::diagnostic::Diagnostic {
                    id: "G::analyze::ambiguous-role".into(),
                    classification: crate::diagnostic::Classification::Repairable,
                    message: format!(
                        "bare name `{}` at body level is ambiguous — add a keyword prefix (`require`/`avoid`/`must`/`context`) to clarify intent",
                        name
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
        if !text_names.contains(name.as_str()) {
            bag.push(
                Diagnostic::error(
                    "G::analyze::undefined-name",
                    format!("`{}` is not a declared `text` in this file", name),
                    SourceSpan::from_byte_span(file_label, span, line_index),
                ),
                span,
            );
        }
    }
}

fn analyze_export_block(
    spanned: &crate::span::Spanned<crate::ast::ExportBlockDecl>,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
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
}
