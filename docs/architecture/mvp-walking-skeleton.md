# MVP Walking Skeleton

The walking skeleton is the smallest Glyph source file that exercises every
deterministic compiler phase end to end. It exists so the pipeline cannot be
"almost there" — every phase must run, even if its work on this input is a
trivial pass-through.

## Source: `update_docs.glyph`

```glyph
skill update_docs()
    description: "Update repository documentation to match current code."
    require accuracy
    avoid stale_references

    effects: reads_files, writes_files

    flow:
        "Scan the repository for files with documentation."
        "Compare each document against the current code for accuracy."
        "Update any sections that are outdated or incorrect."
        "Verify all cross-references and links are still valid."

const accuracy = "Ensure all documentation accurately reflects the current code."
const stale_references = "Leaving references to removed or renamed symbols."
```

## What the skeleton forces every phase to do

- **Parse** — header, two `const` declarations, two constraint markers,
  `flow:` block with four inline strings. No imports, so the file is its
  own trivial import DAG.
- **Analyze** — `accuracy` and `stale_references` resolve to same-file
  `const` bindings; `require`/`avoid` markers set role and polarity;
  declared effects match inferred. Zero diagnostics, so the pipeline does
  not stop after Phase 2.
- **Lower** — inline strings become `InlineInstruction` nodes with
  `role: Step`. Constraint markers plus resolved text become `Constraint`
  nodes with strength and polarity. Every node gets a stable ID.
- **Validate** — node IDs unique, no unresolved callees, no cycles, no
  empty steps.
- **Expand Step 1** — `const` references already resolved to strings;
  inline strings pass through; no `Call` nodes means no projection-tier
  decisions.
- **Emit** — assembles YAML frontmatter, peer-level `## Steps` (four
  items) and `## Constraints` (two items). No `## Parameters`,
  no `## Context`. Writes [[update_docs]].

## Expected compiled output

```md
---
name: update_docs
description: Update repository documentation to match current code.
effects: [reads_files, writes_files]
---

## Steps

1. Scan the repository for files with documentation.
2. Compare each document against the current code for accuracy.
3. Update any sections that are outdated or incorrect.
4. Verify all cross-references and links are still valid.

## Constraints

- Ensure all documentation accurately reflects the current code.
- Do not leave references to removed or renamed symbols.
```

## Why the design constraints are what they are

The skeleton is deliberately starved of every feature that introduces
non-trivial phase work:

- **Parameterless** — no `## Parameters` block in output; no parameter
  reference resolution.
- **All names explicitly defined** — zero `repairable` diagnostics, so the
  pipeline never stops after Analyze.
- **All flow items are inline strings** — no `Call` nodes, so Expand
  Step 1 is trivial and there is no `with` modifier surface to exercise.
- **Explicit `effects:` and `description:`** — no inference gaps.
- **No imports** — single-file build, trivial DAG.

Anything beyond this is exercised by the larger acceptance corpus
(parameters, calls, `with` modifiers, branching, imports, libraries).
The skeleton's job is the floor.
