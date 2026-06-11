# BUG-069: SectionCatalogue::set_enabled does not case-normalize the section name, diverging from get/is_known

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/sections/mod.rs:130-134`
**Found by:** registry-slot | **Audit date:** unknown-date

## Description

The catalogue stores keys in canonical lowercase (`catalogue.toml` defines `[goal]`, `[effects]`, etc.), and every sibling lookup normalizes the incoming name: `get` does `name.to_ascii_lowercase()` (line 116-117), `is_known` does `name.to_ascii_lowercase()` (line 137), and `effects_enabled` routes through `get`. But `set_enabled` looks up `self.entries.get_mut(name)` with the raw, un-normalized `name`. Because the `BTreeMap` keys are all lowercase, any caller passing a mixed-case or upper-case name (e.g. `set_enabled("Effects", true)`) finds no entry and silently no-ops — the `if let Some(...)` simply falls through with no error. This is a contract divergence within one component: a name that `get`/`is_known` would resolve is unmatched by `set_enabled`. The method is `pub`. Today the only caller passes a hardcoded lowercase `"effects"`, so the defect is not triggered by current code; it is a latent inconsistency that would surface the moment any caller passes a non-lowercase section name.

## Trigger / Reproduction

Call `catalogue.set_enabled("Effects", true)` or `catalogue.set_enabled("EFFECTS", true)`. Both silently no-op — the enabled flag is unchanged — while `catalogue.is_known("Effects")` returns `true` and `catalogue.get("Effects")` resolves correctly. The inconsistency is only latent today because the one existing caller uses hardcoded `"effects"`.

## Evidence

```rust
pub fn set_enabled(&mut self, name: &str, value: bool) {
    if let Some(entry) = self.entries.get_mut(name) {   // raw name; siblings use to_ascii_lowercase()
        entry.enabled = value;
    }
}
```

## Recommended Resolution

Normalize the key the same way the lookups do:

```rust
pub fn set_enabled(&mut self, name: &str, value: bool) {
    let lower = name.to_ascii_lowercase();
    if let Some(entry) = self.entries.get_mut(&lower) {
        entry.enabled = value;
    }
}
```

This makes `set_enabled` consistent with the case-insensitive contract of `get`/`is_known`/`effects_enabled`.

## Verification Notes

Code at lines 130-134 confirmed: `set_enabled` calls `self.entries.get_mut(name)` with raw name. `get` (line 116) and `is_known` (line 137) both call `to_ascii_lowercase()` before the BTreeMap lookup. Only current caller is `glyph-cli/src/main.rs:140` with hardcoded `"effects"` — no other callers exist (grep confirmed). The bug is real as a latent contract divergence. The proposed fix is correct and sufficient.

## Independent Agent Finding

**Verdict:** Reproduced / confirmed.

**Reproduction/Refutation:** A temporary scratch crate under `tmp/bug069-repro` loaded `SectionCatalogue`, verified that mixed-case lookup APIs resolve `"Effects"`, then compared `set_enabled` behavior for mixed-case, upper-case, and lowercase names. `set_enabled("Effects", true)` and `set_enabled("EFFECTS", true)` left `effects_enabled()` false, while `set_enabled("effects", true)` changed it to true. This reproduces the reported latent case-normalization divergence. The scratch crate was removed after the run.

**Evidence:**

- Graphify query for `SectionCatalogue set_enabled case normalization` located `SectionCatalogue` and its `.get()`, `.set_enabled()`, `.is_known()`, and `.effects_enabled()` methods in `crates/glyph-core/src/sections/mod.rs`.
- Bounded source read of `crates/glyph-core/src/sections/mod.rs:70-175` showed `get` lowercases with `name.to_ascii_lowercase()`, `is_known` lowercases with `name.to_ascii_lowercase()`, and `set_enabled` still calls `self.entries.get_mut(name)` with the raw input.
- `rg -n "set_enabled|effects_enabled|SectionCatalogue" crates tests docs/reference production/bugs -g '!target'` found the production caller at `crates/glyph-cli/src/main.rs:140` passing hardcoded lowercase `"effects"`.
- `rg -n "^\\[effects\\]|enabled" crates/glyph-core/src/sections -g '*.toml' -g '*.rs'` and `sed -n '1,80p' crates/glyph-core/src/sections/catalogue.toml` confirmed `[effects] enabled = false`, giving the reproduction a visible before/after state.
- Reproduction command: `cargo run --quiet --manifest-path tmp/bug069-repro/Cargo.toml`

```text
is_known("Effects")=true
get("Effects").is_some()=true
effects before=false
after set_enabled("Effects", true)=false
after set_enabled("EFFECTS", true)=false
after set_enabled("effects", true)=true
```

**Resolution Input:** The existing recommended resolution should be preserved: normalize `name` with `to_ascii_lowercase()` inside `set_enabled` before calling `get_mut`. No source-code fix was made in this independent pass.
