//! Pure validator for identifiers in type position.
//!
//! Answers a single question: is this identifier one of the banned generic
//! type names that author-facing source must avoid (per issue #83 AC3), or is
//! it a legitimate domain-type name?
//!
//! Pure deep module — no dependencies on `ast`, `parse`, `lower`, `ir`,
//! `emit`, or `emit_ir`. The caller is responsible for identifier
//! well-formedness; this module's sole concern is the banned-vs-not check.
//! Wired into `analyze.rs` in chunk 2 of the slate.

/// A validated, non-banned domain-type name as written in source.
///
/// Construction is gated through [`validate_type_position`]; the inner string
/// is the verbatim source identifier (no canonicalization).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainType(String);

impl DomainType {
    /// The verbatim source identifier.
    pub fn name(&self) -> &str {
        &self.0
    }
}

/// Banned-generic warning value produced by [`validate_type_position`].
///
/// Carries everything needed to construct a `Diagnostic` at the call site,
/// but holds no `SourceSpan` — this module is pure. The caller (chunk 2)
/// owns the span and lifts this into a tier-`warning` diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    /// Diagnostic id; equals
    /// [`crate::diagnostic::GENERIC_TYPE_NAME_DIAG_ID`].
    pub id: &'static str,
    /// The offending identifier exactly as written in source (no
    /// canonicalization — even if a case-folded match triggered the warning).
    pub offending: String,
    /// Human-readable warning message naming the offending identifier.
    pub message: String,
    /// Suggestion text pointing the author at domain-type alternatives.
    pub hint: String,
}

/// Banned generic type names, per issue #83 AC3.
const BANNED_GENERIC_NAMES: &[&str] = &[
    "String", "Int", "Float", "Bool", "None", "List", "Set", "Map", "Array",
    "Dict", "Tuple", "Object", "Any",
];

/// Validate an identifier in type position.
///
/// Returns `Ok(DomainType)` when `ident` is not on the banned-generic list
/// (case-insensitive ASCII match), and `Err(Warning)` otherwise.
pub fn validate_type_position(ident: &str) -> Result<DomainType, Warning> {
    if BANNED_GENERIC_NAMES.iter().any(|b| b.eq_ignore_ascii_case(ident)) {
        return Err(Warning {
            id: crate::diagnostic::GENERIC_TYPE_NAME_DIAG_ID,
            offending: ident.to_string(),
            message: format!(
                "`{}` is a generic type name; use a domain-type instead",
                ident
            ),
            hint: "use a domain-type — e.g. `BranchName`, `FilePath`, `Summary`".to_string(),
        });
    }
    Ok(DomainType(ident.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::GENERIC_TYPE_NAME_DIAG_ID;

    #[test]
    fn valid_domain_name_returns_ok() {
        let result = validate_type_position("BranchName");
        match result {
            Ok(d) => assert_eq!(d.name(), "BranchName"),
            Err(w) => panic!("expected Ok, got Err({:?})", w),
        }
    }

    #[test]
    fn banned_string_returns_warning_with_id_and_offender() {
        let result = validate_type_position("String");
        let w = match result {
            Err(w) => w,
            Ok(d) => panic!("expected Err, got Ok({:?})", d),
        };
        assert_eq!(w.id, GENERIC_TYPE_NAME_DIAG_ID);
        assert_eq!(w.offending, "String");
        assert!(
            w.message.contains("String"),
            "message should contain the offending identifier verbatim, got: {:?}",
            w.message
        );
        let suggested_examples = ["BranchName", "FilePath", "Summary"];
        assert!(
            suggested_examples.iter().any(|e| w.hint.contains(e)),
            "hint should mention at least one of {:?}, got: {:?}",
            suggested_examples,
            w.hint
        );
    }

    #[test]
    fn all_thirteen_banned_names_flagged() {
        // AC3: full banned list (13 names) per issue #83.
        let banned = [
            "String", "Int", "Float", "Bool", "None", "List", "Set", "Map",
            "Array", "Dict", "Tuple", "Object", "Any",
        ];
        for name in banned {
            let result = validate_type_position(name);
            let w = match result {
                Err(w) => w,
                Ok(d) => panic!("expected Err for banned name `{}`, got Ok({:?})", name, d),
            };
            assert_eq!(
                w.offending, name,
                "offending should echo the input verbatim for banned name `{}`",
                name
            );
        }
    }

    #[test]
    fn case_folded_banned_names_match() {
        // Case-insensitive ASCII match per `values-and-names.md` §Case
        // Normalization (mirrors codebase convention `eq_ignore_ascii_case`).
        // Underscore-strip is design-TBD and not part of this validator.
        for variant in ["string", "STRING", "StRiNg"] {
            let result = validate_type_position(variant);
            assert!(
                result.is_err(),
                "case variant `{}` should match banned `String`, got Ok",
                variant
            );
        }
    }

    #[test]
    fn verbatim_offender_in_warning_when_case_folded() {
        // Survives a refactor that mistakenly canonicalizes the offender.
        let result = validate_type_position("string");
        let w = match result {
            Err(w) => w,
            Ok(d) => panic!("expected Err for `string`, got Ok({:?})", d),
        };
        assert_eq!(
            w.offending, "string",
            "offending must echo the source identifier verbatim, not the canonical banned name"
        );
    }

    #[test]
    fn banned_match_does_not_apply_to_substring() {
        // Guards against a normalization-too-aggressive bug; banned check
        // must be exact (modulo case), not substring.
        let result = validate_type_position("StringList");
        match result {
            Ok(d) => assert_eq!(d.name(), "StringList"),
            Err(w) => panic!(
                "`StringList` should not match banned `String` or `List` as a substring, got Err({:?})",
                w
            ),
        }
    }

    #[test]
    fn empty_identifier_returns_ok() {
        // The validator's narrow contract: it answers banned-vs-not.
        // Identifier well-formedness is the caller's responsibility.
        let result = validate_type_position("");
        match result {
            Ok(d) => assert_eq!(d.name(), ""),
            Err(w) => panic!("expected Ok for empty identifier, got Err({:?})", w),
        }
    }

    #[test]
    fn agent_is_not_banned() {
        // `Agent` is a legitimate IR-internal `TypeTag` variant for stdlib
        // `subagent()`; explicitly not on the banned list (issue #83 AC3).
        let result = validate_type_position("Agent");
        match result {
            Ok(d) => assert_eq!(d.name(), "Agent"),
            Err(w) => panic!("`Agent` must not be banned, got Err({:?})", w),
        }
    }

    #[test]
    fn several_valid_domain_names_return_ok() {
        // Belt-and-suspenders on the happy path: catches a too-loose banned check.
        for name in ["BranchName", "RepoContext", "Plan", "Diagnosis"] {
            let result = validate_type_position(name);
            match result {
                Ok(d) => assert_eq!(d.name(), name),
                Err(w) => panic!("expected Ok for valid domain name `{}`, got Err({:?})", name, w),
            }
        }
    }
}
