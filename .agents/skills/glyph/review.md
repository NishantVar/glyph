---
name: review
description: Use during a /glyph:compile run after expand_and_validate has produced the final compiled .md artifacts. Read the .md plus its source IR, classify every finding as simple or contradiction, rewrite the .md in place when only simple findings are present, and emit a structured JSON review report covering rewrites, contradictions, and any residual warnings.
---

## Parameters

- **md_path**. Required.
- **resolved_ir**. Required.

## Instructions

### Context

- **runs-after-expand-and-validate**

  The review pass runs after expand_and_validate has emitted the final compiled .md artifacts and they have passed `glyph validate-output`. The review pass is a prose-level audit independent of the structural validator.

- **invocation-shape-is-one-md-with-resolved-ir**

  The LLM receives one compiled Markdown file plus the resolved IR side-map for the same source in a single prompt, and returns one structured JSON review report. When the report indicates a rewrite, the LLM also writes the updated Markdown back to md_path.

- **md-is-the-agent-facing-artifact**

  The compiled Markdown at md_path is the artifact a consuming coding agent will read at runtime. Prose-level defects degrade the consuming agent's behavior even when the IR is structurally correct.

- **ir-is-the-ground-truth-for-missed-item-checks**

  The IR is the compiler's ground truth for what the source declared. A missing-from-prose finding is an IR node that has no surfaced counterpart in the Markdown. Use IR node identity, not lexical similarity, when deciding whether an item is surfaced.

- **caller-owns-retry-budget-and-revalidation**

  The /glyph:compile pipeline owns the retry budget and any post-rewrite re-run of `glyph validate-output`. The review skill performs one cross-read, one classification, and one optional rewrite per invocation.

### Steps

1. Read the compiled Markdown at md_path and the resolved IR at resolved_ir. For every Step, Constraint, Context entry, Parameter, and OutputContract in the IR, locate its surfaced counterpart in the Markdown's `## Parameters`, `### Context`, `### Steps`, and `### Constraints` sections, and note any IR item that is absent from the prose. Independently scan the prose for grammar errors, awkward phrasing, and internal contradictions between sentences. Independently scan the prose for contradictions against the IR — for example, a constraint whose polarity is reversed, or a step whose intent is flipped.
2. For each finding produced by the cross-read, assign a severity of `simple` or `contradiction`. Grammar errors, spelling errors, awkward phrasing, and minor missing-from-prose items (for example, a Context entry not surfaced) are `simple`. Any contradiction within the prose, or any contradiction between the prose and the IR (reversed polarity, flipped intent, missing constraint that materially changes meaning), is `contradiction`.
3. If every finding is severity `simple`, rewrite the Markdown in place at md_path with each fix applied: correct grammar inline, smooth awkward phrasing, and surface any missing-from-prose item by inserting a sentence into the appropriate Step, Context bullet, or Constraint bullet. If any finding is severity `contradiction`, do not rewrite — leave md_path untouched on disk.
4. Emit one JSON object of the form `{ outcome: "rewritten" | "contradiction" | "no_findings", change_summary: [{ location: "...", before: "...", after: "...", reason: "..." }, ...], contradictions: [{ location: "...", finding: "...", ir_evidence: "..." }, ...], residual_simple: [{ location: "...", finding: "..." }, ...] }`. The `change_summary` array is non-empty only when `outcome` is `rewritten`. The `contradictions` array is non-empty only when `outcome` is `contradiction`. The `residual_simple` array carries simple findings the rewrite intentionally did not address. Produce: the structured JSON review report.

### Constraints

- Classify every finding produced by the cross-read pass with a severity. A finding without a severity fails validation.
- You must use exactly one of `simple` or `contradiction` as the severity of each finding. Any other value fails validation.
- You must preserve every deterministic structure the emitter laid down: YAML frontmatter, section headings, numbered Step ordering, sub-step lettering, `### Context` bullets, `### Constraints` bullets, the `## Parameters` bullet structure, and the `If <condition>:` and `Otherwise:` arm structure. Edit only the prose inside Step bodies, sub-step bodies, parameter descriptions, Context bullets, and Constraint bullets.
- You must preserve every `{param}` token whose name matches a declared InputContract parameter verbatim into the output. Silent dropping is forbidden.
- You must never rewrite the Markdown when any finding is severity `contradiction`. Contradictions require author attention; the pipeline hard-fails on them and the rewrite is intentionally suppressed.
- You must never modify the Glyph source file or the IR JSON sidecar. The review pass writes only to md_path.
- You must never add, merge, split, or reorder Steps, sub-steps, sections, constraints, or parameters during a rewrite. Edits are span-scoped within existing prose.
- You must never introduce a `{name}` token for any name that is not declared in the skill's InputContract. Inventing parameter references is forbidden.

