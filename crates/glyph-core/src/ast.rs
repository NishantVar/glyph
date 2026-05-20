//! Loose AST emitted by the parser (Phase 1).
//!
//! Names are unresolved, types unchecked, roles unassigned.
//! Walking-skeleton subset — covers the constructs in `update_docs.glyph`.

use crate::output_target::OutputTargetExpr;
use crate::span::{Span, Spanned};

/// One source file's parsed declarations, in source order.
#[derive(Clone, Debug)]
pub struct SourceFile {
    pub decls: Vec<Decl>,
}

/// Source position of a sub-section header (the `<name>:` line). Used by
/// Phase 6 (Emit) to merge author-positioned sections with synthetic ones
/// (see `docs/architecture/ir-schema.md` §Freeform sections for the D9 merge algorithm).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionSpan {
    /// 0-based source line of the `<name>:` header token.
    pub line: u32,
}

#[derive(Clone, Debug)]
pub enum Decl {
    Skill(Spanned<Skill>),
    /// Minimal `export block` placeholder — slice 4 only needs to identify the
    /// declaration shape and its parameter list. Body content (flow, return,
    /// constraints) is parsed structurally but not lowered to IR in slice 4 —
    /// full `export block` lowering ships in slice 7/13.
    ExportBlock(Spanned<ExportBlockDecl>),
    Block(Spanned<BlockDecl>),
    /// `import "<path>" { name1, name2 }` or `import "<path>" as <alias>`.
    Import(Spanned<ImportDecl>),
    /// `const NAME = <literal>` value binding. The sole value-binding decl
    /// post-issue-#81 — supersedes the prior `text NAME = "..."` form by
    /// covering all four primitive kinds (String, Int, Float, Bool).
    Const(Spanned<ConstDecl>),
    /// `type Name = <"…">` — see `TypeDecl`. Compile-time only; lowers to a
    /// type-registry entry (Phase B.5) and is consumed by the emitter's
    /// description-lookup (Phase B.6). Emits nothing directly.
    TypeDecl(Spanned<TypeDecl>),
}

/// An `import` declaration at the top of a source file.
#[derive(Clone, Debug)]
pub struct ImportDecl {
    /// The path string from the source (e.g., `"./prefs.glyph"`).
    pub path: String,
    /// The import form: selective `{ name1, name2 as alias }` or whole-module `as alias`.
    pub kind: ImportKind,
}

/// Selective vs. whole-module import.
#[derive(Clone, Debug)]
pub enum ImportKind {
    /// `import "<path>" { name1, name2 as alias2 }` — named imports.
    Selective(Vec<ImportName>),
    /// `import "<path>" as <alias>` — whole-module import.
    WholeModule { alias: Spanned<String> },
}

/// A single name in a selective import, optionally aliased.
#[derive(Clone, Debug)]
pub struct ImportName {
    /// The name as declared in the imported file. The `Spanned` wrapper carries
    /// the source span of the name token, used by the LSP go-to-def handler
    /// (M2 onwards) to map cursor → import → declaration.
    pub name: Spanned<String>,
    /// Optional local alias (`as <alias>`). The `Spanned` wrapper carries the
    /// source span of the alias token so case-violation diagnostics can pin
    /// the alias rather than the whole import.
    pub alias: Option<Spanned<String>>,
}

#[derive(Clone, Debug)]
pub struct Skill {
    pub name: String,
    /// `description:` body — exactly one quoted string literal in the skeleton.
    pub description: Option<String>,
    /// Header parameters (in source order), added in slice 4.
    pub params: Vec<Param>,
    /// Body-level constraint markers (e.g., `require accuracy`, `avoid stale_references`).
    pub body_constraints: Vec<ConstraintMarker>,
    /// Body-level context markers (e.g., `context project_conventions`).
    pub body_context: Vec<ContextEntry>,
    /// Entries from the `context:` sub-section.
    pub context_section: Vec<ContextEntry>,
    /// Skill-ref entries from the `constraints:` sub-section.
    pub constraints_section: Vec<ContextEntry>,
    /// Inline `effects:` keyword list.
    pub effects: Vec<String>,
    /// Flow statements — inline strings only in the skeleton.
    pub flow: Vec<FlowStmt>,
    /// True iff the source declared a `flow:` sub-section (even if its body was
    /// empty). Used to distinguish a constraint-only skill (legal) from a skill
    /// with an explicitly empty `flow:` (illegal — `G::parse::empty-flow`).
    pub flow_present: bool,
    /// Bare names at body level (indent 1) that don't match any recognized
    /// keyword. Used by analyze to fire `G::analyze::ambiguous-role` when
    /// the name resolves to a `const` declaration.
    pub body_bare_names: Vec<String>,
    /// Optional `-> DomainType` return-type annotation on the header per
    /// `design/language-surface.md` §3.1 line 161. Stored on the AST so
    /// later phases (analyze, lower) can read it; `Skill`-level enforcement
    /// is out of scope for issue #82 (export-block-only — see
    /// `analyze::analyze_export_block`).
    pub return_type: Option<Spanned<String>>,
    /// Recovered duplicate sub-sections (issue #109). The first occurrence of
    /// each sub-section kind populates the corresponding singleton field above
    /// (e.g., `description`, `context_section`); any subsequent occurrence
    /// lands here instead of being silently dropped, so a `glyph fmt` merge
    /// pass can splice them into the singleton without conversion. Per
    /// `design/language-surface.md` §2.5 line 88 and
    /// `docs/architecture/tree-sitter.md` §2.1 line 147.
    pub extra_subsections: Vec<DuplicateSubsection>,
    /// Source line of the `description:` sub-section header. `None` when the
    /// sub-section is absent. Populated by the parser in Phase 3.B; consumed
    /// by Phase 6 (Emit) for the D9 author-positioned vs synthetic merge.
    pub description_span: Option<SectionSpan>,
    /// Source line of the `context:` sub-section header. `None` when absent.
    pub context_section_span: Option<SectionSpan>,
    /// Source line of the `constraints:` sub-section header. `None` when absent.
    pub constraints_section_span: Option<SectionSpan>,
    /// Source line of the `effects:` sub-section header. `None` when absent.
    pub effects_span: Option<SectionSpan>,
    /// Source line of the `flow:` sub-section header. `None` when absent.
    pub flow_span: Option<SectionSpan>,
    /// Colon-keyword sections whose name is not in the catalogue. Empty by
    /// default in Phase 3.A; the parser will populate this in Phase 3.B.
    pub freeform_sections: Vec<FreeformSection>,
}

/// A sub-section that appeared more than once in a `Skill`, `BlockDecl`, or
/// `ExportBlockDecl`. Each variant carries the body content in the same shape
/// the corresponding singleton field uses, so `glyph fmt` can merge a duplicate
/// into the first occurrence without re-parsing or shape conversion (issue
/// #109; `design/language-surface.md` §2.5 line 88;
/// `docs/architecture/tree-sitter.md` §2.1 line 147).
#[derive(Clone, Debug)]
pub enum DuplicateSubsection {
    /// Body of a duplicate `description:` sub-section (a single inline string).
    Description(String),
    /// Body of a duplicate `context:` sub-section.
    Context(Vec<ContextEntry>),
    /// Body of a duplicate `flow:` sub-section.
    Flow(Vec<FlowStmt>),
    /// Body of a duplicate `effects:` keyword list.
    Effects(Vec<String>),
    /// Body of a duplicate `constraints:` sub-section. Carries
    /// `Vec<ConstraintMarker>` to mirror what the singleton path actually
    /// populates today (the parser routes a `constraints:` sub-section's
    /// `require`/`avoid`/`must`/`must avoid` markers into `body_constraints`,
    /// not into the dormant `constraints_section: Vec<ContextEntry>` field —
    /// see decisions.md for the rationale). A future merge can splice these
    /// markers back into `body_constraints` without conversion.
    Constraints(Vec<ConstraintMarker>),
}

/// A colon-keyword section whose name is not in the catalogue (or whose
/// catalogue entry uses `body_grammar = "content_items"` with the default
/// freeform shape). Authored as `quality:`, `acceptance_criteria:`, etc.
/// Built-in `context:` is NOT modeled this way today; in Phase 5 the
/// catalogue migration may unify those paths.
#[derive(Clone, Debug)]
pub struct FreeformSection {
    pub name: String,
    pub span: SectionSpan,
    /// Byte span of the `<name>` header token. Used by analyze-tier
    /// diagnostics (e.g. `G::analyze::duplicate-section`) that need to anchor
    /// on the header position; `span` carries only a line index for the
    /// emit-side D9 merge.
    pub header_span: crate::span::Span,
    pub items: Vec<FreeformItem>,
}

#[derive(Clone, Debug)]
pub enum FreeformItem {
    StringLiteral(Spanned<String>),
    NameRef(Spanned<String>),
    MarkerClause {
        marker: ReservedMarker,
        text: Spanned<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReservedMarker {
    Require,
    Avoid,
    Must,
    MustAvoid,
    Context,
}

/// Minimal `export block` declaration — slice 4 captures the header shape only.
/// Drives same-file and cross-file call-arg validation
/// (`G::analyze::missing-required-arg`).
#[derive(Clone, Debug)]
pub struct ExportBlockDecl {
    pub name: String,
    pub params: Vec<Param>,
    /// Whether the body contains an explicit `return` statement.
    /// Slice 8 needs this to fire `G::analyze::missing-return`.
    pub has_return: bool,
    /// Whether the body contains a `return <expr>` whose `<expr>` is not the
    /// `none` value-keyword. Bare `return` and `return none` both leave this
    /// `false` while `has_return` stays `true`. Issue #82 AC2 uses this
    /// together with `return_type` to fire
    /// `G::analyze::export-missing-return-type` when an export block returns
    /// a meaningful value but its header has no `-> DomainType`.
    pub has_meaningful_return: bool,
    /// Bare-name references found in the body (calls, constraint/context refs).
    /// Used by analyze to detect closure violations: an export block must not
    /// reference private (non-exported, non-parameter) names.
    pub body_refs: Vec<String>,
    /// Body-level constraint markers (e.g., `require accuracy`, `avoid stale_references`).
    /// Mirrors [`BlockDecl::body_constraints`] (issue #166). `constraints:` sub-section
    /// bodies populate this field on first occurrence; duplicates land in
    /// `extra_subsections` as [`DuplicateSubsection::Constraints`].
    pub body_constraints: Vec<ConstraintMarker>,
    /// Body-level context markers (e.g., `context project_conventions`,
    /// `context "..."`). Mirrors [`BlockDecl::body_context`] (issue #166).
    /// `context:` sub-section bodies populate this field on first occurrence;
    /// duplicates land in `extra_subsections` as [`DuplicateSubsection::Context`].
    pub body_context: Vec<ContextEntry>,
    /// Approximate word count of the body content (string literals + identifiers).
    /// Used to decide if the export block should be emitted as a standalone
    /// procedure file (>= 150 words) in Slice 15.
    pub body_word_count: usize,
    /// Description text from `description:` sub-section, if present.
    /// Used for procedure file frontmatter in Tier 3 emission.
    pub description: Option<String>,
    /// Effects declared in `effects:` sub-section.
    /// Used for procedure file frontmatter in Tier 3 emission.
    pub effects: Vec<String>,
    /// Flow statement strings for Tier 3 procedure file emission.
    /// Each entry is the text of a string literal from the `flow:` section.
    pub flow_strings: Vec<String>,
    /// Optional `-> DomainType` return-type annotation on the header per
    /// `design/language-surface.md` §3.3 lines 224/227/230. The
    /// `analyze_export_block` rule in issue #82 fires
    /// `G::analyze::export-missing-return-type` when this is `None` and
    /// `has_meaningful_return` is `true`.
    pub return_type: Option<Spanned<String>>,
    /// Issue #85 chunk 4b (D4): structurally-parsed return expression from
    /// the body's last `return ...` line. Populated last-write-wins over a
    /// flow with multiple `return` statements (the language requires exactly
    /// one per `data-flow.md` §Return Semantics line 401–403). `None` when
    /// the body has no `return` statement at all (the analyze rule
    /// `G::analyze::missing-return` already covers that case via
    /// `has_return: bool`).
    ///
    /// Decoupled from `has_return` / `has_meaningful_return` / `flow_strings`
    /// — those existing fields keep their pre-#85 semantics. This field is
    /// the structural counterpart used by issue-#85 lowering once the
    /// follow-up `IrExportBlock` work lands; it is dormant downstream until
    /// that issue ships.
    pub terminal_return: Option<ReturnExpr>,
    /// B03 GAP 1: non-return call sites collected from the `flow:` section —
    /// standalone root-level calls and calls inside `if`/`elif`/`else` branch
    /// bodies. The terminal `return foo(...)` lives in `terminal_return`.
    /// `analyze_export_block` iterates this vector to fire
    /// `G::analyze::undefined-call` and `G::analyze::missing-required-arg`
    /// on the export-block path, mirroring the FlowStmt::Call resolver.
    pub flow_calls: Vec<FlowCallRef>,
    /// Recovered duplicate sub-sections (issue #109). See
    /// [`Skill::extra_subsections`] for semantics.
    pub extra_subsections: Vec<DuplicateSubsection>,
    /// Source line of the `description:` sub-section header. `None` when
    /// absent. See [`Skill::description_span`].
    pub description_span: Option<SectionSpan>,
    /// Source line of the `context:` sub-section header. `None` when absent.
    pub context_section_span: Option<SectionSpan>,
    /// Source line of the `constraints:` sub-section header. `None` when absent.
    pub constraints_section_span: Option<SectionSpan>,
    /// Source line of the `effects:` sub-section header. `None` when absent.
    pub effects_span: Option<SectionSpan>,
    /// Source line of the `flow:` sub-section header. `None` when absent.
    pub flow_span: Option<SectionSpan>,
    /// Colon-keyword sections whose name is not in the catalogue. See
    /// [`Skill::freeform_sections`].
    pub freeform_sections: Vec<FreeformSection>,
}

/// Single parameter on a `skill`, `block`, or `export block` header.
///
/// Slice-4 surface forms (post-#119, post-A.1):
/// - `name` — bare ident, no annotation, no default
/// - `name: Type` — typed param (issue #119; syntactically reserved only,
///   no resolution yet — see `type_annotation` field)
/// - `name = "default"` — string-literal default
/// - `name = <"description">` — per-param description (issue #119+ Phase A)
/// - any combination of the above (e.g. `name: Type = "default" <"desc">`)
///
/// Defaults are constrained to literal forms in MVP — currently only string
/// literals are accepted (see `language-surface.md` §3.10). The `default`
/// field stores the **rendered** form (with surrounding quotes preserved for
/// string defaults) because that string is what eventually lands in the
/// `## Parameters` compiled-output section.
///
/// `type_annotation` is reserved for future type-system work (no semantic
/// resolution today). `description` is the prose authored alongside the
/// param at the call site (Phase A wires it into the compiled output).
#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    /// Pre-rendered default value (e.g., `"."` including quotes for strings).
    /// `None` means the parameter is required: skill parameters become
    /// runtime-required inputs (rendered in `## Parameters`), while `block`
    /// and `export block` parameters become callee-required positional
    /// arguments — call sites that omit them surface
    /// `G::analyze::missing-required-arg`.
    pub default: Option<String>,
    /// `true` when `default` carries a name reference (e.g. `default_risk`
    /// or `M.foo`) that must resolve to an in-scope `const` at compile
    /// time; `false` for literal defaults (string / number / bool / `none`)
    /// and for parameters with no default. Lower uses this flag to
    /// substitute the referenced const's rendered value (with type-aware
    /// quoting for string consts) instead of leaking the bare identifier
    /// into the IR / `## Parameters` output.
    pub default_is_name_ref: bool,
    /// Optional `name: Type` annotation captured at parse time. Slice 4
    /// (Phase 0) reserves the syntactic position only — the type name is
    /// stored verbatim with its source span and **no** semantic resolution,
    /// validation, hover, completion, or goto-def is wired up. A later type
    /// system tier interprets this field; until then it exists purely so
    /// authors can write the documented `name: Type` form without tripping
    /// the parser. See `design/types.md` and `design/language-surface.md`.
    pub type_annotation: Option<Spanned<String>>,
    /// Per-param description authored as `<"…">` after the `=` slot
    /// (or as `= <"…">` standalone). The `Spanned` wrapper carries the
    /// full descriptive form's span (including the angle brackets).
    /// Wired into the AST in Phase A.2 (parser); Phase A.1 only adds
    /// the field with `None` at every construction site.
    pub description: Option<Spanned<String>>,
    /// Span covering the parameter (header position, used for diagnostic
    /// reporting in slice 4).
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ConstraintMarker {
    /// Raw marker keyword: `require` | `avoid` | `must` | `must avoid`.
    pub marker: ConstraintMarkerKind,
    /// The bare-name reference (e.g., `accuracy`). Resolution happens later.
    /// The `Spanned` wrapper carries the source span of the name token so
    /// the LSP can answer go-to-def (M2).
    pub name: Spanned<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstraintMarkerKind {
    Require,
    Avoid,
    Must,
    MustAvoid,
}

#[derive(Clone, Debug)]
pub enum FlowStmt {
    InlineString(String),
    /// A constraint marker inside `flow:` (e.g., `require X`, `avoid Y`).
    ConstraintMarker(ConstraintMarker),
    /// A `context` marker inside `flow:` (e.g., `context project_conventions`).
    ContextMarker(ContextEntry),
    /// A bare name in `flow:` that is not preceded by a keyword prefix.
    /// Detected during analyze as `G::analyze::text-in-flow`. The `Spanned`
    /// wrapper carries the source span of the name token for go-to-def (M2).
    BareName(Spanned<String>),
    /// A call expression: `name()` or `name(arg1, arg2)`, with optional
    /// `with "modifier"` site modifier. `target` carries the source span of
    /// the callee token so the LSP can answer go-to-def (M2).
    ///
    /// `bound_name = Some(_)` indicates a flow-position assignment
    /// (`<name> = <call>`); the spanned binding name is captured at parse
    /// time. Analyze, lower, and emit consume this to wire flow-local
    /// bindings (see flow-position-assignments design §2-§9).
    Call {
        target: Spanned<String>,
        args: Vec<String>,
        site_modifier: Option<String>,
        bound_name: Option<Spanned<String>>,
    },
    /// `return <expr>` — terminal-only at flow root.
    Return(ReturnExpr),
    /// `if`/`elif`/`else` branch chain.
    Branch {
        condition: String,
        then_body: Vec<FlowStmt>,
        elif_branches: Vec<ElifBranch>,
        else_body: Option<Vec<FlowStmt>>,
        condition_classification: Option<crate::condition::ConditionClassification>,
        /// Spanned identifier tokens from `condition` that are real reference
        /// candidates — receivers of calls, bare-name predicates, or
        /// composed via `not`/`and`/`or`. Excludes the `applies` method-name
        /// token in `.applies()`, any other dotted-method token, and the
        /// boolean operator words. Wired into the resolution table so
        /// goto-def works for imports/locals used only in branch conditions.
        condition_refs: Vec<Spanned<String>>,
    },
}

/// An `elif` arm in a branch chain.
#[derive(Clone, Debug)]
pub struct ElifBranch {
    pub condition: String,
    pub body: Vec<FlowStmt>,
    pub condition_classification: Option<crate::condition::ConditionClassification>,
    /// See [`FlowStmt::Branch::condition_refs`].
    pub condition_refs: Vec<Spanned<String>>,
}

/// The expression following `return`.
#[derive(Clone, Debug)]
pub enum ReturnExpr {
    /// `return none` or bare `return` (no expression).
    None,
    /// `return some_call()`. `target` is `Spanned` for go-to-def (M2).
    Call {
        target: Spanned<String>,
        args: Vec<String>,
    },
    /// `return some_name` (binding reference). `Spanned` for go-to-def (M2).
    Name(Spanned<String>),
    /// `return "inline string"`.
    Inline(String),
    /// `return <IDENT>` — output-target identifier form (issue #85).
    OutputTarget(OutputTargetExpr),
}

/// B03 GAP 1: a non-return call site collected from an export block's
/// `flow:` section — standalone root-level call or call inside an `if` /
/// `elif` / `else` branch body. Terminal `return foo(...)` calls land in
/// [`ExportBlockDecl::terminal_return`] instead and are NOT mirrored here.
/// Mirrors the (`target`, `args`) shape of [`ReturnExpr::Call`].
#[derive(Debug, Clone)]
pub struct FlowCallRef {
    /// Callee name (`foo` in `foo(x, y)`). Spanned for go-to-def parity.
    pub target: Spanned<String>,
    /// Positional arguments by name or inline string. Mirrors
    /// [`ReturnExpr::Call`]'s `args`; argument-count validation runs
    /// against the callee's `Param` list via `validate_call_args`.
    pub args: Vec<String>,
}

/// An entry inside the `context:` sub-section or a body-level `context` marker.
/// Can be a bare-name reference to a `const` declaration or an inline string.
#[derive(Clone, Debug)]
pub enum ContextEntry {
    /// Bare name reference (e.g., `project_conventions`). The `Spanned`
    /// wrapper carries the source span of the name token for go-to-def (M2).
    NameRef(Spanned<String>),
    /// Inline string literal (e.g., `"The bug is reproducible locally."`).
    InlineString(String),
}

/// A private `block` declaration. Also carries `generated block`
/// declarations (per `design/language-surface.md` §3.7) via the
/// `generated` flag — repair-authored blocks share `BlockDecl` since
/// lowering and analysis treat them identically; the flag is metadata.
#[derive(Clone, Debug)]
pub struct BlockDecl {
    pub name: String,
    /// Optional `description:` sub-section.
    pub description: Option<String>,
    /// Body-level constraint markers (e.g., `require accuracy`, `avoid stale_references`).
    /// Mirrors [`Skill::body_constraints`] (issue #165). `constraints:` sub-section
    /// bodies populate this field on first occurrence; duplicates land in
    /// `extra_subsections` as [`DuplicateSubsection::Constraints`].
    pub body_constraints: Vec<ConstraintMarker>,
    /// Body-level context markers (e.g., `context project_conventions`,
    /// `context "..."`). Mirrors [`Skill::body_context`] (issue #165).
    /// `context:` sub-section bodies populate this field on first occurrence;
    /// duplicates land in `extra_subsections` as [`DuplicateSubsection::Context`].
    pub body_context: Vec<ContextEntry>,
    pub params: Vec<Param>,
    /// Inline `effects:` keyword list (same syntax as skill effects).
    pub effects: Vec<String>,
    /// Flow statements — inline strings, calls, etc.
    pub flow: Vec<FlowStmt>,
    /// Optional `-> DomainType` return-type annotation on the header per
    /// `design/language-surface.md` §3.2 line 198. Stored on the AST so
    /// later phases can read it; private-block enforcement is out of scope
    /// for issue #82.
    pub return_type: Option<Spanned<String>>,
    /// Whether this block was declared with `generated` (repair-authored
    /// per `design/language-surface.md` §3.7). Mutually exclusive with
    /// `export block` at the grammar level.
    pub generated: bool,
    /// Recovered duplicate sub-sections (issue #109). See
    /// [`Skill::extra_subsections`] for semantics.
    pub extra_subsections: Vec<DuplicateSubsection>,
    /// Source line of the `description:` sub-section header. `None` when
    /// absent. See [`Skill::description_span`].
    pub description_span: Option<SectionSpan>,
    /// Source line of the `context:` sub-section header. `None` when absent.
    pub context_section_span: Option<SectionSpan>,
    /// Source line of the `constraints:` sub-section header. `None` when absent.
    pub constraints_section_span: Option<SectionSpan>,
    /// Source line of the `effects:` sub-section header. `None` when absent.
    pub effects_span: Option<SectionSpan>,
    /// Source line of the `flow:` sub-section header. `None` when absent.
    pub flow_span: Option<SectionSpan>,
    /// Colon-keyword sections whose name is not in the catalogue. See
    /// [`Skill::freeform_sections`].
    pub freeform_sections: Vec<FreeformSection>,
}

/// `const NAME = <literal>` declaration — unifies value bindings across the
/// four primitive kinds in scope for issue #81 (String, Int, Float, Bool).
///
/// `value` carries the rendered source-text form so the inferer in
/// `crate::kind_infer` can disambiguate Int vs Float by `'.'` presence per
/// `design/values-and-names.md` §Numeric Coercion. String contents are stored
/// without surrounding quotes.
#[derive(Clone, Debug)]
pub struct ConstDecl {
    pub name: String,
    pub value: ConstValue,
    /// Whether this const was declared with `export`.
    pub exported: bool,
    /// Whether this const was declared with `generated` (string-only RHS per
    /// `design/language-surface.md` §3.6). `generated` and `exported` are
    /// mutually exclusive at the grammar level.
    pub generated: bool,
}

/// `type Name = <"…">` top-level declaration. Carries the canonical,
/// importable description for a domain type. Compile-time only — emits
/// nothing into compiled output directly. See spec §6.2.
#[derive(Clone, Debug)]
pub struct TypeDecl {
    pub name: String,
    /// Description content (string literal RHS, quotes stripped, dedent
    /// applied for block strings).
    pub description: Spanned<String>,
    /// Whether this decl was declared with `export`.
    pub exported: bool,
}

/// Rendered literal RHS of a `const` declaration. Each variant carries the
/// source-text slice (with surrounding quotes stripped for `String`) — same
/// shape as `kind_infer::Literal` so adapter is one-to-one.
#[derive(Clone, Debug)]
pub enum ConstValue {
    /// String literal contents (quotes already stripped by the tokenizer).
    String(String),
    /// Integer literal source text — e.g. `"3"`, `"42"`.
    Int(String),
    /// Float literal source text — e.g. `"0.0"`, `"3.14"`.
    Float(String),
    /// Boolean literal source text — e.g. `"true"`, `"True"`, `"TRUE"`.
    /// IR normalizes to lowercase per `design/values-and-names.md` §Booleans;
    /// the AST preserves the original casing.
    Bool(String),
}

impl ConstValue {
    /// Return the rendered source-text form for inline-site substitution.
    /// Raw text without surrounding quotes. For `Bool`, casing is preserved
    /// as authored — IR lowercase normalization (per
    /// `design/values-and-names.md` §Booleans) is applied at the lowering
    /// boundary in `crate::lower::collect_consts`, not here.
    pub fn rendered(&self) -> &str {
        match self {
            ConstValue::String(s)
            | ConstValue::Int(s)
            | ConstValue::Float(s)
            | ConstValue::Bool(s) => s.as_str(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_stmt_branch_carries_optional_condition_classification() {
        let br = FlowStmt::Branch {
            condition: "x".into(),
            then_body: vec![],
            elif_branches: vec![],
            else_body: None,
            condition_classification: None,
            condition_refs: vec![],
        };
        if let FlowStmt::Branch {
            condition_classification,
            ..
        } = br
        {
            assert!(condition_classification.is_none());
        } else {
            panic!("expected Branch");
        }
    }
}
