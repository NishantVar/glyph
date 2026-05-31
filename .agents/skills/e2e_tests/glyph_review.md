---
name: glyph_review
description: 'Compare two Glyph source files semantically — the original .glyph against the .glyph produced by /glyph:decompile of its sibling .md — and report every item that is missing, changed, or added between them. Used by the `roundtrip` skill as the decompile-direction oracle.'
---

## Parameters

- **original_glyph**:
  path to the source-of-truth .glyph file — the original that was compiled into the .md that /glyph:decompile reverse-mapped.
  Default: "".
- **roundtrip_glyph**: path to the .glyph file produced by /glyph:decompile from the original's sibling .md. Default: "".

## Constraints

- **Require:** Compare the two files by meaning, not by surface form. Equivalent intent with different surface phrasing, different identifier names, or different factoring choices is not a semantic difference. Only items whose semantic content has shifted count.
- **Require:** Report every semantic difference produced by the cross-read pass under the appropriate heading. A difference left unreported defeats the purpose of the round-trip equivalence check.
- **Must avoid:** modify, rewrite, or normalize either the original or the roundtrip Glyph file. The equivalence review is read-only — both files stay on disk untouched.
- **Must avoid:** emit findings as JSON, code-fenced data, or any other structured payload. The report is plain prose for a human reader, with section headings and bullets only.
- **Must avoid:** flag any of these surface-only differences as semantic differences: block or const naming differences (the decompile pipeline is LLM-driven and may rename declarations), top-level declaration order, sub-section order within a body (`description:` vs `context:` vs `constraints:` vs `flow:` position), inline-string-vs-named-const factoring when intent matches, extracted-block-vs-inline-string-sequence factoring when actions match, or surface phrasing of any prose.

## Context

- **original-is-source-of-truth**

  The original .glyph is the source of truth — the hand-authored file that was compiled into the .md that /glyph:decompile reverse-mapped back into Glyph source.

- **roundtrip-is-post-decompile-artifact**

  The roundtrip .glyph is the post-decompile artifact — the file written by /glyph:decompile from the original's sibling .md.

- **report-is-plain-prose-for-a-human**

  The report is plain prose for a human reader: a summary line, a `Missing` section, a `Changed` section, and an `Added` section. Each heading is omitted when its section has no entries. No JSON, no code fences containing structured data.

- **equivalence-table**

  Cross-read each Glyph construct category by meaning, not by surface form:

  - Skill declaration: same role; parameter count, parameter roles, and defaults match; return type tag matches in intent; presence vs absence of a return type tag on either side is a difference.
  - Per-parameter descriptions: same meaning; surface phrasing differences ignored.
  - `description:` sub-section: same intent.
  - `context:` entries: set-equality by meaning, order ignored.
  - `constraints:` markers: set-equality including polarity; a polarity flip (`require` ↔ `avoid`, soft ↔ hard) is always a difference.
  - `flow:` steps: ordered comparison by intent; inline-string vs block-call vs const-reference factoring is not a difference when the resulting action is the same.
  - Branches (`if`/`elif`/`else`): same predicate intent; same per-arm action set; arm order matters when predicates are mutually exclusive.
  - `block` / `export block` definitions: compared by body intent, not by name (the decompiler may rename); a block in one file with no semantic counterpart in the other is flagged as missing or added.
  - `const` definitions: set-equality by value-meaning; inline-string ↔ named-const refactoring is not flagged when intent matches.
  - `import` statements: not compared directly. The effect — which names resolve in each file — is what matters; flag only when a referenced name has no counterpart on the other side.
  - `type` declarations: compared by attached description meaning.

- **non-differences-to-ignore**

  Surface-only differences the reviewer must ignore and never flag:

  - Block or const naming differences (LLM rename during decompile).
  - Top-level declaration order.
  - Sub-section order within a body (`description:` vs `context:` vs `constraints:` vs `flow:` position).
  - Inline string vs named `const` factoring when intent matches.
  - Extracted `block` vs inline string sequence when actions match.
  - Surface phrasing of any prose.

## Steps

1. Read the Glyph source at {original_glyph} and the Glyph source at {roundtrip_glyph}. Both files are .glyph sources with the declarations and sub-sections defined by the Glyph DSL — `skill`, `block`, `export block`, `const`, `type`, and `import` declarations; `description:`, `context:`, `constraints:`, and `flow:` sub-sections.
2. Cross-read the two files construct by construct rather than line by line. Use the equivalence_table as the rule book for what counts as `equivalent` versus `different` within each Glyph construct category — skill declaration, parameters, per-param descriptions, `description:`, `context:` entries, constraint markers with polarity, `flow:` steps, branches, `block` and `export block` definitions, `const` definitions, `import` statements, and `type` declarations. Mark items present in the original but absent from the roundtrip as `missing`. Mark items present in the roundtrip but absent from the original as `added`. Mark items present in both whose meaning has shifted as `changed`. Items whose surface form differs but whose semantic content is preserved are equivalent — do not report them, and ignore the surface-only differences enumerated in non_differences_to_ignore.
3. Print a human-readable report directly to the user as plain text. Lead with one summary line in the shape `Glyph round-trip review for {original_glyph} vs {roundtrip_glyph} — equivalent` when no semantic differences exist, or `Glyph round-trip review for {original_glyph} vs {roundtrip_glyph} — N differences` otherwise. Then under a `Missing` heading, list each item present in the original but absent from the roundtrip as a short bullet showing the construct category and a brief snippet. Then under a `Changed` heading, list each item whose meaning shifted with a one-line before-and-after snippet. Then under an `Added` heading, list each item present in the roundtrip but absent from the original. Omit a heading whose section has no entries. Never emit JSON, code-fenced data, or any other structured payload — the report is for a human reader. Produce: the human-readable equivalence report shown to the user.

