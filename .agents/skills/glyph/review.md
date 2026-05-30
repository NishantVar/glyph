---
name: review
description: 'Use during a /glyph:compile run after expand_and_validate has produced the final compiled .md artifacts. Read the .md plus its Glyph source and resolved IR, classify every finding as auto-fixable or needing user input, apply the auto-fixable rewrites in place, and print a human-readable review report covering the fixes made and the items still requiring author attention.'
---

## Parameters

- **md_path**. Required.
- **source_path**. Required.
- **resolved_ir**. Required.

## Constraints

- **Require:** Classify every finding produced by the cross-read pass with a severity. A finding without a severity fails validation.
- **Must:** Use exactly one of `auto_fix` or `needs_user_input` as the severity of each finding. `auto_fix` means the repair is unambiguous and safe to apply without author judgement; `needs_user_input` means the right fix requires author attention. Any other value fails validation.
- **Must:** Preserve every deterministic structure the emitter laid down: YAML frontmatter, section headings, numbered Step ordering, sub-step lettering, `### Context` bullets, `### Constraints` bullets, the `## Parameters` bullet structure, and the `If <condition>:` and `Otherwise:` arm structure. Edit only the prose inside Step bodies, sub-step bodies, parameter descriptions, Context bullets, and Constraint bullets.
- **Must:** Preserve every `{param}` token whose name matches a declared InputContract parameter verbatim into the output. Silent dropping is forbidden.
- **Must avoid:** rewrite the Markdown for any finding classified `needs_user_input`. Those findings — which include every contradiction — require author attention; the pipeline surfaces them and the author edits the source and recompiles.
- **Must avoid:** modify the Glyph source file at source_path or the IR JSON sidecar at resolved_ir. The review pass writes only to md_path; source and IR are read-only inputs.
- **Must avoid:** add, merge, split, or reorder Steps, sub-steps, sections, constraints, or parameters during a rewrite. Edits are span-scoped within existing prose.
- **Must avoid:** introduce a `{name}` token for any name that is not declared in the skill's InputContract. Inventing parameter references is forbidden.
- **Must avoid:** emit findings as JSON, code-fenced data, or any other structured payload. The review report is plain prose for a human reader, with section headings and bullets only.

## Context

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

## Steps

1. Read the compiled Markdown at md_path, the Glyph source at source_path, and the resolved IR at resolved_ir. Treat the source as the record of author intent and the IR as the resolved structural index — every Step, Constraint, Context entry, Parameter, and OutputContract in the IR has a counterpart somewhere in the source. For each IR node, locate its surfaced counterpart in the Markdown's `## Parameters`, `### Context`, `### Steps`, and `### Constraints` sections, and note any item that is absent from the prose; when the source phrases that item differently than the IR's resolved text, prefer the source's phrasing as the intent reference. Independently scan the prose for grammar errors, awkward phrasing, and internal contradictions between sentences. Independently scan the prose for contradictions against the source or IR — for example, a constraint whose polarity is reversed, or a step whose intent is flipped relative to what the author wrote.
2. For each finding produced by the cross-read, assign a severity of `auto_fix` or `needs_user_input`. A finding is `auto_fix` when the correct repair is unambiguous and safe to apply without author judgement — for example, a typo, a clearly mis-tensed sentence, or a Context or Constraint bullet that is missing from the prose but mechanically derivable from a single source or IR entry. A finding is `needs_user_input` whenever the right fix requires author judgement — including every contradiction (within the prose, or against the source or IR), any missing-from-prose item whose phrasing is genuinely ambiguous, and anything where multiple reasonable rewrites would produce different meanings.
3. If every finding is severity `auto_fix`, rewrite the Markdown in place at md_path with each fix applied: correct typos and grammar inline, smooth awkward phrasing, and surface any unambiguously missing item by inserting a sentence into the appropriate Step, Context bullet, or Constraint bullet. If any finding is severity `needs_user_input`, do not rewrite — leave md_path untouched on disk and defer every change to the author.
4. Print a human-readable report directly to the user as plain text. Lead with a one-line summary that names the file reviewed and the overall outcome — one of `clean`, `auto-fixed N items`, or `N items need your attention`. Then under an `Auto-fixed` heading, list each applied change as a short bullet showing the section or step, a brief before-and-after snippet, and a one-sentence reason. Then under a `Needs your attention` heading, list each `needs_user_input` finding as a short bullet showing the section or step, the issue stated in plain prose, and any source or IR evidence when relevant. Omit a heading whose section has no entries. Never emit JSON, code-fenced data, or any other structured payload — the report is for a human reader. Produce: the human-readable review report shown to the user.

