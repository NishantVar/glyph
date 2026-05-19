# Compiled Markdown Output — Reference Contract

This document is the **stable external contract** for the Markdown files Glyph produces. Tools, downstream agents, and integrations that consume `glyph compile` output should rely only on what is documented here.

For design rationale and authoring guidance, see [[design/compiled-output]].

## File Layout

`glyph compile <file.glyph>` produces:

- One top-level `.md` per source file (named after the `skill` declaration; library files may emit zero).
- Zero or more procedure `.md` files placed in a subdirectory named after the source file (e.g., `repo_tools.glyph` → `repo_tools/inspect-repo.md`). The procedure filename is the **kebab-case** form of the export block's identifier.

## Frontmatter

Every compiled `.md` begins with YAML frontmatter:

```yaml
---
name: <skill-name>
description: <when this skill should be used>
# effects: [<effect-keyword>, ...]   # only present when --enable-effects
# kind: procedure                    # only present for procedure files
---
```

Fields:

| Field | When present | Type |
|---|---|---|
| `name` | always | string (the skill identifier) |
| `description` | always | string |
| `effects` | only when `--enable-effects` is passed AND the effect set is non-empty | YAML flow-sequence of effect keywords |
| `kind` | only on procedure files | string `procedure` |

The compiler never emits `effects: none`, `effects: []`, or any other placeholder. The field is absent when the gate is off or the effect set is empty.

The compiled file does **not** emit a `# <Skill Name>` heading; the frontmatter `name` is authoritative.

## Body Sections

The compiled file emits peer-level H2 sections in the order determined by the source. Section names and presence rules:

| H2 heading | Presence | Format |
|---|---|---|
| `## Goal` | when `goal:` is declared | One-line statement |
| `## Parameters` | when the skill declares one or more parameters | Bulleted list |
| `## Context` | when `context:` is declared (or freeform sections present in canonical slot) | Bulleted list |
| `## Steps` | when `flow:` has statements | Numbered list |
| `## Constraints` | when constraints are declared | Bulleted list |
| `## <Freeform>` | when the source declares a freeform colon-keyword section (e.g. `quality:` → `## Quality`) | Bullet list or paragraph, shape-determined |

At least one of `## Steps` or `## Constraints` must be present.

Order is canonical-default with source-position override: a sub-section the author wrote moves to its source-relative slot; sub-sections not declared keep the canonical default order (`Goal`, `Parameters`, `Context`, `Constraints`, `Steps`).

### `## Goal`

A one-line statement of the skill's success condition — what "done" looks like for the consuming agent. Emitted when the source declares `goal:` (singular: exactly one inline string or one bare-name `const` reference). The body renders as a single line directly under the `## Goal` heading, with no list marker. `{param}` slots in the source string survive verbatim. No `## Goal` heading is emitted when `goal:` is absent.

### `## Parameters`

Each parameter is one bullet. The full shape grid:

```
- **<name>** (<Type>): <description>. Default: <literal>.
- **<name>** (<Type>): <description>. Required.
- **<name>**: <description>. Default: <literal>.
- **<name>**: <description>. Required.
```

Every parameter bullet carries a description. A parameter that resolves to no effective description (no inline `<"…">`, no type-registry entry, and the LLM expand pass did not fill the span) causes `G::expand::llm-required-for-param-description` at fill time and the file fails to compile. See [[docs/architecture/expand]] §3.5.

Rules:

- The colon after `**name** (<Type>)` appears only when a description follows.
- A description longer than ~120 characters or containing a newline renders as a multi-line bullet (continuation indented two spaces).
- `Default: <literal>` shows the resolved literal value (named-reference defaults like `default_temperature` are inlined to their concrete value).
- `Required.` marks parameters without a default — for `skill`s, these must be supplied from user context at runtime; for procedure files, they must be supplied by the caller.

### `## Context`

Bulleted list. Each entry is one column-0 `- ` bullet:

- Inline-string entries put the body directly after the bullet.
- Bare-name (`const`) entries lead with a bold kebab-case label on the bullet's first line, a blank line, then a two-space-indented body. Multi-paragraph bodies, nested lists, and code spans inside the body are preserved verbatim.

### `## Steps`

Numbered list. Each item is one instruction. The `return` expression folds into the final item (see Return Folding below); there is no separate `## Output` section.

`{param}` slots appear verbatim — consuming agents resolve them from runtime context.

Conditional logic projects to a **single numbered Step with lettered sub-steps per arm**:

```md
3. If the risk is high and tests exist:
   a. Run the full test suite.
   b. Request a code review.
   If the risk is high but no tests are available:
   a. Flag for manual review.
   Otherwise:
   a. No action needed.
```

Letters reset per arm. Only one level of structured sub-steps is supported; deeper nesting flattens to prose.

#### Predicate-Driven Branch Projection

When a Branch's arm conditions are written as natural-language predicate forms, the compiled rendering selects one of two prose shapes:

- **Pure-predicate form.** When every arm's condition is *purely* one or more predicate-form tokens combined by `or`, the Step is introduced with a fixed lead-in and each arm is rendered as a lettered sub-step keyed by the resolved predicate prose. Example:

  ```md
  3. Decide which of the following applies and follow only that path:
     a. When the user asks to fork a terminal pre-loaded with a plan: identify the plan content, save it to disk, and fork the agentic tool with delayed input.
     b. When a complex change is required: plan the full edit sequence before touching any file.
     Otherwise:
     c. Understand the user's request and route to the appropriate launcher.
  ```

- **Mixed-condition form.** When an arm's condition combines predicate-form tokens with boolean operators or non-predicate names, the resolved predicate prose inlines into the standard `If <condition>:` arm header (e.g., the condition `complex_change_required and not is_dry_run` produces "If a complex change is required and this is not a dry run:"). Sub-steps follow the lettered convention above.

The two forms compose within a single Branch: each arm independently selects its header form, all under one numbered Step. See [[design/compiled-output]] §Predicate-Driven Branch Projection for the full discussion.

#### Branch-Scoped Constraints Inlining

A `require`/`avoid`/`must` marker that appears inside an `if`/`elif`/`else` branch in `flow:` is **inlined into the prose of an adjacent sub-step** within that arm. It is **not** emitted in `## Constraints` and **not** given its own lettered sub-step. The inlined wording makes the conditional applicability explicit (e.g., a sub-step like "Run the migration, never dropping existing columns."). Only flow-top-level and body-level constraints hoist to `## Constraints`. See [[design/compiled-output]] §Constraint Rendering for the full discussion.

### `## Constraints`

Bulleted list. Constraint wording uses a bold colon-marker template:

| Strength × Polarity | Template |
|---|---|
| `must` (hard require) | `**Must:** <text>` |
| `must avoid` (hard avoid) | `**Must avoid:** <text>` |
| `require` (soft require) | `**Require:** <text>` |
| `avoid` (soft avoid) | `**Avoid:** <text>` |

The body is preserved verbatim. A terminal `.` is appended only when the body does not already end in sentence punctuation.

### Freeform Sections

A source `freeform_keyword:` section (any colon-keyword not in the built-in catalogue) projects to `## <Title-Cased Keyword>` at peer-H2 level (or `####` when nested inside a Tier-2 procedure). Underscores in the keyword become spaces; each word is title-cased.

Shape:

- **Bullet list** — when the body has more than one item, or any reserved marker clause (`require`, `avoid`, `must`, `must avoid`, `context`), or a bare-name reference to a string-valued `const`.
- **Paragraph** — when the body has exactly one inline string and nothing else.

Reserved marker clauses inside a freeform body render through the same four-form template as `## Constraints`; they do **not** hoist to `## Constraints` itself.

## Three-Tier Block Projection

When a `Call` targets a block, the compiler chooses one of three projections:

| Tier | Projection |
|---|---|
| 1 — Inline | Body becomes Step prose. Eligible only when expanded prose is < 150 words. |
| 2 — Same-file procedure | `### Procedure: <name>` section after the body H2s, nested under whichever H2 came last. |
| 3 — External file | A `Load and follow the procedure in \`<path>\`.` Step prose pointing at a separate `.md` file. |

Tier selection rules:

- 1+ flow statements, no own constraints, no body-level context, called once, < 150 words → Tier 1.
- 4+ flow statements OR own constraints OR any body-level constraint markers OR any body-level `context` markers OR called 2+ times in the same skill → Tier 2.
- Imported `export block` inside a Branch arm, OR shared across multiple skills → Tier 3.
- Tier promotion is one-directional: Tier 1 → Tier 2 → Tier 3.

The 150-word threshold is hard-coded; the word counter treats backticked code spans as 1 word each and ignores Markdown formatting markers.

### Same-File Procedure Sections

```md
### Procedure: review-code

**Must:** Never modify generated files.

**Require:** Read the diff before commenting.

**monorepo-layout:** This codebase uses a monorepo layout with per-crate Cargo.toml files.

**Context:** Reviewers should prioritize public API changes.

1. <numbered list, expanded from the callee's flow>
```

Heading: `### Procedure: <kebab-case-callee-name>`. The kebab-case heading uses the same name as the on-disk procedure filename.

Referencing Steps include a parenthetical cross-reference (e.g., "(follow the review-code procedure below)").

The optional preamble paragraphs between the H3 heading and the numbered step list are governed by [§Procedure Preamble](#procedure-preamble-tier-2-and-tier-3) below.

### External Procedure Files

A procedure file has the same shape as a skill — frontmatter (with `kind: procedure`), then peer-level H2 body sections. The Step that references it uses the **locked template**:

```md
Load and follow the procedure in `<relative-path>`.
```

Inside a conditional branch arm, the same template appears as a lettered sub-step.

When the referenced `export block` declares body-level constraint or `context` markers, the standalone procedure `.md` carries the same preamble described in [§Procedure Preamble](#procedure-preamble-tier-2-and-tier-3), positioned between `## Parameters` (when present) and `## Steps`.

### Procedure Preamble (Tier 2 and Tier 3)

When a Tier 2 same-file procedure section, or a Tier 3 standalone procedure file, derives from a callee whose body declares body-level constraint markers (`require` / `avoid` / `must` / `must avoid`) or body-level `context` markers, those markers render as a **preamble** of standalone paragraphs immediately before the numbered step list (Tier 2: between `### Procedure: <name>` and the numbered list; Tier 3: between `## Parameters` (when present) and `## Steps` in the standalone `.md`).

**Constraint entries.** Body-level constraint markers reuse the four-form bold-label template defined in [§`## Constraints`](#-constraints) — no separate template:

| Strength × Polarity | Template |
|---|---|
| `must` (hard require) | `**Must:** <text>` |
| `must avoid` (hard avoid) | `**Must avoid:** <text>` |
| `require` (soft require) | `**Require:** <text>` |
| `avoid` (soft avoid) | `**Avoid:** <text>` |

**`context` entries.** `context` markers carry one of two label forms depending on the operand:

| Source form | Rendered preamble paragraph |
|---|---|
| `context <ident>` where `<ident>` resolves to a string-valued `const` (name-ref form) | `**<kebab-name>:** <resolved-text>` |
| `context "<text>"` (inline-string form) | `**Context:** <text>` |

The kebab-case label on the name-ref form is derived from the `const` identifier by the same kebab transform used for procedure filenames (e.g., `monorepo_layout` → `monorepo-layout`). The inline-string form always renders under the generic label `**Context:**`.

**Shape and ordering.**

- Each entry renders as its own paragraph — never as a bullet or a numbered item.
- Entries are separated from each other by a single blank line, and the entire preamble is separated from the numbered step list by a single blank line.
- A terminal `.` is appended only when the entry body does not already end in sentence punctuation (same rule as `## Constraints`).
- Entries are **grouped by role**: all constraint entries are emitted first (in their source order, using the four-form template above), then all `context` entries (in their source order, using the two label forms above). The emitter never interleaves constraints and `context` entries even if the source order alternates them.
- The preamble is byte-identical between Tier 2 and Tier 3 — the same callee produces the same paragraphs regardless of which projection the call site selects.

**Validator interaction.** Preamble paragraphs are **not** counted as Steps by the procedure-section step-count validation ([[docs/architecture/expand]] §Procedure section validation). The Step count for a procedure section equals the number of items in the numbered list only.

## Return Folding

`return <expr>` in `flow:` folds into the final numbered Step rather than producing a `## Output` section.

| Return form | Suffix template | Standalone template (return-only body) |
|---|---|---|
| Identifier (`return <name>`) | `, and return that as your result.` | `Return <name-as-words> as your result.` |
| Description (`return <"…">`) | `, and return <description> as your result.` | `Return <description> as your result.` |

The literal `<name>` or `<"…">` token must never appear in compiled Markdown. Agent-typed returns (`return <agent>`) say "Your result is the <agent> agent spawned above"; the compiler does **not** interpret this as returning the agent's findings.

## Parameter Slots

Step and Constraint prose may contain `{param}` slots:

- A `{param}` slot whose `param` matches a declared parameter is preserved verbatim. The consuming LLM substitutes at runtime.
- A `{name}` slot referring to a local binding does **not** survive — it resolves to a natural-language cross-reference in compiled prose.
- The slot grammar is strict `{IDENTIFIER}` only; arbitrary brace content is literal text.

## Formatting

- One instruction per list item (except the final Step, which may carry the return-fold suffix).
- Numbered lists for Steps, bulleted lists for Context and Constraints. Parameters and freeform sections follow their own shape rules.
- No hard line-wrapping mid-sentence in Steps or Constraints. Context entries may include multi-paragraph bodies with continuation indented two spaces.
- Single blank line between sections. No trailing whitespace.
- Standard Markdown only: headings, lists, bold, code spans. No inline HTML.

## Authoring Constructs That Disappear

The following source constructs do **not** appear in compiled output:

- `import` statements and `@glyph/` namespace references.
- `const` / `export const` references — the resolved body inlines.
- `generated const` / `generated block` declarations — the `generated` marker is stripped; only the expanded content appears.
- `with "modifier"` clauses — consumed during expansion to shape Step wording.
- Parameter slots for non-parameter local bindings — resolved into prose.
- Provenance comments — none are emitted.

Unused imports are auto-removed from the source `.glyph` file (source-to-source) before compilation continues.

## Stability

This contract is intended for downstream consumers of compiled Markdown. Changes to the shape, naming, or templates documented here are breaking changes and will be versioned accordingly.
