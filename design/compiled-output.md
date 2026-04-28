# Glyph Compiled Output

This document defines the shape of compiled Markdown files that the Glyph compiler emits. It covers the MVP output format: a `.glyph.md` source file compiles into a same-basename `.md` file that serves as the executing agent's skill instructions. Compilation is parameterless — parameters appear as named slots resolved by the consuming LLM at runtime.

## Guiding Principles

- **Reliability beats elegance** (foundations). Favor explicitness, clarity, and followability over compression or style.
- **Targets agents broadly** (foundations). The output must be consumable by general-purpose agents, not tied to one execution environment.
- **Authoring and execution are separate** (foundations). Source constructs compile away completely. The compiled file is self-contained agent instructions.
- **The IR is the semantic contract** (foundations). Compiled output is a projection of the IR, not a direct transformation of source.
- **Novice learnability** (foundations). Compiled output stays radically simple — frontmatter plus one instruction section — so new authors see exactly how their source maps onto agent-facing Markdown.

## Parameterless Compilation

MVP compilation is parameterless. `glyph compile skill.glyph.md` produces one `.md` file per source file, regardless of how the skill will be invoked. Parameters are not resolved at compile time — they appear in the compiled output as named slots that the consuming LLM resolves from user context at runtime.

Practical consequences:

- The `.glyph.md` source is the authoring artifact; it is what authors share, import, and version.
- The `.md` compiled output is a single, stable artifact per source file. There is no argument-dependent variation.
- The compiled file contains a `## Parameters` section listing each parameter with its name, a brief description, and an optional default value. Steps and Constraints may reference parameters by name using `{param}` syntax.
- The consuming LLM reads the Parameters section, resolves each parameter from the user's request context (or falls back to the listed default), and executes the Steps with those values in mind.
- Since compilation is parameterless, there is no need for a separate "abstract card" output — the compiled file already serves that role.

## Source-To-Compiled-Output Mapping

Every source form maps to exactly one compiled location. This is the authoritative mapping.

| Source form | Compiled location |
|-------------|-------------------|
| `skill <name>` | Frontmatter `name` |
| `description:` | Frontmatter `description` |
| `effects:` (declared or inferred) | Frontmatter `effects` (YAML list) |
| `flow:` steps (non-`return`) | `### Steps` under `## Instructions` |
| `return <expr>` in flow | Closing sentence of the final `### Steps` item |
| `constraints:` content + body-level markers | `### Constraints` under `## Instructions` |
| Header parameters + defaults | `## Parameters` section (names, descriptions, optional defaults) |

Constraint strength (`soft`/`hard`) and polarity (`require`/`avoid`) affect compiled wording and prominence per [ir-and-semantics.md](ir-and-semantics.md).

## Frontmatter

Every compiled file starts with YAML frontmatter. Three fields in MVP:

```yaml
---
name: <skill-name>
description: <when this skill should be used>
effects: [<effect-keyword>, <effect-keyword>, ...]
---
```

- `name` — the skill identifier, taken from the `skill` declaration name. Machine-readable, used for skill selection and referencing.
- `description` — a concise statement of when and why an agent should use this skill. Primary trigger for coding agents that select skills from frontmatter. Sourced from the `description:` sub-section (see [ir-and-semantics.md](ir-and-semantics.md)). If the source omits `description:`, Repair (Phase 3) generates one from the skill name and body and adds it to the source as a `description:` sub-section.
- `effects` — YAML flow-sequence list of the skill's full inferred effect set. **Omitted unconditionally when the effect set is empty** — that is, when the skill has no meaningful effects or is explicitly `effects: none`. The compiler never emits `effects: none`, `effects: []`, or any other "no effects" placeholder; the field is simply absent. An absent `effects` key and `effects: none` are operationally identical for the consuming agent, and omitting is one fewer surface and one fewer ambiguity. Effects live in frontmatter so selectors and routing tools can read them without parsing the body; they are not repeated in the prose.

The compiled file does not emit a `# <Skill Name>` heading. The frontmatter `name` is the authoritative title.

## Sections

MVP compiled output emits two H2 sections: `## Parameters` (conditional) and `## Instructions`. No other sections are produced.

Deferred sections (`## Output`, `## Effects` as a prose section, `## When To Use`) are logged in [todo.md](todo.md) for possible post-MVP restoration.

### `## Parameters`

Emitted when the skill declares one or more parameters. Omitted for parameterless skills. Contains a bulleted list where each item names a parameter, provides a brief description (generated by the expand pass from the parameter's name, type, and usage context), and lists the default value. **Every parameter in compiled output carries a default value** — this is enforced at the source level (see `language-surface.md` §3.10). The consuming LLM always has a fallback.

```md
## Parameters
- **scope**: Area of codebase to focus on (default: ".")
- **risk**: Risk level — "low" | "medium" | "high" (default: "medium")
```

The consuming LLM reads this section before executing the Steps. It resolves each parameter from the user's request context; if the user does not specify a value, the LLM uses the listed default. Parameter descriptions are guidance for the LLM, not rigid schemas.

### `## Instructions`

Always emitted. Contains the compiled workflow and behavioral rules via H3 sub-sections:

- **`### Steps`** — numbered list (order matters). Each item is one instruction. The `return` expression from the source folds into the final item rather than producing a separate section.
- **`### Constraints`** — bulleted list (order usually does not matter). Each item is one `Constraint` node. Strength (`soft`/`hard`) and polarity (`require`/`avoid`) affect wording, not placement in MVP.
- **`### Procedure: <name>`** — zero or more procedure sections for blocks projected at Tier 2 (same-file procedure). Each contains a numbered list of the callee's expanded flow, with an optional constraint preamble. See §Three-Tier Block Projection for format and ordering rules.

`### Steps` and `### Constraints` are conditional: `### Constraints` is omitted when there are no explicit constraints; `### Steps` may be omitted only for pure instruction-only skills (all content is constraints). At least one of `### Steps` or `### Constraints` must be present. `### Procedure:` sections are conditional on the projection tier selected for each callee.

```md
## Instructions

### Steps

1. Inspect the failure and reproduce it.
2. Identify the root cause before proposing a fix.
3. Patch minimally and report the summary.

### Constraints

- Do not make unrelated edits outside the requested scope.
- Follow the repository's existing patterns before introducing new abstractions.
```

## Projection Rules

Compiled output projects from the typed IR role model defined in [ir-and-semantics.md](ir-and-semantics.md). See that file for role semantics. This section covers only the output-side rules: which location each role projects into, formatting, and ordering.

| IR role / metadata | Compiled target | Format |
|--------------------|-----------------|--------|
| Skill name | Frontmatter `name` | String |
| Skill description | Frontmatter `description` | String |
| Effect set | Frontmatter `effects` | YAML list; field omitted if effect set is empty or `none` |
| `Step` | `### Steps` | Numbered list, one concrete instruction per item |
| `Constraint` | `### Constraints` | Bulleted list, wording shaped by constraint keyword (`require`/`avoid`/`must`/`must avoid`) |
| `InputContract` + parameters | `## Parameters` section (names, descriptions, defaults) | Bulleted list |
| `OutputContract` + `return` | Closing sentence of the final `### Steps` item | No dedicated section |
| Block call (referenced) | `### Procedure: <name>` section | Numbered list with optional constraint preamble |
| Block call (external) | "Load and follow `<path>`" in Step prose | File path reference |

### Three-Tier Block Projection

When a `Call` node targets a block (same-file or imported), the compiler chooses one of three projection tiers based on callee complexity, conditionality, and reuse. The decision is made in Expand Step 1 (deterministic).

| Condition | Tier | Projection |
|-----------|------|------------|
| Callee body has 1 flow statement, no own constraints, called once, **expanded prose < 150 words** | **Inline** | Body becomes Step prose (default behavior) |
| Callee body has 2–3 flow statements, no own constraints, called once, **expanded prose < 150 words** | **Inline** | Body concatenated into one Step paragraph |
| Callee body has 4+ flow statements | **Same-file procedure** | `### Procedure: <name>` section under `## Instructions` |
| Callee declares its own constraints (any flow count) | **Same-file procedure** | Constraints need a scoping home in the procedure preamble |
| Callee is called 2+ times in the same skill (same-file block) | **Same-file procedure** | Avoids prose duplication |
| Imported `export block` called inside a `Branch` | **External file** | Might not be needed — defers context cost until the branch is taken |
| Imported `export block` called from multiple skills in the same project | **External file** | Compile once, reference everywhere |
| Imported `export block` called unconditionally, not shared | **Same-file procedure** | Always needed, keep it nearby |

**Word count threshold.** The tier heuristic includes a word count check on the callee's expanded prose to guard the Tier 1 boundary:

- **< 150 words**: eligible for Tier 1 (inline). Small enough to fold into a single Step paragraph.
- **>= 150 words**: not eligible for Tier 1. Structural heuristics (statement count, constraints, call count) determine Tier 2 vs. Tier 3.

Size alone does **not** trigger Tier 3. A 600-word block that is unconditional and single-consumer projects as Tier 2 (same-file procedure). Tier 3 is reserved for blocks that are **conditional** (inside a `Branch` — defers context cost until the branch is taken) or **shared** (called from multiple skills — single source of truth). The rationale: for unconditional loads, externalizing to Tier 3 does not reduce runtime context — the agent reads the external file anyway — so the structural complication of a separate file must be justified by conditionality or sharing, not size.

Word counts are checked in Expand Step 1 after the callee's prose is resolved — that is the earliest point where the actual expanded text is available. Promotion is one-directional: Tier 1 → Tier 2 → Tier 3, never downward. A block initially assigned Tier 1 by statement count but exceeding 150 words is promoted to Tier 2.

**Cross-file word-count sourcing.** When the call site is in a downstream skill and the callee is an imported `export block`, the consumer's Step 1 cannot recompute the callee's word count from scratch — it does not own the callee's resolved expanded prose. Instead, it reads the **derived `resolved_word_count` field** that the library file's own Phase 6 Step 1 attached to the imported `ExportBlock` node when the library compiled (`ir-schema.md` §Top-Level Compilation Units). This field is populated once per export block during the library's compilation and propagated in-memory via the import-resolution mechanism. It is not part of the IR JSON serialization (`ir-json-schema.md`); the consumer relies on the multi-file build seeing the imported library's in-memory IR (`pipeline.md` §Multi-File Compilation Order — strictly serial topological order guarantees that a library's Phase 6 Step 1 has run before any consumer needs the field). For same-file callees, Step 1 computes the count directly from the local resolved prose; no derived field is needed.

**Word counting rule.** A "word" is a whitespace-separated token in the Step 1 projection prose. Backticked code spans count as 1 word each (one ident-blob = one unit of cognitive load). Markdown formatting markers (`**`, list bullets, headings) do not count. Comments are stripped before counting.

**Configurability.** The 150-word threshold is hard-coded for MVP — not exposed via project config. The load-bearing properties are determinism and documentation; the exact value is tunable post-MVP from real-corpus telemetry. See `todo.md`.

Conditions are checked top-to-bottom; the first `referenced` or `external` trigger wins. The tier is a property of the *(callee, skill)* pair — a block called once in skill A might inline, but the same block called twice in skill B gets a procedure section.

**Library file emission.** Library files emit standalone procedure `.md` files for `export block` declarations whose expanded prose is >= 150 words (i.e., above the Tier 1 inline threshold). The library's Phase 7 writes these to a subdirectory named after the source file (e.g., `repo_tools.glyph.md` → `repo_tools/inspect-repo.md`). Export blocks below the threshold emit nothing from the library — consumers inline them. Note: a procedure `.md` may exist on disk but go unused at a consumer call site that projects the block as Tier 2 (same-file procedure) rather than Tier 3 — this is intentional, not an error. See `language-surface.md` §File-Level Rules for the full library emission model.

#### Same-File Procedure Sections

A `### Procedure: <name>` H3 section appears under `## Instructions`, after `### Steps` and `### Constraints`:

```md
## Instructions

### Steps

1. Gather the relevant files in {scope}.
2. Review the code for issues (follow the review-code procedure below).
3. Summarize findings and return that as your result.

### Constraints

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
- Optional preamble paragraph: the callee's scoped constraints, rendered as prose sentences (not bulleted — they are contextual to this procedure, not top-level skill constraints).
- Numbered list: the callee's flow statements, expanded by Step 2 the same way skill-level Steps are.
- Return folding: if the callee has a `return`, it folds into the last numbered item of the procedure (same rule as skill-level return).
- Ordering: procedure sections appear after `### Steps` and `### Constraints`, in the order of first reference from `### Steps`.

**Referencing from Steps:** The referencing Step includes a parenthetical cross-reference — e.g., "(follow the review-code procedure below)" or "(see the review-code procedure above)." Step 2 chooses natural phrasing. The reference must include the procedure name so Phase 6b can verify the link.

**Multiple references to the same procedure:** The procedure section appears once. Multiple Steps reference it. When called with different `with` modifiers, the modifier shapes the referencing Step's prose, not the procedure section — the procedure stays generic:

```md
1. Review the auth module for security vulnerabilities (follow the review-code procedure below).
2. Review the API layer for contract violations (follow the review-code procedure above).
```

#### External Procedure Files

When the compiler selects the external-file tier, the imported `export block` compiles to a standalone `.md` procedure file. The referencing skill's Step directs the consuming agent to load the file at runtime.

**Procedure file format:** Identical to a skill's compiled format — YAML frontmatter, optional `## Parameters`, `## Instructions` with `### Steps` and `### Constraints`. The frontmatter carries `kind: procedure` to distinguish from top-level skills:

```md
---
name: review-code
kind: procedure
description: Systematic code review procedure.
effects: [reads_files]
---

## Parameters
- **targets**: Files to review

## Instructions

### Steps

1. Scan the target files for style violations and anti-patterns.
2. Check for security vulnerabilities.
3. Check for performance issues in hot paths.
4. Compile a list of findings with severity ratings.

### Constraints

- Do not introduce new abstractions during the review.
```

**File output path:** Procedure files are placed in a subdirectory named after the source file. The procedure filename is the **kebab-case** form of the export block's `snake_case` identifier (each `_` → `-`, no other transformation). E.g., `review_tools.glyph.md` containing `export block review_code(...)` produces `review_tools/review-code.md`. The `.glyph` infix from the source filename is dropped for compiled artifacts: source files are `*.glyph.md`, compiled outputs (top-level skills and procedure files alike) are `*.md`. The same kebab-case rule governs both the on-disk filename and the H3 heading inside same-file procedure sections (see §Same-File Procedure Sections), so a given block always renders under a single canonical name regardless of projection tier.

**Referencing from Steps:** The referencing Step includes a file path — e.g., "load and follow the procedure in `review_tools/review-code.md`." When inside a conditional branch, the load instruction is part of the conditional Step prose:

```md
2. If the files have security concerns, load and follow the procedure in
   `review_tools/review-code.md`, focusing on security vulnerabilities.
```

**`with` modifier interaction:** The `with` modifier shapes the referencing Step's prose (e.g., "focusing on security vulnerabilities"), not the external procedure file. The procedure file is compiled independently and stays generic. The consuming agent applies the Step's emphasis while following the procedure.

**Effect implication:** Referencing an external file implies `reads_files` on the skill's effect set. The compiler infers this automatically when selecting the external-file tier. If the author declared `effects:`, it must include `reads_files` or the compiler emits an error.

**Deployment:** A compiled project may produce multiple files — one `.md` per skill, plus procedure files for externally projected blocks. The `glyph compile` command produces all files in a single output directory.

### Constraint Rendering

- **Strength** affects wording and prominence. `hard` renders as strongest non-negotiable rules; `soft` renders as standard rules. Strength is advisory prose framing — not enforced; target agent compliance is not guaranteed.
- **Polarity** affects phrasing. `polarity: require` renders as a positive obligation; `polarity: avoid` renders as a prohibition.
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

  **Nested branches** (a `Branch` inside another `Branch`'s arm) do **not** receive their own sub-step structure. Instead, they flatten into prose within their parent sub-step (e.g., "If the codebase has public APIs, check backwards compatibility and update the changelog. Otherwise, run internal validation."). Only one level of structured sub-steps is supported. The Repair pass auto-extracts deeply nested branches into helper `generated block` declarations to keep compiled output clean (see `repair.md` §4.9).

- **Branch-scoped constraints.** A `require`/`avoid`/`must` marker that appears inside an `if`/`elif`/`else` branch in `flow:` is **inlined into the prose of an adjacent sub-step**, not surfaced in `### Constraints` and not given its own lettered sub-step. The inlined wording makes the conditional applicability explicit (e.g., a sub-step like "Run the migration, never dropping existing columns."). Only flow-top-level constraint markers (and body-level constraints declared above `flow:`) hoist to `Skill.constraints` and render in `### Constraints`. See [ir-and-semantics.md](ir-and-semantics.md) §Body-Level Constraint Normalization and [pipeline.md](pipeline.md) Phase 4 (Lower) for the hoisting rules.

### Parameter References In Steps

Parameters are **not** resolved at compile time. Steps and Constraints may reference parameters by name using `{param}` syntax. The consuming LLM substitutes the actual values at runtime based on user context and the `## Parameters` section.

- A step like `inspect_failure(scope)` expands to a Step whose prose references `{scope}` — e.g., "Inspect the failure in {scope}, focusing on auth boundaries."
- A `with "modifier"` clause on the call site attaches a specialization prompt that shapes the expanded wording. The modifier itself does not appear in compiled output.
- Parameter names appear in compiled output as `{param}` references. A Step that references a parameter name not declared in the skill's header is a compile error.
- The `{name}` slot is a **runtime parameter slot**, not source-time interpolation. The compiler never substitutes the slot's value during compilation; it preserves the literal `{name}` token in the compiled Markdown for the consuming LLM to fill at execution time. Slots are legal only in instruction-bearing string positions (Step/Constraint prose, generated block bodies, inline `flow:` instruction strings, and stdlib instruction arguments). The slot grammar is strict `{IDENTIFIER}` only; see [values-and-names.md](values-and-names.md) §No Interpolation for the full rules.

### Return Folding

`return <expr>` in `flow:` folds into the final numbered Step. The Step's prose ends with a sentence that names or summarizes what the skill returns.

Example: `return summarize_changes()` as the last flow item becomes a Step like "Summarize what was changed and why, and return that as your result."

**Agent-typed returns.** When the return expression has type `Agent` (e.g., `return researcher`), the return-folded prose says the agent handle itself is the result — e.g., "Your result is the researcher agent spawned above — the caller may continue sending it instructions." The compiler does **not** interpret `return <agent>` as "return the agent's output." If the author wants the agent's findings, they should use an explicit inline string: `return "Report the researcher's findings as your result."` See `stdlib.md` §Agent Value Lifecycle for the full rule.

There is no separate `## Output` section in MVP.

## Authoring Constructs Compile Away

Most authoring machinery does not survive into compiled output:

- **Imports** resolve and either inline (Tier 1/2) or become file-path references (Tier 3). No import paths, module references, or `@glyph/` namespaces appear — only procedure-file paths for Tier 3 projections.
- **Text references** resolve and inline. A bare name like `preserve_existing_patterns` becomes its full text content.
- **Generated text / generated block** declarations resolve and inline. The `generated` marker is stripped; only the expanded content appears.
- **`with` modifiers** are consumed by the expand pass. Their prompt text shapes the Step wording but does not appear in the compiled file.
- **Parameters** resolve to concrete values during expand. No variable names survive.
- **No provenance markers.** No comments like `<!-- expanded from repo_tools.unrelated_edits -->`.

Only imports actually used by the skill are inlined; unused imports are dead code excluded from output. The compiler auto-removes unused import declarations from the source `.glyph.md` file (source-to-source fix, not silent omission).

**Self-containment is tiered.** Skills projected entirely at Tier 1 (inline) and Tier 2 (same-file procedure) are fully self-contained — one `.md` file with no external dependencies. Skills with Tier 3 (external file) projections depend on the referenced procedure files existing at the expected relative paths. The compiler produces all files in a single build; deployment requires shipping the output directory, not just a single file.

## Formatting Rules

1. **One instruction per list item.** No run-on multi-sentence bullets — except the final Step, which may include the return-summary sentence.
2. **Numbered lists for Steps, bulleted lists for Constraints.**
3. **No hard line-wrapping mid-sentence.** Each list item is a single unwrapped line.
4. **Single blank line between sections.** No double blank lines, no trailing whitespace.
5. **No inline HTML or special formatting.** Standard Markdown only: headings, lists, bold, code spans.

## Complete Example

Source (`fix_bug.glyph.md`) — novice-kernel form, most definitions will be materialized by the repair pass:

```glyph
skill fix_bug(scope = ".")
    avoid unrelated_edits
    require preserve_existing_patterns

    flow:
        inspect_failure(scope) with "focus on auth boundaries"
        identify_root_cause()
        "Don't propose a fix until you've confirmed the root cause."
        patch_minimally()
        validate_before_success
        return summarize_changes()
```

After the repair pass (`fix_bug.glyph.md`, same file — repair appends generated declarations):

```glyph
skill fix_bug(scope = ".")
    avoid unrelated_edits
    require preserve_existing_patterns

    effects: reads_files, writes_files, runs_commands

    flow:
        inspect_failure(scope) with "focus on auth boundaries"
        identify_root_cause()
        "Don't propose a fix until you've confirmed the root cause."
        patch_minimally()
        validate_before_success
        return summarize_changes()

generated text unrelated_edits = "Making changes outside the requested scope."
generated text preserve_existing_patterns = "Follow the repository's existing patterns before introducing new abstractions."
generated text validate_before_success = "Validate that the fix works before reporting success."

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

## Instructions

### Steps

1. Inspect the failure in {scope}, focusing on auth boundaries and permission checks. Identify what is failing and whether any auth-related logic is involved.
2. Identify the root cause of the issue.
3. Don't propose a fix until you've confirmed the root cause.
4. Apply the smallest change that fixes the issue.
5. Validate that the fix works before reporting success.
6. Summarize what was changed and why, and return that as your result.

### Constraints

- Do not make changes outside {scope}.
- Follow the repository's existing patterns before introducing new abstractions.
```

Notes on the example:

- `scope` appears in the `## Parameters` section and is referenced as `{scope}` in Steps 1 and the first Constraint. The consuming LLM resolves `{scope}` from the user's request context at runtime.
- The `with "focus on auth boundaries"` modifier shaped Step 1's wording to mention auth boundaries and permission checks. The modifier string itself does not survive.
- The final flow item `return summarize_changes()` folds into Step 6 as "…and return that as your result." — no `## Output` section.
- Effects appear only in frontmatter. There is no `## Effects` section.

## Interactions With Other Workstreams

- **Effect vocabulary**: `effects` frontmatter content depends on finalized effect keywords ([ir-and-semantics.md](ir-and-semantics.md)).
- **IR role taxonomy**: Role semantics, constraint strength (`soft`/`hard`) and polarity (`require`/`avoid`), and projection guidance are in [ir-and-semantics.md](ir-and-semantics.md). This file covers only the output-side projection.
- **Source syntax**: Compiled output shape is independent of source syntax, since output is a projection of the IR.
- **Type vocabulary**: MVP compiled output does not render parameter or return types; they stay in the IR for validation and visualization.
- **Pipeline**: The expand pass is parameterless — it produces one compiled file per source file (see pipeline doc when canonicalized).

## Open Questions

- The exact wording and prominence rules for `Constraint(strength: hard)` vs `Constraint(strength: soft)`.
- Whether a skill registry / discovery tool wants additional metadata beyond the compiled file's `## Parameters` section and frontmatter. Since compilation is now parameterless, the compiled file already serves as both the execution artifact and the discovery artifact. Logged as a deferred concern; see [todo.md](todo.md).
