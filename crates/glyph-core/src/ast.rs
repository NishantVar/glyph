//! Loose AST emitted by the parser (Phase 1).
//!
//! Names are unresolved, types unchecked, roles unassigned.
//! Walking-skeleton subset — covers the constructs in `update_docs.glyph.md`.

use crate::span::Spanned;

/// One source file's parsed declarations, in source order.
#[derive(Clone, Debug)]
pub struct SourceFile {
    pub decls: Vec<Decl>,
}

#[derive(Clone, Debug)]
pub enum Decl {
    Skill(Spanned<Skill>),
    Text(Spanned<TextDecl>),
}

#[derive(Clone, Debug)]
pub struct Skill {
    pub name: String,
    /// `description:` body — exactly one quoted string literal in the skeleton.
    pub description: Option<String>,
    /// Body-level constraint markers (e.g., `require accuracy`, `avoid stale_references`).
    pub body_constraints: Vec<ConstraintMarker>,
    /// Inline `effects:` keyword list.
    pub effects: Vec<String>,
    /// Flow statements — inline strings only in the skeleton.
    pub flow: Vec<FlowStmt>,
    /// True iff the source declared a `flow:` sub-section (even if its body was
    /// empty). Used to distinguish a constraint-only skill (legal) from a skill
    /// with an explicitly empty `flow:` (illegal — `G::parse::empty-flow`).
    pub flow_present: bool,
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
}

#[derive(Clone, Debug)]
pub struct TextDecl {
    pub name: String,
    pub value: String,
}
