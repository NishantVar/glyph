# Glyph Compiled Output

This document defines the shape of compiled Markdown files that the Glyph compiler emits. It covers the MVP output format: a `.glyph.md` source file compiles, at the time an author or caller invokes it with concrete arguments, into a same-basename `.md` file that is the executing agent's prompt.

## Guiding Principles

- **Reliability beats elegance** (foundations). Favor explicitness, clarity, and followability over compression or style.
- **Targets agents broadly** (foundations). The output must be consumable by general-purpose agents, not tied to one execution environment.
- **Authoring and execution are separate** (foundations). Source constructs compile away completely. The compiled file is self-contained agent instructions.
- **The IR is the semantic contract** (foundations). Compiled output is a projection of the IR, not a direct transformation of source.
- **Novice learnability** (foundations). Compiled output stays radically simple — frontmatter plus one instruction section — so new authors see exactly how their source maps onto agent-facing Markdown.

## Per-Invocation Compilation

MVP compilation is per-invocation. The expand pass takes the source plus **concrete argument values** for the skill's parameters and produces compiled Markdown in which every parameter has already been resolved into prose. The compiled file is a specialization of the source for one use, not a reusable template.

Practical consequences:

- The `.glyph.md` source is the reusable artifact; it is what authors share, import, and version.
- The `.md` compiled output is tied to a specific invocation. Different argument sets produce different compiled files.
- The compiled file contains no variable references, no `{param}` placeholders, no conditional logic in template form. It reads as flat, concrete instructions.
- Tooling (IDEs, repositories, skill registries) that needs a discovery artifact can compile with default arguments or a documentation-style invocation; MVP does not define a separate "abstract card" output.

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
| Header parameters + concrete arguments | Resolved into Step prose at expand time (no dedicated section) |

Constraint strength and polarity affect compiled wording and prominence per [ir-and-semantics.md](ir-and-semantics.md).

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
- `description` — a concise statement of when and why an agent should use this skill. Primary trigger for coding agents that select skills from frontmatter. Sourced from the `description:` sub-section (see [ir-and-semantics.md](ir-and-semantics.md)). If the source omits `description:`, the compiler generates one from the skill name and body during the expand pass.
- `effects` — YAML flow-sequence list of the skill's full inferred effect set. Omitted entirely (the field is not emitted) when the skill has no meaningful effects or is explicitly `effects: none`. Effects live in frontmatter so selectors and routing tools can read them without parsing the body; they are not repeated in the prose.

The compiled file does not emit a `# <Skill Name>` heading. The frontmatter `name` is the authoritative title.

## Sections

MVP compiled output emits exactly one H2 section: `## Instructions`. No other sections are produced.

Deferred sections (`## Inputs`, `## Output`, `## Effects` as a prose section, `## When To Use`) are logged in [todo.md](todo.md) for possible post-MVP restoration.

### `## Instructions`

Always emitted. Contains the compiled workflow and behavioral rules via H3 sub-sections:

- **`### Steps`** — numbered list (order matters). Each item is one instruction. The `return` expression from the source folds into the final item rather than producing a separate section.
- **`### Constraints`** — bulleted list (order usually does not matter). Each item is one `Constraint` node. Strength and polarity affect wording, not placement in MVP.

Both sub-sections are conditional: `### Constraints` is omitted when there are no explicit constraints; `### Steps` may be omitted only for pure instruction-only skills (all content is constraints). At least one must be present.

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
| `Constraint` | `### Constraints` | Bulleted list, wording shaped by strength and polarity |
| `InputContract` + parameters | Folded into `### Steps` prose at expand time | No dedicated section |
| `OutputContract` + `return` | Closing sentence of the final `### Steps` item | No dedicated section |

### Constraint Rendering

- **Strength** affects wording and prominence. `invariant` renders as strongest non-negotiable rules, `required` as mandatory rules, `preferred` as guidance that yields to stronger constraints.
- **Polarity** affects phrasing. `polarity: require` renders as a positive obligation; `polarity: avoid` renders as a prohibition.
- **Conditional logic** (`if` in source) is flattened into prose in `### Steps`. The compiled output does not use code-like branching syntax.

### Parameter Resolution Into Steps

The expand pass receives concrete argument values for every parameter. Those values flow into the Step prose directly:

- A step like `inspect_failure(scope)` with `scope = "auth"` expands to a Step whose prose mentions "the auth area" or "the auth module" (wording determined by the expand pass and any `with` modifier).
- A `with "modifier"` clause on the call site attaches a specialization prompt that shapes the expanded wording. The modifier itself does not appear in compiled output.
- Parameter names never appear in compiled output. If an expanded Step would still contain a `{param}` placeholder, that is a compile error.

### Return Folding

`return <expr>` in `flow:` folds into the final numbered Step. The Step's prose ends with a sentence that names or summarizes what the skill returns.

Example: `return summarize_changes()` as the last flow item becomes a Step like "Summarize what was changed and why, and return that as your result."

There is no separate `## Output` section in MVP.

## Authoring Constructs Compile Away

Compiled output is fully self-contained. No authoring machinery survives:

- **Imports** resolve and inline. The compiled output contains expanded instruction text. No import paths, module references, or library names appear.
- **Text references** resolve and inline. A bare name like `preserve_existing_patterns` becomes its full text content.
- **Generated text / generated block** declarations resolve and inline. The `generated` marker is stripped; only the expanded content appears.
- **`with` modifiers** are consumed by the expand pass. Their prompt text shapes the Step wording but does not appear in the compiled file.
- **Parameters** resolve to concrete values during expand. No variable names survive.
- **No provenance markers.** No comments like `<!-- expanded from repo_tools.unrelated_edits -->`.

Only imports actually used by the skill are inlined; unused imports are dead code excluded from output. The compiler auto-removes unused import declarations from the source `.glyph.md` file (source-to-source fix, not silent omission).

## Formatting Rules

1. **One instruction per list item.** No run-on multi-sentence bullets — except the final Step, which may include the return-summary sentence.
2. **Numbered lists for Steps, bulleted lists for Constraints.**
3. **No hard line-wrapping mid-sentence.** Each list item is a single unwrapped line.
4. **Single blank line between sections.** No double blank lines, no trailing whitespace.
5. **No inline HTML or special formatting.** Standard Markdown only: headings, lists, bold, code spans.

## Complete Example

Source (`fix_bug.glyph.md`) — novice-kernel form, most definitions will be materialized by the repair pass:

```glyph
skill fix_bug(scope)
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
skill fix_bug(scope)
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

Compiled output (`fix_bug.md`), produced with `scope = "auth"`:

```md
---
name: fix_bug
description: Debug and fix a bug in the codebase with minimal, targeted changes.
effects: [reads_files, writes_files, runs_commands]
---

## Instructions

### Steps

1. Inspect the failure within the auth module, focusing on authentication boundaries and permission checks. Identify what is failing and whether any auth-related logic is involved.
2. Identify the root cause of the issue.
3. Don't propose a fix until you've confirmed the root cause.
4. Apply the smallest change that fixes the issue.
5. Validate that the fix works before reporting success.
6. Summarize what was changed and why, and return that as your result.

### Constraints

- Do not make changes outside the requested scope.
- Follow the repository's existing patterns before introducing new abstractions.
```

Notes on the example:

- `scope = "auth"` is resolved into Step 1 as "the auth module" and "authentication boundaries"; the `with` modifier further focuses Step 1 on auth semantics. Neither the parameter name nor the modifier string survive.
- The final flow item `return summarize_changes()` folds into Step 6 as "…and return that as your result." — no `## Output` section.
- Effects appear only in frontmatter. There is no `## Effects` section.

## Interactions With Other Workstreams

- **Effect vocabulary**: `effects` frontmatter content depends on finalized effect keywords ([ir-and-semantics.md](ir-and-semantics.md)).
- **IR role taxonomy**: Role semantics, constraint strength/polarity, and projection guidance are in [ir-and-semantics.md](ir-and-semantics.md). This file covers only the output-side projection.
- **Source syntax**: Compiled output shape is independent of source syntax, since output is a projection of the IR.
- **Type vocabulary**: MVP compiled output does not render parameter or return types; they stay in the IR for validation and visualization.
- **Pipeline**: The expand pass is per-invocation and consumes concrete arguments (see pipeline doc when canonicalized).

## Open Questions

- Whether target-specific renderers should add `### Guidance` for `Constraint(strength: preferred)`, or whether preferred constraints should stay merged with Constraints.
- The exact wording and prominence rules for `Constraint(strength: invariant)`.
- Whether a skill registry / discovery tool wants an "abstract-card" compilation mode (frontmatter plus placeholder body) alongside the per-invocation compiled output. Logged as a deferred concern; see [todo.md](todo.md).
