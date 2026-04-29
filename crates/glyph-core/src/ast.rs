//! Loose AST emitted by the parser (Phase 1).
//!
//! Names are unresolved, types unchecked, roles unassigned.
//! Walking-skeleton subset — covers the constructs in `update_docs.glyph.md`.

use crate::span::{Span, Spanned};

/// One source file's parsed declarations, in source order.
#[derive(Clone, Debug)]
pub struct SourceFile {
    pub decls: Vec<Decl>,
}

#[derive(Clone, Debug)]
pub enum Decl {
    Skill(Spanned<Skill>),
    Text(Spanned<TextDecl>),
    /// Minimal `export block` placeholder — slice 4 only needs to identify the
    /// declaration shape and its parameter list so it can validate
    /// `G::analyze::missing-param-default`. Body content (flow, return,
    /// constraints) is parsed structurally but not lowered to IR in slice 4 —
    /// full `export block` lowering ships in slice 7/13.
    ExportBlock(Spanned<ExportBlockDecl>),
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
    /// the name resolves to a `text` declaration.
    pub body_bare_names: Vec<String>,
}

/// Minimal `export block` declaration — slice 4 captures the header shape only.
/// Used to surface `G::analyze::missing-param-default` (export-block-only rule).
#[derive(Clone, Debug)]
pub struct ExportBlockDecl {
    pub name: String,
    pub params: Vec<Param>,
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
}

/// An entry inside the `context:` sub-section or a body-level `context` marker.
/// Can be a bare-name reference to a `text` declaration or an inline string.
#[derive(Clone, Debug)]
pub enum ContextEntry {
    /// Bare name reference (e.g., `project_conventions`).
    NameRef(String),
    /// Inline string literal (e.g., `"The bug is reproducible locally."`).
    InlineString(String),
}

#[derive(Clone, Debug)]
pub struct TextDecl {
    pub name: String,
    pub value: String,
}
