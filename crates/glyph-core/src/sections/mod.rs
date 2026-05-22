//! Section catalogue. Compile-time-embedded TOML describing each built-in
//! sub-section (heading, gating, ordering slot, body grammar, hooks).
//!
//! Entries in `catalogue.toml` are *exceptions* to freeform defaults. A
//! colon-keyword the catalogue doesn't claim falls through to freeform
//! behavior (`## TitleCase` heading, content-item body grammar, all 5
//! reserved markers allowed, source-position anchored).
//!
//! See `design/glyph-freeform-sections-design-2026-05-12.md` §4.2.

pub mod hooks;

use serde::Deserialize;
use std::collections::BTreeMap;

const CATALOGUE_TOML: &str = include_str!("catalogue.toml");

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CatalogueEntry {
    #[serde(default)]
    pub heading: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub canonical_slot: Option<u32>,
    #[serde(default)]
    pub cardinality: Option<Cardinality>,
    #[serde(default)]
    pub markers_allowed: Option<Vec<String>>,
    #[serde(default)]
    pub markers_required: Option<bool>,
    #[serde(default)]
    pub output_target: Option<OutputTarget>,
    #[serde(default)]
    pub body_grammar: Option<BodyGrammar>,
    #[serde(default)]
    pub synthetic_from: Option<SyntheticSource>,
    #[serde(default)]
    pub expand_hook: Option<String>,
    #[serde(default)]
    pub repair_hook: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Cardinality {
    One,
    Many,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputTarget {
    Body,
    YamlFrontmatter,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BodyGrammar {
    ContentItems,
    Statements,
    EffectKeywords,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyntheticSource {
    SkillSignature,
    BodyMarkers,
}

#[derive(Debug, Clone)]
pub struct SectionCatalogue {
    entries: BTreeMap<String, CatalogueEntry>,
}

impl SectionCatalogue {
    /// Load the compile-time-embedded catalogue. Panics on TOML parse error
    /// or on a catalogue-validation failure (e.g.,
    /// `G::catalogue::cardinality-grammar-mismatch`). Catalogue errors are
    /// compiler-build-time bugs — runtime callers never see them.
    pub fn load() -> Self {
        let entries: BTreeMap<String, CatalogueEntry> =
            toml::from_str(CATALOGUE_TOML).expect("catalogue.toml must parse");
        let cat = Self { entries };
        cat.validate();
        cat
    }

    fn validate(&self) {
        for (name, entry) in &self.entries {
            // MVP rule (spec §4.2.2): cardinality = "one" requires body_grammar = "content_items".
            if entry.cardinality == Some(Cardinality::One) {
                let bg = entry.body_grammar.unwrap_or(BodyGrammar::ContentItems);
                if bg != BodyGrammar::ContentItems {
                    panic!(
                        "G::catalogue::cardinality-grammar-mismatch: \
                         section `{}` has cardinality = \"one\" but body_grammar = {:?}; \
                         only content_items supports `one` in MVP",
                        name, bg
                    );
                }
            }
        }
    }

    pub fn get(&self, name: &str) -> Option<&CatalogueEntry> {
        // Catalogue lookup is case-insensitive (catalogue keys are canonical
        // lowercase). The original author's spelling is preserved upstream in
        // the AST so diagnostics can quote it verbatim.
        let lower = name.to_ascii_lowercase();
        self.entries.get(&lower)
    }

    /// Build a catalogue from an explicit entry list, bypassing the embedded
    /// TOML. Test-only — used by callers that need to verify catalogue-driven
    /// behavior against a known-distinct heading override.
    #[cfg(test)]
    pub fn from_entries(entries: Vec<(String, CatalogueEntry)>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    pub fn set_enabled(&mut self, name: &str, value: bool) {
        if let Some(entry) = self.entries.get_mut(name) {
            entry.enabled = value;
        }
    }

    pub fn is_known(&self, name: &str) -> bool {
        self.entries.contains_key(&name.to_ascii_lowercase())
    }

    /// Whether the `[effects]` entry is enabled. Defaults to `true` when no
    /// entry exists (today's behavior for an unknown section). This is the
    /// single source of truth for the legacy `enable_effects` bool that the
    /// CLI's `--enable-effects` flag, the parser, and the emit pass all
    /// consume; the CLI derives the bool from this lookup at the boundary
    /// and threads the bare bool through internal call sites.
    pub fn effects_enabled(&self) -> bool {
        self.get("effects").is_none_or(|e| e.enabled)
    }

    /// Look up the catalogue-registered `expand_hook` name for `section`,
    /// if any. Call sites resolve the returned name through a small `match`
    /// over the supported hook ids (see `crate::sections::hooks`). Returns
    /// `None` either when the section has no entry or when its entry leaves
    /// `expand_hook` blank — in either case the caller's fallback rendering
    /// path runs unchanged.
    pub fn expand_hook_for(&self, section: &str) -> Option<&str> {
        self.get(section).and_then(|e| e.expand_hook.as_deref())
    }

    /// Look up the catalogue-registered `repair_hook` name for `section`,
    /// if any. See [`expand_hook_for`] for the resolution shape.
    pub fn repair_hook_for(&self, section: &str) -> Option<&str> {
        self.get(section).and_then(|e| e.repair_hook.as_deref())
    }
}

// No `Default` impl: `SectionCatalogue::load()` panics on TOML parse error
// or validation failure (those are compiler-build-time bugs), so wrapping
// `load()` in `Default::default()` would silently propagate panic-on-construct
// to any future `#[derive(Default)]` on a containing type. Callers must call
// `::load()` explicitly.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_catalogue_loads_cleanly() {
        let cat = SectionCatalogue::load();
        assert!(cat.get("anything").is_none());
        assert!(!cat.is_known("anything"));
    }

    #[test]
    #[should_panic(expected = "G::catalogue::cardinality-grammar-mismatch")]
    fn cardinality_one_with_statements_grammar_panics() {
        let toml_src = r#"
            [bad_section]
            cardinality = "one"
            body_grammar = "statements"
        "#;
        let entries: BTreeMap<String, CatalogueEntry> =
            toml::from_str(toml_src).expect("test toml must parse");
        SectionCatalogue { entries }.validate();
    }
}
