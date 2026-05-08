---
name: icompile
description: Use when the user invokes /glyph:icompile on a Glyph skill. Apply a small targeted change to both the .glyph source and the sibling compiled .md output in tandem, bypassing the full compile pipeline so unrelated prose is preserved.
---

## Parameters

- **source_path** — filesystem path to the `.glyph` source whose paired compiled `.md` is to be patched in tandem. Required.
- **change** — plain-language description of the targeted edit to make in both artifacts. Required.

## Instructions

### Steps

1. Read {source_path}. Resolve the sibling compiled output at the same directory and basename with a `.md` extension. If the compiled `.md` does not exist, stop and tell the user to run `/glyph:compile` first — incremental edit requires a prior compiled artifact. Read the IR sidecar `.ir.json` next to the source if present, and use it to map source-level constructs to compiled regions.
2. Decompose {change} into the smallest set of edits needed in the `.glyph` source and the corresponding edits in the compiled `.md`. Identify which source constructs are touched (skill header, parameter, constraint marker, flow step, block body, named constant, import) and the matching compiled regions (frontmatter `description`, `## Parameters` entry, `### Context` bullet, `### Steps` numbered item, `### Steps` lettered branch sub-step, `### Constraints` bullet, `### Procedure: <name>` numbered step). If the change requires regenerating multi-line prose that the LLM repair or prose-reshape pass originally authored — anything beyond a localised wording or value swap — stop and recommend `/glyph:compile` instead.
3. Apply the planned edit to {source_path} using a targeted, exact-text replacement. Preserve 4-space indentation, comment placement, blank lines, and the order of unrelated declarations.
4. Apply the matching edit to the compiled `.md` at the same basename. Mirror exactly the source change in the corresponding compiled region identified during planning. When the source edit rewords a constraint marker text, an inline instruction string, or a constant body, port the new wording into the compiled bullet or numbered step verbatim, adjusting only the minimal surrounding prose needed to read naturally.
5. Re-read both files. Confirm the parameter list, constraint count, top-level step count, and any cross-references in the compiled `.md` still match the `.glyph` source after the edit. If anything is out of sync, surface the mismatch to the user and stop — do not attempt further fix-up edits in this run. Produce: a recap of the edits applied to each artifact, with the absolute path to each file.

### Constraints

- Every edit must touch both the .glyph source and the compiled .md so the two artifacts stay in lockstep.
- You must never invoke `glyph compile`, `glyph fmt`, or any other Glyph CLI subcommand on the source — the whole point of this skill is to bypass the pipeline and preserve unrelated compiled prose verbatim.
- Do not touch sections of either file that the requested change does not affect — no adjacent reflows, no comment cleanup, no reordering of declarations or steps.

