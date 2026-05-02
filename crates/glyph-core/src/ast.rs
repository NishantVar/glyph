//! Loose AST emitted by the parser (Phase 1).
//!
//! Names are unresolved, types unchecked, roles unassigned.
//! Walking-skeleton subset — covers the constructs in `update_docs.glyph.md`.

use crate::output_target::OutputTargetExpr;
use crate::span::{Span, Spanned};

/// One source file's parsed declarations, in source order.
#[derive(Clone, Debug)]
pub struct SourceFile {
    pub decls: Vec<Decl>,
}

#[derive(Clone, Debug)]
pub enum Decl {
    Skill(Spanned<Skill>),
    /// Minimal `export block` placeholder — slice 4 only needs to identify the
    /// declaration shape and its parameter list so it can validate
    /// `G::analyze::missing-param-default`. Body content (flow, return,
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
}

/// An `import` declaration at the top of a source file.
#[derive(Clone, Debug)]
pub struct ImportDecl {
    /// The path string from the source (e.g., `"./prefs.glyph.md"`).
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
    WholeModule { alias: String },
}

/// A single name in a selective import, optionally aliased.
#[derive(Clone, Debug)]
pub struct ImportName {
    /// The name as declared in the imported file.
    pub name: String,
    /// Optional local alias (`as <alias>`).
    pub alias: Option<String>,
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
}

/// Minimal `export block` declaration — slice 4 captures the header shape only.
/// Used to surface `G::analyze::missing-param-default` (export-block-only rule).
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
}

/// A header parameter on `skill`, `block`, or `export block`.
///
/// Slice 4 supports the two MVP forms `name` (no default) and `name = "default"`.
/// Type annotations are deferred. Defaults are constrained to literal forms in
/// MVP — currently only string literals are accepted (see `language-surface.md`
/// §3.10). The original literal text of the default — *with surrounding quotes
/// preserved* for string defaults — is what eventually lands in the
/// `## Parameters` section, so we store the rendered form here.
#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    /// Pre-rendered default value (e.g., `"."` including quotes for strings).
    /// `None` means the parameter is runtime-required (skills) or triggers
    /// `G::analyze::missing-param-default` (export blocks).
    pub default: Option<String>,
    /// Span covering the parameter (header position, used for diagnostic
    /// reporting in slice 4).
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ConstraintMarker {
    /// Raw marker keyword: `require` | `avoid` | `must` | `must avoid`.
    pub marker: ConstraintMarkerKind,
    /// The bare-name reference (e.g., `accuracy`). Resolution happens later.
    pub name: String,
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
    /// Detected during analyze as `G::analyze::text-in-flow`.
    BareName(String),
    /// A call expression: `name()` or `name(arg1, arg2)`, with optional
    /// `with "modifier"` site modifier.
    Call { target: String, args: Vec<String>, site_modifier: Option<String> },
    /// `return <expr>` — terminal-only at flow root.
    Return(ReturnExpr),
    /// `if`/`elif`/`else` branch chain.
    Branch {
        condition: String,
        then_body: Vec<FlowStmt>,
        elif_branches: Vec<ElifBranch>,
        else_body: Option<Vec<FlowStmt>>,
    },
}

/// An `elif` arm in a branch chain.
#[derive(Clone, Debug)]
pub struct ElifBranch {
    pub condition: String,
    pub body: Vec<FlowStmt>,
}

/// The expression following `return`.
#[derive(Clone, Debug)]
pub enum ReturnExpr {
    /// `return none` or bare `return` (no expression).
    None,
    /// `return some_call()`.
    Call { target: String, args: Vec<String> },
    /// `return some_name` (binding reference).
    Name(String),
    /// `return "inline string"`.
    Inline(String),
    /// `return <IDENT>` — output-target identifier form (issue #85).
    OutputTarget(OutputTargetExpr),
}

/// An entry inside the `context:` sub-section or a body-level `context` marker.
/// Can be a bare-name reference to a `const` declaration or an inline string.
#[derive(Clone, Debug)]
pub enum ContextEntry {
    /// Bare name reference (e.g., `project_conventions`).
    NameRef(String),
    /// Inline string literal (e.g., `"The bug is reproducible locally."`).
    InlineString(String),
}

/// A private `block` declaration.
#[derive(Clone, Debug)]
pub struct BlockDecl {
    pub name: String,
    /// Optional `description:` sub-section.
    pub description: Option<String>,
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
