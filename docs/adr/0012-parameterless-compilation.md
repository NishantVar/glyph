# ADR 0012: Parameterless Compilation — Parameters Survive As Named Slots

## Status

Accepted.

## Context

A skill takes parameters at invocation time (e.g., `fix_bug(scope = ".")` takes a `scope` argument). A naive compiler would substitute argument values into the compiled output: each invocation produces a different `.md` file specialized to that call's arguments.

This was rejected because it would mean:

- The `.md` artifact is not stable per source file — different invocations produce different outputs.
- Caching by source hash is impossible; cache keys would need to include argument values.
- The compiled file cannot be inspected or distributed independently; it is meaningful only relative to a specific invocation.
- The consuming LLM cannot pick up the same skill across different contexts without recompilation.

## Decision

Compilation is **parameterless**. Expand does not receive concrete argument values. Parameters appear in the compiled output as named slots:

- `## Parameters` section lists each parameter with its name, optional default, and description (filled by the LLM during Expand Step 2).
- Inside the body sections (`## Steps`, `## Constraints`, `## Context`), parameter references survive as `{param}` tokens (e.g., "Inspect the failure in `{scope}`...").

The consuming LLM resolves `{param}` slots from user context at runtime — it sees the user's request, looks at the `## Parameters` block, and binds the slot itself. Local-binding references (`{name}` for values produced by earlier steps) are not preserved as literal tokens; Step 2 resolves those into natural-language cross-references in prose.

## Consequences

- One `.glyph` source file produces exactly one `.md` artifact, regardless of how many times it will be invoked.
- Cache keys depend only on source content and imports. There is no argument-dependent variation in the pipeline.
- The compiled `.md` is a stable, distributable artifact. A skill can be shipped, version-controlled, and re-used across invocation contexts.
- The consuming LLM has more work at runtime (it must resolve `{param}` slots from context), but this is the right place for that work — the LLM is where context lives.
- The pipeline does not need an "argument resolution" phase. Phase 4 (Lower) handles default value filling at IR level; concrete argument values never enter the compiler.
