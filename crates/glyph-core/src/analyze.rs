//! Phase 2 (Analyze) — name and effect resolution.
//!
//! Slice 4 wires two parameter-related rules:
//!
//! - `G::analyze::unknown-param-slot` — error. A `{name}` slot inside an
//!   instruction-bearing string (the walking-skeleton subset = inline `flow:`
//!   strings) refers to an identifier that is not a declared header parameter
//!   on the enclosing skill.
//! - `G::analyze::missing-required-arg` — error. A call site whose callee is
//!   a private `block`, a same-file `export block`, or an imported
//!   `export block` (PRD #103 / Slice 1 (#104) and Slice 2 (#105)) omits a
//!   positional argument for a parameter that has no default. Reported at the
//!   call site span, naming the missing parameter and the callee.
//!
//! Both fire from the parsed AST, before lowering, so they surface
//! through `glyph check` as well as `glyph compile`.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use crate::ast::{
    self, BlockDecl, ContextEntry, Decl, DuplicateSubsection, FlowStmt, Param, ReturnExpr,
    SourceFile,
};
use crate::diagnostic::{Classification, DiagBag, Diagnostic, SourceSpan};
use crate::kind_infer::TypeTag;
use crate::output_target::OutputTargetExpr;
use crate::slot::scan_slots;
use crate::span::{LineIndex, Span, Spanned};

// ---------------------------------------------------------------------------
// Flow-position assignments — scope structures
// (`.flow-assign-spec.md` §6.1; Codex Round 3 Med 4 + High 2).
//
// Today the analyzer tracks per-flow `param_names` as a bare HashSet<&str>
// at each call site. The flow-position assignment work needs richer scope
// state: a `bound_names` set for collision checks, plus a typed map for
// the return-type matcher / branch-condition classifier / slot-visibility
// lookups.
//
// `FlowScope` lives next to the existing flow walker. A child scope is
// created for each branch arm by cloning the parent (Codex/spec §6.1
// (X)): outer bindings stay visible inside arms; arm-local bindings do
// NOT leak back to the enclosing flow.
//
// `ContainerKind` threads the enclosing-decl kind into the walk so the
// per-call handler can reject block-flow assignments (Codex Round 3 High
// 2) without changing the walk shape.
// ---------------------------------------------------------------------------

/// Typed metadata captured for a single flow-position assignment.
///
/// Stored in [`FlowScope::flow_local_types`] and consumed by:
/// - the return-type matcher (`tag` + `raw_type` for the LHS type),
/// - branch-condition classification (live snapshot — see §6.3),
/// - emit's agent-shape selection (`is_agent`, §9.1).
#[derive(Debug, Clone)]
pub(crate) struct FlowLocalType {
    /// `TypeTag` for the nominal-matcher fast path. `String` for every
    /// non-Agent return type today (the existing matcher works on
    /// `raw_type` text); `Agent` when the callee returns an agent-shape.
    pub tag: TypeTag,
    /// The declared `-> Type` text with its original span. The existing
    /// nominal matcher consumes raw text plus span (banned-name skipping,
    /// related-span pinning), so we keep both rather than collapsing
    /// to `tag` only.
    pub raw_type: Spanned<String>,
    /// Span of the producing assignment statement. Used for "bound at line
    /// N" hints on `use-before-bind` and other binding-related diagnostics.
    pub producer_span: Span,
    /// Whether the callee's return is agent-shape. Decided by the
    /// agent-shape rule in §9.1 (Codex Round 3 Med 5): rule (1) — match
    /// against `TypeTag::Agent`.
    pub is_agent: bool,
}

/// Per-flow scope state. Replaces the bare `param_names: HashSet<&str>` at
/// each FlowStmt::Call walk site.
///
/// A child scope (for a branch arm) is created via [`FlowScope::child`].
/// Arm-local additions go into the child and are dropped when the arm
/// finishes — implementing the §6.1 (X) lexical-scoping rule.
#[derive(Debug, Default, Clone)]
pub(crate) struct FlowScope {
    /// Header param names of the enclosing skill / block. Unchanged from
    /// today's `param_names`; renamed for symmetry with the new fields.
    pub param_names: HashSet<String>,
    /// Flow-local binding names declared so far at this point in the
    /// walk. Used for the §6.2.a collision check.
    pub bound_names: HashSet<String>,
    /// `bound_name → typed metadata`. Consulted by the return-type
    /// matcher, branch-condition classifier, and slot visibility lookup.
    pub flow_local_types: HashMap<String, FlowLocalType>,
}

impl FlowScope {
    /// Snapshot for a child branch arm. Inherits the parent's view; the
    /// caller pushes arm-local additions onto the child and discards it
    /// when the arm finishes.
    pub(crate) fn child(&self) -> Self {
        self.clone()
    }
}

/// Enclosing-decl kind for the flow walk. Threaded alongside `FlowScope`
/// so the per-call handler can reject `<name> = ...` inside block flow
/// (Codex Round 3 High 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContainerKind {
    Skill,
    Block,
}

/// Auxiliary state for the §6.3 use-before-bind specialization (Codex
/// Round 2 Med 6).
///
/// Built by a pre-pass over the skill's flow before the main analyze
/// walk: every `FlowStmt::Call.bound_name` is registered here regardless
/// of branch position. The main walk consults this when a name lookup
/// fails in the current `FlowScope`:
/// - in `all_names_ever_bound` ⇒ use-before-bind (specialized diag),
/// - not in the set ⇒ truly unknown (existing diag path).
#[derive(Debug, Default, Clone)]
pub(crate) struct SkillBindingTrace {
    pub all_names_ever_bound: HashSet<String>,
}

impl SkillBindingTrace {
    /// Walk the entire flow tree (including branch bodies) collecting every
    /// `FlowStmt::Call.bound_name` whose RHS callee actually resolves to a
    /// declared return type. Branch bodies are walked recursively so an
    /// arm-local binding can be detected by the use-before-bind classifier
    /// even when referenced from outside the arm.
    ///
    /// **Codex round-2 M3 — gated registration.** Names whose RHS callee
    /// is unresolved (no matching block in this file or its imports, no
    /// matching stdlib signature) are intentionally *omitted* from the
    /// trace. Such bindings will already trigger the primary
    /// `G::analyze::undefined-call` (or `stdlib-missing-import`)
    /// diagnostic at the call site; including them in the trace makes
    /// every downstream `{name}` / `return name` reference fire a
    /// secondary `G::analyze::use-before-bind` (Error tier) on top, which
    /// upgrades a repairable single-fault failure to an exit-1 cascade.
    /// `handle_flow_assign` performs the same filter at registration
    /// time (it bails on the no-value path before inserting into
    /// `scope.bound_names`); the trace must agree with that decision so
    /// the §6.3 specialization only fires for names that were *actually*
    /// bound in some sibling scope.
    pub(crate) fn collect(
        flow: &[FlowStmt],
        local_callee_return_types: &HashMap<&str, &Spanned<String>>,
        imported_block_return_types: &HashMap<String, Spanned<String>>,
    ) -> Self {
        fn walk(
            stmts: &[FlowStmt],
            trace: &mut SkillBindingTrace,
            local_callee_return_types: &HashMap<&str, &Spanned<String>>,
            imported_block_return_types: &HashMap<String, Spanned<String>>,
        ) {
            for stmt in stmts {
                match stmt {
                    FlowStmt::Call {
                        bound_name: Some(spanned),
                        target,
                        ..
                    } => {
                        if resolve_callee_return_for_assign(
                            target.node.as_str(),
                            target.span,
                            local_callee_return_types,
                            imported_block_return_types,
                        )
                        .is_some()
                        {
                            trace.all_names_ever_bound.insert(spanned.node.clone());
                        }
                    }
                    FlowStmt::Branch {
                        then_body,
                        elif_branches,
                        else_body,
                        ..
                    } => {
                        walk(
                            then_body,
                            trace,
                            local_callee_return_types,
                            imported_block_return_types,
                        );
                        for elif in elif_branches {
                            walk(
                                &elif.body,
                                trace,
                                local_callee_return_types,
                                imported_block_return_types,
                            );
                        }
                        if let Some(eb) = else_body {
                            walk(
                                eb,
                                trace,
                                local_callee_return_types,
                                imported_block_return_types,
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut trace = SkillBindingTrace::default();
        walk(
            flow,
            &mut trace,
            local_callee_return_types,
            imported_block_return_types,
        );
        trace
    }
}

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
    /// these — they have no `.glyph` source to jump to.
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

/// Kind of type-position use, drives the diagnostic emitted on collision /
/// drift. Spec §"Implicit type registration semantics".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TypeUseKind {
    /// `type Foo = <"…">` declaration header.
    ExplicitDecl,
    /// `-> Foo` return-type annotation on a skill/block/export-block header.
    ReturnAnnotation,
    /// `param: Foo` parameter type annotation.
    ParamAnnotation,
    /// Selective type-import alias (`import "x" { Foo as Bar }` where `Foo`
    /// is an exported `type`).
    SelectiveImport,
}

/// Unified registration helper for every type-position site (spec §"Unified
/// implicit-type-registration helper"). Idempotent on D6 canonical form;
/// diagnoses three drift scenarios:
///
/// - Two `ExplicitDecl` calls with canonical-equal keys → existing
///   `G::analyze::duplicate-type-decl`.
/// - Any subsequent call with a raw spelling differing from the registered
///   `raw_first_use` (when at least one of the two sides is implicit) →
///   warning `G::analyze::inconsistent-type-spelling`.
/// - First call (any kind) → registers, no diagnostic.
///
/// Builtins (`Agent`, `String`, …) are skipped per the existing
/// `is_builtin_type_name` gate. Banned-generic names (issue #83) must be
/// handled by the caller (`warn_if_banned_return_type` still owns that check)
/// before invoking this helper.
pub(crate) fn register_type_use(
    raw: &str,
    span: Span,
    use_kind: TypeUseKind,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    registry: &mut crate::domain_registry::Registry,
    explicit_decl_seen: &mut HashSet<String>,
) {
    if is_builtin_type_name(raw) {
        return;
    }
    // For `ExplicitDecl`, drive `G::analyze::duplicate-type-decl` from the
    // per-file set of canonical keys that have been observed as an in-file
    // `type` declaration — NOT from `registry.lookup`. A prior selective
    // type-import alias populates the registry too, but a local `type X`
    // that shadows an import is a name-collision (handled by
    // `sweep_type_decl_name_collisions`), not a duplicate-decl. The first
    // ExplicitDecl falls through to the spelling-drift / first-use arms.
    if let TypeUseKind::ExplicitDecl = use_kind {
        let canon = crate::domain_registry::canonicalize_identifier(raw);
        if explicit_decl_seen.contains(&canon) {
            bag.push(
                Diagnostic::error(
                    crate::diagnostic::DUPLICATE_TYPE_DECL_DIAG_ID,
                    format!("duplicate `type {}` declaration in this file", raw),
                    SourceSpan::from_byte_span(file_label, span, line_index),
                ),
                span,
            );
            return;
        }
        explicit_decl_seen.insert(canon);
    }
    let already = registry.lookup(raw).cloned();
    match (already, use_kind) {
        (Some(prev), _) if prev.raw_first_use != raw => {
            bag.push(
                Diagnostic {
                    id: crate::diagnostic::INCONSISTENT_TYPE_SPELLING_DIAG_ID.into(),
                    classification: Classification::Warning,
                    message: format!(
                        "type spelling `{}` refers to existing type `{}` (canonically equal); use one spelling consistently",
                        raw, prev.raw_first_use
                    ),
                    span: SourceSpan::from_byte_span(file_label, span, line_index),
                    related: vec![SourceSpan::from_byte_span(
                        file_label,
                        prev.first_use_span,
                        line_index,
                    )],
                    hints: vec![format!(
                        "rename this occurrence to `{}` (the first-use spelling)",
                        prev.raw_first_use
                    )],
                },
                span,
            );
        }
        (Some(_), _) => {
            // Same raw spelling as the registered first use — idempotent no-op.
        }
        (None, _) => {
            registry.register_first_use(raw, span);
        }
    }
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
    case_bad: &HashSet<Span>,
    explicit_decl_seen: &mut HashSet<String>,
) {
    let Some(rt) = rt else { return };
    if case_bad.contains(&rt.span) {
        return;
    }
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
            // Issue #84 Chunk 2 (B.5 unification): legitimate domain-type name
            // → record first use through the unified `register_type_use`
            // helper. Idempotent on canonical form; if a prior site (explicit
            // decl / param / selective import) already registered a different
            // raw spelling, emits `G::analyze::inconsistent-type-spelling`.
            register_type_use(
                &rt.node,
                rt.span,
                TypeUseKind::ReturnAnnotation,
                file_label,
                line_index,
                bag,
                registry,
                explicit_decl_seen,
            );
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

/// Spec §"New pass `validate_identifier_case()`". Validates every
/// identifier-position pair in the file against its required case form.
/// Emits hard errors on mismatch; returns a `HashSet<Span>` of declaration /
/// annotation spans that failed case validation so downstream sweeps and
/// type-registration can short-circuit on those decls (avoids cascade
/// diagnostics).
fn validate_identifier_case(
    file: &SourceFile,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) -> HashSet<Span> {
    use crate::name_kind::{is_pascal_case, is_snake_case};
    let mut bad: HashSet<Span> = HashSet::new();

    fn emit_value(
        raw: &str,
        span: Span,
        site: &str,
        file_label: &str,
        line_index: &LineIndex,
        bag: &mut DiagBag,
    ) {
        let mut diag = Diagnostic::error(
            crate::diagnostic::VALUE_CASE_VIOLATION_DIAG_ID,
            format!(
                "{} identifier `{}` must be snake_case (lowercase letters, digits, underscores; first character lowercase or underscore)",
                site, raw
            ),
            SourceSpan::from_byte_span(file_label, span, line_index),
        );
        diag.hints
            .push("rename to snake_case — e.g. `link_mode`, `repo_root`".into());
        bag.push(diag, span);
    }

    fn emit_type(
        raw: &str,
        span: Span,
        site: &str,
        file_label: &str,
        line_index: &LineIndex,
        bag: &mut DiagBag,
    ) {
        let mut diag = Diagnostic::error(
            crate::diagnostic::TYPE_CASE_VIOLATION_DIAG_ID,
            format!(
                "{} identifier `{}` must be PascalCase (first letter uppercase, no underscores)",
                site, raw
            ),
            SourceSpan::from_byte_span(file_label, span, line_index),
        );
        diag.hints
            .push("rename to PascalCase — e.g. `LinkMode`, `Summary`".into());
        bag.push(diag, span);
    }

    fn check_param(
        p: &ast::Param,
        file_label: &str,
        line_index: &LineIndex,
        bag: &mut DiagBag,
        bad: &mut HashSet<Span>,
    ) {
        if !is_snake_case(&p.name) {
            emit_value(&p.name, p.span, "parameter", file_label, line_index, bag);
            bad.insert(p.span);
        }
        if let Some(ta) = &p.type_annotation {
            if crate::type_position::validate_type_position(&ta.node).is_ok()
                && !is_builtin_type_name(&ta.node)
                && !is_pascal_case(&ta.node)
            {
                emit_type(
                    &ta.node,
                    ta.span,
                    "parameter type annotation",
                    file_label,
                    line_index,
                    bag,
                );
                bad.insert(ta.span);
            }
        }
    }

    fn check_return_type(
        rt: &Spanned<String>,
        file_label: &str,
        line_index: &LineIndex,
        bag: &mut DiagBag,
        bad: &mut HashSet<Span>,
    ) {
        if crate::type_position::validate_type_position(&rt.node).is_ok()
            && !is_builtin_type_name(&rt.node)
            && !is_pascal_case(&rt.node)
        {
            emit_type(
                &rt.node,
                rt.span,
                "return type annotation",
                file_label,
                line_index,
                bag,
            );
            bad.insert(rt.span);
        }
    }

    for decl in &file.decls {
        match decl {
            Decl::Skill(s) => {
                if !is_snake_case(&s.node.name) {
                    emit_value(&s.node.name, s.span, "skill", file_label, line_index, bag);
                    bad.insert(s.span);
                }
                for p in &s.node.params {
                    check_param(p, file_label, line_index, bag, &mut bad);
                }
                if let Some(rt) = &s.node.return_type {
                    check_return_type(rt, file_label, line_index, bag, &mut bad);
                }
            }
            Decl::Block(b) => {
                if !is_snake_case(&b.node.name) {
                    emit_value(&b.node.name, b.span, "block", file_label, line_index, bag);
                    bad.insert(b.span);
                }
                for p in &b.node.params {
                    check_param(p, file_label, line_index, bag, &mut bad);
                }
                if let Some(rt) = &b.node.return_type {
                    check_return_type(rt, file_label, line_index, bag, &mut bad);
                }
            }
            Decl::ExportBlock(e) => {
                if !is_snake_case(&e.node.name) {
                    emit_value(
                        &e.node.name,
                        e.span,
                        "export block",
                        file_label,
                        line_index,
                        bag,
                    );
                    bad.insert(e.span);
                }
                for p in &e.node.params {
                    check_param(p, file_label, line_index, bag, &mut bad);
                }
                if let Some(rt) = &e.node.return_type {
                    check_return_type(rt, file_label, line_index, bag, &mut bad);
                }
            }
            Decl::Const(c) => {
                if !is_snake_case(&c.node.name) {
                    emit_value(&c.node.name, c.span, "const", file_label, line_index, bag);
                    bad.insert(c.span);
                }
            }
            Decl::TypeDecl(t) => {
                if !is_pascal_case(&t.node.name) {
                    emit_type(&t.node.name, t.span, "type", file_label, line_index, bag);
                    bad.insert(t.span);
                }
            }
            Decl::Import(imp) => {
                // Import alias case-validation happens in the lib.rs import
                // resolution path where the kind tag (Task 9) is known.
                let _ = imp;
            }
        }
    }

    // Flow-local bindings (skill flow only — block flow rejects assignments).
    for decl in &file.decls {
        if let Decl::Skill(s) = decl {
            for stmt in &s.node.flow {
                walk_flow_for_case(stmt, file_label, line_index, bag, &mut bad);
            }
        }
    }
    bad
}

/// Recursively walks flow statements emitting `value-case-violation` for any
/// `<name> = call(...)` bound name that is not snake_case.
fn walk_flow_for_case(
    stmt: &FlowStmt,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    bad: &mut HashSet<Span>,
) {
    use crate::name_kind::is_snake_case;
    match stmt {
        FlowStmt::Call {
            bound_name: Some(bn),
            ..
        } => {
            if !is_snake_case(&bn.node) {
                let mut diag = Diagnostic::error(
                    crate::diagnostic::VALUE_CASE_VIOLATION_DIAG_ID,
                    format!("binding `{}` must be snake_case", bn.node),
                    SourceSpan::from_byte_span(file_label, bn.span, line_index),
                );
                diag.hints.push("rename to snake_case".into());
                bag.push(diag, bn.span);
                bad.insert(bn.span);
            }
        }
        FlowStmt::Branch {
            then_body,
            elif_branches,
            else_body,
            ..
        } => {
            for s in then_body {
                walk_flow_for_case(s, file_label, line_index, bag, bad);
            }
            for e in elif_branches {
                for s in &e.body {
                    walk_flow_for_case(s, file_label, line_index, bag, bad);
                }
            }
            if let Some(eb) = else_body {
                for s in eb {
                    walk_flow_for_case(s, file_label, line_index, bag, bad);
                }
            }
        }
        _ => {}
    }
}

/// Spec §"Behavior under the new rule" (Task 8): cross-kind canonical-equal
/// pairs are now legal under the two-namespace split. A `type Foo` (type
/// namespace) does not collide with `const foo`, `block foo`, a parameter
/// `foo`, or a flow-local `foo` (value namespace).
///
/// Preserved as a no-op so existing call sites stay valid until Task 9
/// cleanup. The value-namespace duplicate check now lives in
/// `sweep_value_name_collisions`; type-vs-type-import collisions live in
/// `sweep_type_decl_name_collisions`; type-vs-type-decl collisions are
/// emitted by `register_type_use` (`G::analyze::duplicate-type-decl`).
fn sweep_name_collisions(
    file: &SourceFile,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    registry: &crate::domain_registry::Registry,
    case_bad: &HashSet<Span>,
) {
    let _ = (file, file_label, line_index, bag, registry, case_bad);
}

/// Type-namespace duplicate check (Task 8 slim): a `type Foo` decl can only
/// collide with another type-namespace binding. The only remaining cross-decl
/// path is type-vs-type-import (a selective import whose local alias is
/// PascalCase, treated here as a proxy for "type-kinded import" until Task 9
/// plumbs `ResolvedImportKind` through). Type-vs-type-decl collisions are
/// already caught by `register_type_use` emitting
/// `G::analyze::duplicate-type-decl`.
///
/// Cross-kind matches (`type Foo` vs `const foo`, `block foo`, parameter
/// `foo`, flow-local `foo`, whole-module import `foo`) are LEGAL under the
/// two-namespace rule and are not flagged here.
fn sweep_type_decl_name_collisions(
    file: &SourceFile,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    registry: &crate::domain_registry::Registry,
    case_bad: &HashSet<Span>,
    type_alias_locals: &HashSet<String>,
) {
    use crate::domain_registry::canonicalize_identifier;
    let type_decls: Vec<(&str, Span)> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::TypeDecl(t) if !case_bad.contains(&t.span) => {
                Some((t.node.name.as_str(), t.span))
            }
            _ => None,
        })
        .collect();
    if type_decls.is_empty() {
        let _ = (registry, file_label, line_index, bag);
        return;
    }

    // Type-kinded selective imports — looked up by the real
    // `ResolvedImportKind` tag (Task 9). Whole-module imports always bind to
    // the value namespace and are excluded by construction in
    // `import_alias_kinds`.
    let mut type_imports: Vec<(String, Span)> = Vec::new();
    for decl in &file.decls {
        if let Decl::Import(imp) = decl {
            if let ast::ImportKind::Selective(names) = &imp.node.kind {
                for n in names {
                    let local_raw = n
                        .alias
                        .as_ref()
                        .map(|a| a.node.clone())
                        .unwrap_or_else(|| n.name.node.clone());
                    let local_span = n.alias.as_ref().map(|a| a.span).unwrap_or(n.name.span);
                    if case_bad.contains(&local_span) {
                        continue;
                    }
                    if type_alias_locals.contains(&local_raw) {
                        type_imports.push((local_raw, local_span));
                    }
                }
            }
        }
    }

    let _ = registry;

    for (tname, tspan) in &type_decls {
        let canonical = canonicalize_identifier(tname);
        for (iname, ispan) in &type_imports {
            if canonicalize_identifier(iname) == canonical {
                emit_type_decl_collision(
                    tname,
                    *tspan,
                    "type import",
                    iname,
                    *ispan,
                    file_label,
                    line_index,
                    bag,
                );
            }
        }
    }
}

/// Spec §"New `sweep_value_name_collisions`". Detects value-vs-value
/// canonical-key collisions across file-level value-namespace declarations
/// (consts, blocks, export blocks, skill name, value-kinded import aliases)
/// and per-skill flow scope (params + flow-local bindings).
///
/// Excludes any decl whose span is in `case_bad` (spec §"Diagnostic
/// precedence": case-violation short-circuits collision sweeps). Selective
/// type-imports are proxied by PascalCase until Task 9 plumbs
/// `ResolvedImportKind` through; PascalCase aliases are skipped here so they
/// do not contaminate the value namespace.
fn sweep_value_name_collisions(
    file: &SourceFile,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    case_bad: &HashSet<Span>,
    type_alias_locals: &HashSet<String>,
) {
    use crate::domain_registry::canonicalize_identifier;

    fn record(
        raw: &str,
        span: Span,
        kind: &str,
        bag: &mut DiagBag,
        seen: &mut HashMap<String, (String, Span)>,
        case_bad: &HashSet<Span>,
        file_label: &str,
        line_index: &LineIndex,
    ) {
        if case_bad.contains(&span) {
            return;
        }
        let c = canonicalize_identifier(raw);
        if let Some((prev_raw, prev_span)) = seen.get(&c).cloned() {
            let mut diag = Diagnostic::error(
                "G::analyze::name-collision",
                format!(
                    "{} `{}` collides with earlier `{}` (canonically equal)",
                    kind, raw, prev_raw
                ),
                SourceSpan::from_byte_span(file_label, span, line_index),
            );
            diag.related.push(SourceSpan::from_byte_span(
                file_label, prev_span, line_index,
            ));
            bag.push(diag, span);
        } else {
            seen.insert(c, (raw.to_string(), span));
        }
    }

    // (canonical, raw, span) — file-level value-namespace bindings.
    let mut seen: HashMap<String, (String, Span)> = HashMap::new();

    for decl in &file.decls {
        match decl {
            Decl::Const(c) => record(
                c.node.name.as_str(),
                c.span,
                "const",
                bag,
                &mut seen,
                case_bad,
                file_label,
                line_index,
            ),
            Decl::Block(b) => record(
                b.node.name.as_str(),
                b.span,
                "block",
                bag,
                &mut seen,
                case_bad,
                file_label,
                line_index,
            ),
            Decl::ExportBlock(b) => record(
                b.node.name.as_str(),
                b.span,
                "export block",
                bag,
                &mut seen,
                case_bad,
                file_label,
                line_index,
            ),
            Decl::Skill(s) => record(
                s.node.name.as_str(),
                s.span,
                "skill",
                bag,
                &mut seen,
                case_bad,
                file_label,
                line_index,
            ),
            Decl::Import(imp) => match &imp.node.kind {
                ast::ImportKind::Selective(names) => {
                    for n in names {
                        // Skip selective type-imports here — Task 9 lookup
                        // by `ResolvedImportKind`. Value-kinded aliases
                        // (constants, blocks) fall through to `record`.
                        let local_raw = n
                            .alias
                            .as_ref()
                            .map(|a| a.node.as_str())
                            .unwrap_or(n.name.node.as_str());
                        let local_span = n.alias.as_ref().map(|a| a.span).unwrap_or(n.name.span);
                        if type_alias_locals.contains(local_raw) {
                            continue;
                        }
                        record(
                            local_raw,
                            local_span,
                            "import alias",
                            bag,
                            &mut seen,
                            case_bad,
                            file_label,
                            line_index,
                        );
                    }
                }
                ast::ImportKind::WholeModule { alias } => {
                    record(
                        alias.node.as_str(),
                        alias.span,
                        "import alias",
                        bag,
                        &mut seen,
                        case_bad,
                        file_label,
                        line_index,
                    );
                }
            },
            Decl::TypeDecl(_) => {}
        }
    }

    // Per-skill scope: params + flow-local bindings, plus inherited
    // top-level value names (so a param can collide with a top-level const).
    for decl in &file.decls {
        match decl {
            Decl::Skill(s) => {
                let mut local_seen = seen.clone();
                for p in &s.node.params {
                    record(
                        p.name.as_str(),
                        p.span,
                        "parameter",
                        bag,
                        &mut local_seen,
                        case_bad,
                        file_label,
                        line_index,
                    );
                }
                for stmt in &s.node.flow {
                    walk_flow_for_value_bindings(
                        stmt,
                        "flow binding",
                        bag,
                        &mut local_seen,
                        file_label,
                        line_index,
                        case_bad,
                    );
                }
            }
            Decl::Block(b) => {
                let mut local_seen = seen.clone();
                for p in &b.node.params {
                    record(
                        p.name.as_str(),
                        p.span,
                        "parameter",
                        bag,
                        &mut local_seen,
                        case_bad,
                        file_label,
                        line_index,
                    );
                }
            }
            Decl::ExportBlock(e) => {
                let mut local_seen = seen.clone();
                for p in &e.node.params {
                    record(
                        p.name.as_str(),
                        p.span,
                        "parameter",
                        bag,
                        &mut local_seen,
                        case_bad,
                        file_label,
                        line_index,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Recursive walk to apply the value-namespace collision rule to flow-local
/// bindings (`<name> = call(...)`) inside a skill's `flow:`. Branch arms
/// inherit the enclosing skill's `seen` set so a binding in `then_body` can
/// still collide with a top-level const, a parameter, or an earlier binding
/// on the path. Sibling branch arms (`then` / `elif` / `else`) are scope-
/// isolated from each other and from the parent's `seen` — each arm gets a
/// clone, mirroring the per-arm `scope.child()` pattern used by
/// `walk_skill_flow_assign_checks`. Sequential statements at the same level
/// still share `seen` and therefore still collide.
fn walk_flow_for_value_bindings(
    stmt: &FlowStmt,
    kind: &str,
    bag: &mut DiagBag,
    seen: &mut HashMap<String, (String, Span)>,
    file_label: &str,
    line_index: &LineIndex,
    case_bad: &HashSet<Span>,
) {
    use crate::domain_registry::canonicalize_identifier;
    match stmt {
        FlowStmt::Call {
            bound_name: Some(bn),
            ..
        } => {
            if case_bad.contains(&bn.span) {
                return;
            }
            let c = canonicalize_identifier(&bn.node);
            if let Some((prev_raw, prev_span)) = seen.get(&c).cloned() {
                let mut diag = Diagnostic::error(
                    "G::analyze::name-collision",
                    format!(
                        "{} `{}` collides with earlier `{}` (canonically equal)",
                        kind, bn.node, prev_raw
                    ),
                    SourceSpan::from_byte_span(file_label, bn.span, line_index),
                );
                diag.related.push(SourceSpan::from_byte_span(
                    file_label, prev_span, line_index,
                ));
                bag.push(diag, bn.span);
            } else {
                seen.insert(c, (bn.node.clone(), bn.span));
            }
        }
        FlowStmt::Branch {
            then_body,
            elif_branches,
            else_body,
            ..
        } => {
            // Each branch arm is scope-isolated: clone `seen` per arm so
            // bindings in one arm cannot leak to sibling arms or to the
            // parent scope. Within a single arm, statements share the arm's
            // clone and therefore still collide sequentially.
            let mut arm_seen = seen.clone();
            for s in then_body {
                walk_flow_for_value_bindings(
                    s,
                    kind,
                    bag,
                    &mut arm_seen,
                    file_label,
                    line_index,
                    case_bad,
                );
            }
            for e in elif_branches {
                let mut arm_seen = seen.clone();
                for s in &e.body {
                    walk_flow_for_value_bindings(
                        s,
                        kind,
                        bag,
                        &mut arm_seen,
                        file_label,
                        line_index,
                        case_bad,
                    );
                }
            }
            if let Some(eb) = else_body {
                let mut arm_seen = seen.clone();
                for s in eb {
                    walk_flow_for_value_bindings(
                        s,
                        kind,
                        bag,
                        &mut arm_seen,
                        file_label,
                        line_index,
                        case_bad,
                    );
                }
            }
        }
        _ => {}
    }
}

/// Push one `G::analyze::name-collision` Error against a `type` decl.
/// Anchors the primary span on the type declaration (the binding site that
/// introduces the name) and uses `offender_span` as the related span.
fn emit_type_decl_collision(
    type_name: &str,
    type_span: Span,
    offender_kind: &str,
    offender_raw: &str,
    offender_span: Span,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    let primary = SourceSpan::from_byte_span(file_label, type_span, line_index);
    let related = SourceSpan::from_byte_span(file_label, offender_span, line_index);
    let mut diag = Diagnostic::error(
        "G::analyze::name-collision",
        format!(
            "type `{}` collides with {} `{}`",
            type_name, offender_kind, offender_raw
        ),
        primary,
    );
    diag.related.push(related);
    bag.push(diag, type_span);
}

/// Issue #84 Chunk 4 (AC4 / D14): emit `G::analyze::nominal-mismatch` Error
/// at a return-position call boundary when the callee's declared `-> Type`
/// does not canonical-match the enclosing callable's declared `-> Type`.
///
/// `primary_span` is the enclosing decl's span — synthetic-fallback option
/// (3) per `docs/reference/diagnostics.md` §Span Semantics. The AST has no
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

// ---------------------------------------------------------------------------
// Flow-position assignments — per-call helpers (§6.2 / §6.3).
// ---------------------------------------------------------------------------

/// Resolved return-type metadata for a flow-position assignment RHS.
///
/// Returned by [`resolve_callee_return_for_assign`] and consumed by the
/// per-call registration step.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedCalleeReturn {
    pub tag: TypeTag,
    pub raw_type: Spanned<String>,
    pub is_agent: bool,
}

/// Resolve the declared return type of a call target for the purpose of
/// the §6.2.b no-value check / FlowLocalType registration.
///
/// Resolution order:
/// 1. Same-file private/export blocks via `local_callee_return_types`.
/// 2. Imported blocks via `imported_block_return_types`.
/// 3. Stdlib registry via `crate::stdlib_sig` (§4 lib.rs touch row,
///    Codex Round 2 Med 5).
///
/// Returns `None` when the callee has no declared `-> Type`. The
/// per-call handler treats that as the no-value error.
pub(crate) fn resolve_callee_return_for_assign(
    target_name: &str,
    target_span: Span,
    local_callee_return_types: &HashMap<&str, &Spanned<String>>,
    imported_block_return_types: &HashMap<String, Spanned<String>>,
) -> Option<ResolvedCalleeReturn> {
    if let Some(rt) = local_callee_return_types.get(target_name) {
        let raw = (*rt).clone();
        let is_agent = raw.node.eq_ignore_ascii_case("Agent");
        let tag = if is_agent {
            TypeTag::Agent
        } else {
            TypeTag::DomainType(raw.node.clone())
        };
        return Some(ResolvedCalleeReturn {
            tag,
            raw_type: raw,
            is_agent,
        });
    }
    if let Some(rt) = imported_block_return_types.get(target_name) {
        let raw = rt.clone();
        let is_agent = raw.node.eq_ignore_ascii_case("Agent");
        let tag = if is_agent {
            TypeTag::Agent
        } else {
            TypeTag::DomainType(raw.node.clone())
        };
        return Some(ResolvedCalleeReturn {
            tag,
            raw_type: raw,
            is_agent,
        });
    }
    // Stdlib lookup. The `stdlib_sig` registry returns
    // `Some({return_type: Some("Agent"), is_agent: true})` for
    // `subagent` and `Some({return_type: None, ...})` for `send`.
    if let Some(sig) = crate::stdlib_sig(target_name) {
        let rt_text = sig.return_type?;
        let raw = Spanned {
            node: rt_text.to_string(),
            span: target_span,
        };
        let tag = if sig.is_agent {
            TypeTag::Agent
        } else {
            TypeTag::DomainType(rt_text.to_string())
        };
        return Some(ResolvedCalleeReturn {
            tag,
            raw_type: raw,
            is_agent: sig.is_agent,
        });
    }
    None
}

/// Per-call handler for the §6.2 bound-name registration / collision /
/// no-value checks. Mutates `scope` in place when registration succeeds.
///
/// `consts_in_scope` and `declared_texts_in_scope` collapse to one list
/// today (`text_names`) but the spec lists them separately, so we accept
/// the broader iterable to keep call sites readable.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_flow_assign(
    bound_name: &Spanned<String>,
    target: &Spanned<String>,
    container: ContainerKind,
    scope: &mut FlowScope,
    text_names: &HashSet<&str>,
    block_names: &HashSet<&str>,
    local_callee_return_types: &HashMap<&str, &Spanned<String>>,
    imported_block_return_types: &HashMap<String, Spanned<String>>,
    enclosing_span: Span,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    let n = &bound_name.node;
    let _ = enclosing_span;
    // Codex Round 3 High 2: skill-only.
    if container == ContainerKind::Block {
        let mut diag = Diagnostic::error(
            "G::analyze::flow-assign-in-block-unsupported",
            format!(
                "flow-position assignments are only supported in skill flow blocks (`{}` declared inside a block flow)",
                n
            ),
            SourceSpan::from_byte_span(file_label, bound_name.span, line_index),
        );
        diag.hints.push(
            "move the binding into the calling skill, or inline the call without `<name> =`"
                .to_string(),
        );
        bag.push(diag, bound_name.span);
        return;
    }
    // Collision check: any visible value-namespace name at this point
    // in scope (param, const, declared text, earlier flow-local). We
    // also reject collisions with same-file blocks/skills since those
    // are call targets and shadowing them with a binding would be
    // confusing — though parsing today rejects most of these.
    let collider_kind: Option<&'static str> = if scope.param_names.contains(n) {
        Some("parameter")
    } else if text_names.contains(n.as_str()) {
        Some("const")
    } else if scope.bound_names.contains(n) {
        Some("flow-local binding")
    } else if block_names.contains(n.as_str()) {
        Some("block")
    } else {
        None
    };
    if let Some(kind) = collider_kind {
        let mut diag = Diagnostic::error(
            "G::analyze::redeclared-flow-binding",
            format!("`{}` is already declared in this scope", n),
            SourceSpan::from_byte_span(file_label, bound_name.span, line_index),
        );
        diag.hints.push(format!(
            "`{}` is already declared as a {} in this scope",
            n, kind
        ));
        bag.push(diag, bound_name.span);
        // Skip registration on collision so downstream lookups behave
        // as if the binding never existed (the existing name still
        // resolves through its original namespace).
        return;
    }
    // No-value check.
    let resolved = resolve_callee_return_for_assign(
        target.node.as_str(),
        target.span,
        local_callee_return_types,
        imported_block_return_types,
    );
    let resolved = match resolved {
        Some(r) => r,
        None => {
            // Codex M5: when `target` does not resolve to ANY declared
            // block in this file or its imports, the legacy per-stmt
            // resolver in `analyze_skill` will fire
            // `G::analyze::undefined-call` (or `stdlib-missing-import`)
            // for the same call. Suppress the redundant
            // `assignment-rhs-has-no-value` so the user sees one root
            // cause, not two. The bound name is also intentionally
            // *not* registered into the scope below — without a return
            // type there is nothing for downstream `{name}` slots to
            // bind to anyway, and any consumer-side reference will
            // surface its own `unknown-param-slot` / `use-before-bind`.
            let target_is_known = block_names.contains(target.node.as_str());
            if !target_is_known {
                return;
            }
            let span = bound_name.span;
            let mut diag = Diagnostic::error(
                "G::analyze::assignment-rhs-has-no-value",
                format!(
                    "the right-hand side of a flow assignment must return a value (`{}` declares no return type)",
                    target.node
                ),
                SourceSpan::from_byte_span(file_label, span, line_index),
            );
            diag.hints.push(
                "this callee declares no return type — drop the `<name> =` or call a different block"
                    .to_string(),
            );
            bag.push(diag, span);
            return;
        }
    };
    // Register.
    scope.bound_names.insert(n.clone());
    scope.flow_local_types.insert(
        n.clone(),
        FlowLocalType {
            tag: resolved.tag,
            raw_type: resolved.raw_type,
            producer_span: bound_name.span,
            is_agent: resolved.is_agent,
        },
    );
}

/// Recursive flow walker that owns the flow-position-assignment checks:
/// inline-string slot validation, `handle_flow_assign` registration, the
/// `return <name>` resolution path, and (Codex H3) call-arg type checks
/// for arguments that name a flow-local binding.
///
/// Branch arms get a *child* `FlowScope` so the parent's bindings (params +
/// outer flow-locals) stay visible inside the arm but arm-local additions
/// do NOT leak back out (`.flow-assign-spec.md` §6.1 (X) "block-scoped per
/// branch arm"). The recursion mirrors the production AST shape exactly —
/// `then_body`, each `elif_branches[i].body`, and `else_body`. The same
/// per-arm checks that run at the skill flow root run inside each arm.
///
/// Notably this walker does NOT duplicate the per-call `undefined-call` /
/// `validate_call_args` / `check_return_call_undefined` /
/// `check_return_call_nominal` paths — those still live in the legacy
/// per-stmt match in `analyze_skill` for the root walk and in
/// `check_branch_body_names` for branch arms (their behaviour is unchanged).
#[allow(clippy::too_many_arguments)]
pub(crate) fn walk_skill_flow_assign_checks(
    flow: &[FlowStmt],
    scope: &mut FlowScope,
    container: ContainerKind,
    skill_name: &str,
    skill_return_type: Option<&Spanned<String>>,
    decl_span: Span,
    binding_trace: &SkillBindingTrace,
    text_names: &HashSet<&str>,
    block_names: &HashSet<&str>,
    block_decls: &HashMap<&str, &BlockDecl>,
    export_block_decls: &HashMap<&str, &crate::ast::ExportBlockDecl>,
    imported_block_params: &HashMap<String, Vec<crate::ast::Param>>,
    local_callee_return_types: &HashMap<&str, &Spanned<String>>,
    imported_block_return_types: &HashMap<String, Spanned<String>>,
    registry: &crate::domain_registry::Registry,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    for stmt in flow {
        match stmt {
            FlowStmt::InlineString(text) => {
                for slot in scan_slots(text) {
                    if !scope.param_names.contains(&slot.name)
                        && !scope.bound_names.contains(&slot.name)
                    {
                        let span = decl_span;
                        if binding_trace.all_names_ever_bound.contains(&slot.name) {
                            // §6.3 specialization: name bound elsewhere in
                            // this skill (in a sibling arm, in the
                            // post-branch tail, etc.) but not in scope here.
                            bag.push(
                                Diagnostic::error(
                                    "G::analyze::use-before-bind",
                                    format!("`{}` is not in scope here", slot.name),
                                    SourceSpan::from_byte_span(file_label, span, line_index),
                                ),
                                span,
                            );
                        } else {
                            bag.push(
                                Diagnostic::error(
                                    "G::analyze::unknown-param-slot",
                                    format!(
                                        "`{{{}}}` is not a declared parameter of `{}`",
                                        slot.name, skill_name
                                    ),
                                    SourceSpan::from_byte_span(file_label, span, line_index),
                                ),
                                span,
                            );
                        }
                    }
                }
            }
            FlowStmt::Call {
                target,
                args,
                bound_name,
                ..
            } => {
                // Codex H3: when a call's positional argument names a
                // flow-local binding whose recorded type is a domain
                // type, and the callee's param at the same index has a
                // domain `:Type` annotation, run a nominal match. On
                // mismatch emit `G::analyze::call-arg-type-mismatch`.
                // The mirror check at `return <name>` already lives in
                // the `FlowStmt::Return` arm below.
                //
                // Resolution order matches the legacy per-stmt match
                // (`block_decls` → `export_block_decls` → imported
                // export-block params). This walker fires the type
                // check only — undefined-call / `validate_call_args`
                // continue to run from the legacy resolver.
                let callee_params: Option<&[crate::ast::Param]> = block_decls
                    .get(target.node.as_str())
                    .map(|b| b.params.as_slice())
                    .or_else(|| {
                        export_block_decls
                            .get(target.node.as_str())
                            .map(|b| b.params.as_slice())
                    })
                    .or_else(|| {
                        imported_block_params
                            .get(target.node.as_str())
                            .map(|v| v.as_slice())
                    });
                if let Some(params) = callee_params {
                    for (i, arg) in args.iter().enumerate() {
                        let Some(param) = params.get(i) else {
                            continue;
                        };
                        let Some(param_ty) = param.type_annotation.as_ref() else {
                            continue;
                        };
                        let Some(flt) = scope.flow_local_types.get(arg) else {
                            continue;
                        };
                        if crate::type_position::validate_type_position(&param_ty.node).is_err() {
                            continue;
                        }
                        if crate::type_position::validate_type_position(&flt.raw_type.node).is_err()
                        {
                            continue;
                        }
                        if !registry.nominal_match(&param_ty.node, &flt.raw_type.node) {
                            bag.push(
                                Diagnostic::error(
                                    "G::analyze::call-arg-type-mismatch",
                                    format!(
                                        "argument `{}` to `{}()` has type `{}`, expected `{}`",
                                        arg, target.node, flt.raw_type.node, param_ty.node
                                    ),
                                    SourceSpan::from_byte_span(file_label, target.span, line_index),
                                ),
                                target.span,
                            );
                        }
                    }
                }

                // Flow-position assignments (§6.2): register the bound
                // name on success, or emit one of:
                // - G::analyze::flow-assign-in-block-unsupported (skill-only),
                // - G::analyze::redeclared-flow-binding,
                // - G::analyze::assignment-rhs-has-no-value.
                if let Some(name_spanned) = bound_name {
                    handle_flow_assign(
                        name_spanned,
                        target,
                        container,
                        scope,
                        text_names,
                        block_names,
                        local_callee_return_types,
                        imported_block_return_types,
                        decl_span,
                        file_label,
                        line_index,
                        bag,
                    );
                }
            }
            FlowStmt::Return(expr) => {
                if let crate::ast::ReturnExpr::Name(name) = expr {
                    if !scope.param_names.contains(&name.node)
                        && !text_names.contains(name.node.as_str())
                    {
                        if let Some(flt) = scope.flow_local_types.get(&name.node).cloned() {
                            if let Some(caller_rt) = skill_return_type {
                                if crate::type_position::validate_type_position(&caller_rt.node)
                                    .is_ok()
                                    && crate::type_position::validate_type_position(
                                        &flt.raw_type.node,
                                    )
                                    .is_ok()
                                    && !registry.nominal_match(&caller_rt.node, &flt.raw_type.node)
                                {
                                    emit_nominal_mismatch_at_return(
                                        name.node.as_str(),
                                        &caller_rt.node,
                                        &flt.raw_type.node,
                                        decl_span,
                                        caller_rt.span,
                                        file_label,
                                        line_index,
                                        bag,
                                    );
                                }
                            }
                        } else if binding_trace.all_names_ever_bound.contains(&name.node) {
                            // §6.3 specialization: name bound elsewhere
                            // in this skill (e.g. in a sibling arm) but
                            // not in scope here.
                            bag.push(
                                Diagnostic::error(
                                    "G::analyze::use-before-bind",
                                    format!("`{}` is not in scope here", name.node),
                                    SourceSpan::from_byte_span(file_label, name.span, line_index),
                                ),
                                name.span,
                            );
                        }
                    }
                }
            }
            FlowStmt::Branch {
                then_body,
                elif_branches,
                else_body,
                ..
            } => {
                let mut child = scope.child();
                walk_skill_flow_assign_checks(
                    then_body,
                    &mut child,
                    container,
                    skill_name,
                    skill_return_type,
                    decl_span,
                    binding_trace,
                    text_names,
                    block_names,
                    block_decls,
                    export_block_decls,
                    imported_block_params,
                    local_callee_return_types,
                    imported_block_return_types,
                    registry,
                    file_label,
                    line_index,
                    bag,
                );
                for elif in elif_branches {
                    let mut child = scope.child();
                    walk_skill_flow_assign_checks(
                        &elif.body,
                        &mut child,
                        container,
                        skill_name,
                        skill_return_type,
                        decl_span,
                        binding_trace,
                        text_names,
                        block_names,
                        block_decls,
                        export_block_decls,
                        imported_block_params,
                        local_callee_return_types,
                        imported_block_return_types,
                        registry,
                        file_label,
                        line_index,
                        bag,
                    );
                }
                if let Some(eb) = else_body {
                    let mut child = scope.child();
                    walk_skill_flow_assign_checks(
                        eb,
                        &mut child,
                        container,
                        skill_name,
                        skill_return_type,
                        decl_span,
                        binding_trace,
                        text_names,
                        block_names,
                        block_decls,
                        export_block_decls,
                        imported_block_params,
                        local_callee_return_types,
                        imported_block_return_types,
                        registry,
                        file_label,
                        line_index,
                        bag,
                    );
                }
            }
            // The remaining variants (BareName, ConstraintMarker,
            // ContextMarker) carry no flow-local-binding semantics; their
            // diagnostics still fire from the legacy walker in
            // `analyze_skill` / `check_branch_body_names`.
            _ => {}
        }
    }
}

/// Walk a block's flow and emit `G::analyze::flow-assign-in-block-unsupported`
/// for every flow-position assignment encountered. Block-flow assignments
/// are rejected at analyze time per Codex Round 3 High 2 (`.flow-assign-spec.md`
/// §6.1) — the existing private-block lowering has no producer node to
/// attach `bound_name` to.
pub(crate) fn check_block_flow_assign_rejected(
    flow: &[FlowStmt],
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    fn walk(stmts: &[FlowStmt], file_label: &str, line_index: &LineIndex, bag: &mut DiagBag) {
        for stmt in stmts {
            match stmt {
                FlowStmt::Call {
                    bound_name: Some(name_spanned),
                    ..
                } => {
                    let mut diag = Diagnostic::error(
                        "G::analyze::flow-assign-in-block-unsupported",
                        format!(
                            "flow-position assignments are only supported in skill flow blocks (`{}` declared inside a block flow)",
                            name_spanned.node
                        ),
                        SourceSpan::from_byte_span(file_label, name_spanned.span, line_index),
                    );
                    diag.hints.push(
                        "move the binding into the calling skill, or inline the call without `<name> =`"
                            .to_string(),
                    );
                    bag.push(diag, name_spanned.span);
                }
                FlowStmt::Branch {
                    then_body,
                    elif_branches,
                    else_body,
                    ..
                } => {
                    walk(then_body, file_label, line_index, bag);
                    for elif in elif_branches {
                        walk(&elif.body, file_label, line_index, bag);
                    }
                    if let Some(eb) = else_body {
                        walk(eb, file_label, line_index, bag);
                    }
                }
                _ => {}
            }
        }
    }
    walk(flow, file_label, line_index, bag);
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
/// fallback option 3 per `docs/reference/diagnostics.md` §Span Semantics).
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

/// Issue #82 follow-up / PRD #159 (Codex round-1 Issue 1): emit
/// `G::analyze::return-of-no-value-call` (Error) when a `return <call>`
/// targets a callee that resolves to a same-file block or imported
/// export block but declares no `-> Type`. Symmetric to
/// `G::analyze::assignment-rhs-has-no-value` at analyze.rs:1614.
///
/// Suppression: when the callee does not resolve to ANY declared block,
/// skip this diagnostic so the already-emitted `undefined-call` /
/// `stdlib-missing-import` surfaces alone (mirror M5 at
/// analyze.rs:1597-1612). The user sees one root cause, not two.
fn check_return_call_no_value(
    expr: &crate::ast::ReturnExpr,
    local_callee_return_types: &HashMap<&str, &Spanned<String>>,
    imported_block_return_types: &HashMap<String, Spanned<String>>,
    block_names: &HashSet<&str>,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    let crate::ast::ReturnExpr::Call { target, .. } = expr else {
        return;
    };
    if resolve_callee_return_for_assign(
        target.node.as_str(),
        target.span,
        local_callee_return_types,
        imported_block_return_types,
    )
    .is_some()
    {
        return;
    }
    if !block_names.contains(target.node.as_str()) {
        return;
    }
    let span = target.span;
    let mut diag = Diagnostic::error(
        "G::analyze::return-of-no-value-call",
        format!(
            "the return expression must produce a value (`{}` declares no return type)",
            target.node
        ),
        SourceSpan::from_byte_span(file_label, span, line_index),
    );
    diag.hints.push(
        "this callee declares no return type — drop the `return` or call a different block"
            .to_string(),
    );
    bag.push(diag, span);
}

/// Issue #82 follow-up / PRD #159: walker that runs
/// `check_return_call_no_value` on every `FlowStmt::Return` in a flow.
/// Recurses into branch arms for parity with
/// `walk_return_calls_nominal_check` even though the parser rejects
/// `return` inside branches (per design/data-flow.md §Return Semantics).
fn walk_return_of_no_value_call(
    flow: &[FlowStmt],
    local_callee_return_types: &HashMap<&str, &Spanned<String>>,
    imported_block_return_types: &HashMap<String, Spanned<String>>,
    block_names: &HashSet<&str>,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    for stmt in flow {
        match stmt {
            FlowStmt::Return(expr) => {
                check_return_call_no_value(
                    expr,
                    local_callee_return_types,
                    imported_block_return_types,
                    block_names,
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
                walk_return_of_no_value_call(
                    then_body,
                    local_callee_return_types,
                    imported_block_return_types,
                    block_names,
                    file_label,
                    line_index,
                    bag,
                );
                for elif in elif_branches {
                    walk_return_of_no_value_call(
                        &elif.body,
                        local_callee_return_types,
                        imported_block_return_types,
                        block_names,
                        file_label,
                        line_index,
                        bag,
                    );
                }
                if let Some(eb) = else_body {
                    walk_return_of_no_value_call(
                        eb,
                        local_callee_return_types,
                        imported_block_return_types,
                        block_names,
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

/// Issue #109 chunk 3 — Analyze invariant.
///
/// Walks every `Skill` / `BlockDecl` / `ExportBlockDecl` in `file.decls` and,
/// for each declaration whose `extra_subsections` is non-empty, emits a
/// single `G::analyze::unmerged-duplicate-subsection` diagnostic at error
/// tier (one per declaration; the natural fix unit is "rerun glyph fmt on
/// this file" — not a per-extras-entry edit).
///
/// Span attribution: the declaration node's own span. Naturally available,
/// matches the per-decl emission cardinality.
///
/// Called from both `analyze_with_diagnostics` and `analyze_with_imports`
/// (the two callers that walk the AST through the rest of Analyze) so the
/// invariant is uniformly enforced regardless of compile path.
fn check_unmerged_duplicate_subsections(
    file: &SourceFile,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    for decl in &file.decls {
        let (kind_label, name, span, extras_len) = match decl {
            Decl::Skill(s) if !s.node.extra_subsections.is_empty() => (
                "skill",
                s.node.name.as_str(),
                s.span,
                s.node.extra_subsections.len(),
            ),
            Decl::Block(b) if !b.node.extra_subsections.is_empty() => (
                "block",
                b.node.name.as_str(),
                b.span,
                b.node.extra_subsections.len(),
            ),
            Decl::ExportBlock(b) if !b.node.extra_subsections.is_empty() => (
                "export block",
                b.node.name.as_str(),
                b.span,
                b.node.extra_subsections.len(),
            ),
            _ => continue,
        };
        let plural = if extras_len == 1 { "" } else { "s" };
        bag.push(
            Diagnostic::error(
                "G::analyze::unmerged-duplicate-subsection",
                format!(
                    "{} `{}` carries {} unmerged duplicate sub-section{} — \
                     run `glyph fmt` to merge them",
                    kind_label, name, extras_len, plural
                ),
                SourceSpan::from_byte_span(file_label, span, line_index),
            ),
            span,
        );
    }
}

/// Validate that every `Param` whose default is a name reference
/// (`Param.default_is_name_ref == true`) resolves to an in-scope `const`.
///
/// Authors write a name_ref default with the same shape as a literal default
/// — `risk = default_risk` — but the parser cannot tell whether `default_risk`
/// names a `const`, a `block`, a parameter, or nothing. The lowerer
/// substitutes the const's rendered text into the IR; if the ref doesn't
/// resolve, the bare identifier leaks into `## Parameters` as
/// `Default: default_risk.` instead of the intended literal value.
///
/// This sweep emits `G::analyze::undefined-name` (matching the existing
/// flow-side `const` resolver in `analyze_skill_with_usage_tracking`) so the
/// fix-it surface stays consistent. `imported_texts` is the
/// import-aware lookup set (already includes both bare names from selective
/// imports and `alias.name` entries from whole-module imports); pass `None`
/// from the no-imports `analyze_with_diagnostics` path.
fn sweep_param_default_name_refs(
    file: &SourceFile,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    imported_texts: Option<&HashSet<String>>,
) {
    // Same-file `const` names form one half of the resolver's lookup set; the
    // other half is `imported_texts` (already qualified with `alias.` for
    // whole-module imports — see `lib.rs` `imported_texts.insert(...)` sites).
    let local_consts: HashSet<&str> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Const(c) => Some(c.node.name.as_str()),
            _ => None,
        })
        .collect();

    let resolves = |raw: &str| -> bool {
        if local_consts.contains(raw) {
            return true;
        }
        if let Some(set) = imported_texts {
            if set.contains(raw) {
                return true;
            }
        }
        false
    };

    let check_params = |params: &[crate::ast::Param], bag: &mut DiagBag| {
        for p in params {
            if !p.default_is_name_ref {
                continue;
            }
            let raw = match p.default.as_deref() {
                Some(s) => s,
                None => continue,
            };
            if resolves(raw) {
                continue;
            }
            bag.push(
                Diagnostic::error(
                    "G::analyze::undefined-name",
                    format!(
                        "parameter default `{}` does not resolve to an in-scope `const`",
                        raw
                    ),
                    SourceSpan::from_byte_span(file_label, p.span, line_index),
                ),
                p.span,
            );
        }
    };

    for decl in &file.decls {
        match decl {
            Decl::Skill(s) => check_params(&s.node.params, bag),
            Decl::Block(b) => check_params(&b.node.params, bag),
            Decl::ExportBlock(b) => check_params(&b.node.params, bag),
            _ => {}
        }
    }
}

/// Walks a `flow: Vec<FlowStmt>` body looking for any `FlowStmt::Return`
/// whose expression is value-producing (not `none` in any case, not bare
/// `return`). Used by `sweep_typed_decl_missing_return` for `Skill` and
/// `BlockDecl`, both of which carry structured `flow` and (unlike
/// `ExportBlockDecl`) have no precomputed `has_meaningful_return` field.
fn flow_has_meaningful_return(flow: &[FlowStmt]) -> bool {
    for stmt in flow {
        if let FlowStmt::Return(expr) = stmt {
            match expr {
                ReturnExpr::None => continue,
                ReturnExpr::Name(spanned) if spanned.node.eq_ignore_ascii_case("none") => {
                    continue;
                }
                _ => return true,
            }
        }
    }
    false
}

/// `G::analyze::typed-decl-missing-return` — fires when a `skill`, private
/// `block`, or `export block` declares `-> SomeType` but the body has no
/// value-producing `return`. Hard error (no LLM repair): a typed contract
/// must be honored by the author, not synthesized.
fn sweep_typed_decl_missing_return(
    file: &SourceFile,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    for decl in &file.decls {
        match decl {
            Decl::Skill(s) => {
                let rt = match s.node.return_type.as_ref() {
                    Some(rt) => rt,
                    None => continue,
                };
                if flow_has_meaningful_return(&s.node.flow) {
                    continue;
                }
                bag.push(
                    Diagnostic::error(
                        "G::analyze::typed-decl-missing-return",
                        format!(
                            "`skill {}` declares `-> {}` but has no explicit value-producing `return` statement",
                            s.node.name, rt.node
                        ),
                        SourceSpan::from_byte_span(file_label, rt.span, line_index),
                    ),
                    rt.span,
                );
            }
            Decl::Block(b) => {
                let rt = match b.node.return_type.as_ref() {
                    Some(rt) => rt,
                    None => continue,
                };
                if flow_has_meaningful_return(&b.node.flow) {
                    continue;
                }
                bag.push(
                    Diagnostic::error(
                        "G::analyze::typed-decl-missing-return",
                        format!(
                            "`block {}` declares `-> {}` but has no explicit value-producing `return` statement",
                            b.node.name, rt.node
                        ),
                        SourceSpan::from_byte_span(file_label, rt.span, line_index),
                    ),
                    rt.span,
                );
            }
            Decl::ExportBlock(b) => {
                let rt = match b.node.return_type.as_ref() {
                    Some(rt) => rt,
                    None => continue,
                };
                if b.node.has_meaningful_return {
                    continue;
                }
                bag.push(
                    Diagnostic::error(
                        "G::analyze::typed-decl-missing-return",
                        format!(
                            "`export block {}` declares `-> {}` but has no explicit value-producing `return` statement",
                            b.node.name, rt.node
                        ),
                        SourceSpan::from_byte_span(file_label, rt.span, line_index),
                    ),
                    rt.span,
                );
            }
            _ => {}
        }
    }
}

/// Run Phase 2 with diagnostic emission.
///
/// Pushes any structured diagnostics onto `bag` and returns the AST unchanged.
/// `file_label` and `line_index` follow the same contract as the parser entry
/// point (`docs/reference/diagnostics.md` §Span Semantics).
pub fn analyze_with_diagnostics(
    mut file: SourceFile,
    file_id: u32,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    registry: &mut crate::domain_registry::Registry,
) -> SourceFile {
    // Issue #109 chunk 3 — Analyze invariant: every declaration's
    // `extra_subsections` must be empty by the time Analyze runs. The parser
    // captures duplicate sub-sections into `extra_subsections` and emits
    // `G::parse::duplicate-subsection` (repairable). `glyph fmt` is then
    // contracted to merge extras back into the singleton field. If fmt is
    // skipped, Analyze must reject the AST so it never reaches Lower in a
    // state where extras matter.
    check_unmerged_duplicate_subsections(&file, file_label, line_index, bag);
    // Spec §"New pass `validate_identifier_case()`": hard-error on case
    // violations and collect flagged spans so downstream sweeps and the
    // type-registry walk skip them (avoids cascade diagnostics).
    let case_bad: HashSet<Span> = validate_identifier_case(&file, file_label, line_index, bag);
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

    // Phase 3 / Task 3.12 — map from string-const name to its body, used by
    // `check_skill_freeform_and_context_slots` to resolve `NameRef` items in
    // freeform sections / `context:` `NameRef` entries to the const text. Only
    // `ConstValue::String` participates here: other const kinds (Int / Float /
    // Bool) never carry `{slot}` syntax and never lower to instruction prose.
    let text_bodies: HashMap<&str, String> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Const(c) => match &c.node.value {
                crate::ast::ConstValue::String(s) => Some((c.node.name.as_str(), s.clone())),
                _ => None,
            },
            _ => None,
        })
        .collect();

    // Collect block declaration names for call resolution.
    // Includes both private `block` and `export block` so same-file calls to
    // export blocks resolve (PRD #103 / Slice 2 (#105)).
    let block_names: HashSet<&str> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Block(b) => Some(b.node.name.as_str()),
            Decl::ExportBlock(b) => Some(b.node.name.as_str()),
            _ => None,
        })
        .collect();

    // Collect context-only and constraint-only skill names (no imports path).
    let context_skill_names: HashSet<&str> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Skill(s) => {
                let sk = &s.node;
                if sk.body_constraints.is_empty() && sk.flow.is_empty() && !sk.flow_present {
                    Some(s.node.name.as_str())
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();
    let constraint_skill_names: HashSet<&str> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Skill(s) => {
                let sk = &s.node;
                if sk.context_section.is_empty()
                    && sk.body_context.is_empty()
                    && sk.flow.is_empty()
                    && !sk.flow_present
                {
                    Some(s.node.name.as_str())
                } else {
                    None
                }
            }
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

    // PRD #103 / Slice 2 (#105): same-file export-block call-arg validation.
    // The FlowStmt::Call resolver uses this map to verify each required
    // parameter is satisfied by a positional argument, mirroring the
    // private-block path.
    let export_block_decls: HashMap<&str, &crate::ast::ExportBlockDecl> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::ExportBlock(b) => Some((b.node.name.as_str(), &b.node)),
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
            Decl::TypeDecl(_) => None, // TODO: handled in Task B.4+
        })
        .collect();

    // Issue #84 Chunk 4 (AC4 / D13): per-file local-callee return-type map.
    // PRD #103 / #105 (Codex P2 follow-up): include `Decl::ExportBlock` too.
    // Same-file `export block`s are now legal call targets (see `block_names`
    // construction below), so a `return helper()` against a same-file export
    // block must run the same nominal-match check as a private-block target.
    // Pre-fix this map was Block-only, silently skipping the check for
    // export-block callees and allowing typed mismatches to compile.
    // `Decl::Skill` stays out because skills cannot be called from other
    // declarations' flow.
    let local_callee_return_types: HashMap<&str, &Spanned<String>> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Block(b) => b
                .node
                .return_type
                .as_ref()
                .map(|rt| (b.node.name.as_str(), rt)),
            Decl::ExportBlock(eb) => eb
                .node
                .return_type
                .as_ref()
                .map(|rt| (eb.node.name.as_str(), rt)),
            _ => None,
        })
        .collect();
    let empty_imported_block_return_types: HashMap<String, Spanned<String>> = HashMap::new();
    let empty_imported_block_params: HashMap<String, Vec<crate::ast::Param>> = HashMap::new();

    // Per-file set of canonical keys that have been observed as in-file
    // `type` declarations. Drives `G::analyze::duplicate-type-decl` for the
    // ExplicitDecl arm of `register_type_use`, so a selective type-import
    // alias entry in the registry does not falsely flag a local `type`
    // declaration as a duplicate (the alias-shadow case is handled by
    // `sweep_type_decl_name_collisions` instead).
    let mut explicit_decl_seen: HashSet<String> = HashSet::new();

    // Type-position registry pass (spec §"Unified implicit-type-registration
    // helper"): walk every type-position site in declaration order — explicit
    // `type Foo` decls first (so subsequent implicit uses don't fire spurious
    // `duplicate-type-decl`), then param `x: Foo` annotations. Header
    // `-> Foo` return annotations are registered later by
    // `warn_if_banned_return_type` during the per-decl walk below. The helper
    // canonicalizes per §D6 and emits:
    //  - `G::analyze::duplicate-type-decl` on a second `ExplicitDecl`,
    //  - `G::analyze::inconsistent-type-spelling` on raw-spelling drift,
    //  - nothing on first-use or idempotent same-spelling re-registration.
    // Must run before `sweep_name_collisions` /
    // `sweep_type_decl_name_collisions`, which read the populated registry.
    for d in &file.decls {
        if let Decl::TypeDecl(t) = d {
            if case_bad.contains(&t.span) {
                continue;
            }
            register_type_use(
                t.node.name.as_str(),
                t.span,
                TypeUseKind::ExplicitDecl,
                file_label,
                line_index,
                bag,
                registry,
                &mut explicit_decl_seen,
            );
        }
    }
    for decl in &file.decls {
        let params: &[ast::Param] = match decl {
            Decl::Skill(s) => &s.node.params,
            Decl::Block(b) => &b.node.params,
            Decl::ExportBlock(b) => &b.node.params,
            _ => continue,
        };
        for p in params {
            if let Some(ta) = &p.type_annotation {
                if case_bad.contains(&ta.span) {
                    continue;
                }
                if crate::type_position::validate_type_position(&ta.node).is_ok() {
                    register_type_use(
                        &ta.node,
                        ta.span,
                        TypeUseKind::ParamAnnotation,
                        file_label,
                        line_index,
                        bag,
                        registry,
                        &mut explicit_decl_seen,
                    );
                }
            }
        }
    }

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
                &text_bodies,
                &block_names,
                &block_decls,
                &export_block_decls,
                &empty_imported_block_params,
                &HashMap::new(),
                &local_callee_return_types,
                &empty_imported_block_return_types,
                &context_skill_names,
                &constraint_skill_names,
                &case_bad,
                &mut explicit_decl_seen,
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
                    &case_bad,
                    &mut explicit_decl_seen,
                    &text_names,
                    &block_names,
                    &block_decls,
                    &export_block_decls,
                    &empty_imported_block_params,
                    &local_callee_return_types,
                    &empty_imported_block_return_types,
                );
                // PRD #159 / Codex round-1 Issue 1: emit `return-of-no-value-call`
                // (Error) when an export block's `return <call>` targets a callee
                // that resolves but declares no `-> Type`. Symmetric to
                // `assignment-rhs-has-no-value`. Inspects `terminal_return` directly
                // since `ExportBlockDecl` has no `flow: Vec<FlowStmt>` (see ast.rs).
                if let Some(expr) = spanned.node.terminal_return.as_ref() {
                    check_return_call_no_value(
                        expr,
                        &local_callee_return_types,
                        &empty_imported_block_return_types,
                        &block_names,
                        file_label,
                        line_index,
                        bag,
                    );
                }
                // Phase 3 / Task 3.12 — block-scope `{param}` slot validation
                // for freeform colon-keyword sections (`quality:`, `risks:`,
                // …). Uses the export block's own param scope, not the
                // enclosing file's skills.
                check_block_freeform_slots(
                    &spanned.node.name,
                    &spanned.node.params,
                    &spanned.node.freeform_sections,
                    spanned.span,
                    &text_bodies,
                    file_label,
                    line_index,
                    bag,
                );

                // Phase 6 code-review fix — catalogue `cardinality = "one"`
                // enforcement (today: `[goal]`). The check is structurally
                // agnostic, so it applies to export blocks just as it does
                // to skills.
                check_section_cardinality(
                    &spanned.node.freeform_sections,
                    spanned.span,
                    file_label,
                    line_index,
                    bag,
                );
                check_duplicate_sections(
                    &spanned.node.freeform_sections,
                    file_label,
                    line_index,
                    bag,
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
                    &case_bad,
                    &mut explicit_decl_seen,
                );

                // G::analyze::export-missing-return-type — issue #161: broadened from
                // skill + export-block to also fire on private `block` decls whose body
                // has a meaningful `return <expr>` (where `<expr>` is not the `none`
                // value-keyword) and whose header lacks a `-> DomainType` annotation.
                // Same diagnostic ID and Repairable classification as the skill /
                // export-block fire-sites; message text identifies the decl kind.
                if flow_has_meaningful_return(&spanned.node.flow)
                    && spanned.node.return_type.is_none()
                {
                    let span = spanned.span;
                    bag.push(
                        Diagnostic {
                            id: "G::analyze::export-missing-return-type".into(),
                            classification: Classification::Repairable,
                            message: format!(
                                "`block {}` returns a meaningful value but its header lacks a `-> DomainType` annotation",
                                spanned.node.name
                            ),
                            span: SourceSpan::from_byte_span(file_label, span, line_index),
                            related: Vec::new(),
                            hints: vec![
                                "add a return-type annotation to the header — e.g. `block name(...) -> DomainType`"
                                    .into(),
                            ],
                        },
                        span,
                    );
                }

                // PRD #159 / Codex round-1 Issue 1: emit `return-of-no-value-call`
                // (Error) when a private block's `return <call>` targets a callee that
                // resolves but declares no `-> Type`. Symmetric to
                // `assignment-rhs-has-no-value`. Suppression + diagnostic shape live
                // in the helper.
                walk_return_of_no_value_call(
                    &spanned.node.flow,
                    &local_callee_return_types,
                    &empty_imported_block_return_types,
                    &block_names,
                    file_label,
                    line_index,
                    bag,
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
                // Flow-position assignments (§6.1, Codex Round 3 High 2):
                // block flow does not support `<name> = ...` assignments
                // in this MVP. Walk the block's flow and emit
                // `G::analyze::flow-assign-in-block-unsupported` for
                // every flow-position assignment encountered.
                check_block_flow_assign_rejected(&spanned.node.flow, file_label, line_index, bag);

                // Phase 3 / Task 3.12 — block-scope `{param}` slot validation
                // for freeform colon-keyword sections (`quality:`, `risks:`,
                // …). Uses the block's own param scope so a slot named after
                // the enclosing skill's parameter still fires here.
                check_block_freeform_slots(
                    &spanned.node.name,
                    &spanned.node.params,
                    &spanned.node.freeform_sections,
                    spanned.span,
                    &text_bodies,
                    file_label,
                    line_index,
                    bag,
                );

                // Phase 6 code-review fix — catalogue `cardinality = "one"`
                // enforcement (today: `[goal]`). The check is structurally
                // agnostic, so it applies to private blocks just as it does
                // to skills.
                check_section_cardinality(
                    &spanned.node.freeform_sections,
                    spanned.span,
                    file_label,
                    line_index,
                    bag,
                );
                check_duplicate_sections(
                    &spanned.node.freeform_sections,
                    file_label,
                    line_index,
                    bag,
                );
            }
            Decl::Const(_) => {}
            Decl::Import(_) => {}
            Decl::TypeDecl(_) => {} // TODO: handled in Task B.4+
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

    // Spec §"New `sweep_value_name_collisions`": value-vs-value canonical-key
    // collision sweep. Must run before the cross-kind sweeps so a same-kind
    // value collision is reported with the dedicated wording and a single
    // diagnostic rather than via the legacy generic sweeps.
    // No-imports path (`check_source` / single-file callers): the import
    // graph isn't resolved here, so kinds can't come from `dep_exports`.
    // Fall back to the PascalCase proxy for selective-import aliases so the
    // type-decl-vs-type-import collision sweep still fires single-file.
    let mut type_alias_locals: HashSet<String> = HashSet::new();
    for decl in &file.decls {
        if let Decl::Import(imp) = decl {
            if let ast::ImportKind::Selective(names) = &imp.node.kind {
                for n in names {
                    let local = n
                        .alias
                        .as_ref()
                        .map(|a| a.node.as_str())
                        .unwrap_or(n.name.node.as_str());
                    if crate::name_kind::is_pascal_case(local) {
                        type_alias_locals.insert(local.to_string());
                    }
                }
            }
        }
    }
    sweep_value_name_collisions(
        &file,
        file_label,
        line_index,
        bag,
        &case_bad,
        &type_alias_locals,
    );
    // Issue #84 Chunk 3 (AC5): domain-type-vs-param/const collision sweep.
    sweep_name_collisions(&file, file_label, line_index, bag, registry, &case_bad);
    // Universal-namespace check (`design/values-and-names.md` §No-Shadowing):
    // type-decl-vs-param/const/block collision sweep, complementary to the
    // registry-direction sweep above.
    sweep_type_decl_name_collisions(
        &file,
        file_label,
        line_index,
        bag,
        registry,
        &case_bad,
        &type_alias_locals,
    );
    // Reject name_ref param defaults that don't resolve to an in-scope `const`
    // (Codex finding #1 follow-up): without this sweep an unresolved ref like
    // `risk = default_risk` (when `default_risk` is a block / unknown name)
    // leaks into the lowerer's IR as the bare identifier.
    sweep_param_default_name_refs(&file, file_label, line_index, bag, None);
    sweep_typed_decl_missing_return(&file, file_label, line_index, bag);

    // Library detection: file with zero skills.
    let has_skill = file.decls.iter().any(|d| matches!(d, Decl::Skill(_)));
    if !has_skill {
        let has_export = file.decls.iter().any(|d| {
            matches!(d, Decl::ExportBlock(_))
                || matches!(d, Decl::Const(c) if c.node.exported)
                || matches!(d, Decl::TypeDecl(t) if t.node.exported)
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

    // Task 2.4: annotate every Branch/ElifBranch with a ConditionClassification.
    // No imports in scope here — pass empty maps so the classifier reduces
    // to the same-file case (consts, params, bindings only).
    let empty_text_values: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let empty_const_types: std::collections::BTreeMap<String, crate::kind_infer::TypeTag> =
        std::collections::BTreeMap::new();
    annotate_file_branches(&mut file, &empty_text_values, &empty_const_types);

    // Task 3.1: emit G::analyze::condition-non-boolean-non-predicate for
    // numeric-kinded tokens in condition position.
    check_file_numeric_conditions(&file, file_label, line_index, bag);

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
                            if imp_name.name.node == "subagent" || imp_name.name.node == "send" {
                                let local = imp_name
                                    .alias
                                    .as_ref()
                                    .map(|a| a.node.clone())
                                    .unwrap_or_else(|| imp_name.name.node.clone());
                                stdlib_names.insert(local, imp_name.name.span);
                            }
                        }
                    }
                }
            }
            Decl::TypeDecl(_) => {} // TODO: handled in Task B.4+
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
                    &skill_defs,
                    &stdlib_names,
                    &mut out,
                );
                for marker in &skill.body_constraints {
                    record_text_use(
                        &marker.name.node,
                        marker.name.span,
                        &text_defs,
                        file_path,
                        &mut out,
                    );
                }
                for entry in skill
                    .body_context
                    .iter()
                    .chain(skill.context_section.iter())
                {
                    if let ContextEntry::NameRef(name) = entry {
                        record_context_name_use(
                            &name.node,
                            name.span,
                            &text_defs,
                            &block_defs,
                            &export_block_defs,
                            &skill_defs,
                            file_path,
                            &mut out,
                        );
                    }
                }
                // body_bare_names are plain Strings without span info; skip for resolution.
            }
            Decl::Block(spanned) => {
                let block = &spanned.node;
                walk_flow_for_resolutions(
                    &block.flow,
                    file_path,
                    &text_defs,
                    &block_defs,
                    &export_block_defs,
                    &skill_defs,
                    &stdlib_names,
                    &mut out,
                );
                for marker in &block.body_constraints {
                    record_text_use(
                        &marker.name.node,
                        marker.name.span,
                        &text_defs,
                        file_path,
                        &mut out,
                    );
                }
                for entry in &block.body_context {
                    if let ContextEntry::NameRef(name) = entry {
                        record_context_name_use(
                            &name.node,
                            name.span,
                            &text_defs,
                            &block_defs,
                            &export_block_defs,
                            &skill_defs,
                            file_path,
                            &mut out,
                        );
                    }
                }
            }
            Decl::ExportBlock(spanned) => {
                // Issue #166: body-level `require/avoid/must` and `context` markers
                // (and their sub-section bodies) on an `export block` use the same
                // resolution surface as `BlockDecl`. The flow body itself still
                // isn't lowered yet — once §13 ships, walk `flow` here too.
                let block = &spanned.node;
                for marker in &block.body_constraints {
                    record_text_use(
                        &marker.name.node,
                        marker.name.span,
                        &text_defs,
                        file_path,
                        &mut out,
                    );
                }
                for entry in &block.body_context {
                    if let ContextEntry::NameRef(name) = entry {
                        record_context_name_use(
                            &name.node,
                            name.span,
                            &text_defs,
                            &block_defs,
                            &export_block_defs,
                            &skill_defs,
                            file_path,
                            &mut out,
                        );
                    }
                }
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
                            if imp_name.name.node == "subagent" || imp_name.name.node == "send" {
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
            Decl::TypeDecl(_) => {} // TODO: handled in Task B.4+
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
                for entry in skill
                    .body_context
                    .iter()
                    .chain(skill.context_section.iter())
                {
                    if let ContextEntry::NameRef(name) = entry {
                        record_cross_file_any_use(name, targets, &mut out);
                    }
                }
                // body_bare_names are plain Strings without span info; skip for cross-file resolution.
            }
            Decl::Block(spanned) => {
                let block = &spanned.node;
                walk_flow_for_cross_file(&block.flow, targets, &mut out);
                for marker in &block.body_constraints {
                    record_cross_file_text_use(&marker.name, targets, &mut out);
                }
                for entry in &block.body_context {
                    if let ContextEntry::NameRef(name) = entry {
                        record_cross_file_any_use(name, targets, &mut out);
                    }
                }
            }
            Decl::ExportBlock(spanned) => {
                // Issue #166: cross-file resolution mirrors the same-file walker —
                // an `export block` body can reference imported names through its
                // body-level `require/avoid/must` and `context` markers.
                let block = &spanned.node;
                for marker in &block.body_constraints {
                    record_cross_file_text_use(&marker.name, targets, &mut out);
                }
                for entry in &block.body_context {
                    if let ContextEntry::NameRef(name) = entry {
                        record_cross_file_any_use(name, targets, &mut out);
                    }
                }
            }
            Decl::Const(_) => {}
            Decl::TypeDecl(_) => {} // TODO: handled in Task B.4+
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
                            .as_ref()
                            .map(|a| a.node.clone())
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

/// Resolve a context-entry name reference. Tries text, block, export block,
/// and skill defs in that order — context entries can point to any of these.
fn record_context_name_use(
    name: &str,
    use_span: Span,
    text_defs: &HashMap<&str, Span>,
    block_defs: &HashMap<&str, Span>,
    export_block_defs: &HashMap<&str, Span>,
    skill_defs: &HashMap<&str, Span>,
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
    } else if let Some(def_span) = block_defs.get(name) {
        out.push(Resolution {
            use_span,
            def_span: *def_span,
            def_file: file_path.clone(),
            kind: ResolutionKind::Block,
        });
    } else if let Some(def_span) = export_block_defs.get(name) {
        out.push(Resolution {
            use_span,
            def_span: *def_span,
            def_file: file_path.clone(),
            kind: ResolutionKind::ExportBlock,
        });
    } else if let Some(def_span) = skill_defs.get(name) {
        out.push(Resolution {
            use_span,
            def_span: *def_span,
            def_file: file_path.clone(),
            kind: ResolutionKind::Skill,
        });
    }
}

fn walk_flow_for_resolutions(
    stmts: &[FlowStmt],
    file_path: &PathBuf,
    text_defs: &HashMap<&str, Span>,
    block_defs: &HashMap<&str, Span>,
    export_block_defs: &HashMap<&str, Span>,
    skill_defs: &HashMap<&str, Span>,
    stdlib_names: &HashMap<String, Span>,
    out: &mut Vec<Resolution>,
) {
    for stmt in stmts {
        match stmt {
            FlowStmt::Call { target, .. } => {
                record_call_target(
                    target,
                    file_path,
                    block_defs,
                    export_block_defs,
                    stdlib_names,
                    out,
                );
            }
            FlowStmt::ConstraintMarker(marker) => {
                record_text_use(
                    &marker.name.node,
                    marker.name.span,
                    text_defs,
                    file_path,
                    out,
                );
            }
            FlowStmt::ContextMarker(entry) => {
                if let ContextEntry::NameRef(name) = entry {
                    // `context X` at flow level must resolve to the same
                    // target set as body-level / top-level `context: X` —
                    // text | block | export-block | skill. Calling the
                    // text-only helper here previously under-resolved local
                    // blocks (see issue #165 P2). String entries don't
                    // resolve.
                    record_context_name_use(
                        &name.node,
                        name.span,
                        text_defs,
                        block_defs,
                        export_block_defs,
                        skill_defs,
                        file_path,
                        out,
                    );
                }
            }
            FlowStmt::BareName(name) => {
                record_text_use(&name.node, name.span, text_defs, file_path, out);
            }
            FlowStmt::Return(ReturnExpr::Call { target, .. }) => {
                record_call_target(
                    target,
                    file_path,
                    block_defs,
                    export_block_defs,
                    stdlib_names,
                    out,
                );
            }
            FlowStmt::Return(ReturnExpr::Name(name)) => {
                record_text_use(&name.node, name.span, text_defs, file_path, out);
            }
            FlowStmt::Branch {
                then_body,
                elif_branches,
                else_body,
                condition_refs,
                ..
            } => {
                // Branch-condition references: an imported or local name used
                // only as `if name(...)`, `if name`, or `if name.applies()`
                // must produce a Resolution so goto-def works on the use
                // site. The parser filtered `condition_refs` down to real
                // reference candidates (no `not`/`and`/`or`, no `applies`
                // method-name, no other dotted-method ident). Dispatch each
                // through both helpers — they are no-ops on miss, so the
                // first matching def-pool wins, mirroring how `Call` vs
                // `BareName` are routed elsewhere in this walker.
                for r in condition_refs
                    .iter()
                    .chain(elif_branches.iter().flat_map(|e| e.condition_refs.iter()))
                {
                    record_call_target(
                        r,
                        file_path,
                        block_defs,
                        export_block_defs,
                        stdlib_names,
                        out,
                    );
                    record_text_use(&r.node, r.span, text_defs, file_path, out);
                }
                walk_flow_for_resolutions(
                    then_body,
                    file_path,
                    text_defs,
                    block_defs,
                    export_block_defs,
                    skill_defs,
                    stdlib_names,
                    out,
                );
                for elif in elif_branches {
                    walk_flow_for_resolutions(
                        &elif.body,
                        file_path,
                        text_defs,
                        block_defs,
                        export_block_defs,
                        skill_defs,
                        stdlib_names,
                        out,
                    );
                }
                if let Some(eb) = else_body {
                    walk_flow_for_resolutions(
                        eb,
                        file_path,
                        text_defs,
                        block_defs,
                        export_block_defs,
                        skill_defs,
                        stdlib_names,
                        out,
                    );
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
                    // Mirror of the same-file fix above: imported `context X`
                    // must resolve regardless of whether the import target
                    // is Text / Block / ExportBlock / Skill. The text-only
                    // helper previously silently dropped block imports.
                    record_cross_file_any_use(name, targets, out);
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
            FlowStmt::Branch {
                then_body,
                elif_branches,
                else_body,
                condition_refs,
                ..
            } => {
                // Cross-file mirror of the same-file branch-condition sweep
                // in `walk_flow_for_resolutions`. An imported name used only
                // as `if imported_name(...)` / `if imported_const` /
                // `if imported_name.applies()` lands here when its def lives
                // in a sibling .glyph. Dispatch each filtered ref through
                // both cross-file helpers.
                for r in condition_refs
                    .iter()
                    .chain(elif_branches.iter().flat_map(|e| e.condition_refs.iter()))
                {
                    record_cross_file_call(r, targets, out);
                    record_cross_file_text_use(r, targets, out);
                }
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

/// Like [`record_cross_file_text_use`] but accepts any resolution kind — used
/// for context entries which can reference skills, blocks, or text constants.
fn record_cross_file_any_use(
    name: &Spanned<String>,
    targets: &HashMap<String, ImportTarget>,
    out: &mut Vec<Resolution>,
) {
    if let Some(t) = targets.get(&name.node) {
        out.push(Resolution {
            use_span: name.span,
            def_span: t.def_span,
            def_file: t.def_file.clone(),
            kind: t.kind,
        });
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
    imported_context_skills: &HashSet<String>,
    imported_constraint_skills: &HashSet<String>,
    used_import_names: &mut HashSet<String>,
    imported_block_descriptions: &HashMap<String, String>,
    registry: &mut crate::domain_registry::Registry,
    imported_block_return_types: &HashMap<String, Spanned<String>>,
    imported_block_params: &HashMap<String, Vec<crate::ast::Param>>,
    // Task 6 — rendered import bodies + their inferred TypeTags. Threaded
    // into the condition classifier (`condition::ConditionContext`) so a
    // bare-imported-name in a branch condition lands in `PredicateConst`
    // (String) vs `Boolean` / `Numeric` (Bool/Int/Float) per the imported
    // const's actual kind. Empty maps give MVP behaviour (every imported
    // name treated as String) — see `condition::collect_consts_for_file`.
    imported_text_values: &std::collections::BTreeMap<String, String>,
    imported_const_types: &std::collections::BTreeMap<String, crate::kind_infer::TypeTag>,
    // B.5 / spec §"Unified implicit-type-registration helper": consumer-local
    // type-import alias spans, keyed by the consumer-side local name
    // (post-alias when `as Bar` is present, else the producer's exported
    // name). Each entry registers as a `TypeUseKind::SelectiveImport` so the
    // type-position drift sweep (`inconsistent-type-spelling`) sees the
    // imported spelling as the first-use anchor. Whole-module imports do
    // NOT contribute — qualified `alias.Type` refs are MVP-unsupported.
    imported_type_spans: &HashMap<String, Span>,
    // Task 9: per-import-alias resolved namespace kind. Keys are
    // consumer-local names (selective: post-alias spelling; whole-module:
    // the bare alias). Drives the alias-case rule and the kind-aware
    // lookups in the value/type-decl collision sweeps. Empty in the
    // no-imports path (`analyze_with_diagnostics`).
    import_alias_kinds: &HashMap<String, (crate::name_kind::ResolvedImportKind, Span)>,
) -> SourceFile {
    // Issue #109 chunk 3 — Analyze invariant. Enforced on the import-aware
    // path too so multi-file compiles get identical guarantees. See
    // `check_unmerged_duplicate_subsections` doc-comment for rationale.
    check_unmerged_duplicate_subsections(file, file_label, line_index, bag);

    // Spec §"New pass `validate_identifier_case()`": hard-error on case
    // violations and collect flagged spans so downstream sweeps and the
    // type-registry walk skip them (avoids cascade diagnostics).
    let mut case_bad: HashSet<Span> = validate_identifier_case(file, file_label, line_index, bag);

    // Task 9: alias-case rule. Import aliases inherit the kind of the
    // imported declaration (selective: Type if the dep exports a type by
    // that name, Value otherwise; whole-module: always Value). Type aliases
    // must be strict PascalCase; value aliases must be strict snake_case.
    // Flagged alias spans join `case_bad` so subsequent kind-aware sweeps
    // skip them.
    for (local, (kind, span)) in import_alias_kinds {
        let ok = match kind {
            crate::name_kind::ResolvedImportKind::Type => crate::name_kind::is_pascal_case(local),
            crate::name_kind::ResolvedImportKind::Value => crate::name_kind::is_snake_case(local),
        };
        if ok {
            continue;
        }
        if case_bad.contains(span) {
            continue;
        }
        let (id, message) = match kind {
            crate::name_kind::ResolvedImportKind::Type => (
                crate::diagnostic::TYPE_CASE_VIOLATION_DIAG_ID,
                format!("type-import alias `{}` must be PascalCase", local),
            ),
            crate::name_kind::ResolvedImportKind::Value => (
                crate::diagnostic::VALUE_CASE_VIOLATION_DIAG_ID,
                format!("value-import alias `{}` must be snake_case", local),
            ),
        };
        bag.push(
            Diagnostic::error(
                id,
                message,
                SourceSpan::from_byte_span(file_label, *span, line_index),
            ),
            *span,
        );
        case_bad.insert(*span);
    }

    // Per-file set of canonical keys that have been observed as in-file
    // `type` declarations. See `analyze_with_diagnostics` for rationale —
    // mirrors that pass.
    let mut explicit_decl_seen: HashSet<String> = HashSet::new();

    // B.5 / spec §"Unified implicit-type-registration helper": register every
    // selective type-import under the consumer-local name first, so the
    // imported spelling anchors the registry and subsequent same-file param /
    // return-type / explicit-decl uses with a drifted raw spelling fire
    // `G::analyze::inconsistent-type-spelling`. Must precede the per-file
    // type-position pass (TypeDecl + param walk) below.
    for (name, span) in imported_type_spans {
        register_type_use(
            name,
            *span,
            TypeUseKind::SelectiveImport,
            file_label,
            line_index,
            bag,
            registry,
            &mut explicit_decl_seen,
        );
    }

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

    // Phase 3 / Task 3.12 — same shape as the `analyze_with_diagnostics` map,
    // imports path. Only local `ConstValue::String` participates; resolution
    // of imported-text bodies for slot scanning is out of scope today
    // (imported `NameRef` items still produce a `text_names`-only check via
    // the existing `check_context_entry_name` path).
    let text_bodies: HashMap<&str, String> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Const(c) => match &c.node.value {
                crate::ast::ConstValue::String(s) => Some((c.node.name.as_str(), s.clone())),
                _ => None,
            },
            _ => None,
        })
        .collect();

    // Collect local block declaration names.
    // Includes both private `block` and `export block` so same-file calls to
    // export blocks resolve (PRD #103 / Slice 2 (#105)).
    let local_block_names: HashSet<&str> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Block(b) => Some(b.node.name.as_str()),
            Decl::ExportBlock(b) => Some(b.node.name.as_str()),
            _ => None,
        })
        .collect();

    // Build local context-only and constraint-only skill name sets.
    let local_context_skill_names: HashSet<&str> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Skill(s) => {
                let sk = &s.node;
                if sk.body_constraints.is_empty() && sk.flow.is_empty() && !sk.flow_present {
                    Some(s.node.name.as_str())
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();
    let local_constraint_skill_names: HashSet<&str> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Skill(s) => {
                let sk = &s.node;
                if sk.context_section.is_empty()
                    && sk.body_context.is_empty()
                    && sk.flow.is_empty()
                    && !sk.flow_present
                {
                    Some(s.node.name.as_str())
                } else {
                    None
                }
            }
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

    let mut context_skill_names: HashSet<&str> = local_context_skill_names;
    let imported_context_skill_refs: Vec<String> =
        imported_context_skills.iter().cloned().collect();
    for s in &imported_context_skill_refs {
        context_skill_names.insert(s.as_str());
    }

    let mut constraint_skill_names: HashSet<&str> = local_constraint_skill_names;
    let imported_constraint_skill_refs: Vec<String> =
        imported_constraint_skills.iter().cloned().collect();
    for s in &imported_constraint_skill_refs {
        constraint_skill_names.insert(s.as_str());
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

    // PRD #103 / Slice 2 (#105): same-file export-block decls for call-arg
    // validation. Mirrors the `block_decls` map above; cross-file imported
    // export-block params are wired separately via Slice C.
    let export_block_decls: HashMap<&str, &crate::ast::ExportBlockDecl> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::ExportBlock(b) => Some((b.node.name.as_str(), &b.node)),
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
            Decl::TypeDecl(_) => None, // TODO: handled in Task B.4+
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
    // — plus same-file `Decl::ExportBlock` per the PRD #103 / #105 Codex
    // P2 follow-up: same-file export blocks are now legal call targets,
    // so a `return helper()` against one must run the nominal-match
    // check just like a private-block target. Cross-file export-block
    // matching is owned by `imported_block_return_types`. Keyed by
    // callable name; valued by the `-> Type` annotation. Populated for
    // callables that declare a return type only — absence means "skip
    // the type-check" (covers undefined-callee and untyped-callee).
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
            Decl::ExportBlock(eb) => eb
                .node
                .return_type
                .as_ref()
                .map(|rt| (eb.node.name.as_str(), rt)),
            _ => None,
        })
        .collect();

    // Type-position registry pass — imports-path parity with
    // `analyze_with_diagnostics`. Walk explicit `type Foo` decls first, then
    // param `x: Foo` annotations. Header `-> Foo` annotations register later
    // via `warn_if_banned_return_type` during the per-decl walk below. Must
    // run before `sweep_name_collisions` / `sweep_type_decl_name_collisions`.
    for d in &file.decls {
        if let Decl::TypeDecl(t) = d {
            if case_bad.contains(&t.span) {
                continue;
            }
            register_type_use(
                t.node.name.as_str(),
                t.span,
                TypeUseKind::ExplicitDecl,
                file_label,
                line_index,
                bag,
                registry,
                &mut explicit_decl_seen,
            );
        }
    }
    for decl in &file.decls {
        let params: &[ast::Param] = match decl {
            Decl::Skill(s) => &s.node.params,
            Decl::Block(b) => &b.node.params,
            Decl::ExportBlock(b) => &b.node.params,
            _ => continue,
        };
        for p in params {
            if let Some(ta) = &p.type_annotation {
                if case_bad.contains(&ta.span) {
                    continue;
                }
                if crate::type_position::validate_type_position(&ta.node).is_ok() {
                    register_type_use(
                        &ta.node,
                        ta.span,
                        TypeUseKind::ParamAnnotation,
                        file_label,
                        line_index,
                        bag,
                        registry,
                        &mut explicit_decl_seen,
                    );
                }
            }
        }
    }

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
                    &text_bodies,
                    &block_names,
                    &block_decls,
                    &export_block_decls,
                    imported_block_params,
                    imported_texts,
                    imported_blocks,
                    imported_context_skills,
                    imported_constraint_skills,
                    used_import_names,
                    imported_block_descriptions,
                    &local_callee_return_types,
                    imported_block_return_types,
                    &context_skill_names,
                    &constraint_skill_names,
                    &case_bad,
                    &mut explicit_decl_seen,
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
                    &case_bad,
                    &mut explicit_decl_seen,
                    &text_names,
                    &block_names,
                    &block_decls,
                    &export_block_decls,
                    imported_block_params,
                    &local_callee_return_types,
                    imported_block_return_types,
                );

                // B03 GAP 4: track imported-name usage from export-block flow content.
                // `ExportBlockDecl` has no `flow: Vec<FlowStmt>`, so the per-Skill /
                // per-Block `track_flow_usage` sweep does not reach it. Without this
                // local mirror, an import consumed ONLY inside an export block's
                // `terminal_return`, `flow_calls`, `body_constraints`, or
                // `body_context` would leave `used_import_names` empty and the
                // lib.rs unused-import emission step would fire
                // `G::analyze::unused-import` (Repairable, exit 2) on an import the
                // program actually depends on. Symmetric in spirit to
                // `track_flow_usage` for FlowStmt::Return / Call / ConstraintMarker /
                // ContextMarker.
                {
                    let eb = &spanned.node;
                    // terminal_return — `return foo()` / `return foo`.
                    match eb.terminal_return.as_ref() {
                        Some(crate::ast::ReturnExpr::Call { target, .. }) => {
                            if imported_blocks.contains(&target.node) {
                                used_import_names.insert(target.node.clone());
                            }
                        }
                        Some(crate::ast::ReturnExpr::Name(name)) => {
                            if imported_blocks.contains(&name.node)
                                || imported_texts.contains(&name.node)
                            {
                                used_import_names.insert(name.node.clone());
                            }
                        }
                        _ => {}
                    }
                    // flow_calls — non-return flow-position calls collected in GAP 1.
                    for call in &eb.flow_calls {
                        if imported_blocks.contains(&call.target.node) {
                            used_import_names.insert(call.target.node.clone());
                        }
                    }
                    // body_constraints — `require X` / `avoid X` / `must X`.
                    for marker in &eb.body_constraints {
                        if imported_texts.contains(&marker.name.node)
                            || imported_constraint_skills.contains(&marker.name.node)
                        {
                            used_import_names.insert(marker.name.node.clone());
                        }
                    }
                    // body_context — `context X` / `context "..."` entries.
                    for entry in &eb.body_context {
                        if let crate::ast::ContextEntry::NameRef(name) = entry {
                            if imported_texts.contains(&name.node)
                                || imported_context_skills.contains(&name.node)
                            {
                                used_import_names.insert(name.node.clone());
                            }
                        }
                    }
                    // B03 GAP 5: condition_refs — `if`/`elif` condition expressions.
                    // Two responsibilities run in this sweep so we stay inside the
                    // single scope where `imported_block_descriptions`, `text_names`,
                    // `block_names`, and `block_decls` are all in scope:
                    //
                    // 1. Import-usage tracking: any imported name (block, text,
                    //    constraint-skill, context-skill) that appears as an identifier
                    //    inside a condition expression counts as a use. Without this,
                    //    an `import { ready } ... if ready.applies():` would leave
                    //    `used_import_names` empty and lib.rs would fire
                    //    `G::analyze::unused-import` (Repairable, exit 2) on `ready`.
                    //
                    // 2. `.applies()` validation: invoke `check_applies_in_condition`
                    //    on each captured condition string so receiver checks
                    //    (`G::analyze::applies-on-non-block`,
                    //    `G::analyze::applies-on-undescribed-block`) fire on the
                    //    export-block path. Skill / private-block flows already
                    //    invoke this validator; mirrors that behaviour. An empty
                    //    `flow_local_types` map is correct here — flow-local agent
                    //    bindings live inside `FlowStmt::Let` walks which the
                    //    export-block AST does not carry.
                    for cref in &eb.condition_refs {
                        // Import-usage: split on non-identifier chars and probe each
                        // word against every imported namespace set.
                        for ident in cref.raw.split(|c: char| !c.is_alphanumeric() && c != '_') {
                            if ident.is_empty() {
                                continue;
                            }
                            if imported_blocks.contains(ident)
                                || imported_texts.contains(ident)
                                || imported_constraint_skills.contains(ident)
                                || imported_context_skills.contains(ident)
                            {
                                used_import_names.insert(ident.to_string());
                            }
                        }
                        // `.applies()` validation — empty flow_local_types is fine;
                        // export blocks have no `let` walks.
                        check_applies_in_condition(
                            &cref.raw,
                            cref.span,
                            file_id,
                            file_label,
                            line_index,
                            bag,
                            &text_names,
                            &block_names,
                            &block_decls,
                            imported_block_descriptions,
                            &std::collections::HashMap::new(),
                        );
                    }
                }
                // PRD #159 / Codex round-1 Issue 1: emit `return-of-no-value-call`
                // (Error) when an export block's `return <call>` targets a callee
                // that resolves but declares no `-> Type`. Symmetric to
                // `assignment-rhs-has-no-value`. Inspects `terminal_return` directly
                // since `ExportBlockDecl` has no `flow: Vec<FlowStmt>` (see ast.rs).
                if let Some(expr) = spanned.node.terminal_return.as_ref() {
                    check_return_call_no_value(
                        expr,
                        &local_callee_return_types,
                        imported_block_return_types,
                        &block_names,
                        file_label,
                        line_index,
                        bag,
                    );
                }
                // Phase 3 / Task 3.12 — block-scope `{param}` slot validation
                // on the imports path. Mirrors the non-imports arm.
                check_block_freeform_slots(
                    &spanned.node.name,
                    &spanned.node.params,
                    &spanned.node.freeform_sections,
                    spanned.span,
                    &text_bodies,
                    file_label,
                    line_index,
                    bag,
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
                    &case_bad,
                    &mut explicit_decl_seen,
                );

                // G::analyze::export-missing-return-type — issue #161: broadened from
                // skill + export-block to also fire on private `block` decls whose body
                // has a meaningful `return <expr>` (where `<expr>` is not the `none`
                // value-keyword) and whose header lacks a `-> DomainType` annotation.
                // Same diagnostic ID and Repairable classification as the skill /
                // export-block fire-sites; message text identifies the decl kind.
                if flow_has_meaningful_return(&spanned.node.flow)
                    && spanned.node.return_type.is_none()
                {
                    let span = spanned.span;
                    bag.push(
                        Diagnostic {
                            id: "G::analyze::export-missing-return-type".into(),
                            classification: Classification::Repairable,
                            message: format!(
                                "`block {}` returns a meaningful value but its header lacks a `-> DomainType` annotation",
                                spanned.node.name
                            ),
                            span: SourceSpan::from_byte_span(file_label, span, line_index),
                            related: Vec::new(),
                            hints: vec![
                                "add a return-type annotation to the header — e.g. `block name(...) -> DomainType`"
                                    .into(),
                            ],
                        },
                        span,
                    );
                }

                // PRD #159 / Codex round-1 Issue 1: emit `return-of-no-value-call`
                // (Error) when a private block's `return <call>` targets a callee that
                // resolves but declares no `-> Type`. Symmetric to
                // `assignment-rhs-has-no-value`. Suppression + diagnostic shape live
                // in the helper.
                walk_return_of_no_value_call(
                    &spanned.node.flow,
                    &local_callee_return_types,
                    imported_block_return_types,
                    &block_names,
                    file_label,
                    line_index,
                    bag,
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

                // Phase 3 / Task 3.12 — block-scope `{param}` slot validation
                // on the imports path. Mirrors the non-imports arm.
                check_block_freeform_slots(
                    &spanned.node.name,
                    &spanned.node.params,
                    &spanned.node.freeform_sections,
                    spanned.span,
                    &text_bodies,
                    file_label,
                    line_index,
                    bag,
                );
            }
            Decl::Const(_) | Decl::Import(_) => {}
            Decl::TypeDecl(_) => {} // TODO: handled in Task B.4+
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

    // Task 9: derive the set of Type-kinded import alias locals so the
    // value/type-decl sweeps can skip type aliases (resp. recognise them).
    let type_alias_locals: HashSet<String> = import_alias_kinds
        .iter()
        .filter_map(|(local, (kind, _))| match kind {
            crate::name_kind::ResolvedImportKind::Type => Some(local.clone()),
            crate::name_kind::ResolvedImportKind::Value => None,
        })
        .collect();
    // Spec §"New `sweep_value_name_collisions`": value-vs-value canonical-key
    // collision sweep. Imports-path parity with `analyze_with_diagnostics`.
    sweep_value_name_collisions(
        file,
        file_label,
        line_index,
        bag,
        &case_bad,
        &type_alias_locals,
    );
    // Issue #84 Chunk 3 (AC5): domain-type-vs-param/const collision sweep.
    // Imports-path parity with `analyze_with_diagnostics`.
    sweep_name_collisions(file, file_label, line_index, bag, registry, &case_bad);
    // Universal-namespace check (`design/values-and-names.md` §No-Shadowing):
    // type-decl-vs-param/const/block collision sweep, complementary to the
    // registry-direction sweep above. Imports-path parity.
    sweep_type_decl_name_collisions(
        file,
        file_label,
        line_index,
        bag,
        registry,
        &case_bad,
        &type_alias_locals,
    );
    // Codex finding #1 follow-up: reject name_ref param defaults that don't
    // resolve to an in-scope `const` (same-file or imported). `imported_texts`
    // already carries `alias.name` entries for whole-module imports, so a
    // single-shape lookup covers both selective and aliased forms.
    sweep_param_default_name_refs(file, file_label, line_index, bag, Some(imported_texts));
    sweep_typed_decl_missing_return(file, file_label, line_index, bag);

    // Library detection: file with zero skills.
    let has_skill = file.decls.iter().any(|d| matches!(d, Decl::Skill(_)));
    if !has_skill {
        let has_export = file.decls.iter().any(|d| {
            matches!(d, Decl::ExportBlock(_))
                || matches!(d, Decl::Const(c) if c.node.exported)
                || matches!(d, Decl::TypeDecl(t) if t.node.exported)
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

    // Task 2.4: annotate every Branch/ElifBranch with a ConditionClassification.
    // Task 6: thread imported-const data through so PredicateConst lands on
    // String-typed imported consts that appear bare in condition position.
    let mut annotated = file.clone();
    annotate_file_branches(&mut annotated, imported_text_values, imported_const_types);

    // Task 3.1: emit G::analyze::condition-non-boolean-non-predicate for
    // numeric-kinded tokens in condition position.
    check_file_numeric_conditions(&annotated, file_label, line_index, bag);

    annotated
}

/// Like `analyze_skill` but also tracks which imported names are used.
#[allow(clippy::too_many_arguments)]
fn analyze_skill_with_usage_tracking(
    spanned: &Spanned<crate::ast::Skill>,
    file_id: u32,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    registry: &mut crate::domain_registry::Registry,
    text_names: &HashSet<&str>,
    text_bodies: &HashMap<&str, String>,
    block_names: &HashSet<&str>,
    block_decls: &HashMap<&str, &crate::ast::BlockDecl>,
    export_block_decls: &HashMap<&str, &crate::ast::ExportBlockDecl>,
    imported_block_params: &HashMap<String, Vec<crate::ast::Param>>,
    imported_texts: &HashSet<String>,
    imported_blocks: &HashSet<String>,
    imported_context_skills: &HashSet<String>,
    imported_constraint_skills: &HashSet<String>,
    used_import_names: &mut HashSet<String>,
    imported_block_descriptions: &HashMap<String, String>,
    local_callee_return_types: &HashMap<&str, &Spanned<String>>,
    imported_block_return_types: &HashMap<String, Spanned<String>>,
    context_skill_names: &HashSet<&str>,
    constraint_skill_names: &HashSet<&str>,
    case_bad: &HashSet<Span>,
    explicit_decl_seen: &mut HashSet<String>,
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
        text_bodies,
        block_names,
        block_decls,
        export_block_decls,
        imported_block_params,
        imported_block_descriptions,
        local_callee_return_types,
        imported_block_return_types,
        context_skill_names,
        constraint_skill_names,
        case_bad,
        explicit_decl_seen,
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
            if imported_texts.contains(&name.node) || imported_context_skills.contains(&name.node) {
                used_import_names.insert(name.node.clone());
            }
        }
    }
    for entry in &skill.context_section {
        if let crate::ast::ContextEntry::NameRef(name) = entry {
            if imported_texts.contains(&name.node) || imported_context_skills.contains(&name.node) {
                used_import_names.insert(name.node.clone());
            }
        }
    }

    // Check constraints_section skill refs.
    for entry in &skill.constraints_section {
        if let crate::ast::ContextEntry::NameRef(name) = entry {
            if imported_constraint_skills.contains(&name.node) {
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
                condition,
                then_body,
                elif_branches,
                else_body,
                ..
            } => {
                // Condition position can reference imported names directly:
                // `if imported_block(...)`, `if imported_predicate_const`, or
                // composed via `not`/`and`/`or`/`==`. Tokenize and mark any
                // matching imported name as used — symmetric with the
                // `Return(Call)` / `Return(Name)` arms below.
                for cond in std::iter::once(condition.as_str())
                    .chain(elif_branches.iter().map(|e| e.condition.as_str()))
                {
                    for tok in crate::condition::tokenize_condition(cond) {
                        // `tokenize_condition` keeps `name(args...)` and
                        // `name.applies()` as one token. Recover the bare
                        // receiver: drop a `.applies()` suffix first (the
                        // predicate-applies form), then strip any remaining
                        // call paren.
                        let stripped = tok.strip_suffix(".applies()").unwrap_or(tok.as_str());
                        let receiver = match stripped.find('(') {
                            Some(i) => &stripped[..i],
                            None => stripped,
                        };
                        if imported_blocks.contains(receiver) || imported_texts.contains(receiver) {
                            used.insert(receiver.to_string());
                        }
                    }
                }
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

/// Scan `text` for `{name}` slots and emit `G::analyze::unknown-param-slot` for
/// any slot whose `name` is not a header parameter of the enclosing decl.
///
/// `owner_name` is the name of the decl whose parameter scope `param_names`
/// represents (a `skill`, a private `block`, or an `export block`); it lands in
/// the diagnostic message as ``" is not a declared parameter of `<owner>`"``.
///
/// The slot span isn't reachable from the AST today (string-literal bodies and
/// freeform-content cooked text are stored without per-slot offsets), so the
/// diagnostic is attributed to `decl_span` per `docs/reference/diagnostics.md` §Span
/// Semantics synthetic-fallback option 3 — the same fallback the legacy flow
/// inline-string scan uses in `walk_skill_flow_assign_checks`.
fn scan_param_slots_in_text(
    text: &str,
    param_names: &HashSet<&str>,
    owner_name: &str,
    decl_span: Span,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    for slot in scan_slots(text) {
        if !param_names.contains(slot.name.as_str()) {
            bag.push(
                Diagnostic::error(
                    "G::analyze::unknown-param-slot",
                    format!(
                        "`{{{}}}` is not a declared parameter of `{}`",
                        slot.name, owner_name
                    ),
                    SourceSpan::from_byte_span(file_label, decl_span, line_index),
                ),
                decl_span,
            );
        }
    }
}

/// Phase 3 (Task 3.12) — fire `G::analyze::unknown-param-slot` for `{param}`
/// slots in non-flow text positions on a `Skill`:
///
/// - `body_context` and `context_section`: each `ContextEntry::InlineString`
///   is checked directly. `ContextEntry::NameRef` is checked against the
///   resolved const body (file-local + imported) so a slot inside a
///   `const note = "Use {undeclared}…"` body still surfaces here.
/// - `freeform_sections` (e.g. `quality:`, `risks:`): each item's rendered
///   text is checked. For `MarkerClause` the operand text is consulted as
///   both a name (resolved through `text_bodies`) and as a literal; this
///   mirrors `lower::lower_freeform_item`'s resolution.
///
/// `description:` slots are not scanned here — the parser still rejects
/// those with `G::parse::param-slot-in-non-instruction-string`. `flow:`
/// slots are validated by `walk_skill_flow_assign_checks`, which carries
/// the richer `FlowScope` (params + bound flow-locals).
fn check_skill_freeform_and_context_slots(
    skill: &crate::ast::Skill,
    decl_span: Span,
    text_bodies: &HashMap<&str, String>,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    let param_names: HashSet<&str> = skill.params.iter().map(|p| p.name.as_str()).collect();

    // `body_context` and `context_section` — both store `ContextEntry`.
    for entry in skill
        .body_context
        .iter()
        .chain(skill.context_section.iter())
    {
        match entry {
            ContextEntry::InlineString(s) => {
                scan_param_slots_in_text(
                    s,
                    &param_names,
                    &skill.name,
                    decl_span,
                    file_label,
                    line_index,
                    bag,
                );
            }
            ContextEntry::NameRef(name) => {
                if let Some(body) = text_bodies.get(name.node.as_str()) {
                    scan_param_slots_in_text(
                        body,
                        &param_names,
                        &skill.name,
                        decl_span,
                        file_label,
                        line_index,
                        bag,
                    );
                }
            }
        }
    }

    // Freeform colon-keyword sections — every item carries author-written
    // prose that lands in compiled output and is therefore instruction-bearing.
    for section in &skill.freeform_sections {
        for item in &section.items {
            match item {
                crate::ast::FreeformItem::StringLiteral(s) => {
                    scan_param_slots_in_text(
                        &s.node,
                        &param_names,
                        &skill.name,
                        decl_span,
                        file_label,
                        line_index,
                        bag,
                    );
                }
                crate::ast::FreeformItem::NameRef(name) => {
                    if let Some(body) = text_bodies.get(name.node.as_str()) {
                        scan_param_slots_in_text(
                            body,
                            &param_names,
                            &skill.name,
                            decl_span,
                            file_label,
                            line_index,
                            bag,
                        );
                    }
                }
                crate::ast::FreeformItem::MarkerClause { text, .. } => {
                    // Mirror `lower::lower_freeform_item`: the operand may be
                    // a bare-name const reference OR an inline string; if it
                    // resolves via `text_bodies` scan that body, else scan
                    // the literal directly.
                    if let Some(body) = text_bodies.get(text.node.as_str()) {
                        scan_param_slots_in_text(
                            body,
                            &param_names,
                            &skill.name,
                            decl_span,
                            file_label,
                            line_index,
                            bag,
                        );
                    } else {
                        scan_param_slots_in_text(
                            &text.node,
                            &param_names,
                            &skill.name,
                            decl_span,
                            file_label,
                            line_index,
                            bag,
                        );
                    }
                }
            }
        }
    }
}

/// Phase 3 / Task 3.12 — block-scope counterpart to
/// `check_skill_freeform_and_context_slots`. Fires
/// `G::analyze::unknown-param-slot` for `{param}` slots inside
/// `freeform_sections` (`quality:`, `risks:`, `acceptance_criteria:`, …) on a
/// `BlockDecl` or `ExportBlockDecl`.
///
/// The `param_names` set must be the **owning block's** parameter scope
/// (`block_decl.params`), not the enclosing skill's — each block has its own
/// header param list and freeform prose can reference only those names.
/// `owner_name` is the block's name, which the diagnostic message reports as
/// "not a declared parameter of `<block-name>`".
///
/// Note: `BlockDecl` and `ExportBlockDecl` do not carry `body_context` /
/// `context_section` fields (those exist only on `Skill`), so this walker only
/// scans `freeform_sections`. Block-level *flow strings* also still lack slot
/// validation; that is a pre-existing gap orthogonal to Task 3.12 and not
/// addressed here.
fn check_block_freeform_slots(
    block_name: &str,
    params: &[crate::ast::Param],
    freeform_sections: &[crate::ast::FreeformSection],
    decl_span: Span,
    text_bodies: &HashMap<&str, String>,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    let param_names: HashSet<&str> = params.iter().map(|p| p.name.as_str()).collect();
    for section in freeform_sections {
        for item in &section.items {
            match item {
                crate::ast::FreeformItem::StringLiteral(s) => {
                    scan_param_slots_in_text(
                        &s.node,
                        &param_names,
                        block_name,
                        decl_span,
                        file_label,
                        line_index,
                        bag,
                    );
                }
                crate::ast::FreeformItem::NameRef(name) => {
                    if let Some(body) = text_bodies.get(name.node.as_str()) {
                        scan_param_slots_in_text(
                            body,
                            &param_names,
                            block_name,
                            decl_span,
                            file_label,
                            line_index,
                            bag,
                        );
                    }
                }
                crate::ast::FreeformItem::MarkerClause { text, .. } => {
                    // Mirror `lower::lower_freeform_item`: operand may be a
                    // bare-name const reference OR an inline string. Scan the
                    // resolved-const body if known, else the literal.
                    if let Some(body) = text_bodies.get(text.node.as_str()) {
                        scan_param_slots_in_text(
                            body,
                            &param_names,
                            block_name,
                            decl_span,
                            file_label,
                            line_index,
                            bag,
                        );
                    } else {
                        scan_param_slots_in_text(
                            &text.node,
                            &param_names,
                            block_name,
                            decl_span,
                            file_label,
                            line_index,
                            bag,
                        );
                    }
                }
            }
        }
    }
}

/// Phase 6 — emit `G::analyze::cardinality-violation` for every catalogue
/// entry with `cardinality = "one"` whose declared section in this decl has
/// more than one body item.
///
/// Today only `[goal]` carries `cardinality = "one"`, but the check is
/// catalogue-driven so future single-item sections (e.g. a hypothetical
/// `summary:`) participate automatically when added to `catalogue.toml`.
/// The function is structurally agnostic — it only inspects the catalogue
/// entry plus the section's item count, so it applies uniformly to skills,
/// private blocks, and export blocks. Diagnostic anchors on the enclosing
/// decl's span — the AST `FreeformSection` only stores a line index, not a
/// byte span, so a pinpoint anchor isn't available here.
///
/// Renamed from `check_freeform_cardinality` (Phase 6 code-review fix):
/// `[goal]` is a catalogue entry rather than a true freeform section, so
/// the more general `section` name reflects the actual scope.
fn check_section_cardinality(
    freeform_sections: &[crate::ast::FreeformSection],
    decl_span: Span,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    let catalogue = crate::sections::SectionCatalogue::load();
    for section in freeform_sections {
        let Some(entry) = catalogue.get(&section.name) else {
            continue;
        };
        if entry.cardinality != Some(crate::sections::Cardinality::One) {
            continue;
        }
        if section.items.is_empty() {
            bag.push(
                Diagnostic::error(
                    "G::analyze::cardinality-violation",
                    format!(
                        "section `{}:` requires exactly one item but none were provided",
                        section.name
                    ),
                    SourceSpan::from_byte_span(file_label, decl_span, line_index),
                ),
                decl_span,
            );
        } else if section.items.len() > 1 {
            bag.push(
                Diagnostic::error(
                    "G::analyze::cardinality-violation",
                    format!(
                        "section `{}:` accepts only one item but {} were provided",
                        section.name,
                        section.items.len()
                    ),
                    SourceSpan::from_byte_span(file_label, decl_span, line_index),
                ),
                decl_span,
            );
        }
    }
}

/// Analyze invariant: each named sub-section may appear at most once per
/// body. The parser already emits `G::parse::duplicate-subsection` (Repairable)
/// for catalogued built-ins (`description`, `effects`, `flow`, `context`,
/// `constraints`) and `G::analyze::unmerged-duplicate-subsection` (Error)
/// catches the analyze-side residue. This check covers the gap left by those
/// passes: every section parsed through the freeform path (truly-freeform
/// names like `quality:` and catalogue entries like `goal:`) is checked
/// here. Anchors on the second-occurrence header span.
fn check_duplicate_sections(
    freeform_sections: &[crate::ast::FreeformSection],
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    // Duplicate detection is case-insensitive (catalogue lookup is too), so
    // `goal:` and `Goal:` collide and produce one diagnostic. The diagnostic
    // message still quotes the user's spelling of the second occurrence.
    let mut seen: std::collections::HashMap<String, crate::span::Span> =
        std::collections::HashMap::new();
    for section in freeform_sections {
        let name = section.name.as_str();
        let key = name.to_ascii_lowercase();
        if let Some(_first) = seen.get(&key) {
            bag.push(
                Diagnostic::error(
                    "G::analyze::duplicate-section",
                    format!(
                        "duplicate `{}:` sub-section — each named sub-section may appear at most once per body",
                        name
                    ),
                    SourceSpan::from_byte_span(file_label, section.header_span, line_index),
                ),
                section.header_span,
            );
        } else {
            seen.insert(key, section.header_span);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn analyze_skill(
    spanned: &Spanned<crate::ast::Skill>,
    file_id: u32,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    registry: &mut crate::domain_registry::Registry,
    text_names: &HashSet<&str>,
    text_bodies: &HashMap<&str, String>,
    block_names: &HashSet<&str>,
    block_decls: &HashMap<&str, &BlockDecl>,
    export_block_decls: &HashMap<&str, &crate::ast::ExportBlockDecl>,
    imported_block_params: &HashMap<String, Vec<crate::ast::Param>>,
    imported_block_descriptions: &HashMap<String, String>,
    local_callee_return_types: &HashMap<&str, &Spanned<String>>,
    imported_block_return_types: &HashMap<String, Spanned<String>>,
    context_skill_names: &HashSet<&str>,
    constraint_skill_names: &HashSet<&str>,
    case_bad: &HashSet<Span>,
    explicit_decl_seen: &mut HashSet<String>,
) {
    let skill = &spanned.node;
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
        case_bad,
        explicit_decl_seen,
    );

    // G::analyze::export-missing-return-type — issue #160: broadened from
    // export-block to also fire on `skill` decls whose body has a meaningful
    // `return <expr>` (where `<expr>` is not the `none` value-keyword) and
    // whose header lacks a `-> DomainType` annotation. Same diagnostic ID
    // and Repairable classification as the export-block fire-site.
    if flow_has_meaningful_return(&skill.flow) && skill.return_type.is_none() {
        let span = spanned.span;
        bag.push(
            Diagnostic {
                id: "G::analyze::export-missing-return-type".into(),
                classification: Classification::Repairable,
                message: format!(
                    "`skill {}` returns a meaningful value but its header lacks a `-> DomainType` annotation",
                    skill.name
                ),
                span: SourceSpan::from_byte_span(file_label, span, line_index),
                related: Vec::new(),
                hints: vec![
                    "add a return-type annotation to the header — e.g. `skill name(...) -> DomainType`"
                        .into(),
                ],
            },
            span,
        );
    }

    // PRD #159 / Codex round-1 Issue 1: emit `return-of-no-value-call`
    // (Error) when `return <call>` targets a same-file callee that resolves
    // but declares no `-> Type`. Symmetric to `assignment-rhs-has-no-value`.
    // Walks `skill.flow` and recurses into branch arms; suppression and
    // diagnostic shape live in the helper.
    walk_return_of_no_value_call(
        &skill.flow,
        local_callee_return_types,
        imported_block_return_types,
        block_names,
        file_label,
        line_index,
        bag,
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
    // synthetic-fallback option (3) per `docs/reference/diagnostics.md` §Span
    // Semantics. The IDs and messages remain accurate.

    // Flow-position assignments (`.flow-assign-spec.md` §6).
    // Build the per-skill `FlowScope` with the header params, plus the
    // pre-pass binding trace used by the `use-before-bind`
    // specialization (§6.3 — Codex Round 2 Med 6).
    //
    // Codex H1+H2: the assignment-related checks (slot validation,
    // `handle_flow_assign`, return-name resolution, H3 call-arg type
    // check) run through `walk_skill_flow_assign_checks` against a
    // mutable `FlowScope`. The walker recurses into branch arms with a
    // *child* scope so arm-local bindings stay arm-local. The legacy
    // per-stmt match below keeps running for everything else (undefined-
    // call, `validate_call_args`, return-call resolution + nominal
    // checks, constraint/context name resolution) — those have nothing
    // to do with flow-local-binding scoping.
    let mut flow_scope = FlowScope::default();
    for p in &skill.params {
        flow_scope.param_names.insert(p.name.clone());
    }
    let binding_trace = SkillBindingTrace::collect(
        &skill.flow,
        local_callee_return_types,
        imported_block_return_types,
    );
    let container = ContainerKind::Skill;
    walk_skill_flow_assign_checks(
        &skill.flow,
        &mut flow_scope,
        container,
        skill.name.as_str(),
        skill.return_type.as_ref(),
        spanned.span,
        &binding_trace,
        text_names,
        block_names,
        block_decls,
        export_block_decls,
        imported_block_params,
        local_callee_return_types,
        imported_block_return_types,
        registry,
        file_label,
        line_index,
        bag,
    );

    for stmt in &skill.flow {
        match stmt {
            FlowStmt::InlineString(_) => {
                // Slot validation lives in `walk_skill_flow_assign_checks`
                // (Codex H1+H2). The legacy slot scan moved there so it
                // sees the same `FlowScope` (and child scope inside arms)
                // that the binding registration uses.
                let _ = file_id;
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
            FlowStmt::Call {
                target,
                args,
                bound_name,
                ..
            } => {
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
                                hints: vec![format!(
                                    "declare `block {}()` or check the name for typos",
                                    target.node
                                )],
                            },
                            span,
                        );
                    }
                } else if let Some(callee) = block_decls.get(target.node.as_str()) {
                    // PRD #103 / Slice 1 (#104): private-block callee — verify
                    // each required parameter is satisfied by a positional arg.
                    // Pin the diagnostic to the callee identifier's span so a
                    // skill with multiple calls highlights the offending call,
                    // not the enclosing skill declaration.
                    for d in validate_call_args(
                        &target.node,
                        &callee.params,
                        args,
                        target.span,
                        file_label,
                        line_index,
                    ) {
                        bag.push(d, target.span);
                    }
                } else if let Some(callee) = export_block_decls.get(target.node.as_str()) {
                    // PRD #103 / Slice 2 (#105): same-file export-block callee —
                    // export-block params may now omit a default, so a caller
                    // that omits the corresponding positional argument must
                    // surface `G::analyze::missing-required-arg` at the call
                    // site, mirroring the private-block path above.
                    for d in validate_call_args(
                        &target.node,
                        &callee.params,
                        args,
                        target.span,
                        file_label,
                        line_index,
                    ) {
                        bag.push(d, target.span);
                    }
                } else if let Some(params) = imported_block_params.get(target.node.as_str()) {
                    // PRD #103 / Slice 2 (#105) — Slice C: imported export-block
                    // callee — the consumer-side resolver consults the
                    // alias-/prefix-keyed parameter list captured by
                    // `extract_exports::block_params` (lib.rs) and re-keyed in
                    // `build_resolved_imports`. Same `validate_call_args`
                    // contract as the local paths above.
                    for d in validate_call_args(
                        &target.node,
                        params.as_slice(),
                        args,
                        target.span,
                        file_label,
                        line_index,
                    ) {
                        bag.push(d, target.span);
                    }
                }
                // Flow-position-assignment registration (`handle_flow_assign`)
                // now lives in `walk_skill_flow_assign_checks` (Codex
                // H1+H2 / M5). The walker fires the same diagnostics
                // and mutates the same `FlowScope`, but with proper
                // child-scope recursion into branch arms and with the
                // `undefined-call` dedupe applied.
                let _ = bound_name;
            }
            FlowStmt::ConstraintMarker(marker) => {
                // Check that the constraint name resolves to a text declaration.
                if !text_names.contains(marker.name.node.as_str()) {
                    let span = spanned.span;
                    bag.push(
                        Diagnostic::error(
                            "G::analyze::undefined-name",
                            format!(
                                "`{}` is not a declared `const` in this file",
                                marker.name.node
                            ),
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
                    context_skill_names,
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
                // Codex P2 follow-up to PRD #103 / #105: a `return foo(..)`
                // must run the same required-arg check as a top-level
                // `call foo(..)`. Pre-fix only the FlowStmt::Call arm
                // wired `validate_call_args`, so `return helper()` against
                // a callee with a required parameter compiled silently.
                if let crate::ast::ReturnExpr::Call { target, args } = expr {
                    let params: Option<&[crate::ast::Param]> =
                        if let Some(c) = block_decls.get(target.node.as_str()) {
                            Some(&c.params)
                        } else if let Some(c) = export_block_decls.get(target.node.as_str()) {
                            Some(&c.params)
                        } else {
                            imported_block_params
                                .get(target.node.as_str())
                                .map(|v| v.as_slice())
                        };
                    if let Some(params) = params {
                        for d in validate_call_args(
                            &target.node,
                            params,
                            args,
                            target.span,
                            file_label,
                            line_index,
                        ) {
                            bag.push(d, target.span);
                        }
                    }
                }
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
                // Flow-position-assignment return-name resolution
                // (§6.3 / §6.4 — `return <name>` against a flow-local
                // binding) now lives in `walk_skill_flow_assign_checks`
                // (Codex H1+H2). The walker also fires the
                // `use-before-bind` specialization when `name` is bound
                // somewhere else in the skill (e.g. inside an arm).
            }
            FlowStmt::Branch {
                condition,
                then_body,
                elif_branches,
                else_body,
                ..
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
                    &flow_scope.flow_local_types,
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
                        &flow_scope.flow_local_types,
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
                    context_skill_names,
                    &block_decls,
                    export_block_decls,
                    imported_block_params,
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
                        context_skill_names,
                        &block_decls,
                        export_block_decls,
                        imported_block_params,
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
                        context_skill_names,
                        &block_decls,
                        export_block_decls,
                        imported_block_params,
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
                        "`{}` is not a declared `const` in this file",
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
        check_context_entry_name(
            entry,
            text_names,
            context_skill_names,
            spanned.span,
            file_label,
            line_index,
            bag,
        );
    }

    // Check context: section name refs.
    for entry in &skill.context_section {
        check_context_entry_name(
            entry,
            text_names,
            context_skill_names,
            spanned.span,
            file_label,
            line_index,
            bag,
        );
    }

    // Phase 3 / Task 3.12 — `{param}` slot validation for non-flow text
    // positions (context: bodies and freeform colon-keyword sections such
    // as `quality:`, `risks:`). The parse-time rejection was removed for
    // `context:` bodies; the spec keeps the compile-time check that the
    // named parameter actually exists. `flow:` slots still flow through
    // `walk_skill_flow_assign_checks` so the `use-before-bind` /
    // `unknown-param-slot` discrimination survives there.
    check_skill_freeform_and_context_slots(
        skill,
        spanned.span,
        text_bodies,
        file_label,
        line_index,
        bag,
    );

    // Phase 6 — `G::analyze::cardinality-violation` for catalogue entries
    // with `cardinality = "one"` (today: `[goal]`) whose author-declared
    // section has more than one body item. Catalogue-driven; adding another
    // single-item section to `catalogue.toml` enrolls it automatically.
    check_section_cardinality(
        &skill.freeform_sections,
        spanned.span,
        file_label,
        line_index,
        bag,
    );
    check_duplicate_sections(&skill.freeform_sections, file_label, line_index, bag);

    // Check constraints: section skill refs.
    for entry in &skill.constraints_section {
        if let crate::ast::ContextEntry::NameRef(name) = entry {
            if !constraint_skill_names.contains(name.node.as_str()) {
                let span = spanned.span;
                bag.push(
                    Diagnostic::error(
                        "G::analyze::undefined-name",
                        format!(
                            "`{}` is not a constraint-only skill in this file or its imports",
                            name.node
                        ),
                        SourceSpan::from_byte_span(file_label, span, line_index),
                    ),
                    span,
                );
            }
        }
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

    // Check missing description — repairable. The "repair" today is the
    // Repairable classification itself; auto-generation routes through the
    // catalogue's `[description].repair_hook`
    // (`crate::sections::hooks::dispatch_description_repair`). The default
    // hook returns `None` (no synthesised text), so we currently still emit
    // the diagnostic in every case. When a real generator is wired in
    // (e.g. distilling text from the skill's flow), this branch will pivot
    // on the hook's `Option<String>` — `Some(text)` would suppress the
    // diagnostic and inject text downstream; `None` keeps the existing
    // Repairable-only shape. Routing through the dispatcher now means that
    // future change is one fn body away.
    if skill.description.is_none() {
        let _generated = crate::sections::hooks::dispatch_description_repair(&skill.name);
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
    context_skill_names: &HashSet<&str>,
    span: crate::span::Span,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    if let ContextEntry::NameRef(name) = entry {
        if !text_names.contains(name.node.as_str())
            && !context_skill_names.contains(name.node.as_str())
        {
            bag.push(
                Diagnostic::error(
                    "G::analyze::undefined-name",
                    format!(
                        "`{}` is not a declared `const` or context-only skill in this file",
                        name.node
                    ),
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

// ---------------------------------------------------------------------------
// Condition classifier
// ---------------------------------------------------------------------------
//
// Task 6: the local `classify_condition` / `classify_token` were retired in
// favour of `crate::condition::classify_condition` (the single classification
// authority — see `design/data-flow.md`). Analyze now consults the shared
// classifier with a `ConditionContext` built once per enclosing decl, and
// `check_file_numeric_conditions` reads the cached `condition_classification`
// populated by `annotate_file_branches` instead of re-tokenizing.

// ---------------------------------------------------------------------------
// Branch annotation walker (Task 2.4 + Task 6 Decl::Block extension)
// ---------------------------------------------------------------------------

/// Recursively walk `flow` and (a) classify every `Branch`/`ElifBranch`
/// condition via the shared `condition::classify_condition` and (b) store the
/// result on the AST node's `condition_classification` slot. Reused for both
/// `Decl::Skill` and `Decl::Block` flows.
///
/// `Decl::ExportBlock` flow walking is OUT OF SCOPE — `ExportBlockDecl` only
/// carries `flow_strings: Vec<String>` (no structured `FlowStmt`), so there
/// are no branch nodes to classify there. See design spec Out of Scope §7.
fn annotate_branch_classifications(
    flow: &mut [FlowStmt],
    ctx: &crate::condition::ConditionContext,
) {
    for stmt in flow.iter_mut() {
        if let FlowStmt::Branch {
            condition,
            condition_classification,
            then_body,
            elif_branches,
            else_body,
            condition_refs: _,
        } = stmt
        {
            *condition_classification = Some(crate::condition::classify_condition(condition, ctx));
            for elif in elif_branches.iter_mut() {
                elif.condition_classification =
                    Some(crate::condition::classify_condition(&elif.condition, ctx));
                annotate_branch_classifications(&mut elif.body, ctx);
            }
            annotate_branch_classifications(then_body, ctx);
            if let Some(eb) = else_body {
                annotate_branch_classifications(eb, ctx);
            }
        }
    }
}

/// Skill-flow branch annotator that walks the flow tree live, accumulating
/// flow-local bindings as it goes (spec `.flow-assign-spec.md` §6.3 / Codex
/// Round 2 High 4 — option i: consolidate per-branch annotation INTO the
/// flow walk).
///
/// At each `FlowStmt::Branch`, snapshot the current
/// `flow_local_bindings` into a transient `ConditionContext::for_branch_with_consts`
/// before classifying the condition. Branch bodies recurse with a CLONED
/// snapshot so arm-local bindings declared inside the arm do not leak to
/// sibling arms or back to the enclosing scope (matches the lexical rule in
/// spec §6.1).
///
/// Producer-side resolution: when a `FlowStmt::Call` carries a `bound_name`,
/// look up the callee's return type via `local_callee_return_types` /
/// `imported_block_return_types` / `crate::stdlib_sig` and stash the
/// agent-shape flag.
fn annotate_skill_flow_with_locals(
    flow: &mut [FlowStmt],
    consts: &std::collections::HashMap<String, crate::kind_infer::TypeTag>,
    params_set: &std::collections::HashSet<String>,
    local_callee_return_types: &std::collections::HashMap<String, String>,
    imported_block_return_types: &std::collections::HashMap<String, String>,
    flow_local_bindings: &mut std::collections::HashMap<
        String,
        crate::condition::ConditionFlowLocal,
    >,
) {
    for stmt in flow.iter_mut() {
        match stmt {
            FlowStmt::Call {
                target, bound_name, ..
            } => {
                if let Some(spanned_name) = bound_name {
                    let n = spanned_name.node.clone();
                    let raw = local_callee_return_types
                        .get(target.node.as_str())
                        .cloned()
                        .or_else(|| {
                            imported_block_return_types
                                .get(target.node.as_str())
                                .cloned()
                        })
                        .or_else(|| {
                            crate::stdlib_sig(target.node.as_str())
                                .and_then(|s| s.return_type.map(str::to_string))
                        });
                    let is_agent = raw
                        .as_deref()
                        .map(|s| s.eq_ignore_ascii_case("Agent"))
                        .unwrap_or(false)
                        || crate::stdlib_sig(target.node.as_str())
                            .map(|s| s.is_agent)
                            .unwrap_or(false);
                    flow_local_bindings
                        .insert(n, crate::condition::ConditionFlowLocal { is_agent });
                }
            }
            FlowStmt::Branch {
                condition,
                condition_classification,
                then_body,
                elif_branches,
                else_body,
                condition_refs: _,
            } => {
                // Snapshot the bindings at this branch site. Build a
                // ConditionContext with the snapshot and classify.
                let consts_ref: std::collections::HashMap<&str, crate::kind_infer::TypeTag> =
                    consts
                        .iter()
                        .map(|(k, v)| (k.as_str(), v.clone()))
                        .collect();
                let params_ref: std::collections::HashSet<&str> =
                    params_set.iter().map(|s| s.as_str()).collect();
                let bindings_ref: std::collections::HashMap<
                    &str,
                    crate::condition::ConditionFlowLocal,
                > = flow_local_bindings
                    .iter()
                    .map(|(k, v)| (k.as_str(), *v))
                    .collect();
                let ctx = crate::condition::ConditionContext::for_branch_with_consts(
                    consts_ref,
                    params_ref,
                    bindings_ref,
                );
                *condition_classification =
                    Some(crate::condition::classify_condition(condition, &ctx));
                for elif in elif_branches.iter_mut() {
                    elif.condition_classification =
                        Some(crate::condition::classify_condition(&elif.condition, &ctx));
                }

                // Arm bodies recurse with cloned snapshots — arm-local
                // bindings do not leak (spec §6.1 — branch-arm scoping (X)).
                let mut then_locals = flow_local_bindings.clone();
                annotate_skill_flow_with_locals(
                    then_body,
                    consts,
                    params_set,
                    local_callee_return_types,
                    imported_block_return_types,
                    &mut then_locals,
                );
                for elif in elif_branches.iter_mut() {
                    let mut elif_locals = flow_local_bindings.clone();
                    annotate_skill_flow_with_locals(
                        &mut elif.body,
                        consts,
                        params_set,
                        local_callee_return_types,
                        imported_block_return_types,
                        &mut elif_locals,
                    );
                }
                if let Some(eb) = else_body {
                    let mut else_locals = flow_local_bindings.clone();
                    annotate_skill_flow_with_locals(
                        eb,
                        consts,
                        params_set,
                        local_callee_return_types,
                        imported_block_return_types,
                        &mut else_locals,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Annotate every `Branch`/`ElifBranch` node in each `Decl::Skill` AND
/// `Decl::Block` flow inside a `SourceFile`.
///
/// Pre-Task-6 only walked `Decl::Skill`, so a branch inside a private `block`
/// flow was emitted to IR with the all-false default `predicate_shape`. The
/// borrow-checker concern: `ConditionContext::for_decl` takes `&'a SourceFile`
/// AND `&'a [Param]`/`&'a [FlowStmt]` from the enclosing decl — we cannot
/// hold those borrows while mutating `flow.condition_classification` inside
/// the same decl. Instead we pre-bake everything the classifier consults
/// (consts table, params-with-string-default set) into owned strings BEFORE
/// the mutable walk, then construct `ConditionContext` from references that
/// are independent of the AST nodes we're mutating.
fn annotate_file_branches(
    file: &mut SourceFile,
    imported_text_values: &std::collections::BTreeMap<String, String>,
    imported_const_types: &std::collections::BTreeMap<String, crate::kind_infer::TypeTag>,
) {
    // Pre-bake the consts table with OWNED string keys so its lifetime is
    // independent of `file.decls`. We can't use `&str` keys from `file.decls`
    // and then mutate `file.decls` afterwards — the keys would dangle from
    // the borrow checker's perspective even though the actual string data
    // doesn't move.
    let mut consts_owned: std::collections::HashMap<String, crate::kind_infer::TypeTag> =
        std::collections::HashMap::new();
    {
        let borrowed = crate::condition::collect_consts_for_file(
            file,
            imported_text_values,
            imported_const_types,
        );
        for (k, v) in borrowed {
            consts_owned.insert(k.to_string(), v);
        }
    }

    // Pre-bake (params_with_string_default) for every Skill / Block by index,
    // again with owned strings so we can release the immutable borrow before
    // the mutable walk.
    let mut per_decl_params: Vec<std::collections::HashSet<String>> =
        Vec::with_capacity(file.decls.len());
    for decl in &file.decls {
        let params: &[crate::ast::Param] = match decl {
            Decl::Skill(s) => &s.node.params,
            Decl::Block(b) => &b.node.params,
            _ => &[],
        };
        let mut set: std::collections::HashSet<String> = std::collections::HashSet::new();
        for p in params {
            if p.default_is_name_ref {
                continue;
            }
            // Skip params with an explicit non-String built-in type annotation.
            // `reviewable: Bool = "true"` stores its default as a quoted string
            // (the AST `default` field is always pre-rendered text), but the
            // param itself is a Bool. Without this guard the classifier would
            // emit PredicateConst and Expand would substitute the literal
            // `true` text into the condition, displacing the runtime param
            // reference the author wrote.
            if let Some(ta) = &p.type_annotation {
                let name_lc = ta.node.to_ascii_lowercase();
                if matches!(name_lc.as_str(), "bool" | "int" | "float") {
                    continue;
                }
            }
            if let Some(d) = &p.default {
                if d.starts_with('"') {
                    set.insert(p.name.clone());
                }
            }
        }
        per_decl_params.push(set);
    }

    // Pre-bake the producer-side return-type tables for skill flow walks.
    // Spec §6.3 / Codex Round 2 High 4 — option (i): the per-branch
    // `ConditionContext` snapshot needs to know the agent-shape flag of
    // each flow-local binding declared upstream in the same skill flow.
    // Owned-string keyed so the maps outlive `file.decls`.
    let mut local_callee_return_types: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for decl in &file.decls {
        match decl {
            Decl::Block(b) => {
                if let Some(rt) = &b.node.return_type {
                    local_callee_return_types.insert(b.node.name.clone(), rt.node.clone());
                }
            }
            Decl::ExportBlock(eb) => {
                if let Some(rt) = &eb.node.return_type {
                    local_callee_return_types.insert(eb.node.name.clone(), rt.node.clone());
                }
            }
            _ => {}
        }
    }
    // Imported block return types are not threaded into this function today
    // (callers only pass imported texts + const types). Pass an empty map;
    // the producer-side fallback to `crate::stdlib_sig` covers `subagent`.
    let imported_block_return_types_owned: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    // Build a per-call context whose lifetime is tied to the local owned
    // tables, NOT to `file.decls`. This is what frees us to mutate
    // `flow.condition_classification` inside the loop.
    for (idx, decl) in file.decls.iter_mut().enumerate() {
        match decl {
            Decl::Skill(spanned) => {
                // Spec §6.3 / option (i): walk the skill flow live so each
                // branch sees the live `flow_local_bindings` snapshot.
                let mut flow_local_bindings: std::collections::HashMap<
                    String,
                    crate::condition::ConditionFlowLocal,
                > = std::collections::HashMap::new();
                annotate_skill_flow_with_locals(
                    &mut spanned.node.flow,
                    &consts_owned,
                    &per_decl_params[idx],
                    &local_callee_return_types,
                    &imported_block_return_types_owned,
                    &mut flow_local_bindings,
                );
            }
            Decl::Block(spanned) => {
                // Block flow has no flow-locals (rejected at analyze time),
                // so the pre-bake path is sufficient.
                annotate_decl_branches(
                    &mut spanned.node.flow,
                    &consts_owned,
                    &per_decl_params[idx],
                );
            }
            // `Decl::ExportBlock` has only `flow_strings: Vec<String>` — no
            // structured FlowStmt::Branch nodes to annotate. See design spec
            // Out of Scope §7.
            _ => {}
        }
    }
}

/// Per-decl helper for `annotate_file_branches`: borrow the pre-baked owned
/// consts table and params-with-string-default set, build a transient
/// `ConditionContext`, and run `annotate_branch_classifications` over the
/// decl's flow. Extracted so the `Decl::Skill` and `Decl::Block` arms reduce
/// to a single call each.
fn annotate_decl_branches(
    flow: &mut [FlowStmt],
    consts_owned: &std::collections::HashMap<String, crate::kind_infer::TypeTag>,
    params_owned: &std::collections::HashSet<String>,
) {
    let consts: std::collections::HashMap<&str, crate::kind_infer::TypeTag> = consts_owned
        .iter()
        .map(|(k, v)| (k.as_str(), v.clone()))
        .collect();
    let params_set: std::collections::HashSet<&str> =
        params_owned.iter().map(|s| s.as_str()).collect();
    let ctx = crate::condition::ConditionContext {
        consts,
        params_with_string_default: params_set,
        bindings: std::collections::HashSet::new(),
        flow_local_bindings: std::collections::HashMap::new(),
    };
    annotate_branch_classifications(flow, &ctx);
}

/// Walk a flow body and push
/// `G::analyze::condition-non-boolean-non-predicate` for every Branch /
/// ElifBranch whose cached `condition_classification` reports a numeric
/// bare-condition token. Reads the classification populated by
/// `annotate_file_branches`; never re-classifies.
fn check_flow_numeric_conditions(
    flow: &[FlowStmt],
    span: crate::span::Span,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    for stmt in flow {
        if let FlowStmt::Branch {
            condition_classification,
            then_body,
            elif_branches,
            else_body,
            ..
        } = stmt
        {
            if condition_classification
                .as_ref()
                .map_or(false, |c| c.has_numeric_bare_condition)
            {
                push_numeric_condition_diag(span, file_label, line_index, bag);
            }
            for elif in elif_branches {
                if elif
                    .condition_classification
                    .as_ref()
                    .map_or(false, |c| c.has_numeric_bare_condition)
                {
                    push_numeric_condition_diag(span, file_label, line_index, bag);
                }
                check_flow_numeric_conditions(&elif.body, span, file_label, line_index, bag);
            }
            check_flow_numeric_conditions(then_body, span, file_label, line_index, bag);
            if let Some(eb) = else_body {
                check_flow_numeric_conditions(eb, span, file_label, line_index, bag);
            }
        }
    }
}

fn push_numeric_condition_diag(
    span: crate::span::Span,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    bag.push(
        Diagnostic {
            id: "G::analyze::condition-non-boolean-non-predicate".into(),
            classification: Classification::Error,
            message: "condition expression must be boolean or a string predicate".into(),
            span: SourceSpan::from_byte_span(file_label, span, line_index),
            related: Vec::new(),
            hints: vec![
                "Bind to a boolean (e.g., a Bool-returning call), use a string predicate const, or compare with == or !=. Glyph does not implicitly truth-test integers."
                    .into(),
            ],
        },
        span,
    );
}

/// Emit `G::analyze::condition-non-boolean-non-predicate` for every skill AND
/// private block in `file` that has a numeric-kinded token in a branch
/// condition.
///
/// Called once at the end of `analyze_with_diagnostics` and
/// `analyze_with_imports` after `annotate_file_branches`, so the classifier
/// results are already populated. Task 6 extends this to walk `Decl::Block`
/// in addition to `Decl::Skill` (closes Finding 1: a numeric-bare condition
/// inside a private block's flow used to slip past analyze).
fn check_file_numeric_conditions(
    file: &SourceFile,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    for decl in &file.decls {
        // `Decl::ExportBlock` flow walking deferred per design spec Out of
        // Scope §7 — `ExportBlockDecl.flow_strings` carries no structured
        // FlowStmt::Branch.
        let (flow, span) = match decl {
            Decl::Skill(spanned) => (&spanned.node.flow, spanned.span),
            Decl::Block(spanned) => (&spanned.node.flow, spanned.span),
            _ => continue,
        };
        check_flow_numeric_conditions(flow, span, file_label, line_index, bag);
    }
}

// ---------------------------------------------------------------------------

/// Check applies() calls in a branch condition string.
/// Validates: applies-on-non-block, applies-on-undescribed-block.
///
/// Flow-position assignments (`.flow-assign-spec.md` §6.3): the
/// `flow_local_types` parameter carries the live `FlowScope.flow_local_types`
/// at the branch site. An agent-shape flow-local (`is_agent == true`) is
/// a valid `.applies()` receiver — a `subagent(...)` binding — so a hit
/// short-circuits the resolver before falling through to the existing
/// non-block diagnostic.
#[allow(clippy::too_many_arguments)]
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
    flow_local_types: &HashMap<String, FlowLocalType>,
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
            // §6.3: an agent-shape flow-local binding is a valid
            // `.applies()` receiver. Plumb the live FlowScope so this
            // branch annotation runs against the binding state at the
            // current walk position.
            if let Some(flt) = flow_local_types.get(receiver_name) {
                if flt.is_agent {
                    search_from = abs_pos + applies_suffix.len();
                    continue;
                }
            }
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
    context_skill_names: &HashSet<&str>,
    block_decls: &HashMap<&str, &crate::ast::BlockDecl>,
    export_block_decls: &HashMap<&str, &crate::ast::ExportBlockDecl>,
    imported_block_params: &HashMap<String, Vec<crate::ast::Param>>,
) {
    // Codex P2 follow-up to PRD #103 / #105: a call inside an `if`/`elif`/
    // `else` body must run the same required-arg check as a top-level
    // call. Pre-fix this walker only verified name resolution — branch-
    // body callees with required parameters compiled silently.
    let lookup_params = |name: &str| -> Option<&[crate::ast::Param]> {
        if let Some(c) = block_decls.get(name) {
            Some(&c.params)
        } else if let Some(c) = export_block_decls.get(name) {
            Some(&c.params)
        } else {
            imported_block_params.get(name).map(|v| v.as_slice())
        }
    };
    for stmt in body {
        match stmt {
            FlowStmt::Call { target, args, .. } => {
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
                                hints: vec![format!(
                                    "declare `block {}()` or check the name for typos",
                                    target.node
                                )],
                            },
                            span,
                        );
                    }
                } else if let Some(params) = lookup_params(target.node.as_str()) {
                    for d in validate_call_args(
                        &target.node,
                        params,
                        args,
                        target.span,
                        file_label,
                        line_index,
                    ) {
                        bag.push(d, target.span);
                    }
                }
            }
            FlowStmt::ConstraintMarker(marker) => {
                if !text_names.contains(marker.name.node.as_str()) {
                    bag.push(
                        Diagnostic::error(
                            "G::analyze::undefined-name",
                            format!(
                                "`{}` is not a declared `const` in this file",
                                marker.name.node
                            ),
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
                    context_skill_names,
                    span,
                    file_label,
                    line_index,
                    bag,
                );
            }
            // Issue #84 codex pass 4 — AC-pass4-5: a `return some_callee()`
            // nested inside an `if`/`elif`/`else` body must run the same
            // undefined-call resolver as a top-level Return. Pre-fix this
            // arm fell into the catch-all and the diagnostic was silently
            // dropped — symmetric in spirit to pass-2's branch-body
            // nominal-walk extension.
            FlowStmt::Return(expr) => {
                check_return_call_undefined(expr, span, block_names, file_label, line_index, bag);
                if let crate::ast::ReturnExpr::Call { target, args } = expr {
                    if let Some(params) = lookup_params(target.node.as_str()) {
                        for d in validate_call_args(
                            &target.node,
                            params,
                            args,
                            target.span,
                            file_label,
                            line_index,
                        ) {
                            bag.push(d, target.span);
                        }
                    }
                }
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

/// PRD #103 / Slice 1 (#104): pure validator for call-site argument
/// satisfaction.
///
/// Given a call's positional `args` and the resolved `callee_params`, return
/// one `G::analyze::missing-required-arg` Error diagnostic per required
/// parameter (i.e. `default.is_none()`) that no positional argument satisfies.
/// Pure: no I/O, no bag, no reliance on the rest of the analyze pipeline —
/// the caller pushes returned diagnostics into its own `DiagBag`.
///
/// Binding rule for MVP: positional. Param at index `i` is satisfied iff
/// `i < args.len()`. Defaulted params are never reported. Named arguments
/// are out of scope (PRD §"Out of Scope").
///
/// Reusable across `block`, `export block`, and `skill` callees. Slice 1
/// only wires it for private `block` callees; later slices route export-block
/// calls through the same function once the defaults-required rule is dropped.
pub(crate) fn validate_call_args(
    callee_name: &str,
    callee_params: &[ast::Param],
    args: &[String],
    call_span: Span,
    file_label: &str,
    line_index: &LineIndex,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (i, p) in callee_params.iter().enumerate() {
        if p.default.is_none() && i >= args.len() {
            out.push(Diagnostic::error(
                "G::analyze::missing-required-arg",
                format!(
                    "call to `{}()` is missing required argument `{}`",
                    callee_name, p.name
                ),
                SourceSpan::from_byte_span(file_label, call_span, line_index),
            ));
        }
    }
    out
}

fn analyze_export_block(
    spanned: &crate::span::Spanned<crate::ast::ExportBlockDecl>,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    registry: &mut crate::domain_registry::Registry,
    private_names: &HashSet<&str>,
    visible_binding_names: &HashSet<&str>,
    case_bad: &HashSet<Span>,
    explicit_decl_seen: &mut HashSet<String>,
    text_names: &HashSet<&str>,
    block_names: &HashSet<&str>,
    block_decls: &HashMap<&str, &crate::ast::BlockDecl>,
    export_block_decls: &HashMap<&str, &crate::ast::ExportBlockDecl>,
    imported_block_params: &HashMap<String, Vec<crate::ast::Param>>,
    local_callee_return_types: &HashMap<&str, &Spanned<String>>,
    imported_block_return_types: &HashMap<String, Spanned<String>>,
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
        case_bad,
        explicit_decl_seen,
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

        // B03: G::analyze::undefined-call — emit when `terminal_return` is a
        // call to a name that resolves to no `block`/`export block` (same-file
        // or imported). Mirrors the FlowStmt::Return(Call) walk used by
        // skills/blocks; ExportBlockDecl has no `flow: Vec<FlowStmt>`, so we
        // inspect `terminal_return` directly.
        check_return_call_undefined(expr, spanned.span, block_names, file_label, line_index, bag);

        // B03: G::analyze::missing-required-arg — when `terminal_return` is a
        // call, verify each required parameter is satisfied by a positional
        // argument. Mirrors the FlowStmt::Call resolver used elsewhere.
        if let crate::ast::ReturnExpr::Call { target, args, .. } = expr {
            let callee_params: Option<&[crate::ast::Param]> = block_decls
                .get(target.node.as_str())
                .map(|b| b.params.as_slice())
                .or_else(|| {
                    export_block_decls
                        .get(target.node.as_str())
                        .map(|eb| eb.params.as_slice())
                })
                .or_else(|| {
                    imported_block_params
                        .get(target.node.as_str())
                        .map(|v| v.as_slice())
                });
            if let Some(params) = callee_params {
                // B03 GAP 2: pin missing-required-arg diagnostic span to the callee
                // identifier (`target.span`) so the squiggle covers `foo`, not the
                // entire export-block declaration. Matches the FlowStmt::Call
                // resolver convention used by skill/block callers.
                let diags = validate_call_args(
                    target.node.as_str(),
                    params,
                    args,
                    target.span,
                    file_label,
                    line_index,
                );
                let sp = target.span;
                for d in diags {
                    bag.push(d, sp);
                }
            }
        }

        // B03: G::analyze::nominal-mismatch — when `terminal_return` is a call
        // to a typed callee, the callee's `-> Type` must canonically match the
        // export block's own `-> Type`. Mirrors `check_block_return_calls`.
        let return_stmt = crate::ast::FlowStmt::Return(expr.clone());
        check_return_call_nominal(
            decl.return_type.as_ref(),
            &return_stmt,
            spanned.span,
            registry,
            local_callee_return_types,
            imported_block_return_types,
            file_label,
            line_index,
            bag,
        );
    }

    // B03 GAP 1: validate non-return flow-position calls collected from the
    // export block's `flow:` section. Each `FlowCallRef` represents a call
    // that is NOT the terminal `return foo(...)` — either a standalone
    // root-level call or a call inside an `if`/`elif`/`else` branch body.
    // Mirrors the FlowStmt::Call resolver's validation suite used elsewhere:
    // `G::analyze::undefined-call` and `G::analyze::missing-required-arg`.
    // Diagnostic spans pin to `target.span` (the callee identifier), not the
    // surrounding export block, so the squiggle covers `foo`, not the whole
    // declaration. Nominal-mismatch is intentionally skipped — these are not
    // the export block's return value; only the terminal-return path runs the
    // nominal check.
    for call in &decl.flow_calls {
        let synthetic = ReturnExpr::Call {
            target: call.target.clone(),
            args: call.args.clone(),
        };
        check_return_call_undefined(
            &synthetic,
            call.target.span,
            block_names,
            file_label,
            line_index,
            bag,
        );
        let callee_params: Option<&[Param]> = block_decls
            .get(call.target.node.as_str())
            .map(|b| b.params.as_slice())
            .or_else(|| {
                export_block_decls
                    .get(call.target.node.as_str())
                    .map(|eb| eb.params.as_slice())
            })
            .or_else(|| {
                imported_block_params
                    .get(call.target.node.as_str())
                    .map(|v| v.as_slice())
            });
        if let Some(params) = callee_params {
            let diags = validate_call_args(
                call.target.node.as_str(),
                params,
                &call.args,
                call.target.span,
                file_label,
                line_index,
            );
            let sp = call.target.span;
            for d in diags {
                bag.push(d, sp);
            }
        }
    }

    // PRD #103 / Slice 2 (#105): the previous `G::analyze::missing-param-default`
    // rule (which required every export-block parameter to declare a default)
    // has been retired. Export-block parameters may now be required, matching
    // the private-`block` semantics. Call-site enforcement lives in
    // `validate_call_args` (FlowStmt::Call resolver above) and surfaces
    // `G::analyze::missing-required-arg` when a caller omits the positional
    // argument for a required parameter.

    // G::analyze::missing-return — export block must have an explicit return.
    // Typed export blocks route through G::analyze::typed-decl-missing-return instead.
    if !decl.has_return && decl.return_type.is_none() {
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

    // B03: G::analyze::undefined-name — body-level constraint markers
    // (`require X`, `avoid X`, `must X`, `must avoid X`) must reference a
    // declared `const`. Mirrors the skill body_constraints sweep at the
    // `analyze_skill` site (~line 5566).
    for marker in &decl.body_constraints {
        if !text_names.contains(marker.name.node.as_str()) {
            let span = spanned.span;
            bag.push(
                Diagnostic::error(
                    "G::analyze::undefined-name",
                    format!(
                        "`{}` is not a declared `const` in this file",
                        marker.name.node
                    ),
                    SourceSpan::from_byte_span(file_label, span, line_index),
                ),
                span,
            );
        }
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
            Decl::Const(c) => {
                bound.insert(c.node.name.clone());
            }
            Decl::Block(b) => {
                bound.insert(b.node.name.clone());
            }
            Decl::ExportBlock(b) => {
                bound.insert(b.node.name.clone());
            }
            Decl::Skill(s) => {
                bound.insert(s.node.name.clone());
            }
            Decl::Import(imp) => match &imp.node.kind {
                ast::ImportKind::Selective(names) => {
                    for n in names {
                        let local = n
                            .alias
                            .as_ref()
                            .map(|a| a.node.clone())
                            .unwrap_or_else(|| n.name.node.clone());
                        bound.insert(local);
                    }
                }
                ast::ImportKind::WholeModule { alias } => {
                    bound.insert(alias.node.clone());
                }
            },
            Decl::TypeDecl(_) => {} // TODO: handled in Task B.4+
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
            for stmt in &s.node.flow {
                collect_refs_from_flow_stmt(stmt, out);
            }
            for n in &s.node.body_bare_names {
                out.insert(n.clone());
            }
            for m in &s.node.body_constraints {
                out.insert(m.name.node.clone());
            }
            for entry in &s.node.body_context {
                if let ContextEntry::NameRef(n) = entry {
                    out.insert(n.node.clone());
                }
            }
            for entry in &s.node.context_section {
                if let ContextEntry::NameRef(n) = entry {
                    out.insert(n.node.clone());
                }
            }
            for entry in &s.node.constraints_section {
                if let ContextEntry::NameRef(n) = entry {
                    out.insert(n.node.clone());
                }
            }
            collect_refs_from_params(&s.node.params, out);
            collect_refs_from_extra_subsections(&s.node.extra_subsections, out);
        }
        Decl::Block(b) => {
            for stmt in &b.node.flow {
                collect_refs_from_flow_stmt(stmt, out);
            }
            for m in &b.node.body_constraints {
                out.insert(m.name.node.clone());
            }
            for entry in &b.node.body_context {
                if let ContextEntry::NameRef(n) = entry {
                    out.insert(n.node.clone());
                }
            }
            collect_refs_from_params(&b.node.params, out);
            collect_refs_from_extra_subsections(&b.node.extra_subsections, out);
        }
        Decl::ExportBlock(b) => {
            if let Some(expr) = &b.node.terminal_return {
                collect_refs_from_return_expr(expr, out);
            }
            // Issue #166: walk structurally-captured body refs so
            // `glyph fmt` can preserve imports referenced only by an
            // `export block` body. Mirrors the Skill / Block arms above.
            for n in &b.node.body_refs {
                out.insert(n.clone());
            }
            for m in &b.node.body_constraints {
                out.insert(m.name.node.clone());
            }
            for entry in &b.node.body_context {
                if let ContextEntry::NameRef(n) = entry {
                    out.insert(n.node.clone());
                }
            }
            collect_refs_from_params(&b.node.params, out);
            collect_refs_from_extra_subsections(&b.node.extra_subsections, out);
        }
        Decl::Const(_) | Decl::Import(_) => {}
        Decl::TypeDecl(_) => {} // TODO: handled in Task B.4+
    }
}

fn collect_refs_from_flow_stmt(stmt: &FlowStmt, out: &mut HashSet<String>) {
    match stmt {
        FlowStmt::Call { target, .. } => {
            out.insert(target.node.clone());
        }
        FlowStmt::Return(expr) => collect_refs_from_return_expr(expr, out),
        FlowStmt::Branch {
            then_body,
            elif_branches,
            else_body,
            ..
        } => {
            for s in then_body {
                collect_refs_from_flow_stmt(s, out);
            }
            for eb in elif_branches {
                for s in &eb.body {
                    collect_refs_from_flow_stmt(s, out);
                }
            }
            if let Some(eb) = else_body {
                for s in eb {
                    collect_refs_from_flow_stmt(s, out);
                }
            }
        }
        FlowStmt::BareName(n) => {
            out.insert(n.node.clone());
        }
        FlowStmt::ConstraintMarker(m) => {
            out.insert(m.name.node.clone());
        }
        FlowStmt::ContextMarker(ContextEntry::NameRef(n)) => {
            out.insert(n.node.clone());
        }
        FlowStmt::ContextMarker(ContextEntry::InlineString(_)) | FlowStmt::InlineString(_) => {}
    }
}

/// Walk parameter defaults and emit any name-ref defaults into `out`.
/// `Param.default` is a pre-rendered string; `default_is_name_ref = true`
/// indicates that the string is a bare-name reference that must resolve to
/// an in-scope `const` at compile time. The import-pruner needs to see these
/// names so it does not delete the import they refer to (reviewer P1.2).
fn collect_refs_from_params(params: &[Param], out: &mut HashSet<String>) {
    for p in params {
        if p.default_is_name_ref {
            if let Some(default) = &p.default {
                out.insert(default.clone());
            }
        }
    }
}

/// Walk a decl's recovered duplicate sub-sections (issue #109) and emit any
/// name refs they carry into `out`. Pre-fix, `glyph fmt`'s import-pruner ran
/// *before* the duplicate-section merge, so any import referenced ONLY from
/// a duplicate sub-section body was silently dropped (reviewer P1.1).
fn collect_refs_from_extra_subsections(extras: &[DuplicateSubsection], out: &mut HashSet<String>) {
    for extra in extras {
        match extra {
            DuplicateSubsection::Constraints(markers) => {
                for m in markers {
                    out.insert(m.name.node.clone());
                }
            }
            DuplicateSubsection::Context(entries) => {
                for entry in entries {
                    if let ContextEntry::NameRef(n) = entry {
                        out.insert(n.node.clone());
                    }
                }
            }
            DuplicateSubsection::Flow(stmts) => {
                for stmt in stmts {
                    collect_refs_from_flow_stmt(stmt, out);
                }
            }
            // `Description(String)` is a quoted literal; `Effects(Vec<String>)`
            // is a keyword list. Neither carries a name reference.
            DuplicateSubsection::Description(_) | DuplicateSubsection::Effects(_) => {}
        }
    }
}

fn collect_refs_from_return_expr(expr: &ReturnExpr, out: &mut HashSet<String>) {
    match expr {
        ReturnExpr::Call { target, .. } => {
            out.insert(target.node.clone());
        }
        ReturnExpr::Name(n) => {
            out.insert(n.node.clone());
        }
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
                    for e in eff {
                        effects.insert((*e).to_string());
                    }
                }
            }
            FlowStmt::Return(ReturnExpr::Call { target, .. }) => {
                if let Some(eff) = stdlib_block_effects(target.node.as_str()) {
                    for e in eff {
                        effects.insert((*e).to_string());
                    }
                }
            }
            FlowStmt::Branch {
                then_body,
                elif_branches,
                else_body,
                ..
            } => {
                for s in then_body {
                    walk(s, effects);
                }
                for eb in elif_branches {
                    for s in &eb.body {
                        walk(s, effects);
                    }
                }
                if let Some(eb) = else_body {
                    for s in eb {
                        walk(s, effects);
                    }
                }
            }
            _ => {}
        }
    }
    for stmt in flow {
        walk(stmt, &mut effects);
    }
    effects.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_ids(src: &str) -> Vec<String> {
        crate::check_source(src, 0, "test.glyph")
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
        let bag = crate::check_source(src, 0, "test.glyph");
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
        let bag = crate::check_source(src, 0, "test.glyph");
        let ids: Vec<String> = bag.iter().map(|d| d.id.clone()).collect();
        assert!(
            ids.iter()
                .any(|id| id == "G::analyze::placeholder-string-return"),
            "expected placeholder-string-return for descriptive form, got {ids:?}"
        );
        assert_eq!(bag.exit_code(), 2, "diagnostic must be repairable-tier");
        let hints: Vec<String> = bag.iter().flat_map(|d| d.hints.iter().cloned()).collect();
        assert!(
            hints
                .iter()
                .any(|h| h.contains("<\"root cause and severity\">")),
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
            "test.glyph",
            &line_index,
            &mut bag,
            &text_names,
            &block_names,
            &block_decls,
            &HashMap::new(),
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
            "test.glyph",
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
                body_constraints: Vec::new(),
                body_context: Vec::new(),
                flow: vec![FlowStmt::InlineString("Write files.".to_string())],
                description: None,
                effects: vec!["writes_files".to_string()],
                return_type: None,
                generated: false,
                extra_subsections: Vec::new(),
                description_span: None,
                context_section_span: None,
                constraints_section_span: None,
                effects_span: None,
                flow_span: None,
                freeform_sections: Vec::new(),
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
                    bound_name: None,
                }],
                flow_present: true,
                body_constraints: Vec::new(),
                body_context: Vec::new(),
                body_bare_names: Vec::new(),
                effects: Vec::new(),
                context_section: Vec::new(),
                constraints_section: Vec::new(),
                return_type: None,
                extra_subsections: Vec::new(),
                description_span: None,
                context_section_span: None,
                constraints_section_span: None,
                effects_span: None,
                flow_span: None,
                freeform_sections: Vec::new(),
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
        analyze_with_diagnostics(
            file.clone(),
            0,
            "test.glyph",
            &li,
            &mut bag_on,
            &mut registry_on,
        );
        let ids_on: Vec<&str> = bag_on.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids_on.contains(&"G::analyze::missing-effects"),
            "expected missing-effects diagnostic, got: {:?}",
            ids_on
        );

        // Verifying the diagnostic fires (effects tracking is always active).
        let mut bag_off = DiagBag::new();
        let mut registry_off = crate::domain_registry::Registry::new();
        analyze_with_diagnostics(file, 0, "test.glyph", &li, &mut bag_off, &mut registry_off);
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
            "test.glyph",
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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);
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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);
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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);
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
            "test.glyph",
            &line_index,
            &mut bag,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &mut used,
            &HashMap::new(),
            &mut registry,
            &HashMap::new(),
            &HashMap::new(),
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
            &HashMap::new(),
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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);
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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);
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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);
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
        let path = PathBuf::from("test.glyph");
        let (_file, res) =
            analyze_with_resolutions(file, 0, "test.glyph", &path, &line_index, &mut bag, false);
        let block_res = res.iter().find(|r| r.kind == ResolutionKind::Block);
        assert!(
            block_res.is_some(),
            "expected a Block resolution, got: {:?}",
            res
        );
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
        let path = PathBuf::from("t.glyph");
        let (_, res) =
            analyze_with_resolutions(file, 0, "t.glyph", &path, &line_index, &mut bag, false);
        let text_res = res.iter().find(|r| r.kind == ResolutionKind::Text);
        assert!(
            text_res.is_some(),
            "expected a Text resolution, got: {:?}",
            res
        );
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
        let path = PathBuf::from("t.glyph");
        let (_, res) =
            analyze_with_resolutions(file, 0, "t.glyph", &path, &line_index, &mut bag, false);
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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);
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

    // Task 8 — `t1`/`t2`/`t3`/`t4`/`t6` (cross-kind type-vs-param/const
    // collision assertions) were deleted: under the two-namespace rule a
    // domain-type return annotation no longer collides with a same-canonical
    // value-namespace binding. Same-namespace collisions are exercised by
    // `sweep_value_name_collisions` tests and the type-decl tests below.

    #[test]
    fn t7_no_collision_when_canonical_names_differ() {
        // Negative control: param `repository` does NOT collide with type
        // `Report` — different canonical keys. Catches a substring-instead-of-
        // equality regression in the canonical comparison.
        let src = "skill main(repository = \"x\") -> Report\n    description: \"Main.\"\n    flow:\n        \"do\"\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

        let diags = collision_diags(&bag);
        assert!(
            diags.is_empty(),
            "empty registry must yield zero collision diagnostics, got: {:?}",
            diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
        );
    }

    // Task 8 — `t10_imports_path_parity_emits_collision` and
    // `t3_private_block_return_type_collides_with_block_param` were deleted:
    // both asserted cross-kind type-vs-param collisions which are now legal
    // under the two-namespace split.

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
            "test.glyph",
            &line_index,
            &mut bag,
            &HashSet::new(),
            &imported_blocks,
            &HashSet::new(),
            &HashSet::new(),
            &mut used,
            &HashMap::new(),
            &mut registry,
            &imported_block_return_types,
            &HashMap::new(),
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
            &HashMap::new(),
            &HashMap::new(),
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
            "test.glyph",
            &line_index,
            &mut bag,
            &HashSet::new(),
            &imported_blocks,
            &HashSet::new(),
            &HashSet::new(),
            &mut used,
            &HashMap::new(),
            &mut registry,
            &imported_block_return_types,
            &HashMap::new(),
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
            &HashMap::new(),
            &HashMap::new(),
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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
            "test.glyph",
            &line_index,
            &mut bag,
            &HashSet::new(),
            &imported_blocks,
            &HashSet::new(),
            &HashSet::new(),
            &mut used,
            &HashMap::new(),
            &mut registry,
            &HashMap::new(),
            &HashMap::new(),
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
            &HashMap::new(),
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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
            "test.glyph",
            &line_index,
            &mut bag,
            &HashSet::new(),
            &imported_blocks,
            &HashSet::new(),
            &HashSet::new(),
            &mut used,
            &HashMap::new(),
            &mut registry,
            &imported_block_return_types,
            &HashMap::new(),
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
            &HashMap::new(),
            &HashMap::new(),
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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
    fn t13_same_file_return_call_to_export_block_fires_nominal_mismatch_on_type_mismatch() {
        // PRD #103 / Slice 2 (#105) Codex P2 follow-up: same-file export
        // blocks are now legal call targets (Slice A made
        // `return exported_fn()` resolve via `export_block_decls`), so
        // the chunk-4 nominal-match must run against export-block return
        // types just like it does for `Decl::Block`. Pre-fix the
        // `local_callee_return_types` map was restricted to `Decl::Block`,
        // which silently skipped the type check for same-file export
        // calls — a real type bug would slip through with no diagnostic.
        //
        // Fixture uses mismatched types (`Plan` vs `Report`) so the
        // check has something to fire on; matching-type fixtures
        // short-circuit `nominal_match` to true regardless.
        let src = "export block exported_fn() -> Plan\n    description: \"Make a plan.\"\n    flow:\n        return \"x\"\n\nskill main() -> Report\n    description: \"Main.\"\n    flow:\n        return exported_fn()\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

        let mismatches = nominal_mismatches(&bag);
        assert_eq!(
            mismatches.len(),
            1,
            "same-file `return` to export block with mismatched type must fire nominal-mismatch; got: {:?}",
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
            "test.glyph",
            &line_index,
            &mut bag,
            &HashSet::new(),
            &imported_blocks,
            &HashSet::new(),
            &HashSet::new(),
            &mut used,
            &HashMap::new(),
            &mut registry,
            &imported_block_return_types,
            &HashMap::new(),
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
            &HashMap::new(),
            &HashMap::new(),
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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
        let src = "import \"./lib.glyph\" { imported_foo }\n\nskill main()\n    description: \"Main.\"\n    flow:\n        helper()\n\nblock helper()\n    description: \"Helper.\"\n    flow:\n        return imported_foo()\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let mut used: HashSet<String> = HashSet::new();

        let mut imported_blocks: HashSet<String> = HashSet::new();
        imported_blocks.insert("imported_foo".to_string());

        let _ = analyze_with_imports(
            &file,
            0,
            "test.glyph",
            &line_index,
            &mut bag,
            &HashSet::new(),
            &imported_blocks,
            &HashSet::new(),
            &HashSet::new(),
            &mut used,
            &HashMap::new(),
            &mut registry,
            &HashMap::new(),
            &HashMap::new(),
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
            &HashMap::new(),
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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
            "test.glyph",
            &line_index,
            &mut bag,
            &HashSet::new(),
            &imported_blocks,
            &HashSet::new(),
            &HashSet::new(),
            &mut used,
            &HashMap::new(),
            &mut registry,
            &HashMap::new(),
            &HashMap::new(),
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
            &HashMap::new(),
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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
    fn t20_return_call_to_same_file_export_block_resolves() {
        // PRD #103 / Slice 2 (#105): same-file `export block` is now a valid
        // call target — the prior asymmetry (Decl::Block-only `block_names`)
        // has been retired so the FlowStmt::Call resolver and the Return
        // resolver both recognize sibling export-block callees. A
        // `return same_file_export_block()` boundary therefore resolves
        // cleanly and no `undefined-call` is emitted.
        let src = "export block exported_fn() -> Plan\n    description: \"Make a plan.\"\n    flow:\n        return \"x\"\n\nskill main() -> Report\n    description: \"Main.\"\n    flow:\n        return exported_fn()\n";
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

        let diags = undefined_call_diags(&bag);
        assert_eq!(
            diags.len(),
            0,
            "same-file ExportBlock callee in return position must resolve without undefined-call; got: {:?}",
            bag.iter().map(|d| (d.id.as_str(), d.message.as_str())).collect::<Vec<_>>()
        );
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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
        let path = PathBuf::from("t.glyph");
        let (_, res) =
            analyze_with_resolutions(file, 0, "t.glyph", &path, &line_index, &mut bag, false);
        let stdlib_count = res
            .iter()
            .filter(|r| r.kind == ResolutionKind::Stdlib)
            .count();
        assert_eq!(
            stdlib_count, 2,
            "expected 2 Stdlib resolutions, got: {:?}",
            res
        );
    }

    #[test]
    fn collect_cross_file_resolutions_records_imported_block_call() {
        // Importer references an imported block by its local name.
        let src = r#"import "./repo_tools.glyph" { inspect_repo }

skill main()
    description: "main."
    flow:
        inspect_repo()
"#;
        let file = parse_for_resolutions(src);

        // Build a target table mirroring what `lib::check_source_with_resolutions`
        // would produce after parsing the dependency.
        let mut targets: HashMap<String, ImportTarget> = HashMap::new();
        let dep_path = PathBuf::from("/tmp/repo_tools.glyph");
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
        assert_eq!(
            res.len(),
            2,
            "expected 2 cross-file resolutions, got: {:?}",
            res
        );
        // Both should point at the dep file.
        for r in &res {
            assert_eq!(r.def_file, dep_path);
        }
        let import_kind_count = res
            .iter()
            .filter(|r| r.kind == ResolutionKind::Import)
            .count();
        let block_kind_count = res
            .iter()
            .filter(|r| matches!(r.kind, ResolutionKind::Block | ResolutionKind::ExportBlock))
            .count();
        assert_eq!(import_kind_count, 1, "expected 1 Import-kind resolution");
        assert_eq!(
            block_kind_count, 1,
            "expected 1 Block/ExportBlock-kind resolution"
        );
    }

    #[test]
    fn analyze_with_resolutions_records_flow_context_block() {
        // P2 regression: `flow: context helper` where `helper` is a local
        // `block` must resolve as `ResolutionKind::Block`. Body-level
        // `context helper` (under a top-level `context:` sub-section) already
        // resolves correctly via `record_context_name_use`; the flow-level
        // walker must produce the same target set.
        let src = r#"block helper()
    "help"

skill main()
    description: "main."
    flow:
        context helper
"#;
        let file = parse_for_resolutions(src);
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let path = PathBuf::from("test.glyph");
        let (_file, res) =
            analyze_with_resolutions(file, 0, "test.glyph", &path, &line_index, &mut bag, false);
        let block_res = res.iter().find(|r| r.kind == ResolutionKind::Block);
        assert!(
            block_res.is_some(),
            "expected a Block resolution for flow-level `context helper`, got: {:?}",
            res
        );
        let r = block_res.unwrap();
        let use_text = &src[r.use_span.start as usize..r.use_span.end as usize];
        assert_eq!(use_text, "helper");
        assert_eq!(r.def_file, path);
    }

    #[test]
    fn analyze_with_resolutions_records_flow_context_export_block() {
        // Same shape as `analyze_with_resolutions_records_flow_context_block`
        // but the local definition is an `export block`; resolution kind must
        // be `ExportBlock`.
        let src = "export block helper()\n    \"help\"\n\nskill main()\n    description: \"main.\"\n    flow:\n        context helper\n";
        let file = parse_for_resolutions(src);
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let path = PathBuf::from("test.glyph");
        let (_file, res) =
            analyze_with_resolutions(file, 0, "test.glyph", &path, &line_index, &mut bag, false);
        let xb_res = res.iter().find(|r| r.kind == ResolutionKind::ExportBlock);
        assert!(
            xb_res.is_some(),
            "expected an ExportBlock resolution for flow-level \
             `context helper`, got: {:?}",
            res
        );
        let r = xb_res.unwrap();
        let use_text = &src[r.use_span.start as usize..r.use_span.end as usize];
        assert_eq!(use_text, "helper");
        assert_eq!(r.def_file, path);
    }

    /// Issue #166 reviewer round 1 #26: LSP-resolution test that an
    /// `export block` body-level `context X` resolves to a same-file
    /// `block` target via `record_context_name_use`. Mirrors
    /// `analyze_with_resolutions_records_flow_context_block` but the
    /// `context X` lives at indent 1 inside the export block (not in
    /// a flow statement), exercising the `Decl::ExportBlock` arm of
    /// `collect_same_file_resolutions`.
    #[test]
    fn analyze_with_resolutions_records_export_block_body_context_block() {
        let src = "block target_helper()\n    \"work\"\n\nexport block consumer() -> Text\n    context target_helper\n    flow:\n        return \"ok\"\n";
        let file = parse_for_resolutions(src);
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let path = PathBuf::from("test.glyph");
        let (_file, res) =
            analyze_with_resolutions(file, 0, "test.glyph", &path, &line_index, &mut bag, false);
        let block_res = res.iter().find(|r| {
            r.kind == ResolutionKind::Block
                && &src[r.use_span.start as usize..r.use_span.end as usize] == "target_helper"
        });
        assert!(
            block_res.is_some(),
            "expected a Block resolution for export-block body-level \
             `context target_helper`, got: {:?}",
            res
        );
        let r = block_res.unwrap();
        assert_eq!(r.def_file, path);
    }

    #[test]
    fn collect_cross_file_resolutions_records_flow_context_imported_block() {
        // Importer references an imported block via flow-level `context`.
        // The cross-file walker must use `record_cross_file_any_use` (not the
        // text-only variant) so block/export-block imports resolve here.
        let src = r#"import "./repo_tools.glyph" { repo_layout }

skill main()
    description: "main."
    flow:
        context repo_layout
"#;
        let file = parse_for_resolutions(src);

        let mut targets: HashMap<String, ImportTarget> = HashMap::new();
        let dep_path = PathBuf::from("/tmp/repo_tools.glyph");
        targets.insert(
            "repo_layout".to_string(),
            ImportTarget {
                local_name: "repo_layout".to_string(),
                def_file: dep_path.clone(),
                def_span: Span::new(0, 0, 64),
                kind: ResolutionKind::ExportBlock,
            },
        );

        let res = collect_cross_file_resolutions(&file, &targets);
        // Two cross-file resolutions: the import-line name token + the
        // flow-level `context repo_layout` use.
        assert_eq!(
            res.len(),
            2,
            "expected 2 cross-file resolutions, got: {:?}",
            res
        );
        let xb_kind_count = res
            .iter()
            .filter(|r| r.kind == ResolutionKind::ExportBlock)
            .count();
        assert_eq!(
            xb_kind_count, 1,
            "expected 1 ExportBlock-kind resolution from `context repo_layout`, got: {:?}",
            res
        );
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
        assert!(
            signals.unresolved_names.contains("subagent"),
            "subagent is not imported and not local — should be unresolved"
        );
        assert!(
            !signals.unresolved_names.contains("send"),
            "send is imported, should not be unresolved"
        );
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

        let effects = signals
            .inferred_effects
            .get("main")
            .expect("main should have inferred effects");
        assert!(
            effects.iter().any(|e| e == "spawns_agent"),
            "expected spawns_agent in inferred effects, got {:?}",
            effects
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
            src,
            0,
            "test.glyph",
            &line_index,
            &mut bag,
            true,
        )
        .expect("parse with effects enabled");
        let signals = crate::analyze::fmt_signals(&file);

        // Either the key is absent or its value is empty — either way the
        // inferred_effects map must not contain a non-empty entry for "main".
        let is_empty_or_absent = signals
            .inferred_effects
            .get("main")
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

    // --- Issue #165: collect_referenced_names must walk body markers ---
    //
    // Root-cause regression: even on a Skill, an `import` referenced only by
    // a body-level `require X` / `context X` (or via a flow-level
    // `FlowStmt::ConstraintMarker` / `FlowStmt::ContextMarker`) was invisible
    // to `fmt_signals.referenced_names`, and `remove_unused_imports` deleted
    // the import. Pin every walk path so `glyph fmt` stops dropping these.

    #[test]
    fn fmt_signals_walks_skill_body_constraint_markers() {
        let src = r#"import "./external.glyph" { accuracy }

skill main()
    description: "Main."
    require accuracy
    flow:
        "do work"
"#;
        let (file, _) = crate::parse::parse(src, 0).expect("parse");
        let signals = crate::analyze::fmt_signals(&file);
        assert!(
            signals.referenced_names.contains("accuracy"),
            "body-level `require accuracy` on a Skill must surface in \
             referenced_names so `glyph fmt` does not drop the import; \
             got {:?}",
            signals.referenced_names
        );
    }

    #[test]
    fn fmt_signals_walks_skill_body_context_nameref() {
        let src = r#"import "./external.glyph" { project_conventions }

skill main()
    description: "Main."
    context project_conventions
    flow:
        "do work"
"#;
        let (file, _) = crate::parse::parse(src, 0).expect("parse");
        let signals = crate::analyze::fmt_signals(&file);
        assert!(
            signals.referenced_names.contains("project_conventions"),
            "body-level `context project_conventions` on a Skill must \
             surface in referenced_names; got {:?}",
            signals.referenced_names
        );
    }

    #[test]
    fn fmt_signals_walks_skill_context_section_nameref() {
        let src = r#"import "./external.glyph" { repo_layout }

skill main()
    description: "Main."
    context:
        repo_layout
    flow:
        "do work"
"#;
        let (file, _) = crate::parse::parse(src, 0).expect("parse");
        let signals = crate::analyze::fmt_signals(&file);
        assert!(
            signals.referenced_names.contains("repo_layout"),
            "`context:` sub-section name-ref on a Skill must surface in \
             referenced_names; got {:?}",
            signals.referenced_names
        );
    }

    #[test]
    fn fmt_signals_walks_skill_flow_constraint_marker() {
        let src = r#"import "./external.glyph" { accuracy }

skill main()
    description: "Main."
    flow:
        require accuracy
        "do work"
"#;
        let (file, _) = crate::parse::parse(src, 0).expect("parse");
        let signals = crate::analyze::fmt_signals(&file);
        assert!(
            signals.referenced_names.contains("accuracy"),
            "flow-level `FlowStmt::ConstraintMarker` on a Skill must \
             surface in referenced_names; got {:?}",
            signals.referenced_names
        );
    }

    #[test]
    fn fmt_signals_walks_skill_flow_context_marker() {
        let src = r#"import "./external.glyph" { repo_layout }

skill main()
    description: "Main."
    flow:
        context repo_layout
        "do work"
"#;
        let (file, _) = crate::parse::parse(src, 0).expect("parse");
        let signals = crate::analyze::fmt_signals(&file);
        assert!(
            signals.referenced_names.contains("repo_layout"),
            "flow-level `FlowStmt::ContextMarker` on a Skill must \
             surface in referenced_names; got {:?}",
            signals.referenced_names
        );
    }

    #[test]
    fn fmt_signals_walks_block_body_constraint_markers() {
        let src = r#"import "./external.glyph" { accuracy }

block helper()
    require accuracy
    flow:
        "do work"

skill main()
    flow:
        helper()
"#;
        let (file, _) = crate::parse::parse(src, 0).expect("parse");
        let signals = crate::analyze::fmt_signals(&file);
        assert!(
            signals.referenced_names.contains("accuracy"),
            "body-level `require accuracy` on a Block must surface in \
             referenced_names; got {:?}",
            signals.referenced_names
        );
    }

    #[test]
    fn fmt_signals_walks_block_body_context_nameref() {
        let src = r#"import "./external.glyph" { project_conventions }

block helper()
    context project_conventions
    flow:
        "do work"

skill main()
    flow:
        helper()
"#;
        let (file, _) = crate::parse::parse(src, 0).expect("parse");
        let signals = crate::analyze::fmt_signals(&file);
        assert!(
            signals.referenced_names.contains("project_conventions"),
            "body-level `context project_conventions` on a Block must \
             surface in referenced_names; got {:?}",
            signals.referenced_names
        );
    }

    #[test]
    fn fmt_signals_walks_block_constraints_subsection() {
        let src = r#"import "./external.glyph" { accuracy }

block helper()
    constraints:
        require accuracy
    flow:
        "do work"

skill main()
    flow:
        helper()
"#;
        let (file, _) = crate::parse::parse(src, 0).expect("parse");
        let signals = crate::analyze::fmt_signals(&file);
        assert!(
            signals.referenced_names.contains("accuracy"),
            "`constraints:` sub-section body on a Block must surface in \
             referenced_names; got {:?}",
            signals.referenced_names
        );
    }

    #[test]
    fn fmt_signals_walks_block_context_subsection() {
        let src = r#"import "./external.glyph" { repo_layout }

block helper()
    context:
        repo_layout
    flow:
        "do work"

skill main()
    flow:
        helper()
"#;
        let (file, _) = crate::parse::parse(src, 0).expect("parse");
        let signals = crate::analyze::fmt_signals(&file);
        assert!(
            signals.referenced_names.contains("repo_layout"),
            "`context:` sub-section body on a Block must surface in \
             referenced_names; got {:?}",
            signals.referenced_names
        );
    }

    // Reviewer P1.1: duplicate sub-section bodies recovered into
    // `extra_subsections` (issue #109) must surface their referenced names in
    // `fmt_signals.referenced_names`. Pre-fix, `glyph fmt`'s import-pruner ran
    // *before* the duplicate-section merge, so any import referenced ONLY
    // from a duplicate sub-section was silently dropped. Pin every variant
    // that carries name refs: `DuplicateSubsection::Constraints` and
    // `DuplicateSubsection::Context` for both Skill and Block.

    #[test]
    fn fmt_signals_walks_skill_extra_subsection_constraints() {
        let src = r#"import "./external.glyph" { stale_references }

skill main()
    description: "Main."
    constraints:
        require accuracy
    constraints:
        avoid stale_references
    flow:
        "do work"

const accuracy = "be accurate"
"#;
        let (file, _) = crate::parse::parse(src, 0).expect("parse");
        let signals = crate::analyze::fmt_signals(&file);
        assert!(
            signals.referenced_names.contains("stale_references"),
            "name ref inside a duplicate `constraints:` sub-section on a \
             Skill (recovered into `extra_subsections`) must surface in \
             referenced_names so `glyph fmt` does not drop the import; \
             got {:?}",
            signals.referenced_names
        );
    }

    #[test]
    fn fmt_signals_walks_skill_extra_subsection_context() {
        let src = r#"import "./external.glyph" { repo_layout }

skill main()
    description: "Main."
    context:
        project_conventions
    context:
        repo_layout
    flow:
        "do work"

const project_conventions = "conventions"
"#;
        let (file, _) = crate::parse::parse(src, 0).expect("parse");
        let signals = crate::analyze::fmt_signals(&file);
        assert!(
            signals.referenced_names.contains("repo_layout"),
            "name ref inside a duplicate `context:` sub-section on a Skill \
             (recovered into `extra_subsections`) must surface in \
             referenced_names; got {:?}",
            signals.referenced_names
        );
    }

    #[test]
    fn fmt_signals_walks_block_extra_subsection_constraints() {
        let src = r#"import "./external.glyph" { stale_references }

block helper()
    constraints:
        require accuracy
    constraints:
        avoid stale_references
    flow:
        "do work"

const accuracy = "be accurate"

skill main()
    flow:
        helper()
"#;
        let (file, _) = crate::parse::parse(src, 0).expect("parse");
        let signals = crate::analyze::fmt_signals(&file);
        assert!(
            signals.referenced_names.contains("stale_references"),
            "name ref inside a duplicate `constraints:` sub-section on a \
             Block (recovered into `extra_subsections`) must surface in \
             referenced_names; got {:?}",
            signals.referenced_names
        );
    }

    #[test]
    fn fmt_signals_walks_block_extra_subsection_context() {
        let src = r#"import "./external.glyph" { repo_layout }

block helper()
    context:
        project_conventions
    context:
        repo_layout
    flow:
        "do work"

const project_conventions = "conventions"

skill main()
    flow:
        helper()
"#;
        let (file, _) = crate::parse::parse(src, 0).expect("parse");
        let signals = crate::analyze::fmt_signals(&file);
        assert!(
            signals.referenced_names.contains("repo_layout"),
            "name ref inside a duplicate `context:` sub-section on a Block \
             (recovered into `extra_subsections`) must surface in \
             referenced_names; got {:?}",
            signals.referenced_names
        );
    }

    // Reviewer P1.2: parameter default-value name refs must surface in
    // `fmt_signals.referenced_names`. Per issue #165 AC: "the walk covers
    // parameter default-value name refs, flow-level call targets,
    // return-expr names, ...". `Param.default` is pre-rendered, but
    // `Param.default_is_name_ref = true` indicates the `default` string is
    // a bare-name reference that must resolve to an in-scope `const` at
    // compile time — so import-pruning must keep that import alive.

    #[test]
    fn fmt_signals_walks_skill_param_default_nameref() {
        let src = r#"import "./external.glyph" { default_scope }

skill main(scope = default_scope)
    description: "Main."
    flow:
        "do work"
"#;
        let (file, _) = crate::parse::parse(src, 0).expect("parse");
        let signals = crate::analyze::fmt_signals(&file);
        assert!(
            signals.referenced_names.contains("default_scope"),
            "Skill parameter default-value name ref must surface in \
             referenced_names so `glyph fmt` does not drop the import; \
             got {:?}",
            signals.referenced_names
        );
    }

    #[test]
    fn fmt_signals_walks_block_param_default_nameref() {
        let src = r#"import "./external.glyph" { default_scope }

block helper(scope = default_scope)
    flow:
        "do work"

skill main()
    flow:
        helper()
"#;
        let (file, _) = crate::parse::parse(src, 0).expect("parse");
        let signals = crate::analyze::fmt_signals(&file);
        assert!(
            signals.referenced_names.contains("default_scope"),
            "Block parameter default-value name ref must surface in \
             referenced_names; got {:?}",
            signals.referenced_names
        );
    }

    #[test]
    fn analyze_annotates_branch_with_condition_classification() {
        let src = r#"
const big = "a big change"

skill foo()
    description: "test"
    flow:
        if big:
            "stop"
"#;
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let file =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

        let skill = match &file.decls[1] {
            crate::ast::Decl::Skill(s) => &s.node,
            _ => panic!("expected skill"),
        };
        let branch = match &skill.flow[0] {
            crate::ast::FlowStmt::Branch {
                condition_classification,
                ..
            } => condition_classification,
            _ => panic!("expected branch"),
        };
        let c = branch.as_ref().expect("classification should be populated");
        assert!(c.is_pure_predicate());
        let kinds: Vec<_> = c.tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![crate::condition::ConditionTokenKind::PredicateConst]
        );
    }

    /// Task 6 — Decl::Block flow walking: `annotate_file_branches` must visit
    /// branches inside private `block` declarations and populate their
    /// `condition_classification`. Pre-fix the walker only covered
    /// `Decl::Skill`, so the IR JSON's `predicate_shape` for a block-flow
    /// branch was the all-false default.
    #[test]
    fn block_flow_predicate_classified_via_predicate_shape() {
        let src = r#"
const big_change = "the change is big"

block helper()
    description: "A helper block."
    flow:
        if big_change
            "Significant work."
        else
            "Minor work."

skill main()
    description: "Test."
    flow:
        helper()
"#;
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let file =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

        let helper = file
            .decls
            .iter()
            .find_map(|d| match d {
                crate::ast::Decl::Block(b) if b.node.name == "helper" => Some(&b.node),
                _ => None,
            })
            .expect("helper block must be present in AST");
        let classification = match &helper.flow[0] {
            crate::ast::FlowStmt::Branch {
                condition_classification,
                ..
            } => condition_classification.as_ref(),
            other => panic!("expected branch, got {:?}", other),
        };
        let c = classification
            .expect("block-flow branch must have condition_classification populated by analyze");
        assert!(
            c.has_predicate_token,
            "big_change is a String const → PredicateConst token expected"
        );
        assert!(!c.has_boolean_token);
        assert!(!c.has_compositional_operator);
    }

    #[test]
    fn int_const_in_condition_position_fires_non_boolean_non_predicate() {
        let src = r#"
const max = 3

skill foo()
    description: "test"
    flow:
        if max:
            "stop"
"#;
        let ids = check_ids(src);
        assert!(
            ids.iter()
                .any(|s| s == "G::analyze::condition-non-boolean-non-predicate"),
            "got: {:?}",
            ids
        );
    }

    #[test]
    fn float_literal_in_condition_position_fires_non_boolean_non_predicate() {
        let src = r#"
skill foo()
    description: "test"
    flow:
        if 3.14:
            "stop"
"#;
        let ids = check_ids(src);
        assert!(
            ids.iter()
                .any(|s| s == "G::analyze::condition-non-boolean-non-predicate"),
            "got: {:?}",
            ids
        );
    }

    #[test]
    fn string_const_in_condition_position_does_not_fire_non_boolean_non_predicate() {
        let src = r#"
const big = "a big change"

skill foo()
    description: "test"
    flow:
        if big:
            "stop"
"#;
        let ids = check_ids(src);
        assert!(
            !ids.iter()
                .any(|s| s == "G::analyze::condition-non-boolean-non-predicate"),
            "string const should be a valid predicate, got: {:?}",
            ids
        );
    }

    #[test]
    fn duplicate_type_decl_emits_diagnostic() {
        let src = r#"export type Foo = <"first">
export type Foo = <"second">
"#;
        let ids = check_ids(src);
        assert!(
            ids.iter().any(|id| id == "G::analyze::duplicate-type-decl"),
            "expected duplicate-type-decl; got: {:?}",
            ids
        );
    }

    /// Codex finding #3: the §D6 case+underscore-insensitive identifier rule
    /// applies to type names too. Two type decls whose names canonicalize to
    /// the same key (`RepoContext` vs `Repocontext`) should trigger
    /// `G::analyze::duplicate-type-decl`, since downstream lookups
    /// (TypeRegistry::get) treat them as the same key. Both spellings must
    /// be PascalCase under the type-namespace case rule; cross-case mixing
    /// is now caught earlier by `type-case-violation`.
    #[test]
    fn duplicate_type_decl_canonical_form_collision() {
        let src = r#"type RepoContext = <"first">
type Repocontext = <"second">
"#;
        let ids = check_ids(src);
        assert!(
            ids.iter().any(|id| id == "G::analyze::duplicate-type-decl"),
            "canonical-equal duplicate type decls should collide; got: {:?}",
            ids
        );
    }

    /// Canonical pairing: `type Foo = <"…">` + `-> Foo` annotation is **not**
    /// a collision (both refer to the same nominal type).
    #[test]
    fn canonical_type_decl_with_return_annotation_is_not_a_collision() {
        let src = r#"type Foo = <"a domain type">
skill returns_foo() -> Foo
    description: "test"
    flow:
        return "value"
"#;
        let bag = crate::check_source(src, 0, "test.glyph");
        let collisions: Vec<&str> = bag
            .iter()
            .filter(|d| d.id == "G::analyze::name-collision")
            .map(|d| d.message.as_str())
            .collect();
        assert!(
            collisions.is_empty(),
            "canonical type-decl + `-> Foo` must not fire name-collision, got: {:?}",
            collisions
        );
    }

    /// `type Foo` collides with a selectively-imported type `Foo` (no `as`
    /// alias). Under Task 8 the slimmed `sweep_type_decl_name_collisions`
    /// fires only against type-kinded selective imports (proxied by
    /// PascalCase aliases until Task 9 plumbs `ResolvedImportKind`).
    #[test]
    fn type_decl_collides_with_selective_import() {
        let src = r#"import "./other.glyph" { Foo }
type Foo = <"a domain type">
"#;
        let bag = crate::check_source(src, 0, "test.glyph");
        let collisions: Vec<&str> = bag
            .iter()
            .filter(|d| d.id == "G::analyze::name-collision")
            .map(|d| d.message.as_str())
            .collect();
        assert!(
            collisions
                .iter()
                .any(|m| m.contains("type `Foo`") && m.contains("type import")),
            "expected type-vs-type-import collision, got messages: {:?}",
            collisions
        );
    }

    /// `type Foo` collides with `import { bar as Foo }` (selective + alias).
    /// The PascalCase alias `Foo` is treated as a type-kinded import.
    #[test]
    fn type_decl_collides_with_aliased_selective_import() {
        let src = r#"import "./other.glyph" { bar as Foo }
type Foo = <"a domain type">
"#;
        let bag = crate::check_source(src, 0, "test.glyph");
        let collisions: Vec<&str> = bag
            .iter()
            .filter(|d| d.id == "G::analyze::name-collision")
            .map(|d| d.message.as_str())
            .collect();
        assert!(
            collisions
                .iter()
                .any(|m| m.contains("type `Foo`") && m.contains("type import")),
            "expected type-vs-type-import collision, got messages: {:?}",
            collisions
        );
    }

    // Task 8 — `type_decl_collides_with_whole_module_import` was deleted: a
    // whole-module `import "..." as Foo` binds the module to the value
    // namespace, so it no longer collides with a `type Foo` decl under the
    // two-namespace split. (The alias would also fail the value-namespace
    // case rule, but that is a separate `value-case-violation` path.)

    #[test]
    fn empty_goal_section_fires_cardinality_violation() {
        // A `goal:` (catalogue cardinality=one) with zero items must fail
        // analyze with `G::analyze::cardinality-violation`.
        let src = "\
skill demo()
    description: \"demo\"
    goal:
    flow:
        \"go\"
";
        let ids = check_ids(src);
        assert!(
            ids.iter()
                .any(|id| id == "G::analyze::cardinality-violation"),
            "expected G::analyze::cardinality-violation for empty `goal:`, got {ids:?}"
        );
    }

    #[test]
    fn duplicate_freeform_section_fires_diag() {
        // Two `quality:` sections in one body must emit
        // `G::analyze::duplicate-section` (Error tier).
        let src = "\
skill demo()
    description: \"demo\"
    quality:
        \"first\"
    quality:
        \"second\"
    flow:
        \"go\"
";
        let ids = check_ids(src);
        assert!(
            ids.iter().any(|id| id == "G::analyze::duplicate-section"),
            "expected G::analyze::duplicate-section for duplicate freeform `quality:`, got {ids:?}"
        );
    }

    #[test]
    fn duplicate_catalogued_goal_section_fires_diag() {
        // `goal:` is a catalogued section parsed through the freeform path;
        // two occurrences must emit `G::analyze::duplicate-section` (Error).
        let src = "\
skill demo()
    description: \"demo\"
    goal: \"first\"
    goal: \"second\"
    flow:
        \"go\"
";
        let ids = check_ids(src);
        assert!(
            ids.iter().any(|id| id == "G::analyze::duplicate-section"),
            "expected G::analyze::duplicate-section for duplicate `goal:`, got {ids:?}"
        );
    }

    // Finding 2 regressions: catalogue lookup and duplicate detection are
    // case-insensitive — `Goal:` must still be recognized as the catalogue
    // entry (so `cardinality = "one"` is enforced) and `goal:` + `Goal:` in
    // the same body must collide.

    #[test]
    fn uppercase_goal_section_with_multiple_items_fires_cardinality_violation() {
        // `Goal:` must match the catalogue entry case-insensitively; with two
        // items it violates cardinality = "one".
        let src = "\
skill demo()
    description: \"demo\"
    Goal:
        \"first\"
        \"second\"
    flow:
        \"go\"
";
        let ids = check_ids(src);
        assert!(
            ids.iter()
                .any(|id| id == "G::analyze::cardinality-violation"),
            "expected G::analyze::cardinality-violation for `Goal:` with two items, got {ids:?}"
        );
    }

    #[test]
    fn mixed_case_goal_sections_collide_as_duplicate() {
        // `goal:` and `Goal:` differ only in case and must collide as
        // duplicates of the same catalogue section.
        let src = "\
skill demo()
    description: \"demo\"
    goal: \"first\"
    Goal: \"second\"
    flow:
        \"go\"
";
        let ids = check_ids(src);
        assert!(
            ids.iter().any(|id| id == "G::analyze::duplicate-section"),
            "expected G::analyze::duplicate-section for mixed-case duplicate `goal:`/`Goal:`, got {ids:?}"
        );
    }
}

#[cfg(test)]
mod unmerged_duplicate_subsection_tests {
    //! Issue #109 chunk 3 — Analyze invariant.
    //!
    //! After Chunk 2, the parser recovers a duplicate sub-section into the
    //! declaration's `extra_subsections` and emits the *parse-tier* repairable
    //! `G::parse::duplicate-subsection`. `glyph fmt` is then expected to merge
    //! the extras back into the singleton field. If `fmt` is skipped (or fed
    //! an unrepaired AST programmatically), Lower would receive a node whose
    //! "extras" channel still carries semantic content — a silent contract
    //! violation.
    //!
    //! Analyze closes that hole: it walks every `Skill` / `BlockDecl` /
    //! `ExportBlockDecl` and, if any has a non-empty `extra_subsections`,
    //! emits `G::analyze::unmerged-duplicate-subsection` at error tier. The
    //! pipeline-level `bag.has_error()` gate (lib.rs:110) then prevents Lower
    //! from being called.
    use super::*;
    use crate::ast::{Decl, DuplicateSubsection, FlowStmt, Skill, SourceFile};
    use crate::diagnostic::{Classification, DiagBag};
    use crate::span::{LineIndex, Span, Spanned};

    /// Build a minimal `Skill` AST node with a configurable `extra_subsections`
    /// field. All other fields are filled with empty/default values matching
    /// what `parse_skill` would produce for an empty body.
    fn skill_with_extras(extras: Vec<DuplicateSubsection>) -> Spanned<Skill> {
        Spanned {
            node: Skill {
                name: "the_skill".to_string(),
                params: Vec::new(),
                description: Some("present".to_string()),
                flow: vec![FlowStmt::InlineString("do work".to_string())],
                flow_present: true,
                body_constraints: Vec::new(),
                body_context: Vec::new(),
                body_bare_names: Vec::new(),
                effects: Vec::new(),
                context_section: Vec::new(),
                constraints_section: Vec::new(),
                return_type: None,
                extra_subsections: extras,
                description_span: None,
                context_section_span: None,
                constraints_section_span: None,
                effects_span: None,
                flow_span: None,
                freeform_sections: Vec::new(),
            },
            span: Span::new(0, 0, 10),
        }
    }

    fn run_analyze(file: SourceFile) -> DiagBag {
        let source = "dummy";
        let li = LineIndex::new(source);
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        analyze_with_diagnostics(file, 0, "test.glyph", &li, &mut bag, &mut registry);
        bag
    }

    /// Test (a): an AST whose `Skill` carries a non-empty `extra_subsections`
    /// must fail Analyze with `G::analyze::unmerged-duplicate-subsection` at
    /// `Classification::Error`.
    #[test]
    fn skill_with_unmerged_extras_emits_error_diagnostic() {
        let skill = skill_with_extras(vec![DuplicateSubsection::Description(
            "second body never merged by fmt".to_string(),
        )]);
        let file = SourceFile {
            decls: vec![Decl::Skill(skill)],
        };

        let bag = run_analyze(file);

        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::unmerged-duplicate-subsection")
            .unwrap_or_else(|| {
                let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
                panic!(
                    "expected `G::analyze::unmerged-duplicate-subsection`, got: {:?}",
                    ids
                )
            });
        assert_eq!(
            diag.classification,
            Classification::Error,
            "unmerged-duplicate-subsection must be Error tier"
        );
    }

    /// Test (c): end-to-end through the real parse→analyze pipeline. A
    /// source containing two `constraints:` sub-sections under one skill
    /// must produce BOTH the parse-tier repairable
    /// `G::parse::duplicate-subsection` AND the analyze-tier error
    /// `G::analyze::unmerged-duplicate-subsection` in the same diagnostic
    /// bag. This pins the contract that the two diagnostics co-exist (they
    /// fire from different phases targeting different consumers — agent
    /// repair loop vs. lower-side invariant).
    #[test]
    fn pipeline_two_constraints_emits_both_parse_and_analyze_diagnostics() {
        let src = "\
skill the_skill()
    constraints:
        require accuracy
    constraints:
        avoid stale_references
    flow:
        \"do work\"
";
        let bag = crate::check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();

        assert!(
            ids.contains(&"G::parse::duplicate-subsection"),
            "expected parse-tier `G::parse::duplicate-subsection`, got {:?}",
            ids
        );
        assert!(
            ids.contains(&"G::analyze::unmerged-duplicate-subsection"),
            "expected analyze-tier `G::analyze::unmerged-duplicate-subsection`, \
             got {:?}",
            ids
        );

        let analyze_diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::unmerged-duplicate-subsection")
            .unwrap();
        assert_eq!(
            analyze_diag.classification,
            Classification::Error,
            "analyze-tier diagnostic must be Error (the only fix path is fmt; \
             only parse-tier carries Repairable)"
        );
    }

    /// Issue #109 codex pass-2 finding 5 — end-to-end through parse→analyze
    /// for a `block` declaration. A source containing two `description:`
    /// sub-sections under one `block` must produce BOTH the parse-tier
    /// repairable `G::parse::duplicate-subsection` AND the analyze-tier
    /// error `G::analyze::unmerged-duplicate-subsection` in the same bag,
    /// proving the parser→analyze hand-off works for block declarations
    /// (not just skills).
    #[test]
    fn pipeline_block_two_descriptions_emits_both_parse_and_analyze_diagnostics() {
        let src = "\
block foo()
    description: \"First.\"
    description: \"Second.\"
    flow:
        \"Do something.\"
";
        let bag = crate::check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();

        assert!(
            ids.contains(&"G::parse::duplicate-subsection"),
            "expected parse-tier `G::parse::duplicate-subsection`, got {:?}",
            ids
        );
        assert!(
            ids.contains(&"G::analyze::unmerged-duplicate-subsection"),
            "expected analyze-tier `G::analyze::unmerged-duplicate-subsection`, \
             got {:?}",
            ids
        );
        let analyze_diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::unmerged-duplicate-subsection")
            .unwrap();
        assert_eq!(analyze_diag.classification, Classification::Error);
    }

    /// Issue #109 codex pass-2 finding 4 — end-to-end through parse→analyze
    /// for an `export block` declaration.
    #[test]
    fn pipeline_export_block_two_descriptions_emits_both_parse_and_analyze_diagnostics() {
        let src = "\
export block foo() -> Report
    description: \"First.\"
    description: \"Second.\"
    flow:
        \"Do something.\"
        return <result>
";
        let bag = crate::check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();

        assert!(
            ids.contains(&"G::parse::duplicate-subsection"),
            "expected parse-tier `G::parse::duplicate-subsection`, got {:?}",
            ids
        );
        assert!(
            ids.contains(&"G::analyze::unmerged-duplicate-subsection"),
            "expected analyze-tier `G::analyze::unmerged-duplicate-subsection`, \
             got {:?}",
            ids
        );
        let analyze_diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::unmerged-duplicate-subsection")
            .unwrap();
        assert_eq!(analyze_diag.classification, Classification::Error);
    }

    /// Test (b): a clean AST (every declaration's `extra_subsections` is
    /// empty) must NOT emit the invariant diagnostic. Other unrelated
    /// diagnostics may still fire — we only assert that
    /// `G::analyze::unmerged-duplicate-subsection` is absent.
    #[test]
    fn clean_ast_emits_no_unmerged_diagnostic() {
        let skill = skill_with_extras(Vec::new());
        let file = SourceFile {
            decls: vec![Decl::Skill(skill)],
        };

        let bag = run_analyze(file);

        let dups: Vec<&str> = bag
            .iter()
            .map(|d| d.id.as_str())
            .filter(|id| *id == "G::analyze::unmerged-duplicate-subsection")
            .collect();
        assert!(
            dups.is_empty(),
            "clean AST must not emit unmerged-duplicate-subsection; got {:?}",
            dups
        );
    }
}

// PRD #103 / Slice 1 (#104): pure-validator unit tests for
// `validate_call_args`. Table-driven over (params × args) per the
// acceptance criteria — exercises the validator in isolation, not
// the wiring into the analyze pipeline.

#[cfg(test)]
mod validate_call_tests {
    use super::*;

    fn p(name: &str, default: Option<&str>) -> ast::Param {
        ast::Param {
            name: name.to_string(),
            default: default.map(|s| s.to_string()),
            default_is_name_ref: false,
            type_annotation: None,
            description: None,
            span: Span::new(0, 0, 1),
        }
    }

    #[test]
    fn validate_call_args_emits_diagnostic_for_missing_required() {
        let li = LineIndex::new("");
        let params = vec![p("x", None)];
        let diags = validate_call_args("bar", &params, &[], Span::new(0, 0, 1), "test.glyph", &li);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].id, "G::analyze::missing-required-arg");
        assert_eq!(diags[0].classification, Classification::Error);
        assert!(
            diags[0].message.contains("`x`") && diags[0].message.contains("`bar"),
            "message should name param `x` and callee `bar`, got {:?}",
            diags[0].message
        );
    }

    #[test]
    fn validate_call_args_required_satisfied_positionally() {
        let li = LineIndex::new("");
        let params = vec![p("x", None)];
        let diags = validate_call_args(
            "bar",
            &params,
            &["v1".to_string()],
            Span::new(0, 0, 1),
            "test.glyph",
            &li,
        );
        assert!(diags.is_empty(), "expected no diagnostics, got {:?}", diags);
    }

    #[test]
    fn validate_call_args_all_defaulted_no_diagnostic() {
        let li = LineIndex::new("");
        let params = vec![p("a", Some("\"x\"")), p("b", Some("\"y\""))];
        let diags = validate_call_args(
            "callee",
            &params,
            &[],
            Span::new(0, 0, 1),
            "test.glyph",
            &li,
        );
        assert!(diags.is_empty(), "expected no diagnostics, got {:?}", diags);
    }

    // Positional binding edge cases over `callee(a, b = "d", c)`.
    fn mixed_params() -> Vec<ast::Param> {
        vec![p("a", None), p("b", Some("\"d\"")), p("c", None)]
    }

    fn missing_arg_names(diags: &[Diagnostic]) -> Vec<String> {
        diags
            .iter()
            .filter(|d| d.id == "G::analyze::missing-required-arg")
            .map(|d| d.message.clone())
            .collect()
    }

    #[test]
    fn validate_call_args_mixed_no_args_reports_a_and_c() {
        let li = LineIndex::new("");
        let diags = validate_call_args(
            "callee",
            &mixed_params(),
            &[],
            Span::new(0, 0, 1),
            "test.glyph",
            &li,
        );
        let msgs = missing_arg_names(&diags);
        assert_eq!(msgs.len(), 2, "expected 2 diagnostics, got {:?}", msgs);
        assert!(
            msgs.iter().any(|m| m.contains("`a`")),
            "missing `a`: {:?}",
            msgs
        );
        assert!(
            msgs.iter().any(|m| m.contains("`c`")),
            "missing `c`: {:?}",
            msgs
        );
    }

    #[test]
    fn validate_call_args_mixed_one_arg_reports_only_c() {
        let li = LineIndex::new("");
        let diags = validate_call_args(
            "callee",
            &mixed_params(),
            &["v1".to_string()],
            Span::new(0, 0, 1),
            "test.glyph",
            &li,
        );
        let msgs = missing_arg_names(&diags);
        assert_eq!(msgs.len(), 1, "expected 1 diagnostic, got {:?}", msgs);
        assert!(
            msgs[0].contains("`c`"),
            "expected missing `c`, got {:?}",
            msgs[0]
        );
    }

    #[test]
    fn validate_call_args_mixed_two_args_satisfies_b_via_position_still_reports_c() {
        // Positional binding: arg index 1 binds to param `b` (which has a
        // default) — the default is overridden, but `c` (index 2) is still
        // missing. Pins the rule that defaulted params consume positional
        // slots like ordinary params.
        let li = LineIndex::new("");
        let diags = validate_call_args(
            "callee",
            &mixed_params(),
            &["v1".to_string(), "v2".to_string()],
            Span::new(0, 0, 1),
            "test.glyph",
            &li,
        );
        let msgs = missing_arg_names(&diags);
        assert_eq!(msgs.len(), 1, "expected 1 diagnostic, got {:?}", msgs);
        assert!(
            msgs[0].contains("`c`"),
            "expected missing `c`, got {:?}",
            msgs[0]
        );
    }
}

// `mod classify_condition_tests` retired in Task 6: the tests targeted the
// local `analyze::classify_condition`, which was deleted when the single
// classification authority moved to `crate::condition::classify_condition`.
// Equivalent coverage now lives in `crates/glyph-core/src/condition.rs`
// (`classify_pure_predicates_pass`, `classify_string_const_is_predicate_const`,
// etc.).

#[cfg(test)]
mod param_default_name_ref_tests {
    //! Codex finding #1 follow-up: every name_ref param default must resolve
    //! to an in-scope `const`. These tests exercise both the rejection paths
    //! (block / parameter / unknown identifier) and the resolution paths
    //! (same-file const, literal default — which must NOT trigger the sweep).

    fn diag_ids(src: &str) -> Vec<String> {
        crate::check_source(src, 0, "test.glyph")
            .iter()
            .map(|d| d.id.clone())
            .collect()
    }

    fn diag_messages(src: &str) -> Vec<String> {
        crate::check_source(src, 0, "test.glyph")
            .iter()
            .filter(|d| d.id == "G::analyze::undefined-name")
            .map(|d| d.message.clone())
            .collect()
    }

    #[test]
    fn name_ref_default_resolves_to_same_file_const_passes() {
        // Baseline: a name_ref default that names an in-scope `const` is
        // accepted — the sweep is a hard error so a false positive here would
        // surface as a reported diagnostic.
        let src = "\
const default_risk = \"low\"
skill demo(risk = default_risk)
    flow:
        \"do work\"
";
        let ids = diag_ids(src);
        assert!(
            !ids.iter().any(|id| id == "G::analyze::undefined-name"),
            "expected no undefined-name diagnostic, got {ids:?}"
        );
    }

    #[test]
    fn literal_string_default_is_not_a_name_ref_and_passes() {
        // Sanity guard: literal defaults flow through with
        // `default_is_name_ref = false`; the sweep must skip them so
        // `risk = \"low\"` does not trigger an `undefined-name` lookup
        // for the bare token `\"low\"`.
        let src = "\
skill demo(risk = \"low\")
    flow:
        \"do work\"
";
        let ids = diag_ids(src);
        assert!(
            !ids.iter().any(|id| id == "G::analyze::undefined-name"),
            "literal-default must not trigger name-resolution: {ids:?}"
        );
    }

    #[test]
    fn bool_literal_default_is_not_a_name_ref_and_passes() {
        let src = "\
skill demo(flag = true)
    flow:
        \"do work\"
";
        let ids = diag_ids(src);
        assert!(
            !ids.iter().any(|id| id == "G::analyze::undefined-name"),
            "bool literal must not trigger name-resolution: {ids:?}"
        );
    }

    #[test]
    fn name_ref_default_unknown_identifier_emits_undefined_name() {
        // `default_risk` is not declared anywhere in this file → the sweep
        // emits `G::analyze::undefined-name` so the bare identifier never
        // leaks into the IR / `## Parameters` output.
        let src = "\
skill demo(risk = default_risk)
    flow:
        \"do work\"
";
        let msgs = diag_messages(src);
        assert!(
            msgs.iter().any(|m| m.contains("`default_risk`")),
            "expected diagnostic naming `default_risk`, got {msgs:?}"
        );
    }

    #[test]
    fn name_ref_default_pointing_at_block_is_rejected() {
        // A `block` is in the universal value namespace but is not a
        // `const` value-binding, so it cannot satisfy a name_ref default.
        let src = "\
block helper()
    \"step\"

skill demo(risk = helper)
    flow:
        \"do work\"
";
        let msgs = diag_messages(src);
        assert!(
            msgs.iter().any(|m| m.contains("`helper`")),
            "expected diagnostic naming `helper`, got {msgs:?}"
        );
    }

    #[test]
    fn name_ref_default_pointing_at_sibling_param_is_rejected() {
        // Sibling parameters are not value-bindings either; the resolver
        // only accepts `const` declarations.
        let src = "\
skill demo(other = \"x\", risk = other)
    flow:
        \"do work\"
";
        let msgs = diag_messages(src);
        assert!(
            msgs.iter().any(|m| m.contains("`other`")),
            "expected diagnostic naming `other`, got {msgs:?}"
        );
    }

    #[test]
    fn block_param_with_unknown_name_ref_default_is_rejected() {
        // The sweep walks Skill, Block, and ExportBlock decls — pin the
        // Block arm so a regression in the iteration logic is caught.
        let src = "\
block helper(x = unknown_const)
    \"step\"
";
        let msgs = diag_messages(src);
        assert!(
            msgs.iter().any(|m| m.contains("`unknown_const`")),
            "expected diagnostic naming `unknown_const`, got {msgs:?}"
        );
    }

    #[test]
    fn export_block_param_with_unknown_name_ref_default_is_rejected() {
        let src = "\
export block helper(x = unknown_const)
    \"step\"
";
        let msgs = diag_messages(src);
        assert!(
            msgs.iter().any(|m| m.contains("`unknown_const`")),
            "expected diagnostic naming `unknown_const`, got {msgs:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Flow-position assignments — analyze tests (Phase 2 of the
// flow-position-assignments feature; see `.flow-assign-spec.md`).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod flow_assign_tests {
    use super::{analyze_with_imports, DiagBag};
    use crate::diagnostic::Classification;
    use crate::span::{Span, Spanned};
    use std::collections::{HashMap, HashSet};

    fn diag_ids(src: &str) -> Vec<String> {
        crate::check_source(src, 0, "test.glyph")
            .iter()
            .map(|d| d.id.clone())
            .collect()
    }

    /// Test 1: collision with parameter → `redeclared-flow-binding`.
    #[test]
    fn flow_assign_redecl_param_emits_diag() {
        let src = "\
import \"@glyph/std\" { subagent }

block inspect_repo(scope = \".\") -> RepoContext
    \"inspect\"

skill demo(ctx = \".\")
    description: \"demo\"
    flow:
        ctx = inspect_repo(ctx)
        return ctx
";
        let ids = diag_ids(src);
        assert!(
            ids.iter()
                .any(|id| id == "G::analyze::redeclared-flow-binding"),
            "expected redeclared-flow-binding, got {ids:?}"
        );
    }

    /// Test 2: RHS callee is declared in this file but its header
    /// declares no return type → `assignment-rhs-has-no-value`.
    ///
    /// Codex M5: this diagnostic only fires when the callee resolves
    /// (so the legacy resolver will *not* also fire `undefined-call` /
    /// `stdlib-missing-import`); see
    /// `flow_assign_unknown_callee_emits_only_undefined_call` for the
    /// dedupe contract on truly-unknown callees.
    #[test]
    fn flow_assign_no_value_diag() {
        let src = "\
block helper()
    \"do something\"

skill demo()
    description: \"demo\"
    flow:
        x = helper()
";
        let ids = diag_ids(src);
        assert!(
            ids.iter()
                .any(|id| id == "G::analyze::assignment-rhs-has-no-value"),
            "expected assignment-rhs-has-no-value, got {ids:?}"
        );
    }

    /// Test 3: flow-assign inside a block flow → `flow-assign-in-block-unsupported`.
    #[test]
    fn flow_assign_in_block_diag() {
        let src = "\
block inspect_repo(scope = \".\") -> RepoContext
    \"inspect\"

block helper()
    flow:
        x = inspect_repo(\".\")

skill caller()
    description: \"caller\"
    flow:
        helper()
";
        let ids = diag_ids(src);
        assert!(
            ids.iter()
                .any(|id| id == "G::analyze::flow-assign-in-block-unsupported"),
            "expected flow-assign-in-block-unsupported, got {ids:?}"
        );
    }

    /// Test 4: bound name returned, types don't match → existing
    /// nominal-mismatch / return-type-mismatch diag.
    #[test]
    fn flow_assign_return_type_mismatch() {
        let src = "\
block inspect_repo(scope = \".\") -> RepoContext
    \"inspect\"

skill demo() -> Risk
    description: \"demo\"
    flow:
        ctx = inspect_repo(\".\")
        return ctx
";
        let ids = diag_ids(src);
        // Existing mismatch diag (the analyze nominal matcher uses
        // `G::analyze::nominal-mismatch`).
        assert!(
            ids.iter().any(|id| id == "G::analyze::nominal-mismatch"),
            "expected G::analyze::nominal-mismatch, got {ids:?}"
        );
    }

    /// Test 5: branch condition uses flow-local type — bound via subagent
    /// (agent-shape) — must classify cleanly with no errors. The condition
    /// uses `researcher.applies()`, which is what the live FlowScope-aware
    /// classifier needs to recognize as a predicate against the
    /// agent-typed flow-local binding.
    #[test]
    fn flow_assign_branch_uses_flow_local_type() {
        let src = "\
import \"@glyph/std\" { subagent }

const note = \"step\"

skill demo()
    description: \"demo\"
    flow:
        researcher = subagent(\".\") with \"investigate this area\"
        if researcher.applies():
            require note
";
        let bag = crate::check_source(src, 0, "test.glyph");
        let errors: Vec<_> = bag
            .iter()
            .filter(|d| matches!(d.classification, Classification::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "expected no Error diagnostics, got {errors:?}"
        );
    }

    /// Test 6: use-before-bind — `{ctx}` slot before `ctx = ...` in
    /// the same flow body. The binding exists later in the same skill
    /// but is not in scope at the slot's position.
    #[test]
    fn flow_assign_use_before_bind_specialized() {
        let src = "\
block inspect_repo(scope = \".\") -> RepoContext
    \"inspect\"

skill demo() -> RepoContext
    description: \"demo\"
    flow:
        \"Use {ctx}\"
        ctx = inspect_repo(\".\")
        return ctx
";
        let ids = diag_ids(src);
        assert!(
            ids.iter().any(|id| id == "G::analyze::use-before-bind"),
            "expected G::analyze::use-before-bind, got {ids:?}"
        );
    }

    /// Test 7: truly-unknown name → existing `unknown-param-slot`, NOT
    /// `use-before-bind`.
    #[test]
    fn flow_assign_truly_unknown_uses_existing_diag() {
        let src = "\
skill demo()
    description: \"demo\"
    flow:
        \"Use {never_bound}\"
";
        let ids = diag_ids(src);
        assert!(
            ids.iter().any(|id| id == "G::analyze::unknown-param-slot"),
            "expected G::analyze::unknown-param-slot, got {ids:?}"
        );
        assert!(
            !ids.iter().any(|id| id == "G::analyze::use-before-bind"),
            "did not expect G::analyze::use-before-bind, got {ids:?}"
        );
    }

    /// Codex H3: passing a flow-local binding as a positional call
    /// argument whose recorded type does not nominal-match the
    /// callee's `:Type` annotation must fire
    /// `G::analyze::call-arg-type-mismatch` at the call site.
    #[test]
    fn flow_assign_call_arg_type_mismatch_emits_diag() {
        let src = "\
block produce(scope = \".\") -> RepoContext
    \"produce\"

block consume(input: Risk = \"x\") -> Plan
    \"consume\"

skill demo() -> Plan
    description: \"demo\"
    flow:
        ctx = produce(\".\")
        plan = consume(ctx)
        return plan
";
        let ids = diag_ids(src);
        assert!(
            ids.iter()
                .any(|id| id == "G::analyze::call-arg-type-mismatch"),
            "expected G::analyze::call-arg-type-mismatch, got {ids:?}"
        );
    }

    /// Codex M5: `x = unknown()` where `unknown` is not a declared
    /// block must fire ONLY `G::analyze::undefined-call`. The
    /// `assignment-rhs-has-no-value` diagnostic must be suppressed so
    /// the user sees one root cause, not two.
    #[test]
    fn flow_assign_unknown_callee_emits_only_undefined_call() {
        let src = "\
skill demo()
    description: \"demo\"
    flow:
        x = unknown_block(\".\")
";
        let ids = diag_ids(src);
        assert!(
            ids.iter().any(|id| id == "G::analyze::undefined-call"),
            "expected G::analyze::undefined-call, got {ids:?}"
        );
        assert!(
            !ids.iter()
                .any(|id| id == "G::analyze::assignment-rhs-has-no-value"),
            "did not expect G::analyze::assignment-rhs-has-no-value (Codex M5), got {ids:?}"
        );
    }

    /// Codex H3 negative: when types nominally match, no
    /// `call-arg-type-mismatch` fires.
    #[test]
    fn flow_assign_call_arg_type_match_no_diag() {
        let src = "\
    block produce(scope = \".\") -> RepoContext
        \"produce\"
    
    block consume(input: RepoContext = \"x\") -> Plan
        \"consume\"
    
    skill demo() -> Plan
        description: \"demo\"
        flow:
            ctx = produce(\".\")
            plan = consume(ctx)
            return plan
    ";
        let ids = diag_ids(src);
        assert!(
            !ids.iter()
                .any(|id| id == "G::analyze::call-arg-type-mismatch"),
            "did not expect G::analyze::call-arg-type-mismatch, got {ids:?}"
        );
    }

    // ---- PRD #159 / Codex round-1 Issue 1:
    //      G::analyze::return-of-no-value-call ----

    /// Positive case: `return <call>` where the callee resolves to a
    /// same-file block but the callee declares no `-> Type`. The new
    /// Error-tier diagnostic must fire.
    #[test]
    fn return_of_no_value_call_fires_for_void_local_callee() {
        let src = concat!(
            "block helper()\n",
            "    \"do something\"\n",
            "\n",
            "skill demo() -> Plan\n",
            "    description: \"demo\"\n",
            "    flow:\n",
            "        return helper()\n",
        );
        let bag = crate::check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
                ids.contains(&"G::analyze::return-of-no-value-call"),
                "expected G::analyze::return-of-no-value-call for `return helper()` against void callee, got: {ids:?}"
            );
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::return-of-no-value-call")
            .unwrap();
        assert_eq!(
            diag.classification,
            Classification::Error,
            "return-of-no-value-call must be Error tier"
        );
        assert!(
            diag.message.contains("helper"),
            "diagnostic must mention the callee name `helper`, got: {:?}",
            diag.message
        );
    }

    /// Suppression: `return <call>` where the callee does not resolve to
    /// any declared block. The new diagnostic must be SUPPRESSED so
    /// `undefined-call` surfaces alone (one root cause).
    ///
    /// Mirrors `flow_assign_unknown_callee_emits_only_undefined_call` —
    /// the analyze.rs:1597-1612 M5 suppression rule extended to
    /// return position.
    #[test]
    fn return_of_no_value_call_suppressed_when_callee_undefined() {
        let src = concat!(
            "skill demo() -> Plan\n",
            "    description: \"demo\"\n",
            "    flow:\n",
            "        return missing_block()\n",
        );
        let ids = diag_ids(src);
        assert!(
            ids.iter().any(|id| id == "G::analyze::undefined-call"),
            "expected G::analyze::undefined-call to fire alone, got {ids:?}"
        );
        assert!(
                !ids.iter()
                    .any(|id| id == "G::analyze::return-of-no-value-call"),
                "must NOT fire return-of-no-value-call when callee is undefined (M5 suppression), got {ids:?}"
            );
    }

    /// Negative: typed callee (`-> Type`) must NOT fire the new
    /// diagnostic. Confirms the resolve-then-check path returns early
    /// when the callee declares a return type.
    #[test]
    fn return_of_no_value_call_does_not_fire_for_typed_callee() {
        let src = concat!(
            "block helper() -> Plan\n",
            "    \"produce\"\n",
            "\n",
            "skill demo() -> Plan\n",
            "    description: \"demo\"\n",
            "    flow:\n",
            "        return helper()\n",
        );
        let ids = diag_ids(src);
        assert!(
            !ids.iter()
                .any(|id| id == "G::analyze::return-of-no-value-call"),
            "must NOT fire return-of-no-value-call for typed callee, got {ids:?}"
        );
    }

    /// Block-as-caller: a private `block` whose body is `return <call>`
    /// against a void callee must also fire. Confirms the wiring at the
    /// private-block fire site (analyze.rs Decl::Block arm).
    #[test]
    fn return_of_no_value_call_fires_in_private_block_caller() {
        let src = concat!(
            "block void_helper()\n",
            "    \"do something\"\n",
            "\n",
            "block caller() -> Plan\n",
            "    flow:\n",
            "        return void_helper()\n",
            "\n",
            "skill orchestrate()\n",
            "    description: \"orchestrate\"\n",
            "    flow:\n",
            "        caller()\n",
        );
        let ids = diag_ids(src);
        assert!(
            ids.iter()
                .any(|id| id == "G::analyze::return-of-no-value-call"),
            "expected G::analyze::return-of-no-value-call in private block caller, got {ids:?}"
        );
    }

    /// Export-block-as-caller: an `export block` whose `terminal_return`
    /// is `return <call>` against a void callee must fire. Confirms the
    /// wiring at the export-block fire site
    /// (post-`analyze_export_block(...)` in the entry-point arm).
    #[test]
    fn return_of_no_value_call_fires_in_export_block_caller() {
        let src = concat!(
            "block void_helper()\n",
            "    \"do something\"\n",
            "\n",
            "export block caller() -> Plan\n",
            "    flow:\n",
            "        return void_helper()\n",
        );
        let ids = diag_ids(src);
        assert!(
            ids.iter()
                .any(|id| id == "G::analyze::return-of-no-value-call"),
            "expected G::analyze::return-of-no-value-call in export block caller, got {ids:?}"
        );
    }

    // PRD #159 / Codex round-2 Issue 1 (imported-callee coverage gap):
    // the same-file walks above already pin every caller kind (skill,
    // private block, export block). The two tests below pin the
    // **imports path** via `analyze_with_imports` directly so a
    // regression where the new diagnostic stops respecting
    // `imported_block_return_types` (or where the imported-name union
    // into `block_names` regresses) shows up in this unit cluster.

    /// Imported VOID callee — the new diagnostic must fire when a
    /// skill's `return <imported_call>` targets an imported block that
    /// has no entry in `imported_block_return_types` (i.e. the
    /// upstream `export block` declares no `-> Type`).
    #[test]
    fn return_of_no_value_call_fires_for_imported_void_callee() {
        let src = concat!(
            "skill demo() -> Plan\n",
            "    description: \"demo\"\n",
            "    flow:\n",
            "        return helper()\n",
        );
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let mut used: HashSet<String> = HashSet::new();

        let mut imported_blocks: HashSet<String> = HashSet::new();
        imported_blocks.insert("helper".to_string());

        // Empty return-type map ⇒ `helper` is an imported VOID block:
        // it resolves (in `imported_blocks` → unions into `block_names`)
        // but the chunk-4 lookup against `imported_block_return_types`
        // returns nothing, so chunk-1 must fire.
        let imported_block_return_types: HashMap<String, Spanned<String>> = HashMap::new();

        let _ = analyze_with_imports(
            &file,
            0,
            "test.glyph",
            &line_index,
            &mut bag,
            &HashSet::new(),
            &imported_blocks,
            &HashSet::new(),
            &HashSet::new(),
            &mut used,
            &HashMap::new(),
            &mut registry,
            &imported_block_return_types,
            &HashMap::new(),
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );

        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::return-of-no-value-call"),
            "expected G::analyze::return-of-no-value-call for `return helper()` against imported void callee, got: {ids:?}"
        );
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::return-of-no-value-call")
            .unwrap();
        assert_eq!(
            diag.classification,
            Classification::Error,
            "return-of-no-value-call must be Error tier on the imports path"
        );
        assert!(
            diag.message.contains("helper"),
            "diagnostic must mention the callee name `helper`, got: {:?}",
            diag.message
        );
    }

    /// Imported TYPED callee — negative control. Same source as above
    /// but `imported_block_return_types` carries `helper -> Plan`, so
    /// the lookup succeeds and the new diagnostic must NOT fire.
    /// Guards against a regression where the imports path stops
    /// consulting `imported_block_return_types`.
    #[test]
    fn return_of_no_value_call_does_not_fire_for_imported_typed_callee() {
        let src = concat!(
            "skill demo() -> Plan\n",
            "    description: \"demo\"\n",
            "    flow:\n",
            "        return helper()\n",
        );
        let (file, line_index) = crate::parse::parse(src, 0).expect("parse ok");
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let mut used: HashSet<String> = HashSet::new();

        let mut imported_blocks: HashSet<String> = HashSet::new();
        imported_blocks.insert("helper".to_string());

        let mut imported_block_return_types: HashMap<String, Spanned<String>> = HashMap::new();
        imported_block_return_types.insert(
            "helper".to_string(),
            Spanned::new("Plan".to_string(), Span::new(0, 0, 0)),
        );

        let _ = analyze_with_imports(
            &file,
            0,
            "test.glyph",
            &line_index,
            &mut bag,
            &HashSet::new(),
            &imported_blocks,
            &HashSet::new(),
            &HashSet::new(),
            &mut used,
            &HashMap::new(),
            &mut registry,
            &imported_block_return_types,
            &HashMap::new(),
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );

        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::return-of-no-value-call"),
            "must NOT fire return-of-no-value-call for imported TYPED callee (helper -> Plan), got {ids:?}"
        );
    }
}

#[cfg(test)]
mod register_type_use_tests {
    use super::*;
    use crate::span::Span;

    #[test]
    fn first_explicit_decl_registers() {
        let mut reg = crate::domain_registry::Registry::new();
        let mut bag = DiagBag::new();
        let li = LineIndex::new("type LinkMode = <\"x\">\n");
        register_type_use(
            "LinkMode",
            Span::new(0, 5, 13),
            TypeUseKind::ExplicitDecl,
            "test.glyph",
            &li,
            &mut bag,
            &mut reg,
            &mut HashSet::new(),
        );
        assert!(reg.lookup("LinkMode").is_some());
        assert_eq!(bag.iter().count(), 0);
    }

    #[test]
    fn duplicate_explicit_decl_emits_duplicate_type_decl() {
        let mut reg = crate::domain_registry::Registry::new();
        let mut bag = DiagBag::new();
        let mut explicit_decl_seen: HashSet<String> = HashSet::new();
        let li = LineIndex::new("type LinkMode = <\"x\">\ntype Linkmode = <\"y\">\n");
        register_type_use(
            "LinkMode",
            Span::new(0, 5, 13),
            TypeUseKind::ExplicitDecl,
            "test.glyph",
            &li,
            &mut bag,
            &mut reg,
            &mut explicit_decl_seen,
        );
        register_type_use(
            "Linkmode",
            Span::new(0, 27, 35),
            TypeUseKind::ExplicitDecl,
            "test.glyph",
            &li,
            &mut bag,
            &mut reg,
            &mut explicit_decl_seen,
        );
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::duplicate-type-decl"),
            "expected duplicate-type-decl, got {:?}",
            ids
        );
    }

    #[test]
    fn return_annotation_then_drift_warns() {
        let mut reg = crate::domain_registry::Registry::new();
        let mut bag = DiagBag::new();
        let li = LineIndex::new("block a() -> LinkMode\nblock b() -> Linkmode\n");
        register_type_use(
            "LinkMode",
            Span::new(0, 13, 21),
            TypeUseKind::ReturnAnnotation,
            "test.glyph",
            &li,
            &mut bag,
            &mut reg,
            &mut HashSet::new(),
        );
        register_type_use(
            "Linkmode",
            Span::new(0, 35, 43),
            TypeUseKind::ReturnAnnotation,
            "test.glyph",
            &li,
            &mut bag,
            &mut reg,
            &mut HashSet::new(),
        );
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::inconsistent-type-spelling"),
            "expected inconsistent-type-spelling, got {:?}",
            ids
        );
        assert!(
            !ids.contains(&"G::analyze::duplicate-type-decl"),
            "implicit drift must not fire duplicate-type-decl"
        );
    }

    #[test]
    fn param_annotation_first_use_registers() {
        let mut reg = crate::domain_registry::Registry::new();
        let mut bag = DiagBag::new();
        let li = LineIndex::new("block a(x: LinkMode)\n");
        register_type_use(
            "LinkMode",
            Span::new(0, 11, 19),
            TypeUseKind::ParamAnnotation,
            "test.glyph",
            &li,
            &mut bag,
            &mut reg,
            &mut HashSet::new(),
        );
        assert!(reg.lookup("LinkMode").is_some());
    }

    #[test]
    fn idempotent_same_raw_does_not_warn() {
        let mut reg = crate::domain_registry::Registry::new();
        let mut bag = DiagBag::new();
        let mut explicit_decl_seen: HashSet<String> = HashSet::new();
        let li = LineIndex::new("type LinkMode = <\"x\">\nblock a() -> LinkMode\n");
        register_type_use(
            "LinkMode",
            Span::new(0, 5, 13),
            TypeUseKind::ExplicitDecl,
            "test.glyph",
            &li,
            &mut bag,
            &mut reg,
            &mut explicit_decl_seen,
        );
        register_type_use(
            "LinkMode",
            Span::new(0, 35, 43),
            TypeUseKind::ReturnAnnotation,
            "test.glyph",
            &li,
            &mut bag,
            &mut reg,
            &mut explicit_decl_seen,
        );
        assert_eq!(bag.iter().count(), 0);
    }

    /// Regression: a selective type-import populates the registry first;
    /// a subsequent in-file `ExplicitDecl` with the same raw spelling must
    /// NOT emit `G::analyze::duplicate-type-decl`. The previous registry
    /// entry came from an import alias, not an in-file `type` declaration.
    /// The collision against the import alias is owned by
    /// `sweep_type_decl_name_collisions`, not this helper.
    #[test]
    fn explicit_decl_after_selective_import_does_not_duplicate() {
        let mut reg = crate::domain_registry::Registry::new();
        let mut bag = DiagBag::new();
        let mut explicit_decl_seen: HashSet<String> = HashSet::new();
        let li = LineIndex::new("import \"x\" { LinkMode }\ntype LinkMode = <\"y\">\n");
        register_type_use(
            "LinkMode",
            Span::new(0, 13, 21),
            TypeUseKind::SelectiveImport,
            "test.glyph",
            &li,
            &mut bag,
            &mut reg,
            &mut explicit_decl_seen,
        );
        register_type_use(
            "LinkMode",
            Span::new(0, 29, 37),
            TypeUseKind::ExplicitDecl,
            "test.glyph",
            &li,
            &mut bag,
            &mut reg,
            &mut explicit_decl_seen,
        );
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::duplicate-type-decl"),
            "ExplicitDecl after SelectiveImport must NOT emit duplicate-type-decl, got: {:?}",
            ids
        );
    }
}

// ---------------------------------------------------------------------------
// Phase 3 / Task 3.12 — block-scope `{param}` slot validation tests. The
// `Skill`-scope counterpart (`check_skill_freeform_and_context_slots`) is
// covered by the corpus fixtures `slot_in_freeform_section.glyph` /
// `slot_in_context_unknown_param.glyph`; this module mirrors that coverage at
// `block` scope where the walker is invoked via `check_block_freeform_slots`.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod block_freeform_slot_tests {
    fn diag_ids(src: &str) -> Vec<String> {
        crate::check_source(src, 0, "test.glyph")
            .iter()
            .map(|d| d.id.clone())
            .collect()
    }

    fn diag_messages(src: &str, id: &str) -> Vec<String> {
        crate::check_source(src, 0, "test.glyph")
            .iter()
            .filter(|d| d.id == id)
            .map(|d| d.message.clone())
            .collect()
    }

    /// A `block`'s freeform section (`quality:`) that references an
    /// `{undeclared}` slot must fire `G::analyze::unknown-param-slot` against
    /// the block's own param scope — not the enclosing skill's scope.
    #[test]
    fn block_freeform_unknown_param_slot_fires_diag() {
        let src = "\
skill demo(scope = \".\")
    description: \"demo\"
    flow:
        worker()

block worker(mode = \"default\")
    description: \"worker block\"
    quality:
        \"Apply {undeclared} rigor\"
    flow:
        \"do work\"
";
        let ids = diag_ids(src);
        assert!(
            ids.iter().any(|id| id == "G::analyze::unknown-param-slot"),
            "expected G::analyze::unknown-param-slot, got {ids:?}"
        );
        // Message must report the block's name, not the skill's, since each
        // block has its own param scope.
        let msgs = diag_messages(src, "G::analyze::unknown-param-slot");
        assert!(
            msgs.iter().any(|m| m.contains("worker")),
            "expected message to name block `worker`, got {msgs:?}"
        );
        assert!(
            !msgs.iter().any(|m| m.contains("`demo`")),
            "did not expect message to name skill `demo`, got {msgs:?}"
        );
    }

    /// Negative control: a `{slot}` that DOES name a block parameter must
    /// not fire. The skill parameter `scope` is also in scope at the skill
    /// level but NOT in the block's param scope — referencing `{scope}`
    /// inside the block must fire instead. This proves the walker uses the
    /// block's own param set, not a unioned scope.
    #[test]
    fn block_freeform_known_block_param_passes() {
        let src = "\
skill demo(scope = \".\")
    description: \"demo\"
    flow:
        worker()

block worker(mode = \"default\")
    description: \"worker block\"
    quality:
        \"Apply {mode} rigor\"
    flow:
        \"do work\"
";
        let ids = diag_ids(src);
        assert!(
            !ids.iter().any(|id| id == "G::analyze::unknown-param-slot"),
            "did not expect G::analyze::unknown-param-slot, got {ids:?}"
        );
    }

    /// Negative scope-leak control: a `{scope}` slot inside the block must
    /// fire because `scope` is the SKILL's parameter, not the block's. This
    /// catches a regression where the walker uses the wrong (skill-level)
    /// param scope for block freeform.
    #[test]
    fn block_freeform_skill_param_slot_fires_diag() {
        let src = "\
skill demo(scope = \".\")
    description: \"demo\"
    flow:
        worker()

block worker(mode = \"default\")
    description: \"worker block\"
    quality:
        \"Apply {scope} rigor\"
    flow:
        \"do work\"
";
        let ids = diag_ids(src);
        assert!(
            ids.iter().any(|id| id == "G::analyze::unknown-param-slot"),
            "expected G::analyze::unknown-param-slot for skill-param leak, got {ids:?}"
        );
    }
}
