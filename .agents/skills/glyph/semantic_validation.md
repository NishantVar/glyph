---
name: semantic_validation
description: 'Use during a /glyph:compile run after the compiler has reached exit code 0 on a Glyph source file. Walk the resolved IR''s `constraints:` sets and emit a structured JSON conflict report covering every declaration with two or more constraints.'
---

## Parameters

- **constraint_set**. Required.

## Constraints

- **Require:** Classify every unordered pair of entries in the input constraint set. Pairs classified `none` may be omitted from the explicit `conflicts` list, but every pair must have been considered.
- **Must:** Use exactly one of `contradiction`, `tension`, or `none` as the `type` field on each conflict entry. Any other value fails validation.
- **Must avoid:** modify the Glyph source file. Phase 3c is read-only — it emits structured diagnostics only.

## Context

- **runs-after-compile-exit-zero**

  Phase 3c runs only after `glyph compile` reaches exit code 0. It is independent of Phase 2 diagnostics.

- **invoked-only-when-constraint-set-has-at-least-two-entries**

  Phase 3c is invoked once per declaration whose `constraints:` set contains 2 or more entries. Declarations with 0 or 1 constraints are skipped by the caller without an LLM call.

- **constraint-ids-are-local-indices**

  Each constraint entry in the input carries a declaration-local index `c0`, `c1`, … in source order. Output must reuse those identifiers exactly.

- **each-input-entry-carries-id-text-strength-polarity**

  Each input constraint entry has the shape `{ id, resolved_text, strength, polarity }`. Strength is `hard` or `soft`; polarity is `positive` or `negative`.

## Steps

1. For every unordered pair of entries in the constraint_set, classify the pair as one of `contradiction`, `tension`, or `none`. A contradiction is a strict conflict the constraints cannot both satisfy; a tension is a soft pull in opposing directions that can coexist; `none` means the pair is independent.
2. Emit one JSON object of the form `{ conflicts: [{ pair: [id_A, id_B], type: "contradiction" | "tension" | "none", explanation: "..." }, ...] }`. Pairs classified `none` may be omitted from the explicit list, but every pair must have been considered. Produce: the structured JSON conflict report.

