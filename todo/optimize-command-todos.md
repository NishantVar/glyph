# Glyph Optimize Command — TODOs

Feature idea: add `/glyph:optimize` as an advisory improvement command for authors who want more readable, accountable Glyph source without making the base language harder for beginners.

## Goal

`/glyph:optimize` should identify and optionally rewrite source-level patterns where meaningful artifacts are carried only through sequential agent memory instead of explicit labels.

The core principle:

> Sequential memory is allowed for execution, but not for accountability.

If a later step depends on a thing produced, discovered, decided, imagined, edited, or summarized by an earlier step, that thing should usually be visible as a named artifact, an argument, or an explicit dependency marker.

## Initial Scope

Add a command shape such as:

```text
/glyph:optimize path/to/file.glyph
/glyph:optimize path/to/file.glyph --apply
/glyph:optimize path/to/file.glyph --focus accountability
```

The default mode should produce a reviewable report. `--apply` should only rewrite high-confidence, local changes.

## Accountability Patterns

Detect hidden artifact flow, including:

- Cognitive artifacts: scenarios, hypotheses, plans, decisions, risk models, edge cases, failure modes.
- Observed artifacts: diagnostics, repo context, test output, web findings, user preferences.
- Mutation artifacts: file edits, generated files, git state changes, applied patches.
- Generated artifacts: reports, summaries, checklists, selected candidates.

Suspicious source shape:

```glyph
think_through_failure_modes(scope)
write_tests(scope)
```

More accountable shape:

```glyph
failure_modes = identify_failure_modes(scope)
test_plan = design_tests(scope, failure_modes)
write_tests(test_plan)
```

Suspicious source shape:

```glyph
edit_source(source_path)
verify_consistency(source_path)
```

More accountable shape:

```glyph
source_edit = edit_source(source_path)
verification = verify_consistency(source_path, source_edit)
```

## LLM Optimization Checks

Use an LLM semantic pass where deterministic analysis is insufficient. The pass should look for:

- Calls that appear to produce a reusable artifact but are invoked as bare statements.
- Later calls that appear to rely on earlier work while receiving only the original broad input, such as `scope` or `source_path`.
- Prose like "use the previous result", "based on the above", "then apply that", or "using the scenario" without a visible binding.
- Vague labels such as `result`, `data`, `thing`, or `output` where a more specific artifact name would improve accountability.
- Blocks that mix several products and should be split into separately named artifacts.

The report should include evidence, suggested rewrite, rationale, and confidence.

## Deterministic Support

Possible compiler or IR support that would make optimization easier:

- Track whether a block has a meaningful output contract or return type.
- Flag non-void calls that are invoked as bare statements unless explicitly discarded.
- Introduce or reserve an explicit discard/dependency marker if the language later needs one.
- Preserve enough source spans for safe local rewrites.
- Represent produced artifacts and consumed artifacts in a provenance graph for visualization.

These should start as optimizer findings, not hard language errors.

## Product Boundary

Keep `/glyph:audit` focused on semantic correctness and contradictions. Use `/glyph:optimize` for readability, accountability, provenance, naming, and maintainability improvements.

The base language should remain forgiving. Optimization is for authors who want to raise the quality bar after the first draft works.
