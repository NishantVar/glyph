//! Type vs. value namespace tag, and case predicates.
//!
//! Case rules (spec §"Case enforcement"):
//! - Type namespace: strict PascalCase. First char uppercase ASCII letter;
//!   rest letters or digits; no underscores.
//! - Value namespace: strict snake_case. First char lowercase ASCII letter
//!   or underscore; rest lowercase letters, digits, underscores.

/// Namespace kind for a resolved import alias.
///
/// Named `ResolvedImportKind` to avoid collision with the existing
/// `ast::ImportKind { Selective, WholeModule }` which describes the syntactic
/// import form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolvedImportKind {
    Type,
    Value,
}

/// True iff `s` is strict PascalCase per the spec.
pub fn is_pascal_case(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_uppercase()) {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric())
}

/// True iff `s` is strict snake_case per the spec.
pub fn is_snake_case(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_lowercase()) {
        return false;
    }
    chars.all(|c| c == '_' || c.is_ascii_lowercase() || c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascal_case_accepts_typical() {
        assert!(is_pascal_case("LinkMode"));
        assert!(is_pascal_case("Foo"));
        assert!(is_pascal_case("F"));
        assert!(is_pascal_case("HTTP2"));
    }

    #[test]
    fn pascal_case_rejects_snake_and_mixed() {
        assert!(!is_pascal_case("link_mode"));
        assert!(!is_pascal_case("linkMode"));
        assert!(!is_pascal_case("Link_Mode"));
        assert!(!is_pascal_case("link"));
        assert!(!is_pascal_case(""));
        assert!(!is_pascal_case("_LinkMode"));
    }

    #[test]
    fn snake_case_accepts_typical() {
        assert!(is_snake_case("link_mode"));
        assert!(is_snake_case("foo"));
        assert!(is_snake_case("_internal"));
        assert!(is_snake_case("repo_root_2"));
        assert!(is_snake_case("a"));
    }

    #[test]
    fn snake_case_rejects_pascal_and_mixed() {
        assert!(!is_snake_case("LinkMode"));
        assert!(!is_snake_case("linkMode"));
        assert!(!is_snake_case("LINK_MODE"));
        assert!(!is_snake_case(""));
        assert!(!is_snake_case("2foo"));
    }
}
