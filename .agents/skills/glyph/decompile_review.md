---
name: decompile_review
description: Compare the original compiled .md skill (the pre-decompile artifact) against the .md produced by recompiling the .glyph file written by /glyph:decompile, and report every item that is missing, changed, or added between the two. Used as the final equivalence check after a successful recompile.
---

## Parameters

- **original_md**. Required.
- **recompiled_md**. Required.

## Instructions

### Context

- **runs-after-successful-recompile-of-decompile-output**

  This skill runs as the final step of /glyph:decompile, only after the recompile-via-subagent step completes without errors. If recompile fails, this skill is not invoked.

- **original-is-pre-decompile-artifact**

  The original Markdown is the pre-decompile artifact — the skill that was reverse-mapped into Glyph source. /glyph:decompile renames it with an `old_` prefix so the recompile does not overwrite it; the path passed in as original_md points at that renamed file.

- **recompiled-is-post-decompile-artifact**

  The recompiled Markdown is the post-decompile artifact — the .md produced by recompiling the .glyph file written by /glyph:decompile.

- **report-is-human-readable-text-for-the-user**

  The report is plain prose for a human reader: a summary line, a `Missing` section, a `Changed` section, and an `Added` section. Each heading is omitted when its section has no entries. No JSON, no code fences containing structured data.

### Steps

1. Read the original compiled Markdown at {original_md} and the recompiled Markdown at {recompiled_md}. Both files are agent-facing skill artifacts with YAML frontmatter and the canonical sub-sections `## Parameters`, `### Context`, `### Steps`, and `### Constraints` (each section may or may not be present per skill).
2. Cross-read the two files section by section. For each frontmatter field, parameter, context bullet, step, sub-step, and constraint bullet in the original, locate its counterpart in the recompiled file by meaning rather than by exact wording. Mark items present in the original but absent from the recompiled file as `missing`. Mark items present in the recompiled file but absent from the original as `added`. Mark items present in both whose meaning has shifted as `changed`. Items whose surface wording differs but whose semantic content is preserved are equivalent — do not report them.
3. Print a human-readable report directly to the user as plain text. Lead with a one-line summary that names both files and the overall outcome — one of `equivalent` or `N differences`. Then under a `Missing` heading, list each item present in the original but absent from the recompiled file as a short bullet showing the section and a brief snippet. Then under a `Changed` heading, list each item whose meaning shifted with a one-line before-and-after snippet. Then under an `Added` heading, list each item present in the recompiled file but absent from the original. Omit a heading whose section has no entries. Never emit JSON, code-fenced data, or any other structured payload — the report is for a human reader. Produce: the human-readable equivalence report shown to the user.

### Constraints

- Compare the two files by meaning, not by exact wording. Equivalent prose with different surface phrasing is not a difference. Only items that change in semantic content count as differences.
- You must report every difference produced by the cross-read pass under the appropriate heading. A difference left unreported defeats the purpose of the round-trip equivalence check.
- You must never modify, rewrite, or normalize either the original or the recompiled Markdown file. The equivalence review is read-only — both files stay on disk untouched.
- You must never emit findings as JSON, code-fenced data, or any other structured payload. The report is plain prose for a human reader, with section headings and bullets only.

