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
    "String", "Int", "Float", "Bool", "None", "List", "Set", "Map", "Array", "Dict", "Tuple",
    "Object", "Any",
];

/// Validate an identifier in type position.
///
/// Returns `Ok(DomainType)` when `ident` is not on the banned-generic list,
/// and `Err(Warning)` otherwise. Match is by D6 canonical form — ASCII-
/// lowercase + strip `_` per `values-and-names.md §Case Normalization`,
/// shared via [`crate::domain_registry::canonicalize_identifier`]. Issue
/// #84 codex pass 5: pre-fix this used raw `eq_ignore_ascii_case` only,
/// so underscore-perturbed spellings (`S_t_r_i_n_g`, `I_n_t`, …) bypassed
/// the banned check while still lowering as the corresponding built-in
/// `TypeTag` — analyze and lower disagreed with the validator on what the
/// spelling meant. Closes the D6 propagation triangle:
/// `lower::name_to_typetag` ↔ `analyze::is_builtin_type_name` ↔ this site.
pub fn validate_type_position(ident: &str) -> Result<DomainType, Warning> {
    let canonical = crate::domain_registry::canonicalize_identifier(ident);
    if BANNED_GENERIC_NAMES
        .iter()
        .any(|b| b.eq_ignore_ascii_case(&canonical))
    {
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
            "String", "Int", "Float", "Bool", "None", "List", "Set", "Map", "Array", "Dict",
            "Tuple", "Object", "Any",
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

    // --- Issue #84 codex pass 5 — D6 canonicalization in banned-list match.
    // Pre-pass-5: the validator used `eq_ignore_ascii_case` only, so an
    // underscore-perturbed spelling like `S_t_r_i_n_g` (which canonicalizes
    // to `string`) slipped past the banned check. Combined with pass-3's
    // D6 fix in `lower::name_to_typetag` (lowers as built-in `TypeTag::String`)
    // and `analyze::is_builtin_type_name` (skips collision-sweep), authors
    // could bypass #83's `generic-type-name` warning by inserting underscores
    // — and the analyzer / lowering disagreed on what the spelling meant.
    // Closes the D6 propagation triangle: lower ↔ analyze ↔ type_position. ---

    #[test]
    fn t28_underscore_agent_does_not_fire_generic_type_name() {
        // Codex pass 5 — AC-pass5-3 negative pin. `Agent` is explicitly
        // NOT on the banned list per `agent_is_not_banned` and the
        // `BANNED_GENERIC_NAMES` literal. Underscore-perturbed
        // `A_g_e_n_t` canonicalizes to `agent` and must therefore also
        // not fire — pass-3 already wired this for the lower / analyze
        // sides; pass 5 confirms the third leg of the D6 triangle stays
        // consistent (canonicalization doesn't drag Agent into the
        // banned set by accident).
        let result = validate_type_position("A_g_e_n_t");
        match result {
            Ok(d) => assert_eq!(d.name(), "A_g_e_n_t"),
            Err(w) => panic!(
                "`A_g_e_n_t` is not banned (Agent not on banned list); got Err({:?})",
                w
            ),
        }
    }

    #[test]
    fn t27_underscore_plan_does_not_fire_generic_type_name() {
        // Codex pass 5 — AC-pass5-5 negative pin. A genuine domain-type
        // spelling like `P_l_a_n` (canonical `plan`) must NOT match the
        // banned list — `plan` is not in BANNED_GENERIC_NAMES. Guards
        // against an over-aggressive canonicalization that mistakenly
        // matches domain types onto banned built-ins.
        let result = validate_type_position("P_l_a_n");
        match result {
            Ok(d) => assert_eq!(d.name(), "P_l_a_n"),
            Err(w) => panic!("`P_l_a_n` is a domain-type spelling and must not fire generic-type-name; got Err({:?})", w),
        }
    }

    #[test]
    fn t26_plain_string_still_fires_generic_type_name() {
        // Codex pass 5 — AC-pass5-4 regression pin. Plain `String` (no
        // underscores) must still fire — the canonicalization fix must
        // not break the existing #83 surface. Existing
        // `banned_string_returns_warning_with_id_and_offender` covers
        // the same shape; this test exists per planner contract to
        // explicitly anchor the regression in the pass-5 test set.
        let result = validate_type_position("String");
        let w = match result {
            Err(w) => w,
            Ok(d) => panic!("expected Err for plain `String`, got Ok({:?})", d),
        };
        assert_eq!(w.id, GENERIC_TYPE_NAME_DIAG_ID);
        assert_eq!(w.offending, "String");
    }

    #[test]
    fn t25_underscore_int_fires_generic_type_name() {
        // Codex pass 5 — AC-pass5-2. Generic application of the
        // canonicalization fix to a second built-in. `I_n_t`
        // canonicalizes to `int` and must match banned `Int`. Catches
        // a regression that special-cases one variant only (e.g. by
        // pattern-matching `String` instead of running through the
        // shared `canonicalize_identifier` helper).
        let result = validate_type_position("I_n_t");
        let w = match result {
            Err(w) => w,
            Ok(d) => panic!("expected Err for `I_n_t`, got Ok({:?})", d),
        };
        assert_eq!(w.id, GENERIC_TYPE_NAME_DIAG_ID);
        assert_eq!(w.offending, "I_n_t");
    }

    #[test]
    fn t24_underscore_string_fires_generic_type_name() {
        // Codex pass 5 — AC-pass5-1 tracer. `S_t_r_i_n_g` canonicalizes
        // to `string` per D6 and must match banned `String`. Pre-fix,
        // `eq_ignore_ascii_case` did not strip underscores → the banned
        // check missed and the warning was lost.
        let result = validate_type_position("S_t_r_i_n_g");
        let w = match result {
            Err(w) => w,
            Ok(d) => panic!("expected Err for `S_t_r_i_n_g`, got Ok({:?})", d),
        };
        assert_eq!(w.id, GENERIC_TYPE_NAME_DIAG_ID);
        assert_eq!(
            w.offending, "S_t_r_i_n_g",
            "offending must echo the source identifier verbatim, not the canonical form"
        );
    }

    #[test]
    fn several_valid_domain_names_return_ok() {
        // Belt-and-suspenders on the happy path: catches a too-loose banned check.
        for name in ["BranchName", "RepoContext", "Plan", "Diagnosis"] {
            let result = validate_type_position(name);
            match result {
                Ok(d) => assert_eq!(d.name(), name),
                Err(w) => panic!(
                    "expected Ok for valid domain name `{}`, got Err({:?})",
                    name, w
                ),
            }
        }
    }
}
