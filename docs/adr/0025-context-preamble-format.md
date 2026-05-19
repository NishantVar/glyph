# ADR 0025 — Locked Context Preamble Format For Procedure-Tier Blocks

## Status

Accepted (MVP).

## Context

A private `block` (or imported `export block`) may carry body-level
`context <name|string>` markers describing background relevant to the
block's flow — facts the agent should hold while executing the procedure,
not behavioral rules. Until the body-constraints-parity work, those
body-level `context` markers had no rendering at Tier 2 (same-file
procedure section) or Tier 3 (standalone procedure file); the
[[docs/reference/compiled-output]] §Same-File Procedure Sections block
contained an explicit `<optional preamble: scoped constraints + context as
prose>` placeholder.

Two label forms were realistic candidates for rendering a `context` marker
in the preamble:

- **(A) Preserve const identity** — when the marker is a name-ref
  (`context monorepo_layout`, where `monorepo_layout` is a string-valued
  `const`), label the rendered paragraph with the kebab-cased const name:
  `**monorepo-layout:** <resolved-text>.`
- **(B) Always generic** — regardless of whether the operand is a name-ref
  or an inline string, label every entry `**Context:** <text>.`.

The inline-string form (`context "<text>"`) has no const identity to carry
in either design, so it necessarily renders under some generic label.

A separate question was whether the preamble should be byte-identical
between Tier 2 (same-file `### Procedure:` section) and Tier 3 (standalone
procedure `.md`). Diverging would let each tier optimise for its own
context, but would force downstream consumers to learn two preamble
templates and would break the byte-stability guarantee for content that is
otherwise identical across the two projections.

## Decision

Two label forms, chosen per marker by the parsed source — no author switch,
no per-call override.

| Source form | Rendered preamble paragraph |
|---|---|
| `context <ident>` where `<ident>` resolves to a string-valued `const` (name-ref form) | `**<kebab-name>:** <resolved-text>.` |
| `context "<text>"` (inline-string form) | `**Context:** <text>.` |

The kebab transform applied to the const identifier (`monorepo_layout` →
`monorepo-layout`) is the same transform used to derive procedure
filenames from declaration names. No new transform is introduced.

Body-level constraint markers (`require` / `avoid` / `must` /
`must avoid`) in the same preamble reuse the existing four-form template
defined at [[docs/reference/compiled-output]] §Constraint Rendering;
no new template is introduced for them either.

Shape rules:

- Each entry renders as a standalone paragraph, never as a bullet or
  numbered item.
- Entries are separated from each other by a single blank line, and the
  whole preamble is separated from the numbered step list by a single
  blank line.
- A terminal `.` is appended when the entry body does not already end in
  sentence punctuation (same rule as `## Constraints`).
- Entries are **grouped by role**: all constraint entries are emitted
  first (in their source order), then all `context` entries (in their
  source order). The emitter never interleaves constraints and `context`
  entries even if the source order alternates them. Grouping is the
  deliberate stable order — the rendered preamble is therefore predictable
  from the callee's marker set without re-reading source order.
- The preamble is byte-identical between Tier 2 and Tier 3 — the same
  callee produces the same paragraphs regardless of which projection the
  call site selects.

## Consequences

- Compiled output preserves the const identity for name-ref `context`
  markers, so a downstream consumer or `grep` can correlate
  `**monorepo-layout:**` in the rendered Markdown back to the
  `monorepo_layout` const in source. Inline-string `context` markers get a
  uniform, recognisable `**Context:**` label.
- The kebab transform is shared with procedure-filename generation, so the
  rule set stays small and existing implementation code is reused.
- The format is byte-stable in the sense of [[0006-json-output-determinism]]:
  `glyph fmt` and `glyph compile` produce the same preamble bytes across
  runs, and the same callee produces the same preamble in Tier 2 and Tier
  3.
- Phase 6b structural validation ([[docs/architecture/expand]] §Procedure
  section validation) must count only the numbered step list when checking
  per-procedure step count — preamble paragraphs do not contribute to the
  step count. The validator was updated accordingly.

## Alternatives Considered

- **Always-generic label (`**Context:** <text>.` for every operand).**
  Rejected: drops the const identity for name-ref markers, making it
  harder to correlate compiled output back to source — and the savings
  (one uniform template instead of two) are not worth the loss of
  traceability.
- **Author-controlled label switch (`context as "MyLabel" <text>`).**
  Rejected: adds a new surface form for a tiny benefit; the const name
  already carries author intent for the name-ref form.
- **Diverging preambles between Tier 2 and Tier 3** (e.g., a shorter
  preamble inside a same-file procedure section, a fuller preamble in the
  standalone `.md`). Rejected: breaks byte-stability across projections,
  forces downstream consumers to learn two templates, and complicates the
  emitter for no compelling reason.
