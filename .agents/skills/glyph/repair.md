---
name: repair
description: Use during a /glyph:compile run when the compiler has exited 2 with repairable diagnostics on a single Glyph source file. Read the source plus its NDJSON diagnostics, apply each diagnostic's rewrite pattern, and write the rewritten Glyph source file back to source_path.
---

## Parameters

- **source_path**. Required.
- **diagnostics**. Required.

## Instructions

### Context

- **invocation-shape-is-one-file-with-its-diagnostics**

  The LLM receives one Glyph source file plus its full set of repairable diagnostics in a single prompt, and returns one rewritten source file. One LLM call per file per iteration; up to 3 iterations per file.

- **compound-names-stay-atomic**

  A compound name such as `avoid_unrelated_edits` (when unresolved) must be materialized as a single `generated const` under the full compound name, with polarity baked into the body text. Never split it into a marker and a base name.

- **invalid-output-is-discarded-without-retry**

  When the rewritten output fails Phase 1 parsing, the agent does not retry. The failed rewrite is captured for inspection and the run aborts with `G::repair::output-invalid`.

### Steps

1. For each `G::parse::operator-in-expression` diagnostic, rewrite the offending `x + y` form as a `combine(x, y)` call, or fold the operands into a single inline instruction string.
2. For each `G::parse::param-slot-in-non-instruction-string` diagnostic, strip the `{...}` braces, or move the slot into an instruction-bearing string.
3. For each `G::analyze::undefined-name` diagnostic, append `generated const <name> = "<one sentence>"` after every non-generated declaration in the file.
4. For each `G::analyze::undefined-call` diagnostic, append `generated block <name>(<inferred-params>)` with a single-string body, after every non-generated declaration.
5. For each `G::analyze::ambiguous-role` diagnostic, add an explicit role marker (`require` / `avoid` / `must` / `context`) to the offending entry, or convert it into an instruction string or a call.
6. For each `G::analyze::missing-return` diagnostic, append `return <expr>` as the final statement of the relevant `flow:`. Use `return none` when there is no meaningful value to return.
7. For each `G::analyze::export-missing-return-type` diagnostic, infer a named domain type from the returned value (never a primitive name) and add ` -> DomainType` to the export block's header.
8. For each `G::analyze::nested-branch` diagnostic, extract the inner branch into a `generated block` and replace it with a call passing captured outer-scope bindings.
9. For each `G::analyze::missing-description` diagnostic on a skill, generate a single-line `description:` phrased as a trigger condition the consuming agent can match on.
10. For each `G::analyze::applies-on-undescribed-block` diagnostic where the referenced block is in the same file, generate a trigger-shaped `description:` for that block.
11. For an `G::analyze::applies-on-undescribed-block` diagnostic where the referenced block is imported from another file, leave it alone — the diagnostic is non-repairable and the imported file must not be edited.
12. Skip any unresolved-name or unresolved-call diagnostic whose name already resolves via existing imports, stdlib entries, or other local declarations. Idempotence requires that already-resolved names are never regenerated. Produce: the full rewritten Glyph source file as a single text blob.

### Constraints

- Apply only rewrites that match a diagnostic in the input set, never introducing edits that no diagnostic asked for.
- You must produce a Glyph source file that parses successfully under Phase 1 of the compiler — otherwise the failed rewrite is discarded and the agent aborts without retry, surfacing `G::repair::output-invalid`.
- You must never regenerate a `generated const` or `generated block` whose name already resolves via an existing import, the standard library, or another local declaration.

