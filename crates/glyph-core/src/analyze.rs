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
    //
    // `type Foo` decls are deliberately omitted from this sweep. A previous
    // version iterated over them and fired `name-collision` whenever a
    // `type Foo` matched a domain-registry entry seeded by `-> Foo`
    // annotations — that is a false positive for the canonical
    // `type Foo = <"...">` + `-> Foo` pattern endorsed by the spec
    // (`typed-params-with-descriptions §282`). Legitimate `type Foo` vs
    // `const Foo` / `block Foo` / parameter `Foo` collisions (spec §100)
    // are not currently caught anywhere; that is a pre-existing gap parked
    // for a follow-up universal-namespace pass and is independent of this
    // omission.
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
            Decl::TypeDecl(_) => {}
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

/// Universal-namespace check (`design/values-and-names.md` §No-Shadowing,
/// `design/types.md` §Same-file duplicates): a `type Foo` decl participates in
/// the same flat scope as `const`, `block`, `export block`, parameters, and
/// import aliases. Any other in-scope `Foo` is a hard `name-collision`.
///
/// This sweep complements `sweep_name_collisions` (which fires from the
/// registry direction — `-> Foo` annotations vs param/const names) by firing
/// from the type-decl direction. The canonical `type Foo = <"…">` + `-> Foo`
/// pairing is **not** a collision (`design/types.md` §Same-file duplicates) and
/// the registry sweep already covers param/const collisions when `Foo` is in
/// the registry — so we skip those slots in that case to avoid a double-
/// diagnostic for the same logical issue. Block-decl collisions are not
/// covered by the registry sweep regardless, so they always fire here.
///
/// Import aliases (selective `name as alias`, whole-module `as alias`) are
/// included; the host file's bare imported `name` (without `as`) is also a
/// local binding (`design/imports.md` §Selective Imports) and counts as an
/// alias for collision purposes.
fn sweep_type_decl_name_collisions(
    file: &SourceFile,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    registry: &crate::domain_registry::Registry,
) {
    let type_decls: Vec<(&str, Span)> = file
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::TypeDecl(t) => Some((t.node.name.as_str(), t.span)),
            _ => None,
        })
        .collect();
    if type_decls.is_empty() {
        return;
    }

    let mut params: Vec<(&str, Span)> = Vec::new();
    let mut consts: Vec<(&str, Span)> = Vec::new();
    // (kind_label, raw_name, span) — kept separate from params/consts because
    // block collisions fire even when the registry already covers the name.
    let mut blocks: Vec<(&'static str, &str, Span)> = Vec::new();
    // (raw_local_name, span). `imports` carries owned `String`s because
    // selective alias names live behind an `Option<String>` — the caller
    // owns the storage to keep `&str` lifetimes clean.
    let mut imports: Vec<(String, Span)> = Vec::new();
    for decl in &file.decls {
        match decl {
            Decl::Skill(s) => {
                for p in &s.node.params {
                    params.push((p.name.as_str(), p.span));
                }
            }
            Decl::ExportBlock(e) => {
                blocks.push(("export block", e.node.name.as_str(), e.span));
                for p in &e.node.params {
                    params.push((p.name.as_str(), p.span));
                }
            }
            Decl::Block(b) => {
                blocks.push(("block", b.node.name.as_str(), b.span));
                for p in &b.node.params {
                    params.push((p.name.as_str(), p.span));
                }
            }
            Decl::Const(c) => {
                consts.push((c.node.name.as_str(), c.span));
            }
            Decl::Import(imp) => match &imp.node.kind {
                ast::ImportKind::Selective(names) => {
                    for n in names {
                        let local = n.alias.clone().unwrap_or_else(|| n.name.node.clone());
                        imports.push((local, n.name.span));
                    }
                }
                ast::ImportKind::WholeModule { alias } => {
                    imports.push((alias.clone(), imp.span));
                }
            },
            Decl::TypeDecl(_) => {}
        }
    }

    for (tname, tspan) in &type_decls {
        let canonical = crate::domain_registry::canonicalize_identifier(tname);
        let in_registry = registry.iter().any(|e| e.canonical_name == canonical);
        for (bkind, bname, bspan) in &blocks {
            if crate::domain_registry::canonicalize_identifier(bname) == canonical {
                emit_type_decl_collision(
                    tname, *tspan, bkind, bname, *bspan, file_label, line_index, bag,
                );
            }
        }
        for (iname, ispan) in &imports {
            if crate::domain_registry::canonicalize_identifier(iname) == canonical {
                emit_type_decl_collision(
                    tname,
                    *tspan,
                    "import alias",
                    iname,
                    *ispan,
                    file_label,
                    line_index,
                    bag,
                );
            }
        }
        if !in_registry {
            for (pname, pspan) in &params {
                if crate::domain_registry::canonicalize_identifier(pname) == canonical {
                    emit_type_decl_collision(
                        tname,
                        *tspan,
                        "parameter",
                        pname,
                        *pspan,
                        file_label,
                        line_index,
                        bag,
                    );
                }
            }
            for (cname, cspan) in &consts {
                if crate::domain_registry::canonicalize_identifier(cname) == canonical {
                    emit_type_decl_collision(
                        tname, *tspan, "const", cname, *cspan, file_label, line_index, bag,
                    );
                }
            }
        }
    }
}

/// Push one `G::analyze::name-collision` Error against a `type` decl.
/// Mirrors `emit_name_collision` but anchors the primary span on the type
/// declaration (the binding site that introduces the name) rather than on a
/// `-> Type` use.
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

/// Run Phase 2 with diagnostic emission.
///
/// Pushes any structured diagnostics onto `bag` and returns the AST unchanged.
/// `file_label` and `line_index` follow the same contract as the parser entry
/// point (`design/diagnostics.md` §Span Semantics).
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
                &export_block_decls,
                &empty_imported_block_params,
                &HashMap::new(),
                &local_callee_return_types,
                &empty_imported_block_return_types,
                &context_skill_names,
                &constraint_skill_names,
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

    // Duplicate `type Foo` in the same file is a hard error. Keys are
    // canonicalized per §D6 so `type RepoContext` and `type repo_context` are
    // treated as the same name (the language guide's case-insensitive +
    // underscore-insensitive rule applies to *every* identifier namespace,
    // not just primitive types — Codex finding #3 follow-up).
    {
        use crate::domain_registry::canonicalize_identifier;
        let mut seen_types: HashMap<String, Span> = HashMap::new();
        for d in &file.decls {
            if let Decl::TypeDecl(t) = d {
                let name = t.node.name.as_str();
                let canonical = canonicalize_identifier(name);
                let span = t.span;
                if let Some(_prev_span) = seen_types.get(&canonical) {
                    bag.push(
                        Diagnostic::error(
                            "G::analyze::duplicate-type-decl",
                            format!("duplicate `type {}` declaration in this file", name),
                            SourceSpan::from_byte_span(file_label, span, line_index),
                        ),
                        span,
                    );
                } else {
                    seen_types.insert(canonical, span);
                }
            }
        }
    }

    // Issue #84 Chunk 3 (AC5): domain-type-vs-param/const collision sweep.
    sweep_name_collisions(&file, file_label, line_index, bag, registry);
    // Universal-namespace check (`design/values-and-names.md` §No-Shadowing):
    // type-decl-vs-param/const/block collision sweep, complementary to the
    // registry-direction sweep above.
    sweep_type_decl_name_collisions(&file, file_label, line_index, bag, registry);
    // Reject name_ref param defaults that don't resolve to an in-scope `const`
    // (Codex finding #1 follow-up): without this sweep an unresolved ref like
    // `risk = default_risk` (when `default_risk` is a block / unknown name)
    // leaks into the lowerer's IR as the bare identifier.
    sweep_param_default_name_refs(&file, file_label, line_index, bag, None);

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
    annotate_file_branches(&mut file);

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
                                    .clone()
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
                walk_flow_for_cross_file(&spanned.node.flow, targets, &mut out);
            }
            Decl::ExportBlock(_) | Decl::Const(_) => {}
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
                    record_text_use(&name.node, name.span, text_defs, file_path, out);
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
                ..
            } => {
                walk_flow_for_resolutions(
                    then_body,
                    file_path,
                    text_defs,
                    block_defs,
                    export_block_defs,
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
            FlowStmt::Branch {
                then_body,
                elif_branches,
                else_body,
                ..
            } => {
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
) -> SourceFile {
    // Issue #109 chunk 3 — Analyze invariant. Enforced on the import-aware
    // path too so multi-file compiles get identical guarantees. See
    // `check_unmerged_duplicate_subsections` doc-comment for rationale.
    check_unmerged_duplicate_subsections(file, file_label, line_index, bag);

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

    // Issue #84 Chunk 3 (AC5): domain-type-vs-param/const collision sweep.
    // Imports-path parity with `analyze_with_diagnostics`.
    sweep_name_collisions(file, file_label, line_index, bag, registry);
    // Universal-namespace check (`design/values-and-names.md` §No-Shadowing):
    // type-decl-vs-param/const/block collision sweep, complementary to the
    // registry-direction sweep above. Imports-path parity.
    sweep_type_decl_name_collisions(file, file_label, line_index, bag, registry);
    // Codex finding #1 follow-up: reject name_ref param defaults that don't
    // resolve to an in-scope `const` (same-file or imported). `imported_texts`
    // already carries `alias.name` entries for whole-module imports, so a
    // single-shape lookup covers both selective and aliased forms.
    sweep_param_default_name_refs(file, file_label, line_index, bag, Some(imported_texts));

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
    let mut annotated = file.clone();
    annotate_file_branches(&mut annotated);

    // Task 3.1: emit G::analyze::condition-non-boolean-non-predicate for
    // numeric-kinded tokens in condition position.
    check_file_numeric_conditions(&annotated, file_label, line_index, bag);

    annotated
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
        export_block_decls,
        imported_block_params,
        imported_block_descriptions,
        local_callee_return_types,
        imported_block_return_types,
        context_skill_names,
        constraint_skill_names,
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
    export_block_decls: &HashMap<&str, &crate::ast::ExportBlockDecl>,
    imported_block_params: &HashMap<String, Vec<crate::ast::Param>>,
    imported_block_descriptions: &HashMap<String, String>,
    local_callee_return_types: &HashMap<&str, &Spanned<String>>,
    imported_block_return_types: &HashMap<String, Spanned<String>>,
    context_skill_names: &HashSet<&str>,
    constraint_skill_names: &HashSet<&str>,
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
            FlowStmt::Call { target, args, .. } => {
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

use crate::condition::{tokenize_condition, ConditionClassification, ConditionTokenKind};

const COMPOSITIONAL_OPERATORS: &[&str] = &["not", "and"];

// ---------------------------------------------------------------------------
// Branch annotation walker (Task 2.4)
// ---------------------------------------------------------------------------

/// Recursively annotate every `FlowStmt::Branch` (and their `elif` arms) in
/// `flow` with a `ConditionClassification`.
///
/// `texts` must already contain the file-level const bindings.
/// `params_with_string_default` and `bindings` should be built from the
/// enclosing skill's signature / flow.  For the MVP they may be empty — the
/// classifier still handles the `"big"` → `PredicateConst` case via `texts`.
fn annotate_branch_classifications<'a>(
    flow: &mut Vec<FlowStmt>,
    texts: &HashMap<&'a str, (String, crate::kind_infer::TypeTag)>,
    params_with_string_default: &HashSet<&'a str>,
    bindings: &HashSet<&'a str>,
    block_decls: &HashMap<&'a str, &'a BlockDecl>,
) {
    for stmt in flow.iter_mut() {
        if let FlowStmt::Branch {
            condition,
            condition_classification,
            then_body,
            elif_branches,
            else_body,
        } = stmt
        {
            *condition_classification = Some(classify_condition(
                condition,
                texts,
                params_with_string_default,
                bindings,
                block_decls,
            ));
            for elif in elif_branches.iter_mut() {
                elif.condition_classification = Some(classify_condition(
                    &elif.condition,
                    texts,
                    params_with_string_default,
                    bindings,
                    block_decls,
                ));
                annotate_branch_classifications(
                    &mut elif.body,
                    texts,
                    params_with_string_default,
                    bindings,
                    block_decls,
                );
            }
            annotate_branch_classifications(
                then_body,
                texts,
                params_with_string_default,
                bindings,
                block_decls,
            );
            if let Some(eb) = else_body {
                annotate_branch_classifications(
                    eb,
                    texts,
                    params_with_string_default,
                    bindings,
                    block_decls,
                );
            }
        }
    }
}

/// Build the file-level owned-key `texts` backing store mapping each const
/// name to its `(body, TypeTag)`. Callers convert this to the `&str`-keyed
/// shape that `classify_condition` / `annotate_branch_classifications`
/// consume; that conversion must happen in the caller's scope so the `&str`
/// references stay tied to a binding the caller owns.
fn build_const_texts(file: &SourceFile) -> HashMap<String, (String, crate::kind_infer::TypeTag)> {
    file.decls
        .iter()
        .filter_map(|d| match d {
            crate::ast::Decl::Const(c) => {
                let name = c.node.name.clone();
                let (body, literal) = match &c.node.value {
                    crate::ast::ConstValue::String(s) => {
                        (s.clone(), crate::kind_infer::Literal::String(s.clone()))
                    }
                    crate::ast::ConstValue::Int(s) => {
                        (s.clone(), crate::kind_infer::Literal::Number(s.clone()))
                    }
                    crate::ast::ConstValue::Float(s) => {
                        (s.clone(), crate::kind_infer::Literal::Number(s.clone()))
                    }
                    crate::ast::ConstValue::Bool(s) => {
                        (s.clone(), crate::kind_infer::Literal::Bool(s.clone()))
                    }
                };
                let tag = crate::kind_infer::infer_primitive(&literal);
                Some((name, (body, tag)))
            }
            _ => None,
        })
        .collect()
}

/// Annotate every `Branch` node in each `Skill`'s flow inside a `SourceFile`.
///
/// Called once at the end of `analyze_with_diagnostics` (and `analyze_with_imports`)
/// after all semantic diagnostics have been emitted.
fn annotate_file_branches(file: &mut SourceFile) {
    // Build the texts map with owned keys so the borrow of `file.decls` ends
    // before the mutable iteration below begins.
    let owned_texts = build_const_texts(file);
    let texts: HashMap<&str, (String, crate::kind_infer::TypeTag)> = owned_texts
        .iter()
        .map(|(k, v)| (k.as_str(), v.clone()))
        .collect();

    let empty_params: HashSet<&str> = HashSet::new();
    let empty_bindings: HashSet<&str> = HashSet::new();
    let empty_block_decls: HashMap<&str, &BlockDecl> = HashMap::new();

    for decl in file.decls.iter_mut() {
        if let crate::ast::Decl::Skill(spanned) = decl {
            annotate_branch_classifications(
                &mut spanned.node.flow,
                &texts,
                &empty_params,
                &empty_bindings,
                &empty_block_decls,
            );
        }
    }
}

/// Walk every `Branch`/`ElifBranch` condition in `flow` and push
/// `G::analyze::condition-non-boolean-non-predicate` for any condition that
/// contains a `Numeric`-kinded token.
fn check_flow_numeric_conditions(
    flow: &[FlowStmt],
    span: crate::span::Span,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    texts: &HashMap<&str, (String, crate::kind_infer::TypeTag)>,
    empty_params: &HashSet<&str>,
    empty_bindings: &HashSet<&str>,
    empty_block_decls: &HashMap<&str, &BlockDecl>,
) {
    for stmt in flow {
        match stmt {
            FlowStmt::Branch {
                condition,
                then_body,
                elif_branches,
                else_body,
                ..
            } => {
                let c = classify_condition(
                    condition,
                    texts,
                    empty_params,
                    empty_bindings,
                    empty_block_decls,
                );
                if c.has_numeric_bare_condition {
                    bag.push(
                        Diagnostic {
                            id: "G::analyze::condition-non-boolean-non-predicate".into(),
                            classification: Classification::Error,
                            message: "condition expression must be boolean or a string predicate"
                                .into(),
                            span: SourceSpan::from_byte_span(file_label, span, line_index),
                            related: Vec::new(),
                            hints: vec![
                                "Bind to a boolean (e.g., a Bool-returning call), use a string predicate const, or compare with ==. Glyph does not implicitly truth-test integers."
                                    .into(),
                            ],
                        },
                        span,
                    );
                }
                for elif in elif_branches {
                    let ec = classify_condition(
                        &elif.condition,
                        texts,
                        empty_params,
                        empty_bindings,
                        empty_block_decls,
                    );
                    if ec.has_numeric_bare_condition {
                        bag.push(
                            Diagnostic {
                                id: "G::analyze::condition-non-boolean-non-predicate".into(),
                                classification: Classification::Error,
                                message:
                                    "condition expression must be boolean or a string predicate"
                                        .into(),
                                span: SourceSpan::from_byte_span(file_label, span, line_index),
                                related: Vec::new(),
                                hints: vec![
                                    "Bind to a boolean (e.g., a Bool-returning call), use a string predicate const, or compare with ==. Glyph does not implicitly truth-test integers."
                                        .into(),
                                ],
                            },
                            span,
                        );
                    }
                    check_flow_numeric_conditions(
                        &elif.body,
                        span,
                        file_label,
                        line_index,
                        bag,
                        texts,
                        empty_params,
                        empty_bindings,
                        empty_block_decls,
                    );
                }
                check_flow_numeric_conditions(
                    then_body,
                    span,
                    file_label,
                    line_index,
                    bag,
                    texts,
                    empty_params,
                    empty_bindings,
                    empty_block_decls,
                );
                if let Some(eb) = else_body {
                    check_flow_numeric_conditions(
                        eb,
                        span,
                        file_label,
                        line_index,
                        bag,
                        texts,
                        empty_params,
                        empty_bindings,
                        empty_block_decls,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Emit `G::analyze::condition-non-boolean-non-predicate` for every skill in
/// `file` that has a numeric-kinded token in a branch condition.
///
/// Called once at the end of `analyze_with_diagnostics` and
/// `analyze_with_imports` after `annotate_file_branches`, so the classifier
/// results are already populated.
fn check_file_numeric_conditions(
    file: &SourceFile,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) {
    let owned_texts = build_const_texts(file);
    let texts: HashMap<&str, (String, crate::kind_infer::TypeTag)> = owned_texts
        .iter()
        .map(|(k, v)| (k.as_str(), v.clone()))
        .collect();

    // Allocate the empty params/bindings/block_decls collections ONCE here so
    // the recursive walker doesn't re-allocate at every nesting level.
    let empty_params: HashSet<&str> = HashSet::new();
    let empty_bindings: HashSet<&str> = HashSet::new();
    let empty_block_decls: HashMap<&str, &BlockDecl> = HashMap::new();

    for decl in &file.decls {
        if let crate::ast::Decl::Skill(spanned) = decl {
            check_flow_numeric_conditions(
                &spanned.node.flow,
                spanned.span,
                file_label,
                line_index,
                bag,
                &texts,
                &empty_params,
                &empty_bindings,
                &empty_block_decls,
            );
        }
    }
}

pub fn classify_condition<'a>(
    condition: &str,
    texts: &HashMap<&'a str, (String, crate::kind_infer::TypeTag)>,
    params_with_string_default: &HashSet<&'a str>,
    bindings: &HashSet<&'a str>,
    block_decls: &HashMap<&'a str, &'a BlockDecl>,
) -> ConditionClassification {
    let mut tokens = Vec::new();
    let mut has_boolean = false;
    let mut has_predicate = false;
    let mut has_composition = false;
    let mut has_numeric = false;

    // The parser stores conditions with a trailing ` :` (e.g., `"big :"` for
    // `if big:`).  Strip it before tokenizing so downstream classifiers don't
    // see `:` as an unknown/Boolean token.
    let trimmed = condition.trim().trim_end_matches(':').trim_end();

    for tok in tokenize_condition(trimmed) {
        let kind = classify_token(
            &tok,
            texts,
            params_with_string_default,
            bindings,
            block_decls,
        );
        match kind {
            ConditionTokenKind::Boolean => has_boolean = true,
            ConditionTokenKind::Numeric => has_numeric = true,
            ConditionTokenKind::PredicateApplies
            | ConditionTokenKind::PredicateConst
            | ConditionTokenKind::PredicateLiteral => has_predicate = true,
            ConditionTokenKind::Operator => {
                if COMPOSITIONAL_OPERATORS.contains(&tok.as_str()) {
                    has_composition = true;
                }
            }
        }
        tokens.push(crate::condition::ClassifiedConditionToken {
            text: tok.to_string(),
            kind,
            is_comparison_operand: false, // Task 6 replaces this whole function
        });
    }

    ConditionClassification {
        tokens,
        has_boolean_token: has_boolean,
        has_predicate_token: has_predicate,
        has_compositional_operator: has_composition,
        has_comparison_operator: false, // Task 6 introduces position-aware tracking
        has_numeric_bare_condition: has_numeric,
    }
}

fn classify_token<'a>(
    tok: &str,
    texts: &HashMap<&'a str, (String, crate::kind_infer::TypeTag)>,
    params_with_string_default: &HashSet<&'a str>,
    bindings: &HashSet<&'a str>,
    _block_decls: &HashMap<&'a str, &'a BlockDecl>,
) -> ConditionTokenKind {
    if matches!(tok, "and" | "or" | "not" | "==" | "(" | ")") {
        return ConditionTokenKind::Operator;
    }
    if tok.starts_with('"') {
        return ConditionTokenKind::PredicateLiteral;
    }
    if tok.contains(".applies()") {
        // Syntactic classification: any `NAME.applies()` form is PredicateApplies.
        // Semantic validation (receiver must be a known block) is done by
        // `check_applies_in_condition`, not here.
        return ConditionTokenKind::PredicateApplies;
    }
    // Numeric literal: integer or float token.
    // `f64::from_str` accepts every well-formed integer literal too.
    if tok.parse::<f64>().is_ok() {
        return ConditionTokenKind::Numeric;
    }
    if let Some((_body, tag)) = texts.get(tok) {
        return match tag {
            crate::kind_infer::TypeTag::String => ConditionTokenKind::PredicateConst,
            crate::kind_infer::TypeTag::Bool => ConditionTokenKind::Boolean,
            crate::kind_infer::TypeTag::Int | crate::kind_infer::TypeTag::Float => {
                ConditionTokenKind::Numeric
            }
            _ => ConditionTokenKind::Boolean,
        };
    }
    if params_with_string_default.contains(tok) {
        return ConditionTokenKind::PredicateConst;
    }
    if bindings.contains(tok) {
        return ConditionTokenKind::Boolean;
    }
    ConditionTokenKind::Boolean
}

// ---------------------------------------------------------------------------

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

    // PRD #103 / Slice 2 (#105): the previous `G::analyze::missing-param-default`
    // rule (which required every export-block parameter to declare a default)
    // has been retired. Export-block parameters may now be required, matching
    // the private-`block` semantics. Call-site enforcement lives in
    // `validate_call_args` (FlowStmt::Call resolver above) and surfaces
    // `G::analyze::missing-required-arg` when a caller omits the positional
    // argument for a required parameter.

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
                        let local = n.alias.clone().unwrap_or_else(|| n.name.node.clone());
                        bound.insert(local);
                    }
                }
                ast::ImportKind::WholeModule { alias } => {
                    bound.insert(alias.clone());
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
        }
        Decl::Block(b) => {
            for stmt in &b.node.flow {
                collect_refs_from_flow_stmt(stmt, out);
            }
        }
        Decl::ExportBlock(b) => {
            if let Some(expr) = &b.node.terminal_return {
                collect_refs_from_return_expr(expr, out);
            }
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
        FlowStmt::InlineString(_) | FlowStmt::ConstraintMarker(_) | FlowStmt::ContextMarker(_) => {}
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
                flow: vec![FlowStmt::InlineString("Write files.".to_string())],
                description: None,
                effects: vec!["writes_files".to_string()],
                return_type: None,
                generated: false,
                extra_subsections: Vec::new(),
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
                extra_subsections: Vec::new(),
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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
        let _ =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);

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
    /// applies to type names too. Two type decls that differ only in casing
    /// or underscore placement (`RepoContext` vs `repo_context`) should
    /// trigger `G::analyze::duplicate-type-decl`, since downstream lookups
    /// (TypeRegistry::get) treat them as the same key.
    #[test]
    fn duplicate_type_decl_canonical_form_collision() {
        let src = r#"type RepoContext = <"first">
type repo_context = <"second">
"#;
        let ids = check_ids(src);
        assert!(
            ids.iter().any(|id| id == "G::analyze::duplicate-type-decl"),
            "case+underscore-insensitive duplicate type decls should collide; got: {:?}",
            ids
        );
    }

    /// Universal-namespace check: `type Foo` collides with `const Foo` even
    /// when no `-> Foo` annotation registers `Foo` into the domain registry.
    #[test]
    fn type_decl_collides_with_const() {
        let src = r#"type Foo = <"a domain type">
const Foo = "value"
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
                .any(|m| m.contains("type `Foo`") && m.contains("const")),
            "expected type-vs-const collision, got messages: {:?}",
            collisions
        );
    }

    /// `type Foo` collides with a private `block Foo`. The registry sweep does
    /// not cover block decl names, so this path is exclusive to the new sweep.
    #[test]
    fn type_decl_collides_with_block() {
        let src = r#"type Foo = <"a domain type">
block Foo()
    description: "private helper"
    flow:
        "do work"
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
                .any(|m| m.contains("type `Foo`") && m.contains("block")),
            "expected type-vs-block collision, got messages: {:?}",
            collisions
        );
    }

    /// `type Foo` collides with `export block Foo`.
    #[test]
    fn type_decl_collides_with_export_block() {
        let src = r#"type Foo = <"a domain type">
export block Foo()
    description: "exported helper"
    flow:
        "do work"
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
                .any(|m| m.contains("type `Foo`") && m.contains("export block")),
            "expected type-vs-export-block collision, got messages: {:?}",
            collisions
        );
    }

    /// `type Foo` collides with parameter `Foo` even when no `-> Foo`
    /// annotation registers `Foo` into the registry.
    #[test]
    fn type_decl_collides_with_parameter_without_registry_use() {
        let src = r#"type Foo = <"a domain type">
skill use_it(Foo = "x")
    description: "test"
    flow:
        "do work"
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
                .any(|m| m.contains("type `Foo`") && m.contains("parameter")),
            "expected type-vs-parameter collision, got messages: {:?}",
            collisions
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

    /// Dedupe: when `-> Foo` registers `Foo` AND a parameter named `Foo`
    /// exists, the registry-direction sweep already fires. The type-decl
    /// sweep skips param/const checks for in-registry names so the user sees
    /// exactly one diagnostic per logical issue.
    #[test]
    fn type_decl_param_collision_dedupes_with_registry_sweep() {
        let src = r#"type Foo = <"a domain type">
skill use_it(Foo = "x") -> Foo
    description: "test"
    flow:
        return "value"
"#;
        let bag = crate::check_source(src, 0, "test.glyph");
        let param_collisions: Vec<&str> = bag
            .iter()
            .filter(|d| d.id == "G::analyze::name-collision" && d.message.contains("parameter"))
            .map(|d| d.message.as_str())
            .collect();
        assert_eq!(
            param_collisions.len(),
            1,
            "expected exactly one param-collision message, got: {:?}",
            param_collisions
        );
    }

    /// `type Foo` collides with a selectively-imported `Foo` (no `as` alias).
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
                .any(|m| m.contains("type `Foo`") && m.contains("import alias")),
            "expected type-vs-import-alias collision, got messages: {:?}",
            collisions
        );
    }

    /// `type Foo` collides with `import { bar as Foo }` (selective + alias).
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
                .any(|m| m.contains("type `Foo`") && m.contains("import alias")),
            "expected type-vs-aliased-import collision, got messages: {:?}",
            collisions
        );
    }

    /// `type Foo` collides with whole-module `import "..." as Foo`.
    #[test]
    fn type_decl_collides_with_whole_module_import() {
        let src = r#"import "./other.glyph" as Foo
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
                .any(|m| m.contains("type `Foo`") && m.contains("import alias")),
            "expected type-vs-whole-module-import collision, got messages: {:?}",
            collisions
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

#[cfg(test)]
mod classify_condition_tests {
    use super::*;
    use crate::condition::ConditionTokenKind as K;
    use std::collections::{HashMap, HashSet};

    fn empty_blocks<'a>() -> HashMap<&'a str, &'a BlockDecl> {
        HashMap::new()
    }

    #[test]
    fn classifies_pure_applies_call() {
        let blocks: HashMap<&str, &BlockDecl> = HashMap::new();
        let texts: HashMap<&str, (String, crate::kind_infer::TypeTag)> = HashMap::new();
        let params: HashSet<&str> = HashSet::new();
        let bindings: HashSet<&str> = HashSet::new();
        let c = classify_condition("my_block.applies()", &texts, &params, &bindings, &blocks);
        let kinds: Vec<_> = c.tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![K::PredicateApplies]);
        assert!(c.is_pure_predicate());
    }

    #[test]
    fn classifies_string_kinded_const_as_predicate_const() {
        let blocks = empty_blocks();
        let mut texts: HashMap<&str, (String, crate::kind_infer::TypeTag)> = HashMap::new();
        texts.insert(
            "complex_change",
            (
                "the change is complex".into(),
                crate::kind_infer::TypeTag::String,
            ),
        );
        let params: HashSet<&str> = HashSet::new();
        let bindings: HashSet<&str> = HashSet::new();
        let c = classify_condition("complex_change", &texts, &params, &bindings, &blocks);
        let kinds: Vec<_> = c.tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![K::PredicateConst]);
        assert!(c.is_pure_predicate());
    }

    #[test]
    fn classifies_inline_string_literal_as_predicate_literal() {
        let blocks = empty_blocks();
        let texts: HashMap<&str, (String, crate::kind_infer::TypeTag)> = HashMap::new();
        let params: HashSet<&str> = HashSet::new();
        let bindings: HashSet<&str> = HashSet::new();
        let c = classify_condition("\"the user opted in\"", &texts, &params, &bindings, &blocks);
        let kinds: Vec<_> = c.tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![K::PredicateLiteral]);
        assert!(c.is_pure_predicate());
    }

    #[test]
    fn classifies_bool_kinded_const_as_boolean() {
        let blocks = empty_blocks();
        let mut texts: HashMap<&str, (String, crate::kind_infer::TypeTag)> = HashMap::new();
        texts.insert(
            "is_dry_run",
            ("true".into(), crate::kind_infer::TypeTag::Bool),
        );
        let params: HashSet<&str> = HashSet::new();
        let bindings: HashSet<&str> = HashSet::new();
        let c = classify_condition("is_dry_run", &texts, &params, &bindings, &blocks);
        let kinds: Vec<_> = c.tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![K::Boolean]);
        assert!(!c.is_pure_predicate());
    }

    #[test]
    fn mixed_predicate_const_with_not_is_not_pure() {
        let blocks = empty_blocks();
        let mut texts: HashMap<&str, (String, crate::kind_infer::TypeTag)> = HashMap::new();
        texts.insert(
            "complex_change",
            (
                "the change is complex".into(),
                crate::kind_infer::TypeTag::String,
            ),
        );
        let params: HashSet<&str> = HashSet::new();
        let bindings: HashSet<&str> = HashSet::new();
        let c = classify_condition("not complex_change", &texts, &params, &bindings, &blocks);
        assert!(!c.is_pure_predicate());
        assert!(c.has_compositional_operator);
        assert!(c.has_predicate_token);
    }

    #[test]
    fn or_combination_of_predicates_stays_pure() {
        let blocks = empty_blocks();
        let mut texts: HashMap<&str, (String, crate::kind_infer::TypeTag)> = HashMap::new();
        texts.insert("a", ("alpha".into(), crate::kind_infer::TypeTag::String));
        texts.insert("b", ("beta".into(), crate::kind_infer::TypeTag::String));
        let params: HashSet<&str> = HashSet::new();
        let bindings: HashSet<&str> = HashSet::new();
        let c = classify_condition("a or b", &texts, &params, &bindings, &blocks);
        assert!(c.is_pure_predicate());
    }
}

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
