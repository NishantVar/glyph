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
use crate::output_target::OutputTargetExpr;
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
/// `TypeTag` names per `kind_infer.rs`. Used by `warn_if_banned_return_type`
/// to keep built-ins out of the per-file domain-type registry, and by
/// `check_return_call_nominal` could call this in the future if the
/// banned-list ever ceases to cover the same set.
///
/// Issue #84 codex pass 3 — F1 [P2]: classifies by canonical form per
/// `values-and-names.md §Case Normalization` (D6: ASCII-lowercase + strip
/// `_`). Pre-pass-3 used `eq_ignore_ascii_case` only and missed underscore-
/// perturbed spellings like `A_g_e_n_t` — those slipped past the guard,
/// registered as domain types, and triggered spurious `name-collision`
/// against same-spelling parameters. Symmetric to the pass-3 fix in
/// `lower::name_to_typetag` (must classify by canonical form too).
fn is_builtin_type_name(s: &str) -> bool {
    const CANONICAL_BUILTINS: &[&str] = &["string", "int", "float", "bool", "none", "agent"];
    let canonical = crate::domain_registry::canonicalize_identifier(s);
    CANONICAL_BUILTINS.contains(&canonical.as_str())
}

fn is_domain_return_type(rt: Option<&Spanned<String>>) -> bool {
    let Some(rt) = rt else {
        return false;
    };
    crate::type_position::validate_type_position(&rt.node).is_ok()
        && !is_builtin_type_name(&rt.node)
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
    // Only matches if identifier form doesn't apply (identifier takes precedence).
    if placeholder_identifier(s).is_some() {
        return None;
    }
    let inner = s.strip_prefix('<')?.strip_suffix('>')?;
    if inner.is_empty() {
        return None;
    }
    // Reject contents whose round-trip through `glyph fmt` would not be
    // faithful: literal quotes (would yield `<""foo"">`), or characters that
    // require source-level escaping. The tokenizer has already decoded source
    // escapes by this point, so we'd otherwise emit a "Repairable" diagnostic
    // that the formatter cannot actually repair.
    if inner.contains(|c: char| c == '"' || c == '\\' || c == '\n' || c == '\t' || c == '\r') {
        return None;
    }
    Some(inner)
}

fn output_target_identifier(expr: &ReturnExpr) -> Option<(&str, Span)> {
    match expr {
        ReturnExpr::OutputTarget(OutputTargetExpr::Identifier(id)) => {
            Some((id.name.as_str(), id.span))
        }
        _ => None,
    }
}

fn visible_names_for_decl<'a>(
    params: impl Iterator<Item = &'a str>,
    text_names: &HashSet<&str>,
    block_names: &HashSet<&str>,
) -> HashSet<String> {
    let mut visible: HashSet<String> = params.map(String::from).collect();
    visible.extend(text_names.iter().map(|s| (*s).to_string()));
    visible.extend(block_names.iter().map(|s| (*s).to_string()));
    visible
}

fn check_output_target_shadows_binding(
    expr: &ReturnExpr,
    visible_names: &HashSet<String>,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    let Some((name, span)) = output_target_identifier(expr) else {
        return;
    };
    if !visible_names.contains(name) {
        return;
    }
    bag.push(
        Diagnostic::error(
            "G::analyze::output-target-shadows-binding",
            format!("output target `{name}` shadows an existing visible binding"),
            SourceSpan::from_byte_span(file_label, span, line_index),
        ),
        span,
    );
}

fn check_flow_output_target_shadows_binding(
    flow: &[FlowStmt],
    visible_names: &HashSet<String>,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    for stmt in flow {
        match stmt {
            FlowStmt::Return(expr) => {
                check_output_target_shadows_binding(
                    expr,
                    visible_names,
                    file_label,
                    line_index,
                    bag,
                );
            }
            FlowStmt::Branch {
                then_body,
                elif_branches,
                else_body,
                ..
            } => {
                check_flow_output_target_shadows_binding(
                    then_body,
                    visible_names,
                    file_label,
                    line_index,
                    bag,
                );
                for elif in elif_branches {
                    check_flow_output_target_shadows_binding(
                        &elif.body,
                        visible_names,
                        file_label,
                        line_index,
                        bag,
                    );
                }
                if let Some(else_body) = else_body {
                    check_flow_output_target_shadows_binding(
                        else_body,
                        visible_names,
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

fn check_placeholder_string_return(
    expr: &ReturnExpr,
    enclosing_return_type: Option<&Spanned<String>>,
    span: Span,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    if !is_domain_return_type(enclosing_return_type) {
        return;
    }
    let ReturnExpr::Inline(s) = expr else {
        return;
    };
    if let Some(target) = placeholder_identifier(s) {
        bag.push(
            Diagnostic {
                id: "G::analyze::placeholder-string-return".into(),
                classification: Classification::Repairable,
                message: format!(
                    "string placeholder return `\"<{target}>\"` should use the output target form"
                ),
                span: SourceSpan::from_byte_span(file_label, span, line_index),
                related: Vec::new(),
                hints: vec![format!("rewrite as `return <{target}>`")],
            },
            span,
        );
    } else if let Some(desc) = placeholder_description(s) {
        bag.push(
            Diagnostic {
                id: "G::analyze::placeholder-string-return".into(),
                classification: Classification::Repairable,
                message: format!(
                    "string placeholder return `\"<{desc}>\"` should use the output target form"
                ),
                span: SourceSpan::from_byte_span(file_label, span, line_index),
                related: Vec::new(),
                hints: vec![format!("rewrite as `return <\"{desc}\">`")],
            },
            span,
        );
    }
}

fn check_flow_placeholder_string_returns(
    flow: &[FlowStmt],
    enclosing_return_type: Option<&Spanned<String>>,
    span: Span,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    for stmt in flow {
        match stmt {
            FlowStmt::Return(expr) => {
                check_placeholder_string_return(
                    expr,
                    enclosing_return_type,
                    span,
                    file_label,
                    line_index,
                    bag,
                );
            }
            FlowStmt::Branch {
                then_body,
                elif_branches,
                else_body,
                ..
            } => {
                check_flow_placeholder_string_returns(
                    then_body,
                    enclosing_return_type,
                    span,
                    file_label,
                    line_index,
                    bag,
                );
                for elif in elif_branches {
                    check_flow_placeholder_string_returns(
                        &elif.body,
                        enclosing_return_type,
                        span,
                        file_label,
                        line_index,
                        bag,
                    );
                }
                if let Some(else_body) = else_body {
                    check_flow_placeholder_string_returns(
                        else_body,
                        enclosing_return_type,
                        span,
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
            if crate::domain_registry::canonicalize_identifier(param_raw) == entry.canonical_name {
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
            if crate::domain_registry::canonicalize_identifier(const_raw) == entry.canonical_name {
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
        .get(target.node.as_str())
        .copied()
        .or_else(|| imported_block_return_types.get(target.node.as_str()));
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
        &target.node,
        &caller_rt.node,
        &callee_rt.node,
        decl_span,
        caller_rt.span,
        file_label,
        line_index,
        bag,
    );
}

/// Issue #84 codex pass 4: emit `G::analyze::undefined-call` /
/// `G::analyze::stdlib-missing-import` for a `return some_callee()` whose
/// target does not resolve against the skill-flow `block_names` set
/// (combined local-block + imported-block names on the imports path).
///
/// Mirrors the `FlowStmt::Call` arm's resolver verbatim — same Repairable
/// tier, same message and hint shape — so the diagnostic surface stays
/// position-agnostic. No-op when the expression is `Return(Name)` /
/// `Return(StringLit)` (those non-Call return forms cannot be undefined-
/// callable).
///
/// Skill-flow path only. `check_block_return_calls` deliberately does not
/// invoke this helper: block-flow Calls and Returns continue to bypass
/// undefined-call resolution (the existing asymmetry — block flow is
/// nominal-only).
fn check_return_call_undefined(
    expr: &crate::ast::ReturnExpr,
    span: Span,
    block_names: &HashSet<&str>,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    let crate::ast::ReturnExpr::Call { target, .. } = expr else {
        return;
    };
    if block_names.contains(target.node.as_str()) {
        return;
    }
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
                hints: vec![format!(
                    "add `import \"@glyph/std\" {{ {} }}` at the top of the file",
                    target.node
                )],
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
                hints: vec![format!(
                    "declare `block {}()` or check the name for typos",
                    target.node
                )],
            },
            span,
        );
    }
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
            FlowStmt::Branch {
                then_body,
                elif_branches,
                else_body,
                ..
            } => {
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
    let visible_binding_names: HashSet<&str> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Const(c) => Some(c.node.name.as_str()),
            Decl::Block(b) => Some(b.node.name.as_str()),
            Decl::ExportBlock(b) => Some(b.node.name.as_str()),
            Decl::Skill(s) => Some(s.node.name.as_str()),
            Decl::Import(_) => None,
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
            Decl::Block(b) => b
                .node
                .return_type
                .as_ref()
                .map(|rt| (b.node.name.as_str(), rt)),
            _ => None,
        })
        .collect();
    let empty_imported_block_return_types: HashMap<String, Spanned<String>> = HashMap::new();

    for decl in &file.decls {
        match decl {
            Decl::Skill(spanned) => analyze_skill(
                spanned,
                file_id,
                file_label,
                line_index,
                bag,
                registry,
                &text_names,
                &block_names,
                &block_decls,
                &HashMap::new(),
                &local_callee_return_types,
                &empty_imported_block_return_types,
            ),
            Decl::ExportBlock(spanned) => {
                analyze_export_block(
                    spanned,
                    file_label,
                    line_index,
                    bag,
                    registry,
                    &private_names,
                    &visible_binding_names,
                );
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
                let visible_names = visible_names_for_decl(
                    spanned.node.params.iter().map(|p| p.name.as_str()),
                    &text_names,
                    &block_names,
                );
                check_flow_output_target_shadows_binding(
                    &spanned.node.flow,
                    &visible_names,
                    file_label,
                    line_index,
                    bag,
                );
                check_flow_placeholder_string_returns(
                    &spanned.node.flow,
                    spanned.node.return_type.as_ref(),
                    spanned.span,
                    file_label,
                    line_index,
                    bag,
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
            matches!(d, Decl::ExportBlock(_)) || matches!(d, Decl::Const(c) if c.node.exported)
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
    _enable_effects: bool,
) -> (SourceFile, Vec<Resolution>) {
    let mut registry = crate::domain_registry::Registry::new();
    let file = analyze_with_diagnostics(file, file_id, file_label, line_index, bag, &mut registry);
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
            Decl::Const(t) => {
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
                // body_bare_names are plain Strings without span info; skip for resolution.
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
            Decl::Const(_) => {}
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
                // body_bare_names are plain Strings without span info; skip for cross-file resolution.
            }
            Decl::Block(spanned) => {
                walk_flow_for_cross_file(&spanned.node.flow, targets, &mut out);
            }
            Decl::ExportBlock(_) | Decl::Const(_) => {}
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
    let mut visible_binding_names: HashSet<&str> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Const(c) => Some(c.node.name.as_str()),
            Decl::Block(b) => Some(b.node.name.as_str()),
            Decl::ExportBlock(b) => Some(b.node.name.as_str()),
            Decl::Skill(s) => Some(s.node.name.as_str()),
            Decl::Import(_) => None,
        })
        .collect();
    for t in &imported_text_refs {
        visible_binding_names.insert(t.as_str());
    }
    for b in &imported_block_refs {
        visible_binding_names.insert(b.as_str());
    }

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
            Decl::Block(b) => b
                .node
                .return_type
                .as_ref()
                .map(|rt| (b.node.name.as_str(), rt)),
            _ => None,
        })
        .collect();

    for decl in &file.decls {
        match decl {
            Decl::Skill(spanned) => {
                analyze_skill_with_usage_tracking(
                    spanned,
                    file_id,
                    file_label,
                    line_index,
                    bag,
                    registry,
                    &text_names,
                    &block_names,
                    &block_decls,
                    imported_texts,
                    imported_blocks,
                    used_import_names,
                    imported_block_descriptions,
                    &local_callee_return_types,
                    imported_block_return_types,
                );
            }
            Decl::ExportBlock(spanned) => {
                analyze_export_block(
                    spanned,
                    file_label,
                    line_index,
                    bag,
                    registry,
                    &private_names,
                    &visible_binding_names,
                );
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
                let visible_names = visible_names_for_decl(
                    spanned.node.params.iter().map(|p| p.name.as_str()),
                    &text_names,
                    &block_names,
                );
                check_flow_output_target_shadows_binding(
                    &spanned.node.flow,
                    &visible_names,
                    file_label,
                    line_index,
                    bag,
                );
                check_flow_placeholder_string_returns(
                    &spanned.node.flow,
                    spanned.node.return_type.as_ref(),
                    spanned.span,
                    file_label,
                    line_index,
                    bag,
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

                // Issue #84 codex pass 3 — F2 [P2]: track imported-name
                // usage from private block flows. Pre-fix, only the
                // `Decl::Skill` arm called `track_flow_usage`, so an
                // import consumed only inside a `block helper { return
                // imported_foo() }` body left `used_import_names` empty
                // and the lib.rs unused-import emission step fired
                // `G::analyze::unused-import` (Repairable, exit 2)
                // against an import the program actually depends on.
                // Symmetric in spirit to chunk 7a (extended what counts
                // as a use within `track_flow_usage`); pass 3 closes the
                // per-decl dispatch gap.
                track_flow_usage(
                    &spanned.node.flow,
                    imported_texts,
                    imported_blocks,
                    used_import_names,
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
            matches!(d, Decl::ExportBlock(_)) || matches!(d, Decl::Const(c) if c.node.exported)
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
        spanned,
        file_id,
        file_label,
        line_index,
        bag,
        registry,
        text_names,
        block_names,
        block_decls,
        imported_block_descriptions,
        local_callee_return_types,
        imported_block_return_types,
    );

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
    track_flow_usage(
        &skill.flow,
        imported_texts,
        imported_blocks,
        used_import_names,
    );
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
            crate::ast::FlowStmt::Branch {
                then_body,
                elif_branches,
                else_body,
                ..
            } => {
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
                if imported_blocks.contains(&target.node) {
                    used.insert(target.node.clone());
                }
            }
            // Symmetric to `ContextMarker(NameRef)` above (L753-758): a
            // `return <name>` reference may resolve to either an imported text
            // const or an imported block, so check both pools.
            crate::ast::FlowStmt::Return(crate::ast::ReturnExpr::Name(name)) => {
                if imported_blocks.contains(&name.node) || imported_texts.contains(&name.node) {
                    used.insert(name.node.clone());
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
    let visible_names = visible_names_for_decl(
        skill.params.iter().map(|p| p.name.as_str()),
        text_names,
        block_names,
    );

    // Issue #83 AC2 + AC3: warn on banned generic type names in the
    // header `-> DomainType` annotation. Warning tier — non-blocking;
    // analyze continues so all banned occurrences in the file get flagged.
    warn_if_banned_return_type(
        skill.return_type.as_ref(),
        file_label,
        line_index,
        bag,
        registry,
    );
    check_flow_output_target_shadows_binding(
        &skill.flow,
        &visible_names,
        file_label,
        line_index,
        bag,
    );
    check_flow_placeholder_string_returns(
        &skill.flow,
        skill.return_type.as_ref(),
        spanned.span,
        file_label,
        line_index,
        bag,
    );

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
                            format!("`{}` is not a declared `text` in this file", marker.name.node),
                            SourceSpan::from_byte_span(file_label, span, line_index),
                        ),
                        span,
                    );
                }
            }
            FlowStmt::ContextMarker(entry) => {
                check_context_entry_name(
                    entry,
                    text_names,
                    spanned.span,
                    file_label,
                    line_index,
                    bag,
                );
            }
            FlowStmt::Return(expr) => {
                // Issue #84 codex pass 4: route `return some_call()` through
                // the same `block_names` resolver that `FlowStmt::Call` uses.
                // Pre-fix, the FlowStmt::Return arm only ran the chunk-4
                // nominal-match check; an undefined / unimported callee in
                // return position produced no diagnostic at all (the carry-
                // forward observation in t13). Same Repairable tier and
                // identical `stdlib-missing-import` / `undefined-call`
                // message shape as the FlowStmt::Call arm above so authors
                // see the same fix-it regardless of position.
                check_return_call_undefined(
                    expr,
                    spanned.span,
                    block_names,
                    file_label,
                    line_index,
                    bag,
                );
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
            FlowStmt::Branch {
                condition,
                then_body,
                elif_branches,
                else_body,
            } => {
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
                    condition,
                    spanned.span,
                    file_id,
                    file_label,
                    line_index,
                    bag,
                    &text_names,
                    &block_names,
                    &block_decls,
                    imported_block_descriptions,
                );
                // Check elif conditions too.
                for elif in elif_branches {
                    check_applies_in_condition(
                        &elif.condition,
                        spanned.span,
                        file_id,
                        file_label,
                        line_index,
                        bag,
                        &text_names,
                        &block_names,
                        &block_decls,
                        imported_block_descriptions,
                    );
                }
                // Check flow statements inside branch bodies for name resolution.
                check_branch_body_names(
                    then_body,
                    spanned.span,
                    file_label,
                    line_index,
                    bag,
                    &text_names,
                    &block_names,
                );
                for elif in elif_branches {
                    check_branch_body_names(
                        &elif.body,
                        spanned.span,
                        file_label,
                        line_index,
                        bag,
                        &text_names,
                        &block_names,
                    );
                }
                if let Some(eb) = else_body {
                    check_branch_body_names(
                        eb,
                        spanned.span,
                        file_label,
                        line_index,
                        bag,
                        &text_names,
                        &block_names,
                    );
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
    let effects_count_as_content = !skill.effects.is_empty();
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
                        inferred
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    SourceSpan::from_byte_span(file_label, span, line_index),
                ),
                span,
            );
        }
    } else if has_effects_declaration && !declared_none {
        // Check under-declared: inferred effects not in declared set.
        let missing: BTreeSet<&str> = inferred
            .iter()
            .map(|s| s.as_str())
            .filter(|e| !declared_set.contains(e))
            .collect();
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
        let extra: BTreeSet<&str> = declared_set
            .iter()
            .filter(|e| !inferred.contains(**e))
            .copied()
            .collect();
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
                    hints: vec!["remove unused effects or verify they are needed".into()],
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
                    inferred
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
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
        let receiver_name = receiver
            .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
            .next()
            .unwrap_or("");
        if !receiver_name.is_empty() {
            if text_names.contains(receiver_name) {
                // Receiver is a text declaration — not a block.
                bag.push(
                    Diagnostic::error(
                        "G::analyze::applies-on-non-block",
                        format!(
                            "`{}.applies()` — receiver `{}` is a `text` declaration, not a `block`",
                            receiver_name, receiver_name
                        ),
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
                        format!(
                            "`{}.applies()` — receiver `{}` does not resolve to a `block`",
                            receiver_name, receiver_name
                        ),
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
            // Issue #84 codex pass 4 — AC-pass4-5: a `return some_callee()`
            // nested inside an `if`/`elif`/`else` body must run the same
            // undefined-call resolver as a top-level Return. Pre-fix this
            // arm fell into the catch-all and the diagnostic was silently
            // dropped — symmetric in spirit to pass-2's branch-body
            // nominal-walk extension.
            FlowStmt::Return(expr) => {
                check_return_call_undefined(expr, span, block_names, file_label, line_index, bag);
            }
            _ => {}
        }
    }
}

/// Check if a name is a stdlib block (author-importable from `@glyph/std`).
pub(crate) fn is_stdlib_block_name(name: &str) -> bool {
    matches!(name, "subagent" | "send" | "load")
}

/// Return the effect signature for a stdlib block, if it is one.
pub fn stdlib_block_effects(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "subagent" => Some(&["spawns_agent"]),
        "send" => Some(&["spawns_agent"]),
        "load" => Some(&[]),
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
    visible_binding_names: &HashSet<&str>,
) {
    let decl = &spanned.node;

    // Issue #83 AC2 + AC3: warn on banned generic type names in the
    // header `-> DomainType` annotation. Warning tier — non-blocking.
    warn_if_banned_return_type(
        decl.return_type.as_ref(),
        file_label,
        line_index,
        bag,
        registry,
    );

    if let Some(expr) = decl.terminal_return.as_ref() {
        let mut visible_names: HashSet<String> = visible_binding_names
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        visible_names.extend(decl.params.iter().map(|p| p.name.clone()));
        check_output_target_shadows_binding(expr, &visible_names, file_label, line_index, bag);
        check_placeholder_string_return(
            expr,
            decl.return_type.as_ref(),
            spanned.span,
            file_label,
            line_index,
            bag,
        );
    }

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
                hints: vec!["add a `return` statement at the end of the `flow:` section".into()],
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

// ---------------------------------------------------------------------------
// FmtSignals — structured signals for `glyph fmt` auto-fix pass
// ---------------------------------------------------------------------------

/// Structured signals extracted from a parsed `SourceFile` for `glyph fmt`'s
/// auto-fix pass to consume. Single-file scope — no cross-file resolution.
#[derive(Debug, Default)]
pub struct FmtSignals {
    pub referenced_names: HashSet<String>,
    pub unresolved_names: HashSet<String>,
    pub inferred_effects: HashMap<String, Vec<String>>,
}

pub fn fmt_signals(file: &SourceFile) -> FmtSignals {
    let mut signals = FmtSignals::default();
    let mut bound: HashSet<String> = HashSet::new();

    for decl in &file.decls {
        match decl {
            Decl::Const(c) => { bound.insert(c.node.name.clone()); }
            Decl::Block(b) => { bound.insert(b.node.name.clone()); }
            Decl::ExportBlock(b) => { bound.insert(b.node.name.clone()); }
            Decl::Skill(s) => { bound.insert(s.node.name.clone()); }
            Decl::Import(imp) => match &imp.node.kind {
                ast::ImportKind::Selective(names) => {
                    for n in names {
                        let local = n.alias.clone().unwrap_or_else(|| n.name.node.clone());
                        bound.insert(local);
                    }
                }
                ast::ImportKind::WholeModule { alias } => {
                    bound.insert(alias.clone());
                }
            },
        }
    }

    for decl in &file.decls {
        collect_refs_from_decl(decl, &mut signals.referenced_names);
        if let Some((name, effects)) = infer_decl_effects(decl) {
            if !effects.is_empty() {
                signals.inferred_effects.insert(name, effects);
            }
        }
    }

    for name in &signals.referenced_names {
        if !bound.contains(name) {
            signals.unresolved_names.insert(name.clone());
        }
    }
    signals
}

fn collect_refs_from_decl(decl: &Decl, out: &mut HashSet<String>) {
    match decl {
        Decl::Skill(s) => {
            for stmt in &s.node.flow { collect_refs_from_flow_stmt(stmt, out); }
            for n in &s.node.body_bare_names { out.insert(n.clone()); }
        }
        Decl::Block(b) => {
            for stmt in &b.node.flow { collect_refs_from_flow_stmt(stmt, out); }
        }
        Decl::ExportBlock(b) => {
            if let Some(expr) = &b.node.terminal_return {
                collect_refs_from_return_expr(expr, out);
            }
        }
        Decl::Const(_) | Decl::Import(_) => {}
    }
}

fn collect_refs_from_flow_stmt(stmt: &FlowStmt, out: &mut HashSet<String>) {
    match stmt {
        FlowStmt::Call { target, .. } => { out.insert(target.node.clone()); }
        FlowStmt::Return(expr) => collect_refs_from_return_expr(expr, out),
        FlowStmt::Branch { then_body, elif_branches, else_body, .. } => {
            for s in then_body { collect_refs_from_flow_stmt(s, out); }
            for eb in elif_branches {
                for s in &eb.body { collect_refs_from_flow_stmt(s, out); }
            }
            if let Some(eb) = else_body {
                for s in eb { collect_refs_from_flow_stmt(s, out); }
            }
        }
        FlowStmt::BareName(n) => { out.insert(n.node.clone()); }
        FlowStmt::InlineString(_)
        | FlowStmt::ConstraintMarker(_)
        | FlowStmt::ContextMarker(_) => {}
    }
}

fn collect_refs_from_return_expr(expr: &ReturnExpr, out: &mut HashSet<String>) {
    match expr {
        ReturnExpr::Call { target, .. } => { out.insert(target.node.clone()); }
        ReturnExpr::Name(n) => { out.insert(n.node.clone()); }
        ReturnExpr::None | ReturnExpr::Inline(_) | ReturnExpr::OutputTarget(_) => {}
    }
}

fn infer_decl_effects(decl: &Decl) -> Option<(String, Vec<String>)> {
    match decl {
        Decl::Skill(s) => {
            if !s.node.effects.is_empty() {
                return Some((s.node.name.clone(), Vec::new()));
            }
            Some((s.node.name.clone(), infer_effects_for_flow(&s.node.flow)))
        }
        Decl::Block(b) => {
            if !b.node.effects.is_empty() {
                return Some((b.node.name.clone(), Vec::new()));
            }
            Some((b.node.name.clone(), infer_effects_for_flow(&b.node.flow)))
        }
        _ => None,
    }
}

fn infer_effects_for_flow(flow: &[FlowStmt]) -> Vec<String> {
    let mut effects: BTreeSet<String> = BTreeSet::new();
    fn walk(stmt: &FlowStmt, effects: &mut BTreeSet<String>) {
        match stmt {
            FlowStmt::Call { target, .. } => {
                if let Some(eff) = stdlib_block_effects(target.node.as_str()) {
                    for e in eff { effects.insert((*e).to_string()); }
                }
            }
            FlowStmt::Return(ReturnExpr::Call { target, .. }) => {
                if let Some(eff) = stdlib_block_effects(target.node.as_str()) {
                    for e in eff { effects.insert((*e).to_string()); }
                }
            }
            FlowStmt::Branch { then_body, elif_branches, else_body, .. } => {
                for s in then_body { walk(s, effects); }
                for eb in elif_branches { for s in &eb.body { walk(s, effects); } }
                if let Some(eb) = else_body { for s in eb { walk(s, effects); } }
            }
            _ => {}
        }
    }
    for stmt in flow { walk(stmt, &mut effects); }
    effects.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_ids(src: &str) -> Vec<String> {
        crate::check_source(src, 0, "test.glyph.md")
            .iter()
            .map(|d| d.id.clone())
            .collect()
    }

    #[test]
    fn placeholder_string_return_is_repairable_on_domain_typed_skill() {
        let src = "\
skill current() -> BranchName
    description: \"Return the current branch.\"
    flow:
        return \"<current_branch>\"
";
        let bag = crate::check_source(src, 0, "test.glyph.md");
        let ids: Vec<String> = bag.iter().map(|d| d.id.clone()).collect();
        assert!(
            ids.iter()
                .any(|id| id == "G::analyze::placeholder-string-return"),
            "expected placeholder-string-return, got {ids:?}"
        );
        assert_eq!(bag.exit_code(), 2, "diagnostic must be repairable-tier");
    }

    #[test]
    fn placeholder_string_return_descriptive_is_repairable_on_domain_typed_skill() {
        let src = "\
skill diagnose() -> Confirmation
    description: \"Diagnose the issue.\"
    flow:
        return \"<root cause and severity>\"
";
        let bag = crate::check_source(src, 0, "test.glyph.md");
        let ids: Vec<String> = bag.iter().map(|d| d.id.clone()).collect();
        assert!(
            ids.iter()
                .any(|id| id == "G::analyze::placeholder-string-return"),
            "expected placeholder-string-return for descriptive form, got {ids:?}"
        );
        assert_eq!(bag.exit_code(), 2, "diagnostic must be repairable-tier");
        let hints: Vec<String> = bag
            .iter()
            .flat_map(|d| d.hints.iter().cloned())
            .collect();
        assert!(
            hints.iter().any(|h| h.contains("<\"root cause and severity\">")),
            "hint should suggest descriptive output-target form, got {hints:?}"
        );
    }

    #[test]
    fn placeholder_string_return_not_fired_when_inner_contains_quotes() {
        // "<\"foo\">" has inner content containing literal quotes; the
        // descriptive guard must reject it to avoid emitting broken syntax.
        let src = "\
skill diagnose() -> Confirmation
    description: \"Diagnose the issue.\"
    flow:
        return \"<\\\"foo\\\">\"
";
        let ids = check_ids(src);
        assert!(
            !ids.iter()
                .any(|id| id == "G::analyze::placeholder-string-return"),
            "placeholder with inner quotes must NOT fire placeholder-string-return: {ids:?}"
        );
    }

    #[test]
    fn placeholder_string_return_not_fired_when_inner_contains_escape_chars() {
        // The tokenizer decodes string escapes before analyze runs, so source
        // like `return "<root cause\nseverity>"` reaches us with a literal
        // newline inside the string. If we emit the repairable diagnostic, the
        // suggested rewrite would round-trip through `glyph fmt` as a no-op
        // (decoded form != source form) — so the fix is to NOT fire on
        // contents that contain characters needing source-level escaping.
        let cases: &[(&str, &str)] = &[
            ("newline", "skill d() -> Confirmation\n    flow:\n        return \"<root cause\\nseverity>\"\n"),
            ("tab",     "skill d() -> Confirmation\n    flow:\n        return \"<root\\tcause>\"\n"),
            ("cr",      "skill d() -> Confirmation\n    flow:\n        return \"<root\\rcause>\"\n"),
            ("backslash", "skill d() -> Confirmation\n    flow:\n        return \"<path\\\\to\\\\foo>\"\n"),
        ];
        for (label, src) in cases {
            let ids = check_ids(src);
            assert!(
                !ids.iter()
                    .any(|id| id == "G::analyze::placeholder-string-return"),
                "[{label}] placeholder with escape-requiring inner must NOT fire placeholder-string-return: {ids:?}"
            );
        }
    }

    #[test]
    fn placeholder_string_return_ignored_without_domain_type() {
        let src = "\
skill current()
    description: \"Return the current branch.\"
    flow:
        return \"<current_branch>\"
";
        let ids = check_ids(src);
        assert!(
            !ids.iter()
                .any(|id| id == "G::analyze::placeholder-string-return"),
            "untyped placeholder string returns must not fire issue-85 repairable: {ids:?}"
        );
    }

    #[test]
    fn output_target_name_must_not_shadow_visible_binding() {
        let src = "\
const current_branch = \"main\"

skill current() -> BranchName
    description: \"Return the current branch.\"
    flow:
        return <current_branch>
";
        let ids = check_ids(src);
        assert!(
            ids.iter()
                .any(|id| id == "G::analyze::output-target-shadows-binding"),
            "expected output-target-shadows-binding, got {ids:?}"
        );
    }

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
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::applies-on-undescribed-block")
            .unwrap();
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

        emit_nominal_mismatch(
            "Report",
            "TestResult",
            "my_call",
            span,
            "test.glyph.md",
            &line_index,
            &mut bag,
        );

        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::nominal-mismatch"),
            "ids: {:?}",
            ids
        );
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::nominal-mismatch")
            .unwrap();
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
                return_type: None,
                generated: false,
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
                constraints_section: Vec::new(),
                return_type: None,
            },
            span: Span::new(0, 0, 10),
        };
        let file = SourceFile {
            decls: vec![Decl::Block(block), Decl::Skill(skill)],
        };
        let source = "dummy source";
        let li = LineIndex::new(source);

        // missing-effects fires whenever there are inferred effects and no declared effects.
        let mut bag_on = DiagBag::new();
        let mut registry_on = crate::domain_registry::Registry::new();
        analyze_with_diagnostics(file.clone(), 0, "test.glyph.md", &li, &mut bag_on, &mut registry_on);
        let ids_on: Vec<&str> = bag_on.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids_on.contains(&"G::analyze::missing-effects"),
            "expected missing-effects diagnostic, got: {:?}",
            ids_on
        );

        // Verifying the diagnostic fires (effects tracking is always active).
        let mut bag_off = DiagBag::new();
        let mut registry_off = crate::domain_registry::Registry::new();
        analyze_with_diagnostics(file, 0, "test.glyph.md", &li, &mut bag_off, &mut registry_off);
        let ids_off: Vec<&str> = bag_off.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids_off.contains(&"G::analyze::missing-effects"),
            "expected missing-effects to fire, got: {:?}",
            ids_off
        );
    }

    #[test]
    fn lossy_coercion_fires() {
        let mut bag = DiagBag::new();
        let source = "test";
        let line_index = LineIndex::new(source);
        let span = Span::new(0, 0, source.len() as u32);

        emit_lossy_coercion(
            "float",
            "int",
            "my_param",
            span,
            "test.glyph.md",
            &line_index,
            &mut bag,
        );

        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::lossy-coercion"),
            "ids: {:?}",
            ids
        );
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::lossy-coercion")
            .unwrap();
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );
        let entry = registry
            .lookup("Report")
            .expect("`Report` must be registered");
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
        let src =
            "export block bar(x = \"d\") -> Report\n    flow:\n        \"x\"\n        return x\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );
        let entry = registry
            .lookup("Report")
            .expect("`Report` must be registered");
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );
        let entry = registry
            .lookup("Report")
            .expect("`Report` must be registered from private block");
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
        let entry = registry
            .lookup("Report")
            .expect("`Report` must be registered (imports path)");
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );
        let entry = registry
            .lookup("Report")
            .expect("`Report` must be registered");
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );
        // Lookup hits via either spelling.
        let via_capital = registry
            .lookup("Report")
            .expect("lookup via `Report` must hit");
        let via_lower = registry
            .lookup("report")
            .expect("lookup via `report` must hit");
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );
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

const accuracy = "Be accurate."
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
    fn t7_no_return_type_annotations_yields_empty_registry() {
        // Negative control: a file whose decls all omit `-> DomainType`
        // produces an empty registry. Catches a "register on every decl
        // regardless" regression where the early-return on absent annotation
        // is removed.
        let src = "skill foo()\n    description: \"Foo.\"\n    flow:\n        \"do\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let diags = collision_diags(&bag);
        assert_eq!(
            diags.len(),
            1,
            "expected exactly one domain-type collision diagnostic, got: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let diags = collision_diags(&bag);
        assert_eq!(
            diags.len(),
            1,
            "expected exactly one domain-type collision diagnostic, got: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let diags = collision_diags(&bag);
        assert_eq!(
            diags.len(),
            1,
            "expected exactly one cross-decl collision diagnostic, got: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let diags = collision_diags(&bag);
        assert_eq!(
            diags.len(),
            1,
            "expected one canonicalized collision (`makePlan` vs `make_plan`), got: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let diags = collision_diags(&bag);
        assert_eq!(
            diags.len(),
            1,
            "expected exactly one type-vs-const collision, got: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

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
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let diags = collision_diags(&bag);
        assert_eq!(
            diags.len(),
            1,
            "expected exactly one domain-type collision diagnostic from private block, got: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
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
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
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
            d.related[0].end.col, repo_context_end as u32,
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
            mismatches
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let mismatches = nominal_mismatches(&bag);
        assert_eq!(
            mismatches.len(),
            1,
            "expected one nominal-mismatch from BlockDecl-as-caller, got: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
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
            mismatches
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let mismatches = nominal_mismatches(&bag);
        assert!(
            mismatches.is_empty(),
            "cross-spelling canonical match must skip the diagnostic, got: {:?}",
            mismatches
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let mismatches = nominal_mismatches(&bag);
        assert!(
            mismatches.is_empty(),
            "expected zero nominal-mismatch when callee is untyped, got: {:?}",
            mismatches
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let mismatches = nominal_mismatches(&bag);
        assert!(
            mismatches.is_empty(),
            "expected zero nominal-mismatch when caller is untyped, got: {:?}",
            mismatches
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let mismatches = nominal_mismatches(&bag);
        assert!(
            mismatches.is_empty(),
            "expected zero nominal-mismatch for same-canonical types, got: {:?}",
            mismatches
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let mismatches = nominal_mismatches(&bag);
        assert_eq!(
            mismatches.len(),
            1,
            "expected one nominal-mismatch for same-file mismatched types, got: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
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
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

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
            d.related[0].end.col, repo_context_end as u32,
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let mismatches = nominal_mismatches(&bag);
        assert_eq!(
            mismatches.len(),
            0,
            "banned-generic caller `-> String` must not fire nominal-mismatch; got: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        // No domain-type registration → no name-collision sweep match.
        let collisions = collision_diags(&bag);
        assert_eq!(
            collisions.len(),
            0,
            "built-in `Agent` must not register as domain type; got: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        // Primary contract: no false hard nominal-mismatch.
        let mismatches = nominal_mismatches(&bag);
        assert_eq!(
            mismatches.len(),
            0,
            "same-file call to `export block` must not fire chunk-4 nominal-mismatch; got: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
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
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
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
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let mismatches = nominal_mismatches(&bag);
        assert_eq!(
            mismatches.len(),
            1,
            "expected one nominal-mismatch for return-in-branch with mismatched types, got: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
        );
        let d = mismatches[0];
        assert!(d.message.contains("Report"));
        assert!(d.message.contains("Plan"));
        assert!(d.message.contains("helper"));
    }

    // --- Issue #84 codex pass 3 — D6 underscore-stripping in built-in
    // classification + import tracking through private block flows. ---

    #[test]
    fn t16_builtin_classifier_strips_underscores_per_d6_no_collision() {
        // Codex pass 3 — F1 [P2] (analyze side). `is_builtin_type_name` was
        // pass-1's guard that kept built-in `TypeTag` names (notably `Agent`)
        // out of the per-file domain-type registry, so the chunk-3 collision
        // sweep wouldn't fire `name-collision` against an `agent` parameter.
        // The guard used `eq_ignore_ascii_case` only — D6 / `values-and-
        // names.md §Case Normalization` says underscores are insignificant
        // alongside ASCII case, so an underscore-perturbed spelling like
        // `A_g_e_n_t` (which canonicalizes to `agent`) slipped past the
        // guard, was registered as a domain type, and then collided with
        // the `agent` parameter — a spurious hard `name-collision` error.
        //
        // Post-fix: classifier canonicalizes its input first and compares
        // against the canonical built-in set (`agent`, `string`, etc.).
        // Same fixture as pass-1's t12 but with the Agent spelling
        // perturbed; t12 stays green to lock the original surface.
        let src = "skill main(agent) -> A_g_e_n_t\n    description: \"Main.\"\n    flow:\n        \"Use the agent.\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let collisions = collision_diags(&bag);
        assert_eq!(
            collisions.len(),
            0,
            "underscore-perturbed built-in `A_g_e_n_t` must not register as domain type; got: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
        );
        assert!(
            registry.lookup("A_g_e_n_t").is_none(),
            "underscore-perturbed built-in must not appear in registry"
        );
        assert!(
            registry.lookup("Agent").is_none(),
            "canonical-form lookup of the same built-in must also miss"
        );
    }

    #[test]
    fn t17_builtin_classifier_strips_underscores_per_d6_string_variant() {
        // Codex pass 3 — F1 [P2] generic application. The underscore-strip
        // rule is per-D6, not Agent-specific — apply at least one second
        // built-in spelling so a regression that special-cases `Agent` only
        // (e.g. by pattern-matching one variant) still trips a test.
        // `S_t_r_i_n_g` canonicalizes to `string`; the chunk-2 banned-list
        // check would short-circuit `String` on the un-perturbed spelling
        // (`String` is on the banned list), but with underscores its
        // `validate_type_position` check returns `Ok` and the registration
        // path is reached — exactly the surface the F1 fix has to cover.
        let src = "skill main(string) -> S_t_r_i_n_g\n    description: \"Main.\"\n    flow:\n        \"go\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let collisions = collision_diags(&bag);
        assert_eq!(
            collisions.len(),
            0,
            "underscore-perturbed built-in `S_t_r_i_n_g` must not register as domain type; got: {:?}",
            bag.iter().map(|d| (d.id.as_str(), d.message.as_str())).collect::<Vec<_>>()
        );
        assert!(
            registry.lookup("S_t_r_i_n_g").is_none(),
            "underscore-perturbed built-in must not appear in registry"
        );
    }

    #[test]
    fn t18_block_flow_use_of_imported_block_marks_used_via_imports_path() {
        // Codex pass 3 — F2 [P2]. `analyze_with_imports` previously called
        // `track_flow_usage` only from the `Decl::Skill` arm. An import
        // consumed *only* inside `block helper() { return imported_foo() }`
        // (with helper itself called from the skill) left
        // `used_import_names` empty for that import, and the lib.rs
        // `unused-import` emission step then fired a Repairable diagnostic
        // (exit 2) against an import the program actually depends on at
        // runtime.
        //
        // Post-fix: the `Decl::Block` arm also calls `track_flow_usage`,
        // mirroring the existing `Decl::Skill` arm with the same
        // imported_texts / imported_blocks / used_import_names accumulators.
        // Symmetric in spirit to chunk 7a (which extended what counts as
        // a use *within* `track_flow_usage`); pass 3 closes the per-decl
        // dispatch gap.
        //
        // This is a unit test on the contract: after `analyze_with_imports`
        // returns, `used` must contain `imported_foo`. The integration-level
        // pin (parse → analyze → unused-import suppression) lives in the
        // CLI suite as `ac_codex_pass3_block_flow_import_used_via_binary`.
        let src = "import \"./lib.glyph.md\" { imported_foo }\n\nskill main()\n    description: \"Main.\"\n    flow:\n        helper()\n\nblock helper()\n    description: \"Helper.\"\n    flow:\n        return imported_foo()\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let mut used: HashSet<String> = HashSet::new();

        let mut imported_blocks: HashSet<String> = HashSet::new();
        imported_blocks.insert("imported_foo".to_string());

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

        assert!(
            used.contains("imported_foo"),
            "block-flow consumption of an imported block must mark it as used; \
             used={:?}, bag ids={:?}",
            used,
            bag.iter().map(|d| d.id.as_str()).collect::<Vec<_>>()
        );
    }

    // --- Issue #84 codex pass 4 — route `return some_call()` through the
    // same `block_names` resolver that `FlowStmt::Call` uses. Pre-fix, the
    // skill flow's `FlowStmt::Return(_)` arm only ran the chunk-4 nominal-
    // match check; an undefined / unimported callee in return position
    // produced no diagnostic at all (closes the carry-forward observation
    // documented in t13). The asymmetry where block-flow Calls / Returns
    // still bypass undefined-call resolution is preserved intentionally —
    // `check_block_return_calls` keeps its nominal-only contract. ---

    /// Helper: count `G::analyze::undefined-call` diagnostics in the bag.
    fn undefined_call_diags(bag: &DiagBag) -> Vec<&Diagnostic> {
        bag.iter()
            .filter(|d| d.id == "G::analyze::undefined-call")
            .collect()
    }

    #[test]
    fn t23_return_call_in_branch_body_fires_undefined_call() {
        // Codex pass 4 — AC-pass4-5. Nested coverage: `return some_undefined()`
        // inside an `if`/`elif`/`else` body must fire undefined-call too.
        // Pre-fix the skill-flow Branch arm called `check_branch_body_names`,
        // which matched only Call / ConstraintMarker / ContextMarker — Return
        // fell into the catch-all. Symmetric to pass-2's branch-body nominal
        // walk extension (t14, t15) but for the new pass-4 resolution path.
        let src = "skill main() -> Plan\n    description: \"Main.\"\n    flow:\n        if mode == \"x\"\n            return some_undefined()\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let diags = undefined_call_diags(&bag);
        assert_eq!(
            diags.len(),
            1,
            "expected one undefined-call for branch-nested `return some_undefined()`, got: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
        );
        assert!(diags[0].message.contains("some_undefined"));
    }

    #[test]
    fn t22_return_call_to_imported_block_does_not_fire_undefined_call() {
        // Codex pass 4 — AC-pass4-4 negative pin (imports path). A
        // `return imported_proc()` resolved through the augmented
        // `block_names` set in `analyze_with_imports` (analyze.rs:667-671
        // unions local block names with `imported_blocks`) must not fire
        // undefined-call. Confirms the new resolver shares the same
        // resolution scope as the existing FlowStmt::Call arm — symmetric
        // across positions and across the imports vs no-imports paths.
        let src = "skill main() -> Plan\n    description: \"Main.\"\n    flow:\n        return imported_proc()\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let mut used: HashSet<String> = HashSet::new();

        let mut imported_blocks: HashSet<String> = HashSet::new();
        imported_blocks.insert("imported_proc".to_string());

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

        let diags = undefined_call_diags(&bag);
        assert_eq!(
            diags.len(),
            0,
            "`return imported_proc()` with matching import must not fire undefined-call; got: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn t21_return_call_to_defined_local_block_does_not_fire_undefined_call() {
        // Codex pass 4 — AC-pass4-3 negative pin. A `return local_block()`
        // to a same-file `block local_block() -> Plan` is a well-formed
        // call boundary; the resolver must not fire undefined-call. Pins
        // that the new resolution path doesn't over-fire on the legitimate
        // same-file callee surface.
        let src = "skill main() -> Plan\n    description: \"Main.\"\n    flow:\n        return local_block()\n\nblock local_block() -> Plan\n    description: \"Local.\"\n    flow:\n        \"do\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let diags = undefined_call_diags(&bag);
        assert_eq!(
            diags.len(),
            0,
            "well-formed `return local_block()` must not fire undefined-call; got: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn t20_return_call_to_same_file_export_block_fires_undefined_call() {
        // Codex pass 4 — AC-pass4-2. The same-file resolver's `block_names`
        // is `Decl::Block`-only (analyze.rs:472, codex pass 1 F3 rationale
        // documented in t13); a `return same_file_export_block()` boundary
        // does not resolve against `block_names`, so undefined-call fires.
        // This preserves the existing asymmetry — ExportBlock is not a
        // valid same-file callee target — and closes t13's carry-forward
        // observation about return-position resolution.
        let src = "export block exported_fn() -> Plan\n    description: \"Make a plan.\"\n    flow:\n        return \"x\"\n\nskill main() -> Report\n    description: \"Main.\"\n    flow:\n        return exported_fn()\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let diags = undefined_call_diags(&bag);
        assert_eq!(
            diags.len(),
            1,
            "expected one undefined-call for same-file ExportBlock callee in return position, got: {:?}",
            bag.iter().map(|d| (d.id.as_str(), d.message.as_str())).collect::<Vec<_>>()
        );
        assert!(diags[0].message.contains("exported_fn"));
    }

    #[test]
    fn t19_return_call_to_undefined_name_fires_undefined_call() {
        // Codex pass 4 — AC-pass4-1 tracer. A `return some_undefined()` in
        // skill flow with no matching `block` declaration and no import
        // must emit `G::analyze::undefined-call` (Repairable), matching
        // the FlowStmt::Call arm's existing tier (analyze.rs:1040).
        let src = "skill main() -> Plan\n    description: \"Main.\"\n    flow:\n        return some_undefined()\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ = analyze_with_diagnostics(
            file,
            0,
            "test.glyph.md",
            &line_index,
            &mut bag,
            &mut registry,
        );

        let diags = undefined_call_diags(&bag);
        assert_eq!(
            diags.len(),
            1,
            "expected one undefined-call for `return some_undefined()`, got: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>()
        );
        let d = diags[0];
        assert_eq!(
            d.classification,
            crate::diagnostic::Classification::Repairable
        );
        assert!(
            d.message.contains("some_undefined"),
            "message must name the undefined callee, got: {:?}",
            d.message
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

    #[test]
    fn fmt_signals_extracts_referenced_unresolved_and_effects() {
        let src = r#"import "@glyph/std" { send }

skill main()
    description: "Main."
    flow:
        send("hi")
        subagent("nested")
"#;
        let (file, _) = crate::parse::parse(src, 0).expect("parse");

        let signals = crate::analyze::fmt_signals(&file);

        assert!(signals.referenced_names.contains("send"));
        assert!(signals.referenced_names.contains("subagent"));
        assert!(signals.unresolved_names.contains("subagent"),
            "subagent is not imported and not local — should be unresolved");
        assert!(!signals.unresolved_names.contains("send"),
            "send is imported, should not be unresolved");
    }

    #[test]
    fn fmt_signals_infers_effects_from_stdlib_call() {
        // No `effects:` declared; `send("hi")` should cause `spawns_agent` to
        // be inferred for the skill named "main".
        let src = r#"skill main()
    description: "Test."
    flow:
        send("hi")
"#;
        let (file, _) = crate::parse::parse(src, 0).expect("parse");
        let signals = crate::analyze::fmt_signals(&file);

        let effects = signals.inferred_effects.get("main")
            .expect("main should have inferred effects");
        assert!(
            effects.iter().any(|e| e == "spawns_agent"),
            "expected spawns_agent in inferred effects, got {:?}", effects
        );
    }

    #[test]
    fn fmt_signals_does_not_infer_when_author_declared_effects() {
        // When `effects:` is explicitly declared, infer_decl_effects returns an
        // empty Vec (which the insertion site drops), so the key is absent.
        // Must parse with enable_effects=true so the effects: field is populated.
        let src = r#"skill main()
    description: "Test."
    effects: spawns_agent
    flow:
        send("hi")
"#;
        let line_index = crate::span::LineIndex::new(src);
        let mut bag = crate::diagnostic::DiagBag::new();
        let file = crate::parse::parse_with_diagnostics_opts(
            src, 0, "test.glyph.md", &line_index, &mut bag, true,
        )
        .expect("parse with effects enabled");
        let signals = crate::analyze::fmt_signals(&file);

        // Either the key is absent or its value is empty — either way the
        // inferred_effects map must not contain a non-empty entry for "main".
        let is_empty_or_absent = signals.inferred_effects.get("main")
            .map_or(true, |v| v.is_empty());
        assert!(
            is_empty_or_absent,
            "expected no inferred effects when author declared effects, got {:?}",
            signals.inferred_effects.get("main")
        );
    }

    #[test]
    fn fmt_signals_recurses_into_branch_bodies() {
        // Calls appear only inside `if`/`else` bodies; the walker must recurse
        // into branch arms and surface those call targets in referenced_names.
        let src = r#"skill main()
    description: "Test."
    flow:
        if check == "yes"
            inner_a("x")
        else
            inner_b("y")
"#;
        let (file, _) = crate::parse::parse(src, 0).expect("parse");
        let signals = crate::analyze::fmt_signals(&file);

        assert!(
            signals.referenced_names.contains("inner_a"),
            "inner_a (in then_body) should be in referenced_names, got {:?}",
            signals.referenced_names
        );
        assert!(
            signals.referenced_names.contains("inner_b"),
            "inner_b (in else_body) should be in referenced_names, got {:?}",
            signals.referenced_names
        );
    }
}
