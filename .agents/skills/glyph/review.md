---
name: review
description: Use during a /glyph:compile run after expand_and_validate has produced the final compiled .md artifacts. Read the .md plus its Glyph source and resolved IR, classify every finding as auto-fixable or needing user input, apply the auto-fixable rewrites in place, and print a human-readable review report covering the fixes made and the items still requiring author attention.
---

## Parameters

- **md_path**. Required.
- **source_path**. Required.
- **resolved_ir**. Required.

## Instructions

### Context

- **runs-after-expand-and-validate**

  The review pass runs after expand_and_validate has emitted the final compiled .md artifacts and they have passed `glyph validate-output`. The review pass is a prose-level audit independent of the structural validator.

- **invocation-shape-is-md-with-source-and-resolved-ir**

  The LLM receives one compiled Markdown file plus the Glyph source and the resolved IR side-map for the same source in a single prompt. When findings are auto-fix only, the LLM also writes the updated Markdown back to md_path. The LLM always returns a human-readable report.

- **md-is-the-agent-facing-artifact**

  The compiled Markdown at md_path is the artifact a consuming coding agent will read at runtime. Prose-level defects degrade the consuming agent's behavior even when the source and IR are correct.

- **source-is-intent-and-ir-is-the-resolved-index**

  The Glyph source is the record of author intent — it captures the author's original phrasing, comments, and organization. The IR is the resolved structural index — every Step, Constraint, Param, Context entry, and OutputContract is an enumerable node with imports resolved and references chased through. Use the source to judge fidelity to intent; use the IR to mechanically enumerate what should appear in the prose.

- **caller-owns-retry-budget-and-revalidation**

  The /glyph:compile pipeline owns the retry budget and any post-rewrite re-run of `glyph validate-output`. The review skill performs one cross-read, one classification, one optional rewrite, and one report print per invocation.

- **report-is-human-readable-text-for-the-user**

  The report is plain prose for a human reader: a summary line, an `Auto-fixed` section listing each applied change, and a `Needs your attention` section listing each finding the author must address. No JSON, no code fences containing structured data.

### Steps

1. Read the compiled Markdown at md_path, the Glyph source at source_path, and the resolved IR at resolved_ir, treating the source as the record of author intent and the IR as the resolved structural index where every Step, Constraint, Context entry, Parameter, and OutputContract has a counterpart somewhere in the source. For each IR node, locate its surfaced counterpart in the Markdown's `## Parameters`, `### Context`, `### Steps`, and `### Constraints` sections, note any item absent from the prose, and prefer the source's phrasing as the intent reference when it diverges from the IR's resolved text. Independently scan the prose for grammar errors, awkward phrasing, internal contradictions between sentences, and contradictions against the source or IR — for example, a constraint whose polarity is reversed or a step whose intent is flipped relative to the author's wording.
2. For each finding produced by the cross-read, assign a severity of `auto_fix` or `needs_user_input`. A finding is `auto_fix` when the correct repair is unambiguous and safe to apply without author judgement — for example, a typo, a clearly mis-tensed sentence, or a Context or Constraint bullet that is missing from the prose but mechanically derivable from a single source or IR entry. A finding is `needs_user_input` whenever the right fix requires author judgement — including every contradiction (within the prose, or against the source or IR), any missing-from-prose item whose phrasing is genuinely ambiguous, and anything where multiple reasonable rewrites would produce different meanings.
3. If every finding is severity `auto_fix`, rewrite the Markdown in place at md_path with each fix applied: correct typos and grammar inline, smooth awkward phrasing, and surface any unambiguously missing item by inserting a sentence into the appropriate Step, Context bullet, or Constraint bullet. If any finding is severity `needs_user_input`, do not rewrite — leave md_path untouched on disk and defer every change to the author.
4. Print a human-readable report directly to the user as plain text, leading with a one-line summary that names the file reviewed and the overall outcome (one of `clean`, `auto-fixed N items`, or `N items need your attention`), then listing each applied change under an `Auto-fixed` heading and each `needs_user_input` finding under a `Needs your attention` heading as short bullets — each showing the section or step plus, for fixes, a before-and-after snippet and one-sentence reason, and for findings, the issue in plain prose with any source or IR evidence — and omitting a heading whose section has no entries. Never emit JSON, code-fenced data, or any other structured payload, since the report is plain prose for a human reader. Produce: the human-readable review report shown to the user.

### Constraints

- Classify every finding produced by the cross-read pass with a severity, since a finding without a severity fails validation.
- You must use exactly one of `auto_fix` (the repair is unambiguous and safe to apply without author judgement) or `needs_user_input` (the right fix requires author attention) as the severity of each finding — any other value fails validation.
- You must preserve every deterministic structure the emitter laid down — YAML frontmatter, section headings, numbered Step ordering, sub-step lettering, Context bullets, Constraints bullets, the Parameters bullet structure, and the If/Otherwise arm structure — and edit only the prose inside Step bodies, sub-step bodies, parameter descriptions, Context bullets, and Constraint bullets.
- You must preserve every InputContract parameter slot verbatim in the output, since silent dropping of a declared parameter slot is forbidden.
- You must never rewrite the Markdown for any finding classified `needs_user_input`, since those findings — which include every contradiction — require author attention while the pipeline surfaces them and the author edits the source and recompiles.
- You must never modify the Glyph source file at source_path or the IR JSON sidecar at resolved_ir, since the review pass writes only to md_path while source and IR are read-only inputs.
- You must never add, merge, split, or reorder Steps, sub-steps, sections, constraints, or parameters during a rewrite, since edits are span-scoped within existing prose.
- You must never introduce a parameter-shaped reference for any name that is not declared in the skill's InputContract, since inventing parameter references is forbidden.
- You must never emit findings as JSON, code-fenced data, or any other structured payload, since the review report is plain prose for a human reader with section headings and bullets only.

