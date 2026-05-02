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

/// Issue #83 AC2 + AC3: warn when a header `-> DomainType` annotation names
/// a banned generic type. Warning tier — non-blocking; analyze continues so
/// every banned occurrence in the file gets flagged. No-op when the
/// annotation is absent. Used by every header-bearing decl site
/// (skill / export block / private block, with and without imports).
///
/// Two side-effects, co-located at the single point where `-> DomainType` is
/// processed: (1) emit the banned-generic warning when the name is on the
/// banned list (issue #83), and (2) on the legitimate-domain-type path
/// (issue #84 Chunk 2), record the identifier in the per-file registry under
/// its canonical key so first-use spans are recoverable downstream. Banned
/// names do NOT register (AC1). Helper name kept for surgical-changes
/// reasons; cosmetic rename can land later.
fn warn_if_banned_return_type(
    rt: Option<&Spanned<String>>,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    registry: &mut crate::domain_registry::Registry,
) {
    let Some(rt) = rt else { return };
    match crate::type_position::validate_type_position(&rt.node) {
        Err(w) => {
            bag.push(
                Diagnostic {
                    id: w.id.into(),
                    classification: Classification::Warning,
                    message: w.message,
                    span: SourceSpan::from_byte_span(file_label, rt.span, line_index),
                    related: Vec::new(),
                    hints: vec![w.hint],
                },
                rt.span,
            );
        }
        Ok(_) => {
            // Issue #84 codex pass 1 — F2: skip registration when the name
            // is a built-in `TypeTag` (per `kind_infer.rs`). Of the six
            // built-ins (`String`, `Int`, `Float`, `Bool`, `None`, `Agent`)
            // all but `Agent` are also on #83's banned-generic list and so
            // never reach this `Ok` arm; `Agent` is the only one that today
            // escapes the banned filter and would otherwise be registered
            // as a domain type, falsely colliding with an `agent` parameter
            // via chunk-3's no-shadowing sweep. Filter all six here so a
            // future change to the banned-list does not silently re-expose
            // any built-in.
            if is_builtin_type_name(&rt.node) {
                return;
            }
            // Issue #84 Chunk 2: legitimate domain-type name → record first
            // use. Idempotent on canonical form; subsequent same-canonical
            // calls preserve the original `first_use_span`.
            registry.register_first_use(&rt.node, rt.span);
        }
    }
}

/// Issue #84 codex pass 1 — F2: predicate matching the six built-in
/// `TypeTag` names per `kind_infer.rs`. Case-insensitive ASCII match per
/// F12 / canonical-form rule. Used by `warn_if_banned_return_type` to
/// keep built-ins out of the per-file domain-type registry, and by
/// `check_return_call_nominal` could call this in the future if the
/// banned-list ever ceases to cover the same set.
fn is_builtin_type_name(s: &str) -> bool {
    const BUILTINS: &[&str] = &["String", "Int", "Float", "Bool", "None", "Agent"];
    BUILTINS.iter().any(|b| b.eq_ignore_ascii_case(s))
}

/// Issue #84 Chunk 3 (AC5): post-hoc sweep that flags any domain-type name
/// (registered via `-> DomainType` on a header) that collides — after
/// canonicalization (D6) — with a parameter or `const` declaration in the
/// same file. Emits `G::analyze::name-collision` Error per collision; the
/// primary span pins the `-> Type` annotation that introduced the type, the
/// related span pins the offending param / const.
///
/// File-level scope (not per-decl): a type registered on one decl can collide
/// with a param on a different decl, since the `-> Type` annotation puts the
/// name in scope across the whole file. Banned-generic names (#83) skip
/// registration (D8) and so cannot collide via this path.
///
/// D10 scope-defer: type-vs-import collisions are out of scope for this
/// chunk; only param and const collisions are emitted here.
fn sweep_name_collisions(
    file: &SourceFile,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    registry: &crate::domain_registry::Registry,
) {
    if registry.iter().next().is_none() {
        return;
    }

    // Collect every parameter (across all decl kinds) and every const at
    // file level, paired with the span we want pinned in the `related`
    // field of the collision diagnostic.
    let mut params: Vec<(&str, Span)> = Vec::new();
    let mut consts: Vec<(&str, Span)> = Vec::new();
    for decl in &file.decls {
        match decl {
            Decl::Skill(s) => {
                for p in &s.node.params {
                    params.push((p.name.as_str(), p.span));
                }
            }
            Decl::ExportBlock(e) => {
                for p in &e.node.params {
                    params.push((p.name.as_str(), p.span));
                }
            }
            Decl::Block(b) => {
                for p in &b.node.params {
                    params.push((p.name.as_str(), p.span));
                }
            }
            Decl::Const(c) => {
                consts.push((c.node.name.as_str(), c.span));
            }
            Decl::Import(_) => {}
        }
    }

    for entry in registry.iter() {
        for (param_raw, param_span) in &params {
            if crate::domain_registry::canonicalize_identifier(param_raw)
                == entry.canonical_name
            {
                emit_name_collision(
                    "parameter",
                    entry,
                    param_raw,
                    *param_span,
                    file_label,
                    line_index,
                    bag,
                );
            }
        }
        for (const_raw, const_span) in &consts {
            if crate::domain_registry::canonicalize_identifier(const_raw)
                == entry.canonical_name
            {
                emit_name_collision(
                    "const",
                    entry,
                    const_raw,
                    *const_span,
                    file_label,
                    line_index,
                    bag,
                );
            }
        }
    }
}

/// Construct and push one `G::analyze::name-collision` Error diagnostic.
///
/// `kind` is the human-readable noun for the offending site (`"parameter"`
/// or `"const"`). `entry.raw_first_use` is what the author wrote at the
/// first `-> Type` annotation; `offender_raw` is the param/const spelling.
/// The `Diagnostic::error` constructor seeds an empty `related` vec, which
/// we then populate in-place — this mirrors the existing convention in
/// `analyze.rs` (no `with_related` builder method exists in `diagnostic.rs`).
fn emit_name_collision(
    kind: &str,
    entry: &crate::domain_registry::RegistryEntry,
    offender_raw: &str,
    offender_span: Span,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    let primary = SourceSpan::from_byte_span(file_label, entry.first_use_span, line_index);
    let related = SourceSpan::from_byte_span(file_label, offender_span, line_index);
    let mut diag = Diagnostic::error(
        "G::analyze::name-collision",
        format!(
            "domain type `{}` collides with {} `{}`",
            entry.raw_first_use, kind, offender_raw
        ),
        primary,
    );
    diag.related.push(related);
    bag.push(diag, entry.first_use_span);
}

/// Issue #84 Chunk 4 (AC4 / D14): emit `G::analyze::nominal-mismatch` Error
/// at a return-position call boundary when the callee's declared `-> Type`
/// does not canonical-match the enclosing callable's declared `-> Type`.
///
/// `primary_span` is the enclosing decl's span — synthetic-fallback option
/// (3) per `design/diagnostics.md` §Span Semantics. The AST has no
/// per-statement span (`flow: Vec<FlowStmt>`, not `Vec<Spanned<FlowStmt>>`;
/// `FlowStmt` itself has no span field), so we cannot pin the actual
/// `return foo()` line. `related_span` is the enclosing callable's
/// `-> Type` annotation — the contract being violated (D14).
///
/// Parallel to [`emit_nominal_mismatch`] (placeholder — analyze.rs ~1207):
/// the placeholder predates this work and its lone unit test does not
/// exercise the `related` path; left untouched per surgical-changes
/// principle. A future codex-pass cleanup may fold the two into one helper.
fn emit_nominal_mismatch_at_return(
    call_target: &str,
    expected_type_raw: &str,
    actual_type_raw: &str,
    primary_span: Span,
    related_span: Span,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    let primary = SourceSpan::from_byte_span(file_label, primary_span, line_index);
    let related = SourceSpan::from_byte_span(file_label, related_span, line_index);
    let mut diag = Diagnostic::error(
        "G::analyze::nominal-mismatch",
        format!(
            "type mismatch at call boundary for `{}`: expected `{}`, got `{}`",
            call_target, expected_type_raw, actual_type_raw
        ),
        primary,
    );
    diag.related.push(related);
    bag.push(diag, primary_span);
}

/// Issue #84 Chunk 4 (AC4 / D13, D16): single-statement nominal check.
///
/// Inspect one `FlowStmt`. If it is a `Return(Call { target })` and the
/// enclosing callable declares `-> Type`, look up the callee's `-> Type`
/// (local first, then imports), and emit `G::analyze::nominal-mismatch`
/// when the canonical forms differ. Untyped caller / untyped callee /
/// undefined callee → no check, no diagnostic (`types.md` line 67-76).
///
/// Shared by the skill flow walk in `analyze_skill` and the BlockDecl
/// flow walk in [`check_block_return_calls`]. `decl_span` is the
/// enclosing callable's declaration span (D14 primary, synthetic
/// fallback option 3 per `design/diagnostics.md` §Span Semantics).
fn check_return_call_nominal(
    caller_return_type: Option<&Spanned<String>>,
    stmt: &FlowStmt,
    decl_span: Span,
    registry: &crate::domain_registry::Registry,
    local_callee_return_types: &HashMap<&str, &Spanned<String>>,
    imported_block_return_types: &HashMap<String, Spanned<String>>,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    let (Some(caller_rt), FlowStmt::Return(crate::ast::ReturnExpr::Call { target, .. })) =
        (caller_return_type, stmt)
    else {
        return;
    };
    let callee_rt = local_callee_return_types
        .get(target.as_str())
        .copied()
        .or_else(|| imported_block_return_types.get(target.as_str()));
    let Some(callee_rt) = callee_rt else { return };
    // Issue #84 codex pass 1 — F1: skip the nominal-match check when
    // either side's type name is on the #83 banned-generic list. The
    // banned warning (`G::analyze::generic-type-name`) is the user-
    // visible signal for those names; canonical-equality against a
    // legitimate domain type would fire `nominal-mismatch` (Error,
    // exit 1) on top of the warning and thus silently upgrade a
    // non-blocking issue into a build-breaking one. Banned names
    // carry no domain semantics, so a mismatch verdict is meaningless
    // either way.
    if crate::type_position::validate_type_position(&caller_rt.node).is_err()
        || crate::type_position::validate_type_position(&callee_rt.node).is_err()
    {
        return;
    }
    if registry.nominal_match(&caller_rt.node, &callee_rt.node) {
        return;
    }
    emit_nominal_mismatch_at_return(
        target,
        &caller_rt.node,
        &callee_rt.node,
        decl_span,
        caller_rt.span,
        file_label,
        line_index,
        bag,
    );
}

/// Issue #84 Chunk 4 (AC4 / D13, D16): walk a `BlockDecl`'s `flow:` for
/// `return foo()` statements and delegate each to
/// [`check_return_call_nominal`]. ExportBlockDecl is deliberately not
/// handled — its AST has no `flow: Vec<FlowStmt>`, so cross-file
/// ExportBlock-as-caller is deferred per AST limitation (D16).
fn check_block_return_calls(
    block: &BlockDecl,
    decl_span: Span,
    registry: &crate::domain_registry::Registry,
    local_callee_return_types: &HashMap<&str, &Spanned<String>>,
    imported_block_return_types: &HashMap<String, Spanned<String>>,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    walk_return_calls_nominal_check(
        &block.flow,
        block.return_type.as_ref(),
        decl_span,
        registry,
        local_callee_return_types,
        imported_block_return_types,
        file_label,
        line_index,
        bag,
    );
}

/// Issue #84 codex pass 2 — F1: recursive nominal walker.
///
/// Walks `flow` and runs [`check_return_call_nominal`] on every
/// `FlowStmt::Return`, recursing into `FlowStmt::Branch` bodies (then-arm,
/// each elif-arm, optional else-arm) so nested returns are not missed. Pre-
/// fix, both `analyze_skill::FlowStmt::Branch` and `check_block_return_calls`
/// iterated only the top-level `flow` slice; a `return foo()` inside an
/// `if`/`elif`/`else` body silently bypassed the chunk-4 mismatch check.
///
/// Side note (orthogonal): `G::parse::return-in-branch` is already a parse-
/// time error against return-inside-branch; this walker exists so the
/// invariant "every Return in flow is checked for nominal match" holds
/// regardless of the parse-rule's future evolution and so that authors who
/// see both diagnostics get the more precise type signal alongside the
/// structural one.
#[allow(clippy::too_many_arguments)]
fn walk_return_calls_nominal_check(
    flow: &[FlowStmt],
    caller_return_type: Option<&Spanned<String>>,
    decl_span: Span,
    registry: &crate::domain_registry::Registry,
    local_callee_return_types: &HashMap<&str, &Spanned<String>>,
    imported_block_return_types: &HashMap<String, Spanned<String>>,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    for stmt in flow {
        match stmt {
            FlowStmt::Return(_) => {
                check_return_call_nominal(
                    caller_return_type,
                    stmt,
                    decl_span,
                    registry,
                    local_callee_return_types,
                    imported_block_return_types,
                    file_label,
                    line_index,
                    bag,
                );
            }
            FlowStmt::Branch { then_body, elif_branches, else_body, .. } => {
                walk_return_calls_nominal_check(
                    then_body,
                    caller_return_type,
                    decl_span,
                    registry,
                    local_callee_return_types,
                    imported_block_return_types,
                    file_label,
                    line_index,
                    bag,
                );
                for elif in elif_branches {
                    walk_return_calls_nominal_check(
                        &elif.body,
                        caller_return_type,
                        decl_span,
                        registry,
                        local_callee_return_types,
                        imported_block_return_types,
                        file_label,
                        line_index,
                        bag,
                    );
                }
                if let Some(eb) = else_body {
                    walk_return_calls_nominal_check(
                        eb,
                        caller_return_type,
                        decl_span,
                        registry,
                        local_callee_return_types,
                        imported_block_return_types,
                        file_label,
                        line_index,
                        bag,
                    );
                }
            }
            _ => {}
        }
    }
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
    registry: &mut crate::domain_registry::Registry,
) -> SourceFile {
    // Collect value-binding names for bare-name detection in flow. Post-#81,
    // `const` is the sole value-binding form; the variable name `text_names`
    // is retained to keep diagnostic IDs (`G::analyze::text-in-flow`) and
    // their messages aligned with the legacy term — a doc-only renaming is
    // out of scope for #81.
    let text_names: HashSet<&str> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Const(c) => Some(c.node.name.as_str()),
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
    // A `generated const` has `exported == false`, so it is captured here as
    // a private binding (correct: generated consts are file-private by spec).
    let private_names: HashSet<&str> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Const(c) if !c.node.exported => Some(c.node.name.as_str()),
            Decl::Block(b) => Some(b.node.name.as_str()),
            _ => None,
        })
        .collect();

    // Issue #84 Chunk 4 (AC4 / D13): per-file local-callee return-type map.
    // Issue #84 codex pass 1 — F3: only `Decl::Block` is recognized by the
    // same-file call resolver (`block_names` is `Decl::Block`-only). Including
    // `Decl::ExportBlock` here caused a false hard `nominal-mismatch` to fire
    // against `return exported_fn()` boundaries that the resolver would
    // otherwise reject. Cross-file matching for `export block` callees is
    // owned by the `imported_block_return_types` map, sourced from
    // `extract_exports::block_return_types` (lib.rs:216) — restricting this
    // local map does not break cross-file matching. `Decl::Skill` stays out
    // because skills cannot be called from other declarations' flow.
    // The no-imports path has no cross-file map, so we pass an empty one.
    let local_callee_return_types: HashMap<&str, &Spanned<String>> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Block(b) => b.node.return_type.as_ref().map(|rt| (b.node.name.as_str(), rt)),
            _ => None,
        })
        .collect();
    let empty_imported_block_return_types: HashMap<String, Spanned<String>> = HashMap::new();

    for decl in &file.decls {
        match decl {
            Decl::Skill(spanned) => {
                analyze_skill(
                    spanned, file_id, file_label, line_index, bag, registry,
                    &text_names, &block_names, &block_decls, &HashMap::new(),
                    &local_callee_return_types, &empty_imported_block_return_types,
                )
            }
            Decl::ExportBlock(spanned) => {
                analyze_export_block(spanned, file_label, line_index, bag, registry, &private_names);
            }
            Decl::Block(spanned) => {
                // Issue #83 AC2 + AC3 (D7: private blocks in scope): warn on
                // banned generic type names in the header `-> DomainType`.
                warn_if_banned_return_type(
                    spanned.node.return_type.as_ref(),
                    file_label,
                    line_index,
                    bag,
                    registry,
                );

                // Issue #84 Chunk 4 (AC4 / D13): a `block` may itself be a
                // caller via a `return foo()`. Mirror the skill arm — walk
                // `flow` for `FlowStmt::Return(ReturnExpr::Call)` and check
                // the callee's `-> Type` against this block's caller `-> Type`.
                // ExportBlock-as-caller is deferred per AST limitation
                // (no `flow: Vec<FlowStmt>` on ExportBlockDecl — D16).
                check_block_return_calls(
                    &spanned.node,
                    spanned.span,
                    registry,
                    &local_callee_return_types,
                    &empty_imported_block_return_types,
                    file_label,
                    line_index,
                    bag,
                );
            }
            Decl::Const(_) => {}
            Decl::Import(_) => {}
        }
    }

    // G::analyze::name-collision — duplicate export names.
    {
        let mut seen_exports: HashMap<&str, Span> = HashMap::new();
        for decl in &file.decls {
            let (name, span) = match decl {
                Decl::ExportBlock(b) => (b.node.name.as_str(), b.span),
                Decl::Const(c) if c.node.exported => (c.node.name.as_str(), c.span),
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

    // Issue #84 Chunk 3 (AC5): domain-type-vs-param/const collision sweep.
    sweep_name_collisions(&file, file_label, line_index, bag, registry);

    // Library detection: file with zero skills.
    let has_skill = file.decls.iter().any(|d| matches!(d, Decl::Skill(_)));
    if !has_skill {
        let has_export = file.decls.iter().any(|d| {
            matches!(d, Decl::ExportBlock(_))
                || matches!(d, Decl::Const(c) if c.node.exported)
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
    imported_block_descriptions: &HashMap<String, String>,
    registry: &mut crate::domain_registry::Registry,
    imported_block_return_types: &HashMap<String, Spanned<String>>,
) -> SourceFile {
    // Collect local value-binding names (post-#81: `const` is the sole form;
    // the `local_text_names` variable name is kept for parity with the legacy
    // diagnostic vocabulary — see `analyze_with_diagnostics` notes).
    let local_text_names: HashSet<&str> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Const(c) => Some(c.node.name.as_str()),
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
            Decl::Const(c) if !c.node.exported => Some(c.node.name.as_str()),
            Decl::Block(b) => Some(b.node.name.as_str()),
            _ => None,
        })
        .collect();

    // Issue #84 Chunk 4 (AC4 / D13): per-file local-callee return-type map.
    // Issue #84 codex pass 1 — F3: see the matching site in
    // `analyze_with_diagnostics` for rationale. Restricted to `Decl::Block`
    // — the only kind recognized by the same-file resolver. Cross-file
    // export-block matching is owned by `imported_block_return_types`.
    // Keyed by callable name; valued by the `-> Type` annotation. Populated
    // for callables that declare a return type only — absence means
    // "skip the type-check" (covers undefined-callee and untyped-callee).
    // The borrowed-string keys tie this map's lifetime to the file AST,
    // same pattern as `block_decls`.
    let local_callee_return_types: HashMap<&str, &Spanned<String>> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Block(b) => b.node.return_type.as_ref().map(|rt| (b.node.name.as_str(), rt)),
            _ => None,
        })
        .collect();

    for decl in &file.decls {
        match decl {
            Decl::Skill(spanned) => {
                analyze_skill_with_usage_tracking(
                    spanned, file_id, file_label, line_index, bag, registry,
                    &text_names, &block_names, &block_decls,
                    imported_texts, imported_blocks, used_import_names,
                    imported_block_descriptions,
                    &local_callee_return_types, imported_block_return_types,
                );
            }
            Decl::ExportBlock(spanned) => {
                analyze_export_block(spanned, file_label, line_index, bag, registry, &private_names);
            }
            Decl::Block(spanned) => {
                // Issue #83 AC2 + AC3 (D7: private blocks in scope): warn on
                // banned generic type names in the header `-> DomainType`.
                // Imports-path parity with `analyze_with_diagnostics`.
                warn_if_banned_return_type(
                    spanned.node.return_type.as_ref(),
                    file_label,
                    line_index,
                    bag,
                    registry,
                );

                // Issue #84 Chunk 4 (AC4 / D13, D16): BlockDecl-as-caller
                // walk on the imports path. ExportBlock-as-caller deferred
                // per AST limitation (no `flow: Vec<FlowStmt>` on
                // ExportBlockDecl).
                check_block_return_calls(
                    &spanned.node,
                    spanned.span,
                    registry,
                    &local_callee_return_types,
                    imported_block_return_types,
                    file_label,
                    line_index,
                    bag,
                );
            }
            Decl::Const(_) | Decl::Import(_) => {}
        }
    }

    // G::analyze::name-collision — duplicate export names.
    {
        let mut seen_exports: HashMap<&str, Span> = HashMap::new();
        for decl in &file.decls {
            let (name, span) = match decl {
                Decl::ExportBlock(b) => (b.node.name.as_str(), b.span),
                Decl::Const(c) if c.node.exported => (c.node.name.as_str(), c.span),
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

    // Issue #84 Chunk 3 (AC5): domain-type-vs-param/const collision sweep.
    // Imports-path parity with `analyze_with_diagnostics`.
    sweep_name_collisions(file, file_label, line_index, bag, registry);

    // Library detection: file with zero skills.
    let has_skill = file.decls.iter().any(|d| matches!(d, Decl::Skill(_)));
    if !has_skill {
        let has_export = file.decls.iter().any(|d| {
            matches!(d, Decl::ExportBlock(_))
                || matches!(d, Decl::Const(c) if c.node.exported)
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
    registry: &mut crate::domain_registry::Registry,
    text_names: &HashSet<&str>,
    block_names: &HashSet<&str>,
    block_decls: &HashMap<&str, &crate::ast::BlockDecl>,
    imported_texts: &HashSet<String>,
    imported_blocks: &HashSet<String>,
    used_import_names: &mut HashSet<String>,
    imported_block_descriptions: &HashMap<String, String>,
    local_callee_return_types: &HashMap<&str, &Spanned<String>>,
    imported_block_return_types: &HashMap<String, Spanned<String>>,
) {
    // Run the normal analysis.
    analyze_skill(
        spanned, file_id, file_label, line_index, bag, registry,
        text_names, block_names, block_decls, imported_block_descriptions,
        local_callee_return_types, imported_block_return_types,
    );

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
            // Issue #84 Chunk 7a: a `return imported_block()` consumes the
            // imported name in return position; before this arm it fell into
            // the catch-all `_` and `unused-import` fired spuriously, blocking
            // AC8's exit-0 success contract for cross-file return-position
            // consumers.
            crate::ast::FlowStmt::Return(crate::ast::ReturnExpr::Call { target, .. }) => {
                if imported_blocks.contains(target) {
                    used.insert(target.clone());
                }
            }
            // Symmetric to `ContextMarker(NameRef)` above (L753-758): a
            // `return <name>` reference may resolve to either an imported text
            // const or an imported block, so check both pools.
            crate::ast::FlowStmt::Return(crate::ast::ReturnExpr::Name(name)) => {
                if imported_blocks.contains(name) || imported_texts.contains(name) {
                    used.insert(name.clone());
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
    registry: &mut crate::domain_registry::Registry,
    text_names: &HashSet<&str>,
    block_names: &HashSet<&str>,
    block_decls: &HashMap<&str, &BlockDecl>,
    imported_block_descriptions: &HashMap<String, String>,
    local_callee_return_types: &HashMap<&str, &Spanned<String>>,
    imported_block_return_types: &HashMap<String, Spanned<String>>,
) {
    let skill = &spanned.node;
    let declared: HashSet<&str> = skill.params.iter().map(|p| p.name.as_str()).collect();

    // Issue #83 AC2 + AC3: warn on banned generic type names in the
    // header `-> DomainType` annotation. Warning tier — non-blocking;
    // analyze continues so all banned occurrences in the file get flagged.
    warn_if_banned_return_type(skill.return_type.as_ref(), file_label, line_index, bag, registry);

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
                    // Check if this is a stdlib name used without import.
                    if is_stdlib_block_name(target) {
                        let span = spanned.span;
                        bag.push(
                            crate::diagnostic::Diagnostic {
                                id: "G::analyze::stdlib-missing-import".into(),
                                classification: crate::diagnostic::Classification::Repairable,
                                message: format!(
                                    "`{}` is a standard library block; add `import \"@glyph/std\" {{ {} }}`",
                                    target, target
                                ),
                                span: SourceSpan::from_byte_span(file_label, span, line_index),
                                related: Vec::new(),
                                hints: vec![
                                    format!("add `import \"@glyph/std\" {{ {} }}` at the top of the file", target),
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
                // (check_return_rules). Issue #84 Chunk 4 (AC4 / D13):
                // delegate the cross-/same-file nominal-mismatch check to
                // the shared helper used by the BlockDecl-as-caller walk.
                check_return_call_nominal(
                    skill.return_type.as_ref(),
                    stmt,
                    spanned.span,
                    registry,
                    local_callee_return_types,
                    imported_block_return_types,
                    file_label,
                    line_index,
                    bag,
                );
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
                // Issue #84 codex pass 2 — F1: recurse into branch bodies so
                // a `return foo()` nested inside `if`/`elif`/`else` runs the
                // chunk-4 nominal-mismatch check. Pre-fix this arm only ran
                // structural/name checks; the type check was lost.
                walk_return_calls_nominal_check(
                    then_body,
                    skill.return_type.as_ref(),
                    spanned.span,
                    registry,
                    local_callee_return_types,
                    imported_block_return_types,
                    file_label,
                    line_index,
                    bag,
                );
                for elif in elif_branches {
                    walk_return_calls_nominal_check(
                        &elif.body,
                        skill.return_type.as_ref(),
                        spanned.span,
                        registry,
                        local_callee_return_types,
                        imported_block_return_types,
                        file_label,
                        line_index,
                        bag,
                    );
                }
                if let Some(eb) = else_body {
                    walk_return_calls_nominal_check(
                        eb,
                        skill.return_type.as_ref(),
                        spanned.span,
                        registry,
                        local_callee_return_types,
                        imported_block_return_types,
                        file_label,
                        line_index,
                        bag,
                    );
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

    // G::analyze::empty-skill-body — skill with no description, no flow, no
    // constraints, no effects. A skill must have at least one of flow (with
    // statements) or constraints (with markers) to be projectable.
    if skill.description.is_none()
        && skill.flow.is_empty()
        && skill.body_constraints.is_empty()
        && skill.effects.is_empty()
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
                if !block_names.contains(target.as_str()) {
                    if is_stdlib_block_name(target) {
                        bag.push(
                            Diagnostic {
                                id: "G::analyze::stdlib-missing-import".into(),
                                classification: Classification::Repairable,
                                message: format!(
                                    "`{}` is a standard library block; add `import \"@glyph/std\" {{ {} }}`",
                                    target, target
                                ),
                                span: SourceSpan::from_byte_span(file_label, span, line_index),
                                related: Vec::new(),
                                hints: vec![
                                    format!("add `import \"@glyph/std\" {{ {} }}` at the top of the file", target),
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
    registry: &mut crate::domain_registry::Registry,
    private_names: &HashSet<&str>,
) {
    let decl = &spanned.node;

    // Issue #83 AC2 + AC3: warn on banned generic type names in the
    // header `-> DomainType` annotation. Warning tier — non-blocking.
    warn_if_banned_return_type(decl.return_type.as_ref(), file_label, line_index, bag, registry);

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

    // G::analyze::export-missing-return-type — issue #82 AC2: an export block
    // that returns a meaningful value (a `return <expr>` where `<expr>` is
    // not the `none` value-keyword) must declare its return type with a
    // `-> DomainType` annotation on the header. The reverse direction
    // (`-> DomainType` declared but no meaningful return) is intentionally
    // out of scope per #82 — `missing-return` already covers total absence
    // of `return`.
    if decl.has_meaningful_return && decl.return_type.is_none() {
        let span = spanned.span;
        bag.push(
            Diagnostic {
                id: "G::analyze::export-missing-return-type".into(),
                classification: Classification::Repairable,
                message: format!(
                    "`export block {}` returns a meaningful value but its header lacks a `-> DomainType` annotation",
                    decl.name
                ),
                span: SourceSpan::from_byte_span(file_label, span, line_index),
                related: Vec::new(),
                hints: vec![
                    "add a return-type annotation to the header — e.g. `export block name(...) -> DomainType`".into(),
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

    // --- Issue #84 Chunk 2: domain-type registry wired into analyze ---

    #[test]
    fn t1_skill_return_type_registers_in_registry() {
        // Tracer: a skill header with a legitimate `-> Report` populates
        // the per-file Registry under canonical key `report`. The entry's
        // `first_use_span` matches the parser's `return_type.span`, which
        // covers the whole `-> Report` annotation (start at `->`, end at
        // the identifier's end) — see `Parser::try_parse_return_type`.
        let src = "skill foo() -> Report\n    description: \"Foo.\"\n    flow:\n        \"do\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);
        let entry = registry.lookup("Report").expect("`Report` must be registered");
        assert_eq!(entry.canonical_name, "report");
        let arrow_start = src.find("->").unwrap() as u32;
        let report_end = (src.find("Report").unwrap() + "Report".len()) as u32;
        assert_eq!(entry.first_use_span.start, arrow_start);
        assert_eq!(entry.first_use_span.end, report_end);
        assert_eq!(entry.first_use_span.file_id, 0);
    }

    #[test]
    fn t2_export_block_return_type_registers_in_registry() {
        // Export-block visit site populates the registry the same way the
        // skill site does. Pinpoints the export-block branch of the match.
        let src = "export block bar(x = \"d\") -> Report\n    flow:\n        \"x\"\n        return x\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);
        let entry = registry.lookup("Report").expect("`Report` must be registered");
        assert_eq!(entry.canonical_name, "report");
    }

    #[test]
    fn t3_private_block_return_type_registers_no_imports_path() {
        // Private `block` visit site (no-imports analyze entry) populates
        // the registry. D7: private blocks are in scope for header
        // `-> DomainType` handling.
        let src = "skill main()\n    description: \"Main.\"\n    flow:\n        helper()\n\nblock helper() -> Report\n    description: \"Helper.\"\n    flow:\n        \"work\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);
        let entry = registry.lookup("Report").expect("`Report` must be registered from private block");
        assert_eq!(entry.canonical_name, "report");
    }

    #[test]
    fn t4_private_block_return_type_registers_imports_path() {
        // Imports-path parity with T3: when analyze runs through
        // `analyze_with_imports` (the path used for files that import other
        // files), the private-block branch must also register.
        let src = "skill main()\n    description: \"Main.\"\n    flow:\n        helper()\n\nblock helper() -> Report\n    description: \"Helper.\"\n    flow:\n        \"work\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let mut used: HashSet<String> = HashSet::new();
        let _ = analyze_with_imports(
            &file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &HashSet::new(),
            &HashSet::new(),
            &mut used,
            &HashMap::new(),
            &mut registry,
            &HashMap::new(),
        );
        let entry = registry.lookup("Report").expect("`Report` must be registered (imports path)");
        assert_eq!(entry.canonical_name, "report");
    }

    #[test]
    fn t5_two_decls_same_spelling_preserves_first_use_span() {
        // Two decls both `-> Report`. Registry has one entry; `first_use_span`
        // matches the *first* decl's annotation span — the second is silently
        // discarded (AC3 first-use semantics, surfacing through analyze).
        let src = "skill foo() -> Report\n    description: \"Foo.\"\n    flow:\n        \"do\"\n\nexport block bar(x = \"d\") -> Report\n    flow:\n        \"x\"\n        return x\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);
        let entry = registry.lookup("Report").expect("`Report` must be registered");
        // First `-> Report` is on the skill header; second is on the export
        // block. The registry must surface the *first* (skill) annotation's
        // span, not the second.
        let first_arrow = src.find("->").unwrap() as u32;
        let first_report_end = (src.find("Report").unwrap() + "Report".len()) as u32;
        assert_eq!(entry.first_use_span.start, first_arrow);
        assert_eq!(entry.first_use_span.end, first_report_end);
    }

    #[test]
    fn t5b_two_decls_cross_spelling_canonicalize_first_span_wins() {
        // Cross-spelling first-use: first decl `-> Report`, second `-> report`.
        // Per D6, both canonicalize to `report` and share one registry entry.
        // Lookup by either spelling hits; `canonical_name == "report"` (the
        // canonicalized form, never raw); `first_use_span` matches the
        // *first* (`Report`) decl, not the second (`report`).
        //
        // Why this matters: catches a regression where analyze re-canonicalizes
        // raw text before passing into the registry — both inputs would already
        // match in that bug, so T5 alone wouldn't notice.
        let src = "skill foo() -> Report\n    description: \"Foo.\"\n    flow:\n        \"do\"\n\nblock bar() -> report\n    description: \"Bar.\"\n    flow:\n        \"work\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);
        // Lookup hits via either spelling.
        let via_capital = registry.lookup("Report").expect("lookup via `Report` must hit");
        let via_lower = registry.lookup("report").expect("lookup via `report` must hit");
        assert_eq!(via_capital.canonical_name, "report");
        assert_eq!(via_lower.canonical_name, "report");
        // First-span wins: the entry's span matches the *first* (`Report`)
        // annotation, not the second (`report`).
        let first_arrow = src.find("->").unwrap() as u32;
        let first_report_end = (src.find("Report").unwrap() + "Report".len()) as u32;
        assert_eq!(via_capital.first_use_span.start, first_arrow);
        assert_eq!(via_capital.first_use_span.end, first_report_end);
    }

    #[test]
    fn t6_banned_return_type_warns_but_does_not_register() {
        // AC1 split: a banned generic name (`-> String`) emits the existing
        // `G::analyze::generic-type-name` warning AND must NOT be added to
        // the registry. Lookup via either casing returns None.
        let src = "skill foo() -> String\n    description: \"Foo.\"\n    flow:\n        \"do\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);
        // Existing #83 behavior preserved: warning fires.
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::generic-type-name"),
            "banned `-> String` must still fire generic-type-name warning, got: {:?}",
            ids
        );
        // AC1 add: registry stays empty for banned names.
        assert!(
            registry.lookup("String").is_none(),
            "banned name `String` must NOT be registered"
        );
        assert!(
            registry.lookup("string").is_none(),
            "banned name must not be registered under any spelling"
        );
    }

    #[test]
    fn t7_no_return_type_annotations_yields_empty_registry() {
        // Negative control: a file whose decls all omit `-> DomainType`
        // produces an empty registry. Catches a "register on every decl
        // regardless" regression where the early-return on absent annotation
        // is removed.
        let src = "skill foo()\n    description: \"Foo.\"\n    flow:\n        \"do\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);
        assert!(
            registry.lookup("Foo").is_none(),
            "registry must be empty when no `-> DomainType` annotations exist"
        );
        assert!(
            registry.lookup("foo").is_none(),
            "registry must not pick up the skill name as a domain type"
        );
    }

    // --- Issue #84 Chunk 3: no-shadowing enforcement (AC5) ---
    //
    // The post-hoc sweep at the end of analyze runs the registry against the
    // file's parameters and consts (case-normalized). Any collision emits
    // `G::analyze::name-collision` Error with a primary span at the `-> Type`
    // annotation that introduced the type and a related span at the offending
    // identifier. Banned generic names (#83) skip registration (D8), so they
    // can't collide via this path even if a param shares the spelling.

    /// Helper: count `G::analyze::name-collision` diagnostics whose message
    /// matches the chunk-3 collision shape (mentions "domain type"). The
    /// duplicate-export sweep reuses the same id but says "duplicate export
    /// name", so we filter on substring instead of id alone.
    fn collision_diags(bag: &DiagBag) -> Vec<&Diagnostic> {
        bag.iter()
            .filter(|d| d.id == "G::analyze::name-collision" && d.message.contains("domain type"))
            .collect()
    }

    #[test]
    fn t1_skill_return_type_collides_with_skill_param() {
        // Tracer: skill `foo(report = "x") -> Report` collides — the param
        // `report` and the return type `Report` canonicalize to the same key.
        // Emits one `G::analyze::name-collision` Error; primary span covers
        // the `-> Report` annotation, related span covers the `report` param.
        let src = "skill foo(report = \"x\") -> Report\n    description: \"Foo.\"\n    flow:\n        \"do\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        let diags = collision_diags(&bag);
        assert_eq!(
            diags.len(),
            1,
            "expected exactly one domain-type collision diagnostic, got: {:?}",
            bag.iter().map(|d| (d.id.as_str(), d.message.as_str())).collect::<Vec<_>>()
        );
        let d = diags[0];
        assert_eq!(d.classification, crate::diagnostic::Classification::Error);
        assert!(
            d.message.contains("Report") && d.message.contains("report"),
            "message must name both sides of the collision, got: {:?}",
            d.message
        );
        assert!(
            d.message.contains("parameter"),
            "message must say `parameter` for param-side collision, got: {:?}",
            d.message
        );

        // Primary span: the `-> Report` annotation.
        let arrow_byte = src.find("->").unwrap();
        let report_byte = src.find("Report").unwrap();
        let primary_start_col = (arrow_byte + 1) as u32; // 1-indexed col on line 1
        let primary_end_col = (report_byte + "Report".len()) as u32; // inclusive
        assert_eq!(d.span.start.line, 1);
        assert_eq!(d.span.start.col, primary_start_col);
        assert_eq!(d.span.end.line, 1);
        assert_eq!(d.span.end.col, primary_end_col);

        // Related span: the `report` param identifier inside `foo(...)`.
        assert_eq!(d.related.len(), 1, "expected exactly one related span");
        let related_param_start = (src.find("report").unwrap() + 1) as u32;
        // The Param.span is the parameter's full header position (name plus
        // optional default). We don't want to pin its exact end here — the
        // start-of-line marker is enough to prove the param-side span lands.
        assert_eq!(d.related[0].start.line, 1);
        assert_eq!(d.related[0].start.col, related_param_start);
    }

    #[test]
    fn t2_export_block_return_type_collides_with_export_block_param() {
        // Export-block visit site: `export block bar(report = "x") -> Report`
        // — both the param and the return type canonicalize to `report`. The
        // sweep must enumerate `Decl::ExportBlock` params (not just skill
        // params), so this pinpoints the export-block branch of the match.
        let src = "export block bar(report = \"x\") -> Report\n    flow:\n        \"x\"\n        return report\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        let diags = collision_diags(&bag);
        assert_eq!(
            diags.len(),
            1,
            "expected exactly one domain-type collision diagnostic, got: {:?}",
            bag.iter().map(|d| (d.id.as_str(), d.message.as_str())).collect::<Vec<_>>()
        );
        let d = diags[0];
        assert_eq!(d.classification, crate::diagnostic::Classification::Error);
        assert!(d.message.contains("Report"));
        assert!(d.message.contains("report"));
        assert!(d.message.contains("parameter"));
    }

    #[test]
    fn t4_cross_decl_collision_uses_file_level_scope() {
        // File-level scope: a `-> Report` annotation on the skill collides
        // with a param `report` on a *different* decl. Catches a regression
        // where the sweep is per-decl instead of file-level.
        let src = "skill main() -> Report\n    description: \"Main.\"\n    flow:\n        helper()\n\nblock helper(report = \"x\")\n    description: \"Helper.\"\n    flow:\n        \"work\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        let diags = collision_diags(&bag);
        assert_eq!(
            diags.len(),
            1,
            "expected exactly one cross-decl collision diagnostic, got: {:?}",
            bag.iter().map(|d| (d.id.as_str(), d.message.as_str())).collect::<Vec<_>>()
        );
    }

    #[test]
    fn t5_underscore_cross_spelling_collision_via_canonicalization() {
        // D6 canonicalization (ASCII-lower + strip `_`): `makePlan` and
        // `make_plan` share canonical key `makeplan`. The skill's return
        // type and the block's param spell it differently in source — the
        // sweep must canonicalize before comparing or this regresses.
        let src = "skill main() -> makePlan\n    description: \"Main.\"\n    flow:\n        helper()\n\nblock helper(make_plan = \"x\")\n    description: \"Helper.\"\n    flow:\n        \"work\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        let diags = collision_diags(&bag);
        assert_eq!(
            diags.len(),
            1,
            "expected one canonicalized collision (`makePlan` vs `make_plan`), got: {:?}",
            bag.iter().map(|d| (d.id.as_str(), d.message.as_str())).collect::<Vec<_>>()
        );
        // Message must use raw author spellings on both sides, not the
        // canonicalized `makeplan` form.
        let msg = &diags[0].message;
        assert!(
            msg.contains("makePlan"),
            "message must use raw type spelling `makePlan`, got: {:?}",
            msg
        );
        assert!(
            msg.contains("make_plan"),
            "message must use raw param spelling `make_plan`, got: {:?}",
            msg
        );
    }

    #[test]
    fn t6_skill_return_type_collides_with_const() {
        // Const-side enumeration: `const report = "x"` collides with skill
        // return type `-> Report`. Pinpoints the `Decl::Const` branch of the
        // sweep's enumeration loop and exercises the `"const"` arm of the
        // emit helper (message must say `const`, not `parameter`).
        let src = "skill main() -> Report\n    description: \"Main.\"\n    flow:\n        \"do\"\n\nconst report = \"x\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        let diags = collision_diags(&bag);
        assert_eq!(
            diags.len(),
            1,
            "expected exactly one type-vs-const collision, got: {:?}",
            bag.iter().map(|d| (d.id.as_str(), d.message.as_str())).collect::<Vec<_>>()
        );
        assert!(
            diags[0].message.contains("const"),
            "message must say `const` for const-side collision, got: {:?}",
            diags[0].message
        );
        assert!(
            !diags[0].message.contains("parameter"),
            "const-side collision message must not say `parameter`, got: {:?}",
            diags[0].message
        );
    }

    #[test]
    fn t7_no_collision_when_canonical_names_differ() {
        // Negative control: param `repository` does NOT collide with type
        // `Report` — different canonical keys. Catches a substring-instead-of-
        // equality regression in the canonical comparison.
        let src = "skill main(repository = \"x\") -> Report\n    description: \"Main.\"\n    flow:\n        \"do\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        let diags = collision_diags(&bag);
        assert!(
            diags.is_empty(),
            "expected zero collision diagnostics for distinct canonical names, got: {:?}",
            diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn t8_banned_return_type_skips_collision_per_d8() {
        // D8: banned generic names (`-> String`) skip registry registration,
        // so a param `string` cannot collide via this path. The existing #83
        // banned-warning still fires; the chunk-3 collision does NOT.
        let src = "skill foo(string = \"x\") -> String\n    description: \"Foo.\"\n    flow:\n        \"do\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        // #83 banned-warning still fires.
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::generic-type-name"),
            "banned `-> String` must still fire #83 generic-type-name warning, got: {:?}",
            ids
        );
        // Chunk-3 collision does NOT fire — banned name was never registered.
        let diags = collision_diags(&bag);
        assert!(
            diags.is_empty(),
            "banned name must not produce a chunk-3 collision diagnostic (D8), got: {:?}",
            diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn t9_empty_registry_yields_zero_collision_diagnostics() {
        // No `-> DomainType` annotations anywhere → registry empty → sweep
        // produces zero collision diagnostics, even when params and consts
        // are present. Catches a regression where the sweep emits collisions
        // against an empty registry (would be an infinite false-positive).
        let src = "skill main(report = \"x\")\n    description: \"Main.\"\n    flow:\n        \"do\"\n\nconst report_doc = \"y\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        let diags = collision_diags(&bag);
        assert!(
            diags.is_empty(),
            "empty registry must yield zero collision diagnostics, got: {:?}",
            diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn t10_imports_path_parity_emits_collision() {
        // Imports-path parity with T1: when analyze runs through
        // `analyze_with_imports` (used for files that import other files),
        // the chunk-3 sweep must fire there too. Catches a regression where
        // the sweep landed in `analyze_with_diagnostics` only.
        let src = "skill foo(report = \"x\") -> Report\n    description: \"Foo.\"\n    flow:\n        \"do\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let mut used: HashSet<String> = HashSet::new();
        let _ = analyze_with_imports(
            &file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &HashSet::new(),
            &HashSet::new(),
            &mut used,
            &HashMap::new(),
            &mut registry,
            &HashMap::new(),
        );

        let diags = collision_diags(&bag);
        assert_eq!(
            diags.len(),
            1,
            "imports-path must also emit chunk-3 collision diagnostic, got: {:?}",
            bag.iter().map(|d| (d.id.as_str(), d.message.as_str())).collect::<Vec<_>>()
        );
    }

    #[test]
    fn t3_private_block_return_type_collides_with_block_param() {
        // Private-block visit site (D7: in scope for header `-> DomainType`):
        // `block helper(report = "x") -> Report` — param `report` collides
        // with return type `Report` after canonicalization. Pinpoints the
        // `Decl::Block` branch of the param-enumeration match.
        let src = "skill main()\n    description: \"Main.\"\n    flow:\n        helper()\n\nblock helper(report = \"x\") -> Report\n    description: \"Helper.\"\n    flow:\n        \"work\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        let diags = collision_diags(&bag);
        assert_eq!(
            diags.len(),
            1,
            "expected exactly one domain-type collision diagnostic from private block, got: {:?}",
            bag.iter().map(|d| (d.id.as_str(), d.message.as_str())).collect::<Vec<_>>()
        );
        assert!(diags[0].message.contains("parameter"));
    }

    // --- Issue #84 Chunk 4: cross-file nominal matching at return-position ---
    //
    // AC4 (per D13): when a callable declares `-> Type` and its body's
    // `return foo()` calls a callee whose declared `-> Type` canonical-matches
    // a *different* type, fire `G::analyze::nominal-mismatch` Error. Same-file
    // and cross-file callees both go through `Registry::nominal_match`. Banned
    // generic type names (#83) and untyped sides skip the check.
    //
    // Scope (D16): only return-position is in scope. ExportBlock-as-caller
    // is deferred — the AST lacks `flow: Vec<FlowStmt>` for ExportBlockDecl.

    /// Helper: count `G::analyze::nominal-mismatch` diagnostics in the bag.
    /// The existing placeholder `emit_nominal_mismatch` (analyze.rs:1207)
    /// uses the same id for unit-test purposes, so any nominal-mismatch in a
    /// chunk-4 analyze run came from the chunk-4 path.
    fn nominal_mismatches(bag: &DiagBag) -> Vec<&Diagnostic> {
        bag.iter()
            .filter(|d| d.id == "G::analyze::nominal-mismatch")
            .collect()
    }

    #[test]
    fn t1_cross_file_mismatch_fires_with_related_span() {
        // Tracer: caller `skill main() -> RepoContext` body `return foo()`.
        // Imported map declares `foo: -> Plan`. Different canonical forms
        // (`repocontext` vs `plan`) → exactly one `G::analyze::nominal-mismatch`
        // Error. The diagnostic's `related[0]` pins the caller's
        // `-> RepoContext` annotation (the contract being violated). Per
        // planner note 1: assert byte offsets, not just length, so a
        // future change moving the related span fails loudly.
        let src = "skill main() -> RepoContext\n    description: \"Main.\"\n    flow:\n        return foo()\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let mut used: HashSet<String> = HashSet::new();

        let mut imported_blocks: HashSet<String> = HashSet::new();
        imported_blocks.insert("foo".to_string());

        // Construct the cross-file return-type map manually (per FC5 Q4).
        // The `Plan` span here is irrelevant to chunk-4 (D14 related-span is
        // local-only); chunk-4 captures-but-does-not-render the span per D15.
        let plan_span = Span::new(0, 0, 0);
        let mut imported_block_return_types: HashMap<String, Spanned<String>> = HashMap::new();
        imported_block_return_types.insert(
            "foo".to_string(),
            Spanned::new("Plan".to_string(), plan_span),
        );

        let _ = analyze_with_imports(
            &file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &HashSet::new(),
            &imported_blocks,
            &mut used,
            &HashMap::new(),
            &mut registry,
            &imported_block_return_types,
        );

        let mismatches = nominal_mismatches(&bag);
        assert_eq!(
            mismatches.len(),
            1,
            "expected exactly one nominal-mismatch diagnostic, got: {:?}",
            bag.iter().map(|d| (d.id.as_str(), d.message.as_str())).collect::<Vec<_>>()
        );
        let d = mismatches[0];
        assert_eq!(d.classification, Classification::Error);
        // Message must name caller's expected type, callee's actual type, and
        // the call target so authors can locate the offending site.
        assert!(
            d.message.contains("RepoContext"),
            "message must name caller's expected type `RepoContext`, got: {:?}",
            d.message
        );
        assert!(
            d.message.contains("Plan"),
            "message must name callee's actual type `Plan`, got: {:?}",
            d.message
        );
        assert!(
            d.message.contains("foo"),
            "message must name the call target `foo`, got: {:?}",
            d.message
        );

        // Related span pins the caller's `-> RepoContext` annotation. Byte
        // offsets are computed from the test source string; line is 1-based.
        assert_eq!(
            d.related.len(),
            1,
            "expected exactly one related span (caller's -> Type annotation)"
        );
        let arrow_byte = src.find("->").unwrap();
        let repo_context_end = src.find("RepoContext").unwrap() + "RepoContext".len();
        assert_eq!(d.related[0].start.line, 1);
        assert_eq!(
            d.related[0].start.col,
            (arrow_byte + 1) as u32,
            "related span must start at the `->` token (1-indexed col)"
        );
        assert_eq!(d.related[0].end.line, 1);
        assert_eq!(
            d.related[0].end.col,
            repo_context_end as u32,
            "related span must end at the end of the `RepoContext` identifier"
        );
    }

    #[test]
    fn t2_cross_file_match_emits_no_diagnostic() {
        // Positive control: caller `-> RepoContext`, imported `foo: -> RepoContext`.
        // Same canonical form on both sides → zero nominal-mismatch diagnostics.
        // Catches a regression where the check fires on every return-call
        // regardless of canonical equality.
        let src = "skill main() -> RepoContext\n    description: \"Main.\"\n    flow:\n        return foo()\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let mut used: HashSet<String> = HashSet::new();

        let mut imported_blocks: HashSet<String> = HashSet::new();
        imported_blocks.insert("foo".to_string());

        let mut imported_block_return_types: HashMap<String, Spanned<String>> = HashMap::new();
        imported_block_return_types.insert(
            "foo".to_string(),
            Spanned::new("RepoContext".to_string(), Span::new(0, 0, 0)),
        );

        let _ = analyze_with_imports(
            &file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &HashSet::new(),
            &imported_blocks,
            &mut used,
            &HashMap::new(),
            &mut registry,
            &imported_block_return_types,
        );

        let mismatches = nominal_mismatches(&bag);
        assert!(
            mismatches.is_empty(),
            "expected zero nominal-mismatch for canonical-equal types, got: {:?}",
            mismatches.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn t9_blockdecl_as_caller_export_block_caller_deferred() {
        // BlockDecl-as-caller (private block body): local `block helper() ->
        // Report` body returns `foo()`. Local `block foo() -> Plan` is the
        // callee. Pinpoints the BlockDecl-flow-walk path that Skill-only flow
        // walking misses.
        //
        // D16 (deferred): ExportBlock-as-caller is **not** covered today — the
        // AST has no `flow: Vec<FlowStmt>` for `ExportBlockDecl` (only
        // `flow_strings: Vec<String>` + `has_return: bool`), so a structured
        // `Return(Call)` walk isn't reachable. Future fix: grow
        // `ExportBlockDecl.flow` or add a structured return-target field.
        // Test name encodes this scope-pin so the deferral stays visible.
        let src = "skill main()\n    description: \"Main.\"\n    flow:\n        helper()\n\nblock helper() -> Report\n    description: \"Helper.\"\n    flow:\n        return foo()\n\nblock foo() -> Plan\n    description: \"Foo.\"\n    flow:\n        \"do\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        let mismatches = nominal_mismatches(&bag);
        assert_eq!(
            mismatches.len(),
            1,
            "expected one nominal-mismatch from BlockDecl-as-caller, got: {:?}",
            bag.iter().map(|d| (d.id.as_str(), d.message.as_str())).collect::<Vec<_>>()
        );
        let d = mismatches[0];
        assert!(d.message.contains("Report"));
        assert!(d.message.contains("Plan"));
        assert!(d.message.contains("foo"));
    }

    #[test]
    fn t8_stdlib_callee_skips_check() {
        // Stdlib blocks (`subagent`, `send`) carry no declared `-> Type` in
        // scope of the user file → not in the local-callee map → skip. The
        // skill imports `subagent` from `@glyph/std`, the body returns
        // `subagent()`. Zero nominal-mismatch even though the caller has a
        // `-> Report` annotation.
        let src = "import \"@glyph/std\" { subagent }\n\nskill main() -> Report\n    description: \"Main.\"\n    flow:\n        return subagent()\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let mut used: HashSet<String> = HashSet::new();
        let mut imported_blocks: HashSet<String> = HashSet::new();
        imported_blocks.insert("subagent".to_string());
        let _ = analyze_with_imports(
            &file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &HashSet::new(),
            &imported_blocks,
            &mut used,
            &HashMap::new(),
            &mut registry,
            &HashMap::new(),
        );

        let mismatches = nominal_mismatches(&bag);
        assert!(
            mismatches.is_empty(),
            "stdlib callee must skip the type check (no declared `-> Type` in scope), got: {:?}",
            mismatches.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn t7_cross_spelling_canonical_match_emits_no_diagnostic() {
        // D6 canonicalization in the chunk-4 check: caller `-> RepoContext`,
        // callee `-> repo_context`. Both canonicalize to `repocontext` →
        // `Registry::nominal_match` returns true → zero nominal-mismatch.
        // Catches a regression where the check uses raw-string equality
        // instead of `nominal_match` / `canonicalize_identifier`.
        let src = "skill main() -> RepoContext\n    description: \"Main.\"\n    flow:\n        return helper()\n\nblock helper() -> repo_context\n    description: \"Helper.\"\n    flow:\n        \"do\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        let mismatches = nominal_mismatches(&bag);
        assert!(
            mismatches.is_empty(),
            "cross-spelling canonical match must skip the diagnostic, got: {:?}",
            mismatches.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn t6_callee_untyped_skips_check() {
        // Callee side has no `-> Type` annotation → not in the local-callee
        // map → naturally absent → skip. Same `types.md` rule symmetric to T5.
        let src = "skill main() -> Report\n    description: \"Main.\"\n    flow:\n        return helper()\n\nblock helper()\n    description: \"Helper.\"\n    flow:\n        \"do\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        let mismatches = nominal_mismatches(&bag);
        assert!(
            mismatches.is_empty(),
            "expected zero nominal-mismatch when callee is untyped, got: {:?}",
            mismatches.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn t5_caller_untyped_skips_check() {
        // Caller side has no `-> Type` annotation → no contract to violate.
        // Skill `main()` (no return type) body returns `helper()`, callee
        // `helper() -> Plan`. Zero nominal-mismatch — per `types.md` line
        // 67-76 ("If either side omits the type annotation, no check").
        let src = "skill main()\n    description: \"Main.\"\n    flow:\n        return helper()\n\nblock helper() -> Plan\n    description: \"Helper.\"\n    flow:\n        \"do\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        let mismatches = nominal_mismatches(&bag);
        assert!(
            mismatches.is_empty(),
            "expected zero nominal-mismatch when caller is untyped, got: {:?}",
            mismatches.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn t4_same_file_match_emits_no_diagnostic() {
        // Positive control for same-file path: caller `-> RepoContext`,
        // local callee `-> RepoContext`. Zero nominal-mismatch.
        let src = "skill main() -> RepoContext\n    description: \"Main.\"\n    flow:\n        return helper()\n\nblock helper() -> RepoContext\n    description: \"Helper.\"\n    flow:\n        \"do\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        let mismatches = nominal_mismatches(&bag);
        assert!(
            mismatches.is_empty(),
            "expected zero nominal-mismatch for same-canonical types, got: {:?}",
            mismatches.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn t3_same_file_mismatch_via_analyze_with_diagnostics() {
        // Same-file path: `block helper() -> Plan` is a local callee. Skill
        // `main() -> RepoContext` body returns `helper()`. The same-file
        // local-callee map must be populated in `analyze_with_diagnostics`
        // (the no-imports entry point) for the check to fire here.
        let src = "skill main() -> RepoContext\n    description: \"Main.\"\n    flow:\n        return helper()\n\nblock helper() -> Plan\n    description: \"Helper.\"\n    flow:\n        \"do\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        let mismatches = nominal_mismatches(&bag);
        assert_eq!(
            mismatches.len(),
            1,
            "expected one nominal-mismatch for same-file mismatched types, got: {:?}",
            bag.iter().map(|d| (d.id.as_str(), d.message.as_str())).collect::<Vec<_>>()
        );
        let d = mismatches[0];
        assert!(d.message.contains("RepoContext"));
        assert!(d.message.contains("Plan"));
        assert!(d.message.contains("helper"));
    }

    #[test]
    fn t11_imports_path_blockdecl_as_caller_parity() {
        // Parity test on the imports-path: a local `block helper() -> Report`
        // returns `imported_foo()`, where the imports map declares
        // `imported_foo -> Plan`. The chunk-4 check must fire from the
        // BlockDecl-flow walk on the imports path (analyze_with_imports),
        // mirroring the same-file BlockDecl-as-caller behaviour T9' covers
        // for `analyze_with_diagnostics`.
        //
        // Without this parity walk, a mismatched cross-file return on the
        // imports path through a private-block caller would silently pass.
        let src = "skill main()\n    description: \"Main.\"\n    flow:\n        helper()\n\nblock helper() -> Report\n    description: \"Helper.\"\n    flow:\n        return imported_foo()\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let mut used: HashSet<String> = HashSet::new();

        let mut imported_blocks: HashSet<String> = HashSet::new();
        imported_blocks.insert("imported_foo".to_string());

        let mut imported_block_return_types: HashMap<String, Spanned<String>> = HashMap::new();
        imported_block_return_types.insert(
            "imported_foo".to_string(),
            Spanned::new("Plan".to_string(), Span::new(0, 0, 0)),
        );

        let _ = analyze_with_imports(
            &file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &HashSet::new(),
            &imported_blocks,
            &mut used,
            &HashMap::new(),
            &mut registry,
            &imported_block_return_types,
        );

        let mismatches = nominal_mismatches(&bag);
        assert_eq!(
            mismatches.len(),
            1,
            "expected one nominal-mismatch for imports-path BlockDecl-as-caller, got: {:?}",
            bag.iter().map(|d| (d.id.as_str(), d.message.as_str())).collect::<Vec<_>>()
        );
        let d = mismatches[0];
        assert!(d.message.contains("Report"));
        assert!(d.message.contains("Plan"));
        assert!(d.message.contains("imported_foo"));
    }

    #[test]
    fn t10_same_file_mismatch_pins_related_to_caller_arrow_type() {
        // Canonical related-span pin on the same-file path. T1 covers this on
        // the imports-path (`analyze_with_imports`); this test asserts the
        // identical `related[0]` contract holds when the diagnostic is fired
        // from `analyze_with_diagnostics` (no-imports entry point).
        //
        // Per D14: `related[0]` must point at the **caller's** `-> Type`
        // annotation (the contract being violated), not the callee's. Byte
        // offsets are pinned (not just lengths) so a future refactor that
        // shifts the related-span source fails loudly.
        let src = "skill main() -> RepoContext\n    description: \"Main.\"\n    flow:\n        return helper()\n\nblock helper() -> Plan\n    description: \"Helper.\"\n    flow:\n        \"do\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        let mismatches = nominal_mismatches(&bag);
        assert_eq!(mismatches.len(), 1);
        let d = mismatches[0];

        assert_eq!(
            d.related.len(),
            1,
            "expected exactly one related span (caller's -> Type annotation)"
        );
        // Caller's `-> RepoContext` is on line 1; pin both line and column.
        // Source is the test string above; `find` returns the first occurrence,
        // which is the caller's annotation (callee uses `-> Plan`).
        let arrow_byte = src.find("->").unwrap();
        let repo_context_end = src.find("RepoContext").unwrap() + "RepoContext".len();
        assert_eq!(d.related[0].start.line, 1);
        assert_eq!(
            d.related[0].start.col,
            (arrow_byte + 1) as u32,
            "related span must start at the caller's `->` token (1-indexed col)"
        );
        assert_eq!(d.related[0].end.line, 1);
        assert_eq!(
            d.related[0].end.col,
            repo_context_end as u32,
            "related span must end at the end of the caller's `RepoContext` identifier"
        );
    }

    // --- Issue #84 codex pass 1 — three coupled fixes at the registry /
    // nominal-match call sites. Each cycle pins one finding; the fix is
    // applied immediately after RED to keep the slice vertical. ---

    #[test]
    fn t11_nominal_match_skipped_when_caller_type_is_banned_generic() {
        // Codex pass 1 — F1 [P1]. A skill annotated `-> String` (banned
        // generic per #83) calling `block helper() -> Report` must NOT
        // upgrade the #83 banned-generic warning into a hard
        // `nominal-mismatch` error. The non-blocking `generic-type-name`
        // warning is the user-visible signal; chunk-4's nominal check has
        // no contract to enforce when one side is a banned name (the
        // banned name carries no domain semantics, so canonical-equality
        // against `Report` is meaningless and would fire spuriously).
        //
        // Pre-fix: chunk-4 compares `string` vs `report` canonical forms,
        // they differ, and `nominal-mismatch` (Error, exit 1) fires. Post-
        // fix: the call site short-circuits when either side fails
        // `validate_type_position`, so only the warning remains.
        let src = "skill main() -> String\n    description: \"Main.\"\n    flow:\n        return helper()\n\nblock helper() -> Report\n    description: \"Helper.\"\n    flow:\n        \"do\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        let mismatches = nominal_mismatches(&bag);
        assert_eq!(
            mismatches.len(),
            0,
            "banned-generic caller `-> String` must not fire nominal-mismatch; got: {:?}",
            bag.iter().map(|d| (d.id.as_str(), d.message.as_str())).collect::<Vec<_>>()
        );
        // Sanity: the #83 generic-type-name warning still fires (the
        // banned-skip is a *suppression* on the new chunk-4 path, not a
        // muting of the pre-existing #83 warning).
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::generic-type-name"),
            "expected G::analyze::generic-type-name (banned warning) to still fire, got: {:?}",
            ids
        );
    }

    #[test]
    fn t12_builtin_agent_in_return_position_does_not_register_as_domain_type() {
        // Codex pass 1 — F2 [P2]. `Agent` is a built-in `TypeTag`
        // (`kind_infer.rs`), not a domain type. It is *not* on #83's
        // banned-generic list, so chunk 2's `register_first_use` call
        // formerly recorded `agent` (canonical) in the per-file
        // domain-type registry. Then chunk 3's no-shadowing sweep
        // matched the `agent` parameter against that registry entry
        // and fired `G::analyze::name-collision` (Error, exit 1) —
        // a spurious diagnostic against a built-in type.
        //
        // Post-fix: `warn_if_banned_return_type` skips registration
        // for any built-in name (`String`, `Int`, `Float`, `Bool`,
        // `None`, `Agent`), case-insensitive. `Agent` is the only one
        // not already filtered by the banned-list `Err` branch, so
        // the regression is observable here.
        let src = "skill main(agent) -> Agent\n    description: \"Main.\"\n    flow:\n        \"Use the agent.\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        // No domain-type registration → no name-collision sweep match.
        let collisions = collision_diags(&bag);
        assert_eq!(
            collisions.len(),
            0,
            "built-in `Agent` must not register as domain type; got: {:?}",
            bag.iter().map(|d| (d.id.as_str(), d.message.as_str())).collect::<Vec<_>>()
        );
        // Direct registry assertion: `Agent` (canonicalized to `agent`)
        // must not have a registry entry. Catches a future regression
        // where a sibling code path resurrects the built-in registration.
        assert!(
            registry.lookup("Agent").is_none(),
            "built-in `Agent` must not appear in the per-file domain-type registry"
        );
    }

    #[test]
    fn t13_same_file_return_call_to_export_block_does_not_fire_nominal_mismatch() {
        // Codex pass 1 — F3 [P2]. Chunk 4 populated
        // `local_callee_return_types` from `Decl::Block`,
        // `Decl::ExportBlock`, and `Decl::Skill`. But same-file call
        // resolution recognizes only `Decl::Block` as a valid local
        // callee. The chunk-4 nominal-match still found the
        // export-block's `-> Type` entry though, so when the caller's and
        // callee's declared types differed, a false hard
        // `G::analyze::nominal-mismatch` (Error, exit 1) fired against
        // a same-file `return exported_fn()` boundary that the resolver
        // would otherwise flag as unresolved.
        //
        // Post-fix: `local_callee_return_types` is restricted to
        // `Decl::Block` only. Cross-file matching uses the imports-path
        // `imported_block_return_types` map (populated from
        // `extract_exports::block_return_types` over `Decl::ExportBlock`
        // exports — verified at `lib.rs:216`), so AC8's cross-file
        // contract remains intact.
        //
        // Empirical note (orthogonal to this fix): the
        // `analyze_skill::FlowStmt::Return(_)` arm currently does not run
        // the same `block_names` resolution check that `FlowStmt::Call`
        // does, so `return exported_fn()` to a same-file export-block
        // surfaces no `undefined-call` diagnostic today. Pre-fix, the
        // chunk-4 `nominal-mismatch` was the *only* diagnostic emitted
        // for this fixture; post-fix the bag is empty for the call
        // boundary itself. Adding a return-position resolution check is
        // out of scope for this codex pass.
        //
        // Fixture uses *mismatched* types (`Plan` vs `Report`);
        // matching-type fixtures cannot exercise the bug because
        // `nominal_match` short-circuits to true and no diagnostic
        // fires either way.
        let src = "export block exported_fn() -> Plan\n    description: \"Make a plan.\"\n    flow:\n        return \"x\"\n\nskill main() -> Report\n    description: \"Main.\"\n    flow:\n        return exported_fn()\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        // Primary contract: no false hard nominal-mismatch.
        let mismatches = nominal_mismatches(&bag);
        assert_eq!(
            mismatches.len(),
            0,
            "same-file call to `export block` must not fire chunk-4 nominal-mismatch; got: {:?}",
            bag.iter().map(|d| (d.id.as_str(), d.message.as_str())).collect::<Vec<_>>()
        );
    }

    // --- Issue #84 codex pass 2 — branch-body nominal walk. The skill flow
    // walk and `check_block_return_calls` formerly iterated the top-level
    // `flow` slice flat, so a `return foo()` nested inside an `if` / `elif` /
    // `else` body bypassed the chunk-4 nominal-mismatch check entirely. ---

    #[test]
    fn t15_branch_body_return_call_fires_nominal_mismatch_on_block_walk() {
        // Codex pass 2 — F1 [P1] block walk. Mirrors t14 on the private-
        // block-as-caller path through `check_block_return_calls`. A local
        // `block helper() -> Report` returns `imported_foo()` from inside
        // an `if` body; imports map declares `imported_foo: -> Plan`.
        //
        // Pre-fix: `check_block_return_calls` iterated `block.flow` flat
        // (no Branch recursion), so the imports-path BlockDecl-as-caller
        // contract t11_imports_path_blockdecl_as_caller_parity pinned only
        // top-level returns. Returns nested in a branch slipped through.
        //
        // Post-fix: the helper delegates to the recursive walker shared
        // with the skill-flow path (single nominal-walk surface, no drift).
        let src = "skill main()\n    description: \"Main.\"\n    flow:\n        helper()\n\nblock helper() -> Report\n    description: \"Helper.\"\n    flow:\n        if mode == \"x\"\n            return imported_foo()\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let mut used: HashSet<String> = HashSet::new();

        let mut imported_blocks: HashSet<String> = HashSet::new();
        imported_blocks.insert("imported_foo".to_string());

        let mut imported_block_return_types: HashMap<String, Spanned<String>> = HashMap::new();
        imported_block_return_types.insert(
            "imported_foo".to_string(),
            Spanned::new("Plan".to_string(), Span::new(0, 0, 0)),
        );

        let _ = analyze_with_imports(
            &file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &HashSet::new(),
            &imported_blocks,
            &mut used,
            &HashMap::new(),
            &mut registry,
            &imported_block_return_types,
        );

        let mismatches = nominal_mismatches(&bag);
        assert_eq!(
            mismatches.len(),
            1,
            "expected one nominal-mismatch for branch-nested return on block-as-caller, got: {:?}",
            bag.iter().map(|d| (d.id.as_str(), d.message.as_str())).collect::<Vec<_>>()
        );
        let d = mismatches[0];
        assert!(d.message.contains("Report"));
        assert!(d.message.contains("Plan"));
        assert!(d.message.contains("imported_foo"));
    }

    #[test]
    fn t14_branch_body_return_call_fires_nominal_mismatch_on_skill_walk() {
        // Codex pass 2 — F1 [P1] skill walk. A skill `main() -> Report` has a
        // `return helper()` nested inside an `if` branch body; same-file
        // callee `block helper() -> Plan` has a divergent canonical name.
        //
        // Pre-fix: `analyze_skill::FlowStmt::Branch` only ran
        // `check_nested_branches` (the parse-time nested-branch warning),
        // never `check_return_call_nominal`. The mismatch was silently lost,
        // exit 0 instead of exit 1.
        //
        // Post-fix: the walk recurses into branch bodies and fires
        // `nominal-mismatch` on every Return regardless of nesting depth.
        let src = "skill main() -> Report\n    description: \"Main.\"\n    flow:\n        if mode == \"x\"\n            return helper()\n\nblock helper() -> Plan\n    description: \"Helper.\"\n    flow:\n        \"do\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(file, 0, "test.glyph.md", &line_index, &mut bag, &mut registry);

        let mismatches = nominal_mismatches(&bag);
        assert_eq!(
            mismatches.len(),
            1,
            "expected one nominal-mismatch for return-in-branch with mismatched types, got: {:?}",
            bag.iter().map(|d| (d.id.as_str(), d.message.as_str())).collect::<Vec<_>>()
        );
        let d = mismatches[0];
        assert!(d.message.contains("Report"));
        assert!(d.message.contains("Plan"));
        assert!(d.message.contains("helper"));
    }
}
