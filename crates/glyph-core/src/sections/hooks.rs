//! Named hook functions for the section catalogue.
//!
//! Each fn is referenced by string name from `catalogue.toml` (`expand_hook`
//! / `repair_hook`). The dispatcher (`SectionCatalogue::expand_hook_for`
//! / `SectionCatalogue::repair_hook_for`) returns the hook name; call sites
//! resolve the name into the matching function via a small `match` on the
//! hook string (see `lower.rs` / `emit/scaffold.rs`). This keeps the
//! registry textual (catalogue.toml only mentions names, never function
//! pointers) while still benefitting from a compile-time check at the call
//! site.
//!
//! Phase 5 wires the hooks for the migrated built-ins:
//! - `render_constraint_four_form` — emit-time hook used by Constraints
//! - `generate_description_if_missing` — repair-time hook for missing
//!   description (no-op today; see fn doc-comment for the rationale)
//!
//! Phase 6 will register `goal:` with no hooks (its rendering is the
//! generic content-item path).
//!
//! ## Dispatch wrappers
//!
//! The `dispatch_*` helpers below resolve a section's hook name from the
//! catalogue and `match` on it to call the right implementation. They use
//! a process-wide [`OnceLock`] snapshot of the catalogue so call sites in
//! `lower.rs` / `emit/scaffold.rs` don't have to thread `&SectionCatalogue`
//! through every function signature — those code paths render constraints
//! per-item and don't naturally carry a catalogue handle today. The
//! `OnceLock` is initialized lazily from the compile-time-embedded
//! `catalogue.toml`; loading is the same `SectionCatalogue::load()` call as
//! the explicit-handle code paths, so the two views cannot drift.

use crate::ir::{Polarity, Strength};
use std::sync::OnceLock;

/// Catalogue `expand_hook = "render_constraint_four_form"`.
///
/// Wraps the locked four-form constraint template renderer
/// (`crate::emit::constraint::render`) under the catalogue-aware name.
/// Call sites that previously hardcoded the path
/// (`emit::scaffold::emit_constraints_section`, `lower::lower_freeform_item`)
/// now resolve the function through this name so a future re-skin of the
/// `[constraints]` entry's `expand_hook` field is a one-line change.
pub fn render_constraint_four_form(strength: Strength, polarity: Polarity, text: &str) -> String {
    crate::emit::constraint::render(strength, polarity, text)
}

/// Catalogue `repair_hook = "generate_description_if_missing"`.
///
/// Phase 5 placeholder: today's "repair" for a missing `description:`
/// sub-section is a `G::analyze::missing-description` Repairable
/// diagnostic; the actual auto-injection (synthesised description text) is
/// not yet implemented — the diagnostic's hint directs the author to run
/// `glyph fmt`, but fmt currently has no code path that writes a generated
/// description. Returning `None` is faithful to that behavior. A later
/// slice that wires a real generator (e.g. distilled from the skill's
/// flow) will replace the body with the actual logic; downstream callers
/// already treat `None` as "no auto-generated description available", so
/// the catalogue-aware dispatch site is the only churn needed when that
/// lands.
///
/// Returning `Option<String>` mirrors the call shape the future generator
/// will need: `Some("…")` injects the new description text; `None`
/// preserves the existing Repairable diagnostic without auto-fix.
pub fn generate_description_if_missing(_skill_name: &str) -> Option<String> {
    None
}

/// Process-wide cached `SectionCatalogue` snapshot.
///
/// Used by the `dispatch_*` helpers to look up `expand_hook` / `repair_hook`
/// names without threading `&SectionCatalogue` through every emit / lower
/// signature. Loaded lazily from the same compile-time-embedded TOML the
/// explicit-handle path uses, so the snapshot cannot drift from
/// `SectionCatalogue::load()`.
fn cached_catalogue() -> &'static crate::sections::SectionCatalogue {
    static CAT: OnceLock<crate::sections::SectionCatalogue> = OnceLock::new();
    CAT.get_or_init(crate::sections::SectionCatalogue::load)
}

/// Catalogue-aware dispatch for the `[constraints]` entry's `expand_hook`.
///
/// Looks up the configured hook name (defaulting to
/// `render_constraint_four_form` when the catalogue entry omits it) and
/// routes to the matching function. Call sites that previously hardcoded
/// `crate::emit::constraint::render(...)` now go through this dispatcher
/// so a future re-skin of `[constraints].expand_hook` is a one-line
/// catalogue edit. Per design: panics on an unrecognized hook name — that
/// indicates the catalogue references a hook this build doesn't know about.
pub fn dispatch_constraints_expand(strength: Strength, polarity: Polarity, text: &str) -> String {
    let hook = cached_catalogue()
        .expand_hook_for("constraints")
        .unwrap_or("render_constraint_four_form");
    match hook {
        "render_constraint_four_form" => render_constraint_four_form(strength, polarity, text),
        other => panic!("unknown expand_hook for constraints: {}", other),
    }
}

/// Catalogue-aware dispatch for the `[description]` entry's `repair_hook`.
///
/// Phase 5 wiring: returns whatever the resolved `repair_hook` produces;
/// today's `generate_description_if_missing` returns `None` (no auto-fix),
/// which preserves the legacy Repairable-only behavior. When a real
/// generator lands, only the underlying hook function changes — call sites
/// already treat `None` as "no auto-generated description available".
pub fn dispatch_description_repair(skill_name: &str) -> Option<String> {
    let hook = cached_catalogue()
        .repair_hook_for("description")
        .unwrap_or("generate_description_if_missing");
    match hook {
        "generate_description_if_missing" => generate_description_if_missing(skill_name),
        other => panic!("unknown repair_hook for description: {}", other),
    }
}
