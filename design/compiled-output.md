# Glyph Compiled Output

This document defines the shape of compiled Markdown files that the Glyph compiler emits. It covers the MVP output format: a `.glyph` source file compiles into a same-basename `.md` file that serves as the executing agent's skill instructions. Compilation is parameterless — parameters appear as named slots resolved by the consuming LLM at runtime.

## Guiding Principles

- **Reliability beats elegance** (foundations). Favor explicitness, clarity, and followability over compression or style.
- **Targets agents broadly** (foundations). The output must be consumable by general-purpose agents, not tied to one execution environment.
- **Authoring and execution are separate** (foundations). Source constructs compile away completely. The compiled file is self-contained agent instructions.
- **The IR is the semantic contract** (foundations). Compiled output is a projection of the IR, not a direct transformation of source.
- **Novice learnability** (foundations). Compiled output stays radically simple — frontmatter plus a handful of peer-level body H2s (`## Context`, `## Steps`, `## Constraints`) — so new authors see exactly how their source maps onto agent-facing Markdown.

## Parameterless Compilation

MVP compilation is parameterless. `glyph compile skill.glyph` produces one `.md` file per source file, regardless of how the skill will be invoked. Parameters are not resolved at compile time — they appear in the compiled output as named slots that the consuming LLM resolves from user context at runtime.

Practical consequences:

- The `.glyph` source is the authoring artifact; it is what authors share, import, and version.
- The `.md` compiled output is a single, stable artifact per source file. There is no argument-dependent variation.
- The compiled file contains a `## Parameters` section listing each parameter with its name, a brief description, and either a default value or a `(required)` marker. Steps and Constraints may reference parameters by name using `{param}` syntax.
- The consuming LLM reads the Parameters section, resolves each parameter from the user's request context (falling back to the listed default if one is provided, or asking the user when a required parameter cannot be inferred), and executes the Steps with those values in mind.
- Since compilation is parameterless, there is no need for a separate "abstract card" output — the compiled file already serves that role.

## Source-To-Compiled-Output Mapping

Every source form maps to exactly one compiled location. This is the authoritative mapping.

| Source form | Compiled location |
|-------------|-------------------|
| `skill <name>` | Frontmatter `name` |
| `description:` | Frontmatter `description` |
| `effects:` (declared or inferred) | Frontmatter `effects` (YAML list) — *gated, requires `--enable-effects`; field absent when gate is off* |
| `flow:` steps (non-`return`) | `## Steps` |
| `return <expr>` in flow | Closing sentence of the final `## Steps` item |
| `constraints:` content + body-level markers | `## Constraints` |
| `context:` content | `## Context` (before `## Steps`) |
| Header parameters + defaults | `## Parameters` section (names, descriptions, defaults or `(required)` marker) |

Constraint strength (`soft`/`hard`) and polarity (`require`/`avoid`) affect compiled wording and prominence per [[ir-and-semantics]].

## Frontmatter

Every compiled file starts with YAML frontmatter. Two fields in MVP (three when `--enable-effects` is passed):

```yaml
---
name: <skill-name>
description: <when this skill should be used>
# effects: [<effect-keyword>, ...]   ← only when --enable-effects
---
```

- `name` — the skill identifier, taken from the `skill` declaration name. Machine-readable, used for skill selection and referencing.
- `description` — a concise statement of when and why an agent should use this skill. Primary trigger for coding agents that select skills from frontmatter. Sourced from the `description:` sub-section (see [[ir-and-semantics]]). If the source omits `description:`, the repair pass generates one from the skill name and body and adds it to the source as a `description:` sub-section.
- `effects` — *(Gated — requires `--enable-effects`; field omitted entirely when the gate is off.)* YAML flow-sequence list of the skill's full inferred effect set. **Omitted unconditionally when the effect set is empty** — that is, when the skill has no meaningful effects or is explicitly `effects: none`. The compiler never emits `effects: none`, `effects: []`, or any other "no effects" placeholder; the field is simply absent. An absent `effects` key and `effects: none` are operationally identical for the consuming agent, and omitting is one fewer surface and one fewer ambiguity. Effects live in frontmatter so selectors and routing tools can read them without parsing the body; they are not repeated in the prose.

The compiled file does not emit a `# <Skill Name>` heading. The frontmatter `name` is the authoritative title.

## Sections

MVP compiled output emits peer-level H2 sections in canonical order: `## Parameters` (conditional), `## Context` (conditional), `## Steps`, `## Constraints` (conditional). No `## Instructions` wrapper heading is emitted; body sections sit at the same level as `## Parameters`. Section order is canonical-default with explicit-source-position override; see §Output Order below for the merge rule.

Deferred sections (`## Output`, `## Effects` as a prose section, `## When To Use`) are logged in [[todo]] for possible post-MVP restoration.

### `## Parameters`

Emitted when the skill declares one or more parameters. Omitted for parameterless skills. Each item is a single bullet whose shape depends on whether the parameter has a description (per-param `<"…">` or via a `type Name = <"…">` decl in scope), a type annotation, and a default.

```
- **<name>** (<Type>): <effective description>. Default: <literal>.
- **<name>** (<Type>): <effective description>. Required.
- **<name>** (<Type>). Default: <literal>.                     // no description in scope
- **<name>** (<Type>). Required.                               // no description in scope
- **<name>**: <effective description>. Required.               // no type annotation
- **<name>**. Required.                                        // no type, no description
```

The colon after `**name** (<Type>)` is present only when a description follows; otherwise the type stands alone followed by a period and the next metadata sentence.

Concrete example. Source:

```glyph
type RiskLevel = <"one of: low, medium, high; severity of the change">
type RepoContext = <"the inspected repo state, including file tree and dependencies">

skill fix_bug(
    scope: PathSpec = ".",
    risk: RiskLevel = "medium" <"raise to 'high' if fix touches auth or data layer">,
    repo_ctx: RepoContext,
    target = <"path to the report file">,
)
```

Compiled `## Parameters`:

```
## Parameters
- **scope** (PathSpec). Default: ".".
- **risk** (RiskLevel): raise to 'high' if fix touches auth or data layer. Default: "medium".
- **repo_ctx** (RepoContext): the inspected repo state, including file tree and dependencies. Required.
- **target**: path to the report file. Required.
```

Notes on the example:
- `scope: PathSpec` has no description anywhere → render type only.
- `risk` has both type-level (`RiskLevel`) and per-param description → per-param wins.
- `repo_ctx` has only type-level → type-level used.
- `target` is untyped with a per-param description → no `(Type)` rendered.

#### Block-string descriptions

When the effective description is a `"""…"""` block string, render the parameter as a multi-line list item:

```glyph
type RiskLevel = <"""
The severity of a planned change. One of: low, medium, high.

low    = isolated, well-tested, reversible
medium = touches public API or shared modules
high   = touches auth, data, or destructive ops
""">
```

Renders as:

```
- **risk** (RiskLevel):
  The severity of a planned change. One of: low, medium, high.

  low    = isolated, well-tested, reversible
  medium = touches public API or shared modules
  high   = touches auth, data, or destructive ops

  Default: "medium".
```

**Trigger for the multi-line form:** the description contains a newline OR exceeds ~120 chars. Otherwise inline.

The consuming LLM reads this section before executing the Steps. For optional parameters, it resolves each from the user's request context and falls back to the listed default if the user does not specify a value. For required parameters, it must extract a value from context; if the user has not supplied enough information to determine the value, the LLM should ask the user before proceeding. Parameter descriptions are guidance for the LLM, not rigid schemas.

### Body Sections (`## Context`, `## Steps`, `## Constraints`)

Body sections sit at H2, peer to `## Parameters`. No `## Instructions` wrapper heading is emitted:

- **`## Context`** — bulleted list of background information. Passive framing the agent should understand during execution. Each context entry projects to one column-0 `- ` bullet; multi-line bodies indent continuation lines by two spaces so each entry remains a single Markdown list item. When the source entry was a bare-name reference to a `const` / `export const` (rather than an inline string), the bullet leads with a bold **kebab-case label** (the source name) on its own line, followed by a blank line, followed by the indented body. The label gives consuming agents a stable per-entry handle and matches the kebab-case convention used by `### Procedure: <name>`.
- **`## Steps`** — numbered list (order matters). Each item is one instruction. The `return` expression from the source folds into the final item rather than producing a separate section.
- **`## Constraints`** — bulleted list (order usually does not matter). Each item is one `Constraint` node. Strength (`soft`/`hard`) and polarity (`require`/`avoid`) affect wording, not placement in MVP.
- **`### Procedure: <name>`** — zero or more procedure sections for blocks projected at Tier 2 (same-file procedure). These stay at H3, nested under whichever body H2 came last. Each contains a numbered list of the callee's expanded flow, with an optional constraint preamble. See §Three-Tier Block Projection for format and ordering rules.

`## Context`, `## Steps`, and `## Constraints` are conditional: `## Context` is omitted when no `context:` is declared; `## Constraints` is omitted when there are no explicit constraints; `## Steps` may be omitted only for pure instruction-only skills (all content is constraints). At least one of `## Steps` or `## Constraints` must be present — `## Context` alone is not sufficient for a valid skill. `### Procedure:` sections are conditional on the projection tier selected for each callee.

```md
## Context

- This codebase follows a monorepo layout with shared internal packages.

- **project-conventions**

  Multi-paragraph context entries (typically imported `export const` bodies)
  project as one column-0 bullet whose body is indented by two spaces.

  - Internal bullets in the body stay nested under the entry's bullet.
  - Headings, numbered lists, and code spans inside the body are preserved
    verbatim and read as part of the same Context entry.

## Steps

1. Inspect the failure and reproduce it.
2. Identify the root cause before proposing a fix.
3. Patch minimally and report the summary.

## Constraints

- Do not make unrelated edits outside the requested scope.
- Follow the repository's existing patterns before introducing new abstractions.
```

## Projection Rules

Compiled output projects from the typed IR role model defined in [[ir-and-semantics]]. See that file for role semantics. This section covers only the output-side rules: which location each role projects into, formatting, and ordering.

| IR role / metadata | Compiled target | Format |
|--------------------|-----------------|--------|
| Skill name | Frontmatter `name` | String |
| Skill description | Frontmatter `description` | String |
| Effect set | Frontmatter `effects` | YAML list; field omitted if effect set is empty or `none`. *Gated — requires `--enable-effects`; omitted entirely when gate is off.* |
| `Context` | `## Context` | Bulleted list, one column-0 `- ` per IR `Context` node; body is line-wise 2-space-indented under the bullet. NameRef entries lead with a bold kebab-case label on the bullet's first line; inline-string entries place the body directly after `- ` |
| `Step` | `## Steps` | Numbered list, one concrete instruction per item |
| `Constraint` | `## Constraints` | Bulleted list, wording shaped by constraint keyword (`require`/`avoid`/`must`/`must avoid`) |
| `InputContract` + parameters | `## Parameters` section (names, descriptions, defaults or `(required)` marker) | Bulleted list |
| `OutputContract` + `return` | Closing sentence of the final `## Steps` item | No dedicated section |
| Block call (referenced) | `### Procedure: <name>` section | Numbered list with optional constraint preamble |
| Block call (external) | "Load and follow `<path>`" in Step prose | File path reference |

### Output Order

The compiler emits section H2 blocks in an order determined jointly by a **canonical-default position list** and the **source-position of each sub-section the author declared**. Sub-sections not declared in source fall back to the canonical position; declared sub-sections override the canonical position when their source-position implies a different slot.

The behavior, stated for authors:

1. The canonical output order, top to bottom, is `description (→ Parameters)`, `context (→ ## Context)`, `constraints (→ ## Constraints)`, `flow (→ ## Steps)`. `## Parameters` is injected from the skill header rather than from a sub-section.
2. Each sub-section the author declared lands at a slot consistent with its source order. A sub-section declared before any other sub-section keeps the canonical default; a sub-section declared after another section must emit its H2 after that section's H2 in the compiled `.md`.
3. Freeform sections (§Freeform Sections below) participate on equal footing with built-in sub-sections — their `## Heading` is emitted at the freeform's source-relative slot.
4. When a sub-section is declared multiple times in source, the merged H2 lands at the position of the **first** occurrence; the merge concatenates the bodies in source order.

**Worked example.** Source:

```glyph
skill demo()
    description: "Demo skill."
    quality:
        require accuracy
        "Prefer minimal diffs."
    flow:
        "Investigate."
```

The `quality:` freeform section was declared between `description:` and `flow:` in source. Compiled output:

```md
---
name: demo
description: 'Demo skill.'
---

## Quality

- Accuracy.
- Prefer minimal diffs.

## Steps

1. Investigate.
```

`## Quality` lands between the frontmatter and `## Steps` because the author placed `quality:` before `flow:`. Had they written `quality:` after `flow:`, the compiled output would render `## Steps` before `## Quality`.

**Consequence for `glyph fmt`.** Because compiled order tracks source order, `glyph fmt` does **not** reorder sub-sections. fmt's contract is "no section reordering, no marker hoisting across section boundaries"; the body-level marker hoisting from §4.2a of [[language-surface]] still happens, but hoisted markers never cross a named section boundary.

### Freeform Sections

A *freeform colon-keyword* section (e.g. `quality:`, `risks:`, `acceptance_criteria:`) is any sub-section header at body-level whose name is not in the built-in catalogue (`description`, `effects`, `context`, `constraints`, `flow`). See [[language-surface]] §2.5b for the source-side authoring rules and §Output Order above for placement in the compiled `.md`.

**Heading projection.** A freeform section projects to a peer-level `## Heading` block. The heading is derived from the source colon-keyword by replacing underscores with spaces and title-casing each word: `quality:` → `## Quality`, `acceptance_criteria:` → `## Acceptance Criteria`, `risks:` → `## Risks`.

**Shape-detection rule.** The body grammar of a freeform section mirrors `context:` ([[ir-and-semantics]] §`context:` Section). The compiler examines the section's content items to choose between two rendering shapes:

- **Bullet list shape** — when the section contains more than one item, OR contains any reserved-marker clause (`require`, `avoid`, `must`, `must avoid`, `context`), OR contains a `NameRef` to a string-valued `const`. Each item projects to a `- ` bullet. Marker clauses render through the same four-form template as `## Constraints` (§Constraint Rendering); `context X` and bare-name refs follow `## Context` formatting.
- **Paragraph shape** — when the section contains exactly one inline string and no other items. The string projects as a free-standing paragraph directly under the `## Heading`.

The shape is deterministic given the body; the author does not select it.

**Depth by tier.** When the host declaration is a `skill`, the freeform `## Heading` emits at `##` depth (peer to `## Steps`). When the host declaration is a `block` projected as Tier 2 (same-file procedure under `### Procedure: <name>`), the freeform heading emits at `####` depth, nested under the procedure heading. When the host is a Tier 3 external `export block` (its own `.md` file), the freeform heading emits at `##` depth in the procedure file. A Tier 1 inlined block cannot carry freeform sections — the compiler forces Tier 2 promotion for any block that declares a freeform section (see §Three-Tier Block Projection below).

**Marker semantics inside freeform.** The five reserved marker clauses (`require`, `avoid`, `must`, `must avoid`, `context`) carry their normal semantics inside a freeform body — strength/polarity for `require`/`avoid`/`must`/`must avoid`, context-projection for `context`. They are not "hoisted out" of the freeform section to the canonical `## Constraints` or `## Context` heading; they render under the freeform's own `## Heading`. This is the rule that makes freeform sections useful: an author who wants a `quality:` section showing both pass-criteria markers and prose can use the marker syntax for the deterministic four-form rendering and the prose for context-as-paragraph.

### Three-Tier Block Projection

When a call targets a block (same-file or imported), the compiler chooses one of three projection tiers based on callee complexity, conditionality, and reuse. The decision is deterministic and is fixed before any LLM-driven prose reshaping.

| Condition | Tier | Projection |
|-----------|------|------------|
| Callee body has 1 flow statement, no own constraints, called once, **expanded prose < 150 words** | **Inline** | Body becomes Step prose (default behavior) |
| Callee body has 2–3 flow statements, no own constraints, called once, **expanded prose < 150 words** | **Inline** | Body concatenated into one Step paragraph |
| Callee body has 4+ flow statements | **Same-file procedure** | `### Procedure: <name>` section nested under the last body H2 |
| Callee declares its own constraints (any flow count) | **Same-file procedure** | Constraints need a scoping home in the procedure preamble |
| Callee is called 2+ times in the same skill (same-file block) | **Same-file procedure** | Avoids prose duplication |
| Imported `export block` called inside a `Branch` | **External file** | Might not be needed — defers context cost until the branch is taken |
| Imported `export block` called from multiple skills in the same project | **External file** | Compile once, reference everywhere |
| Imported `export block` called unconditionally, not shared | **Same-file procedure** | Always needed, keep it nearby |

**Word count threshold.** The tier heuristic includes a word count check on the callee's expanded prose to guard the Tier 1 boundary:

- **< 150 words**: eligible for Tier 1 (inline). Small enough to fold into a single Step paragraph.
- **>= 150 words**: not eligible for Tier 1. Structural heuristics (statement count, constraints, call count) determine Tier 2 vs. Tier 3.

Size alone does **not** trigger Tier 3. A 600-word block that is unconditional and single-consumer projects as Tier 2 (same-file procedure). Tier 3 is reserved for blocks that are **conditional** (inside a `Branch` — defers context cost until the branch is taken) or **shared** (called from multiple skills — single source of truth). The rationale: for unconditional loads, externalizing to Tier 3 does not reduce runtime context — the agent reads the external file anyway — so the structural complication of a separate file must be justified by conditionality or sharing, not size.

Word counts are computed after the callee's prose is resolved — the earliest point where the actual expanded text is available. Promotion is one-directional: Tier 1 → Tier 2 → Tier 3, never downward. A block initially assigned Tier 1 by statement count but exceeding 150 words is promoted to Tier 2.

**Cross-file word-count sourcing.** When the call site is in a downstream skill and the callee is an imported `export block`, the consumer cannot recompute the callee's word count from scratch — it does not own the callee's resolved expanded prose. The library's own compilation computes the word count once per export block, and the multi-file build propagates that count to consumers in dependency order. For same-file callees, the word count is computed directly from the local resolved prose.

**Word counting rule.** A "word" is a whitespace-separated token in the resolved Step prose. Backticked code spans count as 1 word each (one ident-blob = one unit of cognitive load). Markdown formatting markers (`**`, list bullets, headings) do not count. Comments are stripped before counting.

**Configurability.** The 150-word threshold is hard-coded for MVP — not exposed via project config. The load-bearing properties are determinism and documentation; the exact value is tunable post-MVP from real-corpus telemetry. See [[todo]].

Conditions are checked top-to-bottom; the first `referenced` or `external` trigger wins. The tier is a property of the *(callee, skill)* pair — a block called once in skill A might inline, but the same block called twice in skill B gets a procedure section.

**Library file emission.** Library files emit standalone procedure `.md` files for `export block` declarations whose expanded prose is >= 150 words (i.e., above the Tier 1 inline threshold). These land in a subdirectory named after the source file (e.g., `repo_tools.glyph` → `repo_tools/inspect-repo.md`). Export blocks below the threshold emit nothing from the library — consumers inline them. Note: a procedure `.md` may exist on disk but go unused at a consumer call site that projects the block as Tier 2 (same-file procedure) rather than Tier 3 — this is intentional, not an error. See [[language-surface]] §File-Level Rules for the full library emission model.

#### Same-File Procedure Sections

A `### Procedure: <name>` H3 section appears after the body H2s (`## Context`, `## Steps`, `## Constraints`), nested under whichever body H2 came last:

```md
## Steps

1. Gather the relevant files in {scope}.
2. Review the code for issues (follow the review-code procedure below).
3. Summarize findings and return that as your result.

## Constraints

- Do not make unrelated edits outside {scope}.

### Procedure: review-code

Do not introduce new abstractions during review.

1. Scan for style violations and anti-patterns.
2. Check for security vulnerabilities.
3. Check for performance issues in hot paths.
4. Compile a list of findings with severity ratings.
```

**Format rules:**

- H3 heading: `### Procedure: <callee-name>`. The callee name in the heading is the **kebab-case** form derived from the source `snake_case` identifier — replace each `_` with `-` and apply no other transformation. For an `export block summarize_section`, the heading is `### Procedure: summarize-section`.
- Optional preamble paragraph: the callee's scoped constraints and context, rendered as prose sentences (not bulleted — they are contextual to this procedure, not top-level skill constraints). If the callee declares its own `context:`, the context items appear in the preamble alongside any scoped constraints.
- Numbered list: the callee's flow statements, expanded the same way skill-level Steps are.
- Return folding: if the callee has a `return`, it folds into the last numbered item of the procedure (same rule as skill-level return).
- Ordering: procedure sections appear after the body H2s (`## Context`, `## Steps`, `## Constraints`), in the order of first reference from `## Steps`. They nest as H3 under whichever body H2 came last.

**Referencing from Steps:** The referencing Step includes a parenthetical cross-reference — e.g., "(follow the review-code procedure below)" or "(see the review-code procedure above)." The compiler chooses natural phrasing. The reference must include the procedure name so the link can be verified at validation time.

**Multiple references to the same procedure:** The procedure section appears once. Multiple Steps reference it. When called with different `with` modifiers, the modifier shapes the referencing Step's prose, not the procedure section — the procedure stays generic:

```md
1. Review the auth module for security vulnerabilities (follow the review-code procedure below).
2. Review the API layer for contract violations (follow the review-code procedure above).
```

#### External Procedure Files

When the compiler selects the external-file tier, the imported `export block` compiles to a standalone `.md` procedure file. The referencing skill's Step directs the consuming agent to load the file at runtime.

**Procedure file format:** Identical to a skill's compiled format — YAML frontmatter, optional `## Parameters`, then peer-level body H2s (`## Context`, `## Steps`, `## Constraints`). The frontmatter carries `kind: procedure` to distinguish from top-level skills:

```md
---
name: review-code
kind: procedure
description: Systematic code review procedure.
effects: [reads_files]
---

## Parameters
- **targets**: Files to review

## Steps

1. Scan the target files for style violations and anti-patterns.
2. Check for security vulnerabilities.
3. Check for performance issues in hot paths.
4. Compile a list of findings with severity ratings.

## Constraints

- Do not introduce new abstractions during the review.
```

**File output path:** Procedure files are placed in a subdirectory named after the source file. The procedure filename is the **kebab-case** form of the export block's `snake_case` identifier (each `_` → `-`, no other transformation). E.g., `review_tools.glyph` containing `export block review_code(...)` produces `review_tools/review-code.md`. The `.glyph` infix from the source filename is dropped for compiled artifacts: source files are `*.glyph`, compiled outputs (top-level skills and procedure files alike) are `*.md`. The same kebab-case rule governs both the on-disk filename and the H3 heading inside same-file procedure sections (see §Same-File Procedure Sections), so a given block always renders under a single canonical name regardless of projection tier.

**Referencing from Steps (locked template):** The Step prose for an external-file Call is the locked template `` Load and follow the procedure in `<procedure_path>`. ``. The compiler renders this verbatim; the LLM is not involved for the top-level case. When inside a conditional branch arm, the same locked template is emitted as a sub-step within the arm's prose:

```md
2. Load and follow the procedure in `review_tools/review-code.md`.
```

```md
3. If the files have security concerns:
   a. Load and follow the procedure in `review_tools/review-code.md`.
```

**`with` modifier interaction:** The `with` modifier shapes the referencing Step's prose (e.g., "focusing on security vulnerabilities"), not the external procedure file. The procedure file is compiled independently and stays generic. The consuming agent applies the Step's emphasis while following the procedure.

**Effect implication:** Referencing an external file implies `reads_files` on the skill's effect set. The compiler infers this automatically when selecting the external-file tier. If the author declared `effects:`, it must include `reads_files` or the compiler emits an error.

**Deployment:** A compiled project may produce multiple files — one `.md` per skill, plus procedure files for externally projected blocks. The `glyph compile` command produces all files in a single output directory.

### Constraint Rendering

Constraint text is rendered through a **bold colon-marker template**. The (strength, polarity) tuple selects the label; the LLM never produces constraint prose. The label is grammatically isolated from the body by the `:` boundary, so the body can be any natural-language shape — declarative, gerund, noun phrase — without the emitter trying to graft a verb onto it. The author owns capitalization; the body is preserved verbatim with a terminal `.` appended only when the body does not already end in sentence punctuation.

| Strength × Polarity | Template |
|---|---|
| `must` (hard require) | `**Must:** <text>` |
| `must avoid` (hard avoid) | `**Must avoid:** <text>` |
| `require` (soft require) | `**Require:** <text>` |
| `avoid` (soft avoid) | `**Avoid:** <text>` |

There is no fallback rendering. Because the polarity label sits in a bold span and is separated from the body by a colon, ungrammatical compositions like `Avoid routing is by …` (declarative body grafted to a verb prefix) are no longer possible — the body simply reads as its own clause after the label.

Strength is advisory prose framing — the wording surfaces non-negotiability for `hard` forms and standard obligation for `soft` forms — but compliance by the consuming agent is not enforced by the compiler.
- **Conditional logic** (`if` in source) projects to a **single numbered Step** with **lettered sub-steps per arm**. Each arm is introduced by a condition header (`If <condition>:`, or `Otherwise:` for `else`), and each Step-projecting node inside the arm becomes a lettered sub-step (`a.`, `b.`, `c.`). Letters **reset per arm**. This preserves the structure of conditional instructions without using code-like syntax. Example:

  ```md
  3. If the risk is high and tests exist:
     a. Run the full test suite.
     b. Request a code review.
     If the risk is high but no tests are available:
     a. Flag for manual review.
     Otherwise:
     a. No action needed.
  ```

  **Nested branches** (a `Branch` inside another `Branch`'s arm) do **not** receive their own sub-step structure. Instead, they flatten into prose within their parent sub-step (e.g., "If the codebase has public APIs, check backwards compatibility and update the changelog. Otherwise, run internal validation."). Only one level of structured sub-steps is supported. The Repair pass auto-extracts deeply nested branches into helper `generated block` declarations to keep compiled output clean (see [[design/repair]] §4.9).

- **Branch-scoped constraints.** A `require`/`avoid`/`must` marker that appears inside an `if`/`elif`/`else` branch in `flow:` is **inlined into the prose of an adjacent sub-step**, not surfaced in `## Constraints` and not given its own lettered sub-step. The inlined wording makes the conditional applicability explicit (e.g., a sub-step like "Run the migration, never dropping existing columns."). Only flow-top-level constraint markers (and body-level constraints declared above `flow:`) hoist to the skill's top-level constraints and render in `## Constraints`. See [[docs/architecture/ir-semantics]] §Body-Level Constraint Normalization for the hoisting rules.

- **Branch-scoped context.** A `context:` declaration inside an `if`/`elif`/`else` branch in `flow:` follows the same pattern as branch-scoped constraints: it is **inlined into the prose of an adjacent sub-step** (e.g., "Note: this module handles authentication only."), not surfaced in `## Context`. Only skill-level `context:` declarations render in the `## Context` section.

### Predicate-Driven Branch Projection

A Branch whose conditions are expressed using natural-language predicate forms (see [[ir-and-semantics]] §Predicates) is rendered using resolved predicate prose. Three predicate forms exist:

| Predicate form | Source example | Resolution |
|---|---|---|
| Block trigger predicate | `fork_with_plan.applies()` | Reads `description:` from the named block |
| String-const predicate | `complex_change_required` | Reads the value of the string-kinded `const` |
| Inline literal predicate | `"the user has explicitly opted out"` | Uses the literal string directly |

The compiled-output rule chooses one of two prose forms based on the shape of the conditions:

- **Pure-predicate form ("decide which applies").** When every arm's condition is *purely* one or more predicate-form tokens combined by `or` — the compiler emits a single numbered Step that introduces the choice ("Decide which of the following applies and follow only that path:") and renders each arm as a lettered sub-step keyed by the resolved predicate prose rather than by a code-like condition expression. Example:

  ```md
  3. Decide which of the following applies and follow only that path:
     a. When the user asks to fork a terminal pre-loaded with a plan: identify the plan content, save it to disk, and fork the agentic tool with delayed input.
     b. When a complex change is required: plan the full edit sequence before touching any file.
     Otherwise:
     c. Understand the user's request and route to the appropriate launcher.
  ```

  The condition headers are written as user-intent / runtime conditions rather than as boolean expressions. The arms remain mutually evaluated by the consuming LLM in source order — the prose simply foregrounds the predicate description over the call-graph mechanic.

- **Mixed-condition form (inline description).** When an arm's condition combines predicate-form tokens with regular boolean operators or non-predicate names (e.g., `complex_change_required and not is_dry_run`), the resolved predicate prose inlines into the larger condition prose using the standard `If <condition>:` arm header. Example: an arm with condition `complex_change_required and not is_dry_run` produces the header "If a complex change is required and this is not a dry run:". Sub-steps inside the arm follow the lettered convention from §Constraint Rendering above.

The two forms compose: a Branch with one pure-predicate arm and one mixed-condition arm uses the pure-predicate header for the first arm and the mixed-condition header for the second, all under a single numbered Step. Inline literal predicates are already prose — they are used as the literal string directly.

### Parameter References In Steps

Parameters are **not** resolved at compile time. Steps and Constraints may reference parameters by name using `{param}` syntax. The consuming LLM substitutes the actual values at runtime based on user context and the `## Parameters` section.

- A step like `inspect_failure(scope)` expands to a Step whose prose references `{scope}` — e.g., "Inspect the failure in {scope}, focusing on auth boundaries."
- A `with "modifier"` clause on the call site attaches a specialization prompt that shapes the expanded wording. The modifier itself does not appear in compiled output.
- **Parameter name references** appear in compiled output as `{param}` slots. A `{param}` slot that does not match a parameter declared in the skill's header is a compile error.
- **Local binding references** (e.g., `{diagnosis}` where `diagnosis = analyze_error(...)`) are valid in source but do **not** survive as literal `{name}` slots in compiled output. The compiler resolves them into natural-language cross-references in the prose (e.g., "based on the diagnosis from your earlier analysis"). A local-ref slot that is not resolved is a compile error.
- The `{name}` slot is a **name reference**, not source-time interpolation. The compiler never substitutes the slot's value during compilation. For parameters, it preserves the literal `{name}` token for the consuming LLM to fill at runtime. For local bindings, it resolves the slot into prose. Slots are legal only in instruction-bearing string positions (Step/Constraint prose, generated block bodies, inline `flow:` instruction strings, and stdlib instruction arguments). The slot grammar is strict `{IDENTIFIER}` only; see [[values-and-names]] §No Interpolation for the full rules.

### Return Folding

`return <expr>` in `flow:` folds into the final numbered Step. A locked return-fold suffix is appended after whatever prose the final Step already carries.

**Locked return-fold suffixes:**

| Output form | Suffix template |
|---|---|
| Identifier (from `return <name>`) | `, and return that as your result.` |
| Description (from `return <"…">`) | `, and return <description> as your result.` where `<description>` is a Step-shaped paraphrase of the descriptive text. |

When the skill or procedure has an output contract but no visible step body (return-only), a standalone Step is emitted instead of appending a comma-prefixed suffix to a non-existent body:

| Output form | Standalone template |
|---|---|
| Identifier | `Return <name as snake-to-words> as your result.` |
| Description | `Return <description> as your result.` |

Example: `return summarize_changes()` as the last flow item becomes a Step like "Summarize what was changed and why, and return that as your result."

For the identifier form, `return <current_branch>` for a return-only skill becomes "Return current branch as your result." The literal `<current_branch>` token must never appear in compiled Markdown; output-target leaks are rejected as a compile error.

For the description form, `return <"root cause analysis including affected files and severity">` folds into a Step-shaped paraphrase, e.g., "..., and return a root cause analysis including affected files and severity as your result." The literal `<"…">` token, the surrounding angle brackets, and the bare quoted description must never appear in compiled Markdown; the same output-target-leak check covers both the identifier and description forms.

**Agent-typed returns.** When the return expression has type `Agent` (e.g., `return researcher`), the return-folded prose says the agent handle itself is the result — e.g., "Your result is the researcher agent spawned above — the caller may continue sending it instructions." The compiler does **not** interpret `return <agent>` as "return the agent's output." If the author wants the agent's findings, they should use an explicit inline string: `return "Report the researcher's findings as your result."` See [[stdlib]] §Agent Value Lifecycle for the full rule.

There is no separate `## Output` section in MVP.

## Authoring Constructs Compile Away

Most authoring machinery does not survive into compiled output:

- **Imports** resolve and either inline (Tier 1/2) or become file-path references (Tier 3). No import paths, module references, or `@glyph/` namespaces appear — only procedure-file paths for Tier 3 projections.
- **Const references** resolve and inline. A bare name like `preserve_existing_patterns` becomes its full string content.
- **Generated const / generated block** declarations resolve and inline. The `generated` marker is stripped; only the expanded content appears.
- **`with` modifiers** are consumed by the expand pass. Their prompt text shapes the Step wording but does not appear in the compiled file.
- **Parameters** resolve to concrete values during expand. No variable names survive.
- **No provenance markers.** No comments like `<!-- expanded from repo_tools.unrelated_edits -->`.

Only imports actually used by the skill are inlined; unused imports are dead code excluded from output. The compiler auto-removes unused import declarations from the source `.glyph` file (source-to-source fix, not silent omission).

**Self-containment is tiered.** Skills projected entirely at Tier 1 (inline) and Tier 2 (same-file procedure) are fully self-contained — one `.md` file with no external dependencies. Skills with Tier 3 (external file) projections depend on the referenced procedure files existing at the expected relative paths. The compiler produces all files in a single build; deployment requires shipping the output directory, not just a single file.

## Formatting Rules

1. **One instruction per list item.** No run-on multi-sentence bullets — except the final Step, which may include the return-summary sentence. Applies to `## Steps` and `## Constraints` items only; `## Context` entries may span multiple paragraphs and contain nested lists (see `## Context` projection rules).
2. **Numbered lists for Steps, bulleted lists for Context and Constraints.**
3. **No hard line-wrapping mid-sentence.** Each `## Steps` and `## Constraints` item is a single unwrapped line. `## Context` items are exempt: a Context entry's body may include paragraphs, blank lines, nested lists, and other block content provided every continuation line is indented past the bullet marker (two spaces) so the entry remains a single Markdown list item.
4. **Single blank line between sections.** No double blank lines, no trailing whitespace.
5. **No inline HTML or special formatting.** Standard Markdown only: headings, lists, bold, code spans.

## Complete Example

Source (`fix_bug.glyph`) — novice-kernel form, most definitions will be materialized by the repair pass:

```glyph
skill fix_bug(scope = ".")
    avoid unrelated_edits
    require preserve_existing_patterns

    context: "This skill assumes the bug is reproducible in the local environment."

    flow:
        inspect_failure(scope) with "focus on auth boundaries"
        identify_root_cause()
        "Don't propose a fix until you've confirmed the root cause."
        patch_minimally()
        validate_before_success
        return summarize_changes()
```

After the repair pass (`fix_bug.glyph`, same file — repair appends generated declarations):

```glyph
skill fix_bug(scope = ".")
    avoid unrelated_edits
    require preserve_existing_patterns

    context: "This skill assumes the bug is reproducible in the local environment."

    effects: reads_files, writes_files, runs_commands

    flow:
        inspect_failure(scope) with "focus on auth boundaries"
        identify_root_cause()
        "Don't propose a fix until you've confirmed the root cause."
        patch_minimally()
        validate_before_success()
        return summarize_changes()

generated const unrelated_edits = "Making changes outside the requested scope."
generated const preserve_existing_patterns = "Follow the repository's existing patterns before introducing new abstractions."

generated block validate_before_success()
    "Validate that the fix works before reporting success."

generated block inspect_failure(area)
    "Inspect the failure in {area} and identify what is failing."

generated block identify_root_cause()
    "Identify the root cause of the issue."

generated block patch_minimally()
    "Apply the smallest change that fixes the issue."

generated block summarize_changes()
    "Summarize what was changed and why."
```

Compiled output (`fix_bug.md`):

```md
---
name: fix_bug
description: Debug and fix a bug in the codebase with minimal, targeted changes.
effects: [reads_files, writes_files, runs_commands]
---

## Parameters
- **scope**: Area of codebase to focus on (default: ".")

## Context

- This skill assumes the bug is reproducible in the local environment.

## Steps

1. Inspect the failure in {scope}, focusing on auth boundaries and permission checks. Identify what is failing and whether any auth-related logic is involved.
2. Identify the root cause of the issue.
3. Don't propose a fix until you've confirmed the root cause.
4. Apply the smallest change that fixes the issue.
5. Validate that the fix works before reporting success.
6. Summarize what was changed and why, and return that as your result.

## Constraints

- Do not make changes outside {scope}.
- Follow the repository's existing patterns before introducing new abstractions.
```

Notes on the example:

- The `context:` declaration compiles into `## Context` as a bulleted item, appearing before `## Steps`. It provides passive background the agent should keep in mind.
- `scope` appears in the `## Parameters` section and is referenced as `{scope}` in Steps 1 and the first Constraint. The consuming LLM resolves `{scope}` from the user's request context at runtime.
- The `with "focus on auth boundaries"` modifier shaped the first Step's wording to mention auth boundaries and permission checks. The modifier string itself does not survive.
- The final flow item `return summarize_changes()` folds into Step 6 as "…and return that as your result." — no `## Output` section.
- Effects appear only in frontmatter. There is no `## Effects` section.

## Interactions With Other Workstreams

- **Effect vocabulary**: `effects` frontmatter content depends on finalized effect keywords ([[ir-and-semantics]]).
- **IR role taxonomy**: Role semantics, constraint strength (`soft`/`hard`) and polarity (`require`/`avoid`), and projection guidance are in [[ir-and-semantics]]. This file covers only the output-side projection.
- **Source syntax**: Compiled output shape is independent of source syntax, since output is a projection of the IR.
- **Type vocabulary**: MVP compiled output does not render parameter or return types; they stay in the IR for validation and visualization.
- **Pipeline**: Compilation is parameterless — it produces one compiled file per source file (see [[compiler-pipeline]]).

## Open Questions

- The exact wording and prominence rules for `Constraint(strength: hard)` vs `Constraint(strength: soft)`.
- Whether a skill registry / discovery tool wants additional metadata beyond the compiled file's `## Parameters` section and frontmatter. Since compilation is now parameterless, the compiled file already serves as both the execution artifact and the discovery artifact. Logged as a deferred concern; see [[todo]].
