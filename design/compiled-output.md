# Glyph Compiled Output

This document defines the shape of compiled Markdown files that the Glyph compiler emits. It covers the MVP output format: one `.glyph.md` source file compiles to exactly one same-basename `.md` file.

## Guiding Principles

- **Reliability beats elegance** (foundations). Favor explicitness, clarity, and followability over compression or style.
- **Targets agents broadly** (foundations). The output must be consumable by general-purpose agents, not tied to one execution environment.
- **Authoring and execution are separate** (foundations). Source constructs compile away completely. The compiled file is self-contained agent instructions.
- **The IR is the semantic contract** (foundations). Compiled output is a projection of the IR, not a direct transformation of source.

## Source-To-Compiled-Output Mapping

Every source form maps to exactly one compiled section. This is the authoritative mapping.

| Source form | Compiled section |
|-------------|-----------------|
| `effects:` | `## Effects` |
| Header params + `inputs:` + body `input` markers | `## Inputs` |
| Body `context` markers + inline non-normative text | `## Inputs` (as informational context) |
| `flow:` steps | `### Steps` under `## Instructions` |
| `constraints:` content | `### Constraints` under `## Instructions` |
| `return` in flow + `outputs:` + body `output` markers | `## Output` |
| `when_to_use:` | `## When To Use` |

Context nodes project into `## Inputs` as informational context or assumptions. The wording must not turn context into a requirement.

Constraint strength and polarity affect compiled wording and prominence per [ir-and-semantics.md](ir-and-semantics.md).

## Frontmatter

Every compiled file starts with YAML frontmatter:

```yaml
---
name: <skill-name>
description: <when this skill should be used>
---
```

- `name` — the skill identifier. Machine-readable, used for skill selection and referencing.
- `description` — a concise statement of when and why an agent should use this skill. Primary trigger for coding agents that select skills from frontmatter.

The compiled file does not emit a `# <Skill Name>` heading. The frontmatter `name` is the authoritative title.

## Sections

The compiled file uses H2 sections in a fixed order. All sections except `## Instructions` are conditional and omitted when their content is empty.

### Fixed Section Order

1. `## Effects` (conditional)
2. `## Inputs` (conditional)
3. `## Instructions` (required)
4. `## Output` (conditional)
5. `## When To Use` (conditional)

### `## Effects`

Emitted when the skill has meaningful effects (not `effects: none`). Each effect is a bullet with a human-readable expansion. Omitted for `effects: none` or no meaningful effects. Effects appear first because agents need them before deciding to execute.

### `## Inputs`

Emitted when the skill declares parameters, `InputContract` nodes, or `Context` nodes visible before execution. Parameter types from `name: Type` annotations render as `(Type)` after the backticked name. No type shown when the annotation is absent.

### `## Instructions`

Always emitted. Contains the compiled workflow and behavioral constraints via H3 sub-sections:

- **`### Steps`** — numbered list (order matters). Each item is one instruction.
- **`### Constraints`** — bulleted list (order usually does not matter). Each item is one `Constraint` node. Strength and polarity affect wording, not placement in the MVP.

Both sub-sections are conditional: `### Constraints` is omitted when there are no explicit constraints; `### Steps` may be omitted for instruction-only skills. At least one must be present. This set may grow (e.g., `### Guidance` for `Constraint(strength: preferred)`), but the MVP starts with two.

```md
## Instructions

### Steps

1. Inspect the failure and reproduce it.
2. Identify the root cause before proposing a fix.

### Constraints

- Do not make unrelated edits outside the requested scope.
- Follow the repository's existing patterns before introducing new abstractions.
```

### `## Output`

Emitted when the skill declares an explicit return contract or `OutputContract` nodes. Return types from `-> ReturnType` annotations render as a type note at the top of the section (e.g., "Returns a `ReviewResult`."). No type note when the source omits the return type.

### `## When To Use`

Emitted only when the source contains trigger guidance that does not fit in the frontmatter `description`. Routing metadata, not an instruction role. Omitted when `description` is sufficient.

## Projection Rules

Compiled output projects from the typed IR role model defined in [ir-and-semantics.md](ir-and-semantics.md). See that file for role semantics. This section covers only the output-side rules: which section each role projects into, formatting, and ordering.

| IR role | Compiled target | Format |
|---------|----------------|--------|
| Effect metadata | `## Effects` | Bulleted list, human-readable expansions |
| `InputContract` + parameters | `## Inputs` | Bulleted list with backticked names and `(Type)` annotations |
| `Context` | `## Inputs` | Bulleted list, informational wording (must not read as a requirement) |
| `Step` | `### Steps` | Numbered list, one instruction per item |
| `Constraint` | `### Constraints` | Bulleted list, wording shaped by strength and polarity |
| `OutputContract` + return | `## Output` | Type note (if declared) then bulleted list |

### Constraint Rendering

- **Strength** affects wording and prominence. `invariant` renders as strongest non-negotiable rules, `required` as mandatory rules, `preferred` as guidance that yields to stronger constraints.
- **Polarity** affects phrasing. `polarity: require` renders as a positive obligation; `polarity: avoid` renders as a prohibition.
- **Conditional logic** (`if` in source) is flattened into prose in the target section. The compiled output does not use code-like branching syntax.

## Authoring Constructs Compile Away

Compiled output is fully self-contained. No authoring machinery survives:

- **Imports** resolve and inline. The compiled output contains expanded instruction text. No import paths, module references, or library names appear.
- **Text references** resolve and inline. A bare name like `preserve_existing_patterns` becomes its full text content.
- **Generated text** declarations resolve and inline. The `generated` marker is stripped; only instruction content appears.
- **No provenance markers.** No comments like `<!-- expanded from repo_tools.unrelated_edits -->`.

Only imports actually used by the skill are inlined; unused imports are dead code excluded from output. The compiler auto-removes unused import declarations from the source `.glyph.md` file (source-to-source fix, not silent omission).

## Formatting Rules

1. **One instruction per list item.** No run-on multi-sentence bullets.
2. **Numbered lists for Steps, bulleted lists for Constraints.**
3. **No hard line-wrapping mid-sentence.** Each list item is a single unwrapped line.
4. **Single blank line between sections.** No double blank lines, no trailing whitespace.
5. **No inline HTML or special formatting.** Standard Markdown only: headings, lists, bold, code spans.

## Complete Example

Source (`fix_bug.glyph.md`):

```glyph
import "./repo_tools.glyph.md" { unrelated_edits, preserve_existing_patterns }

skill fix_bug(scope)
    avoid unrelated_edits
    require preserve_existing_patterns

    effects: reads_files, writes_files, runs_commands

    flow:
        inspect_failure(scope)
        identify_root_cause()
        patch_minimally()
        validate_before_success
        return summarize_changes()
```

Compiled output (`fix_bug.md`):

```md
---
name: fix_bug
description: Debug and fix a bug in the codebase with minimal, targeted changes.
---

## Effects

- Reads files (source code, logs, test output)
- Writes files (source code patches)
- Runs commands (test runners, linters)

## Inputs

- `scope` — the area of code to investigate (file path, module name, or description of the bug)

## Instructions

### Steps

1. Inspect the failure within the given scope and reproduce it.
2. Identify the root cause before proposing or applying a fix.
3. Patch minimally — change only what is necessary to fix the bug.
4. Run validation to confirm the fix works before reporting success.

### Constraints

- Do not make unrelated edits outside the requested scope.
- Follow the repository's existing patterns, helper APIs, naming, and file organization before introducing a new abstraction or style.

## Output

- A summary of changes made, including file paths modified.
- The root cause of the bug.
- Any issues that could not be resolved, with explanation.
```

## Interactions With Other Workstreams

- **Effect vocabulary**: `## Effects` content depends on finalized effect keywords and expansions ([ir-and-semantics.md](ir-and-semantics.md)).
- **IR role taxonomy**: Role semantics, constraint strength/polarity, and projection guidance are in [ir-and-semantics.md](ir-and-semantics.md). This file covers only the output-side projection.
- **Source syntax**: Compiled output shape is independent of source syntax, since output is a projection of the IR.
- **Type vocabulary**: Parameter and return type rendering depends on type names in [types.md](types.md).

## Open Questions

- Whether target-specific renderers should add `### Guidance` for `Constraint(strength: preferred)`, or whether preferred constraints should stay merged with Constraints.
- The exact wording and prominence rules for `Constraint(strength: invariant)`.
- Whether the compiler should emit a `## Effects` section with "None" content vs. omitting entirely for pure skills. Current decision: omit entirely.
