# BUG-043: Return-folding templates in compiled-output.md do not match emitted prose

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `docs/reference/compiled-output.md:243-248`
**Found by:** x-contract | **Audit date:** unknown-date

## Description

`compiled-output.md` §Return Folding documents fold templates such as suffix `, and return that as your result.` and standalone `Return <name-as-words> as your result.` / `Return <description> as your result.`. The string `as your result` appears nowhere in the compiler.

The actual deterministic templates in `crates/glyph-core/src/emit/templates.rs` (lines 86-120) are:
- Descriptive form: `Produce: <desc>.`
- Identifier form: `` Produce `name`. `` / `` Produce `name` (`Type`). `` / `` Produce `name` (`Type`): <desc>. ``

The skill's own output contract renders as a separate `Output: <x>.` step per ADR 0026 (`expand.rs:692-705`, `737-748`).

A downstream tool validating compiled output against these documented templates would falsely reject correct compiler output.

## Trigger / Reproduction

Compile any skill with a `return <"a structured diagnosis">` — the compiler emits `... Produce: a structured diagnosis.`, not `... and return a structured diagnosis as your result.` For `return <diagnosis>` the compiler emits `` ... Produce `diagnosis`. ``, not `Return diagnosis as your result.` Both diverge from the documented templates.

## Evidence

```md
<!-- docs/reference/compiled-output.md lines 243-246 -->
| Return form | Suffix template | Standalone template (return-only body) |
|---|---|---|
| Identifier (`return <name>`) | `, and return that as your result.` | `Return <name-as-words> as your result.` |
| Description (`return <"…">`) | `, and return <description> as your result.` | `Return <description> as your result.` |
```

Actual templates (confirmed by grep across all `.rs` files — "as your result" has zero occurrences):

```rust
// crates/glyph-core/src/emit/templates.rs ~lines 86-120
// Descriptive return form:
"Produce: {desc}."
// Identifier return forms:
"Produce `{name}`."
"Produce `{name}` (`{Type}`)."
"Produce `{name}` (`{Type}`): {desc}."
// Skill output contract (separate step, per ADR 0026):
"Output: {description}."
"Output: {name} from step {M}."
```

## Recommended Resolution

Replace the §Return Folding template table in `docs/reference/compiled-output.md` with the actual `Produce ...` fold forms and document the separate `Output: <x>.` step rendered for skill/block output contracts (per `templates.rs` and ADR 0026).

## Verification Notes

`grep -r "as your result" crates/` returns zero results. Multiple tests in `expand.rs`, `mod.rs`, `scaffold.rs`, and `imported_tier1_output_contract.rs` encode the `Produce ...` and `Output: ...` forms. The `as your result` wording originates from older design documents (`design/compiled-output.md`, `llm_expand_pass.md`) that were never carried forward into the actual implementation.
