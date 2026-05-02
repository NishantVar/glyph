//! Per-file domain-type registry (issue #84).
//!
//! Pure deep module: no I/O, no global state, no diagnostics. Stores the
//! first-use span for each domain-type identifier seen in a single source
//! file, keyed by its canonical form. Identifier canonicalization follows
//! `design/values-and-names.md` §Case Normalization (D6): ASCII-lowercase,
//! then strip `_`. So `makePlan` / `make_plan` / `MakePlan` / `MAKE_PLAN`
//! all map to the canonical key `makeplan`.
//!
//! Cross-file matching at call boundaries is canonicalized-string equality
//! (Chunk 4); this chunk only owns the per-file data structure and the
//! shared canonicalizer.

use crate::span::Span;
use std::collections::HashMap;

/// One entry in a per-file [`Registry`]. Stores the canonical form of the
/// identifier (the form that lands in IR per F12) plus the source span of
/// the *first* use that introduced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryEntry {
    pub canonical_name: String,
    pub first_use_span: Span,
}

/// Per-file domain-type registry.
#[derive(Debug, Default)]
pub struct Registry {
    entries: HashMap<String, RegistryEntry>, // key == canonical_name
}

impl Registry {
    /// Construct an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register the first use of a domain-type identifier. Idempotent on
    /// canonical form: a repeat call with any case/underscore variant of an
    /// already-registered name returns a borrow of the existing entry, with
    /// its original `first_use_span` preserved.
    pub fn register_first_use(&mut self, raw_name: &str, span: Span) -> &RegistryEntry {
        let canonical = canonicalize_identifier(raw_name);
        self.entries
            .entry(canonical.clone())
            .or_insert(RegistryEntry {
                canonical_name: canonical,
                first_use_span: span,
            })
    }

    /// Look up an entry by any case/underscore variant of its identifier.
    /// Returns `Some` iff `register_first_use` has previously been called
    /// with a name that canonicalizes to the same key.
    pub fn lookup(&self, raw_name: &str) -> Option<&RegistryEntry> {
        self.entries.get(&canonicalize_identifier(raw_name))
    }

    /// Pure canonicalized-string equality between two raw author identifiers.
    /// Reads no registry state; method form is for discoverability per D1.
    pub fn nominal_match(&self, a: &str, b: &str) -> bool {
        canonicalize_identifier(a) == canonicalize_identifier(b)
    }
}

/// Canonicalize an identifier per D6: ASCII-lowercase then strip `_`.
pub fn canonicalize_identifier(s: &str) -> String {
    s.chars()
        .filter(|c| *c != '_')
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(file_id: u32, start: u32, end: u32) -> Span {
        Span::new(file_id, start, end)
    }

    #[test]
    fn t1_canonicalize_make_plan_tracer() {
        assert_eq!(canonicalize_identifier("makePlan"), "makeplan");
    }

    #[test]
    fn t2_canonicalize_worked_example_all_variants() {
        // Per `design/values-and-names.md` §Case Normalization (D6):
        // these four spellings of the same identifier must all canonicalize
        // to the same key so the registry treats them as the same name.
        for variant in ["makePlan", "make_plan", "MakePlan", "MAKE_PLAN"] {
            assert_eq!(
                canonicalize_identifier(variant),
                "makeplan",
                "variant `{}` did not canonicalize to `makeplan`",
                variant
            );
        }
    }

    #[test]
    fn t3_canonicalize_is_idempotent() {
        // Applying canonicalize twice must equal applying it once. Guards
        // against a future change that adds non-idempotent transformations.
        for raw in ["makePlan", "Report", "REPORT", "re_port", "BranchName", ""] {
            let once = canonicalize_identifier(raw);
            let twice = canonicalize_identifier(&once);
            assert_eq!(
                once, twice,
                "canonicalize is not idempotent for raw `{}`: once=`{}` twice=`{}`",
                raw, once, twice
            );
        }
    }

    #[test]
    fn t4_register_first_use_returns_canonical_entry_with_span() {
        let mut reg = Registry::new();
        let s = span(0, 10, 16);
        let entry = reg.register_first_use("Report", s);
        assert_eq!(entry.canonical_name, "report");
        assert_eq!(entry.first_use_span, s);
    }

    #[test]
    fn t5_repeat_register_preserves_first_use_span() {
        // AC3: registering the same canonical name twice returns the
        // original entry with its first_use_span unchanged. The second
        // call's span is silently discarded (it's not the *first* use).
        let mut reg = Registry::new();
        let span_a = span(0, 10, 16);
        let span_b = span(0, 42, 48);
        reg.register_first_use("Report", span_a);
        let entry_b = reg.register_first_use("report", span_b);
        assert_eq!(entry_b.canonical_name, "report");
        assert_eq!(
            entry_b.first_use_span, span_a,
            "repeat-register must keep the first span, not overwrite with the second"
        );
    }

    #[test]
    fn t6_lookup_hits_across_canonicalized_variants() {
        // Per F12 / D6: registry stores the canonical form; lookup matches
        // by canonicalization, not raw spelling. Asserts both the canonical
        // form lands in the entry AND the first span is recoverable through
        // any case/underscore variant.
        let mut reg = Registry::new();
        let s = span(0, 5, 11);
        reg.register_first_use("Report", s);
        for variant in ["report", "REPORT", "re_port", "Report"] {
            let hit = reg
                .lookup(variant)
                .unwrap_or_else(|| panic!("lookup(`{}`) returned None", variant));
            assert_eq!(
                hit.canonical_name, "report",
                "lookup(`{}`) must surface the stored canonical name `report`",
                variant
            );
            assert_eq!(
                hit.first_use_span, s,
                "lookup(`{}`) must surface the original first_use_span",
                variant
            );
        }
    }

    #[test]
    fn t7_lookup_miss_returns_none() {
        // Empty registry: any lookup returns None.
        let mut reg = Registry::new();
        assert!(reg.lookup("anything").is_none());
        // Populated registry: a different name still misses.
        reg.register_first_use("Report", span(0, 5, 11));
        assert!(reg.lookup("Diagnosis").is_none());
        assert!(reg.lookup("diagnosis").is_none());
    }

    #[test]
    fn t8_nominal_match_true_for_case_and_underscore_variants() {
        let reg = Registry::new();
        assert!(reg.nominal_match("Report", "report"));
        assert!(reg.nominal_match("makePlan", "make_plan"));
        assert!(reg.nominal_match("MAKE_PLAN", "MakePlan"));
    }

    #[test]
    fn t9_nominal_match_false_for_distinct_names() {
        let reg = Registry::new();
        assert!(!reg.nominal_match("Report", "Diagnosis"));
    }

    #[test]
    fn t10_canonicalize_handles_empty_and_underscore_only() {
        // Degenerate inputs canonicalize without panic. The registry is
        // pure data — identifier well-formedness is the caller's concern
        // (parse/analyze gate upstream).
        assert_eq!(canonicalize_identifier(""), "");
        assert_eq!(canonicalize_identifier("___"), "");
    }
}
