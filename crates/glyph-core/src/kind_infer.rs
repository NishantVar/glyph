//! Primitive-kind type inference for literal RHS values.
//!
//! Pure module — no dependencies on `ast`, `parse`, `lower`, `ir`, `emit`, or
//! `emit_ir`. Consumes a small [`Literal`] enum that downstream wiring (chunks
//! 2+ in the `[type-system-simplification]` slate) will populate from parsed
//! expressions, and returns a [`TypeTag`].
//!
//! Out of scope for this module:
//! - [`TypeTag::None`] — produced by explicit `-> None` annotations (#82).
//! - [`TypeTag::Agent`] — produced by agent-typed positions (later slate work).
//! - [`TypeTag::DomainType`] — produced by the domain-type registry (#84).
//!
//! Single-source-of-truth note: the [`TypeTag`] enum mirrors the canonical
//! design enum in full (`design/ir-schema.md` §Enums) so it is reusable across
//! sibling issues without re-definition. The inferer's bounded output set
//! (only the four primitive variants) is documented on
//! [`infer_primitive`], not enforced by the type.

/// Type tag for values and parameter slots.
///
/// Mirrors the canonical enum from `design/ir-schema.md` §Enums:
/// `String | Int | Float | Bool | None | Agent | DomainType(name)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeTag {
    String,
    Int,
    Float,
    Bool,
    None,
    Agent,
    DomainType(String),
}

/// Literal value handed to the primitive-kind inferer.
///
/// Variants carry source-text strings (mirroring the `ast::Param.default`
/// pattern) so the inferer can perform `Int` vs `Float` disambiguation by
/// textual form, per `design/values-and-names.md` §Numeric Coercion.
///
/// - `String(s)` — string literal; `s` is the raw author text (quotes preserved
///   when chunk 2 wires this up, but the inferer does not inspect contents).
/// - `Number(s)` — integer-or-float literal; `'.'` presence picks `Float`.
/// - `Bool(s)` — boolean literal; case-insensitive `true`/`false` per
///   `design/values-and-names.md` §Booleans. The inferer always returns
///   [`TypeTag::Bool`]; lowercase normalization for IR is a downstream concern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Literal {
    String(String),
    Number(String),
    Bool(String),
}

/// Infer the primitive [`TypeTag`] for a literal RHS.
///
/// Always returns one of [`TypeTag::String`], [`TypeTag::Int`],
/// [`TypeTag::Float`], or [`TypeTag::Bool`]. Never produces [`TypeTag::None`],
/// [`TypeTag::Agent`], or [`TypeTag::DomainType`] — those originate from
/// type-position annotations or the domain-type registry, not from literal
/// RHS, and are owned by sibling issues #82+ in the slate.
pub fn infer_primitive(literal: &Literal) -> TypeTag {
    match literal {
        Literal::String(_) => TypeTag::String,
        Literal::Number(text) => {
            if text.contains('.') {
                TypeTag::Float
            } else {
                TypeTag::Int
            }
        }
        Literal::Bool(_) => TypeTag::Bool,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_literal_infers_string() {
        assert_eq!(
            infer_primitive(&Literal::String("\"hello\"".into())),
            TypeTag::String
        );
    }

    #[test]
    fn integer_literal_infers_int() {
        // `values-and-names.md` §Numeric Coercion: no `'.'` → Int.
        assert_eq!(
            infer_primitive(&Literal::Number("3".into())),
            TypeTag::Int
        );
    }

    #[test]
    fn float_literal_infers_float() {
        // `values-and-names.md` §Numeric Coercion: `'.'` present → Float.
        assert_eq!(
            infer_primitive(&Literal::Number("3.0".into())),
            TypeTag::Float
        );
    }

    #[test]
    fn bool_true_infers_bool() {
        assert_eq!(
            infer_primitive(&Literal::Bool("true".into())),
            TypeTag::Bool
        );
    }

    #[test]
    fn bool_false_infers_bool() {
        assert_eq!(
            infer_primitive(&Literal::Bool("false".into())),
            TypeTag::Bool
        );
    }

    #[test]
    fn bool_mixed_case_infers_bool() {
        // `values-and-names.md` §Booleans: case-insensitive on input.
        assert_eq!(
            infer_primitive(&Literal::Bool("True".into())),
            TypeTag::Bool
        );
        assert_eq!(
            infer_primitive(&Literal::Bool("TRUE".into())),
            TypeTag::Bool
        );
    }

    #[test]
    fn numeric_disambiguation_by_dot_presence() {
        // Sweep additional forms on the disambiguation rule.
        assert_eq!(
            infer_primitive(&Literal::Number("0".into())),
            TypeTag::Int
        );
        assert_eq!(
            infer_primitive(&Literal::Number("0.0".into())),
            TypeTag::Float
        );
        assert_eq!(
            infer_primitive(&Literal::Number("42".into())),
            TypeTag::Int
        );
        assert_eq!(
            infer_primitive(&Literal::Number("3.14".into())),
            TypeTag::Float
        );
    }
}
