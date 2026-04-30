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

use crate::ast::{BlockDecl, ContextEntry, Decl, FlowStmt, SourceFile};
use crate::diagnostic::{Classification, DiagBag, Diagnostic, SourceSpan};
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
                analyze_skill(spanned, file_id, file_label, line_index, bag, &text_names, &block_names, &block_decls)
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
) {
    // Run the normal analysis.
    analyze_skill(spanned, file_id, file_label, line_index, bag, text_names, block_names, block_decls);

    // Track usage: walk flow/constraints/context to see which imported names are referenced.
    let skill = &spanned.node;

    // Check constraint markers.
    for marker in &skill.body_constraints {
        if imported_texts.contains(&marker.name) {
            used_import_names.insert(marker.name.clone());
        }
    }

    // Check context entries.
    for entry in &skill.body_context {
        if let crate::ast::ContextEntry::NameRef(name) = entry {
            if imported_texts.contains(name) {
                used_import_names.insert(name.clone());
            }
        }
    }
    for entry in &skill.context_section {
        if let crate::ast::ContextEntry::NameRef(name) = entry {
            if imported_texts.contains(name) {
                used_import_names.insert(name.clone());
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
                if imported_blocks.contains(target) {
                    used.insert(target.clone());
                }
            }
            crate::ast::FlowStmt::ConstraintMarker(marker) => {
                if imported_texts.contains(&marker.name) {
                    used.insert(marker.name.clone());
                }
            }
            crate::ast::FlowStmt::ContextMarker(entry) => {
                if let crate::ast::ContextEntry::NameRef(name) = entry {
                    if imported_texts.contains(name) {
                        used.insert(name.clone());
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
                    &text_names, &block_names, &block_decls,
                );
                // Check elif conditions too.
                for elif in elif_branches {
                    check_applies_in_condition(
                        &elif.condition, spanned.span, file_id, file_label, line_index, bag,
                        &text_names, &block_names, &block_decls,
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

    // --- Effect inference and validation ---
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
            FlowStmt::Call { target, .. } => Some(target.clone()),
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
                    worklist.push(inner.clone());
                }
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
                } else {
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
                if !block_names.contains(target.as_str()) {
                    bag.push(
                        Diagnostic {
                            id: "G::analyze::undefined-call".into(),
                            classification: Classification::Repairable,
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
                if !text_names.contains(marker.name.as_str()) {
                    bag.push(
                        Diagnostic::error(
                            "G::analyze::undefined-name",
                            format!("`{}` is not a declared `text` in this file", marker.name),
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
}
