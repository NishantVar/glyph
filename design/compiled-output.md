# Glyph Compiled Output

This document defines the shape of compiled Markdown files that the Glyph compiler emits. It covers the MVP output format: one `.glyph.md` source file compiles to exactly one same-basename `.md` file.

## Guiding Principles

- **Reliability beats elegance** (principle 16). Compiled output should favor explicitness, clarity, and followability over compression or style.
- **Target agents broadly, with special care for current coding agents** (boundary 4). The output must be consumable by general-purpose agents, not tied to one execution environment.
- **Authoring and execution are separate** (principle 8). Source constructs compile away completely. The compiled file is self-contained agent instructions with no traces of authoring machinery.
- **The IR is the semantic contract** (principle 9). Compiled output is a projection of the IR, not a direct transformation of source.

## Frontmatter

Every compiled file starts with YAML frontmatter:

```yaml
---
name: <skill-name>
description: <when this skill should be used>
---
```

### Required Keys

- `name` — the skill identifier. Machine-readable, used for skill selection and referencing.
- `description` — a concise statement of when and why an agent should use this skill. This is the primary trigger for coding agents that select skills from frontmatter.

### No H1 Heading

The compiled file does not emit a `# <Skill Name>` heading. The frontmatter `name` is the authoritative title. All agents that consume compiled skills are assumed to parse YAML frontmatter.

**Future TODO:** If Glyph needs to support agents that do not parse frontmatter, add an optional compiler flag to emit a `# <Skill Name>` heading after the frontmatter close.

## Sections

The compiled file uses H2 sections in a fixed order. All sections except `## Instructions` are conditional and omitted when their content is empty.

### Fixed Section Order

1. `## Effects` (conditional)
2. `## Inputs` (conditional)
3. `## Instructions` (required)
4. `## Output` (conditional)
5. `## When To Use` (conditional)

### Section Definitions

#### `## Effects`

Emitted when the skill has meaningful effects (not `effects: none`). Each effect is a bullet with a human-readable expansion describing what the skill touches. Effects come from effect metadata, not from instruction roles.

```md
## Effects

- Reads files (repository structure, source code, logs)
- Writes files (source code, configuration)
- Runs commands (git, test runners)
```

Omitted entirely when the skill declares `effects: none` or has no meaningful effects.

Effects appear first because they are one of the first things users and agents need to see before deciding to execute a skill.

#### `## Inputs`

Emitted when the skill declares parameters, `InputContract` nodes, or `Context` nodes that should be visible before execution.

```md
## Inputs

- `scope` — the area of code to inspect (file path, module name, or description)
- `risk` — risk level for the change; defaults to `"medium"`
```

Omitted when the skill has no parameters, input contracts, or visible context.

#### `## Instructions`

Always emitted. Contains the compiled workflow and behavioral constraints. Uses H3 sub-sections for internal structure.

```md
## Instructions

### Steps

1. Inspect the failure and reproduce it.
2. Identify the root cause before proposing a fix.
3. Patch minimally — do not refactor unrelated code.
4. Run validation before reporting success.

### Constraints

- Do not make unrelated edits outside the requested scope.
- Follow the repository's existing patterns, helper APIs, naming, and file organization before introducing a new abstraction or style.
```

**`### Steps`** uses numbered lists because order matters. Each item is one instruction.

**`### Constraints`** uses bulleted lists because order usually does not matter. Each item is one `Constraint` node. Constraint strength and polarity affect wording and prominence, not the basic placement in the MVP.

Both sub-sections are conditional within Instructions: if a skill has no explicit constraints, `### Constraints` is omitted; if a skill has no workflow steps (instruction-only skills), `### Steps` may be omitted. However, at least one of the two sub-sections must be present since `## Instructions` is always emitted.

#### `## Output`

Emitted when the skill declares an explicit return contract or `OutputContract` nodes.

```md
## Output

- A concise summary of changes made, including file paths modified.
- Any issues that could not be resolved, with explanation.
```

Omitted when the skill has no explicit output contract (implicit outputs do not warrant a section).

#### `## When To Use`

Emitted only when the source contains trigger guidance that does not fit cleanly in the frontmatter `description`. This is for detailed situational triggers, not a restatement of `description`. Trigger guidance is routing metadata, not an instruction role.

```md
## When To Use

- The user reports a test failure or unexpected behavior.
- A CI pipeline fails and the user asks for help debugging.
- The user says "fix" or "debug" in relation to existing code.
```

Omitted when `description` is sufficient.

## Instructions Internal Structure

The `## Instructions` section uses H3 sub-sections to separate workflow from constraints. The MVP sub-section set is:

- `### Steps` — ordered workflow actions (numbered list)
- `### Constraints` — behavioral constraints and rules (bulleted list)

This set may grow in the future (e.g., `### Guidance` for preferred constraints distinct from required constraints), but the MVP starts with two.
If a later target splits preferred constraints into guidance, that should be a projection choice over `Constraint(strength: preferred)`, not a new IR role.

### Projection Rules

Compiled output projects from the typed IR role model in [ir-roles.md](ir-roles.md):

- **Effects** project into `## Effects` from effect metadata. Effects are not instruction roles.
- **Input contracts** (IR `InputContract` nodes and parameters) project into `## Inputs`.
- **Context** (IR `Context` nodes) projects into `## Inputs` as informational context or assumptions. The wording must not turn context into a requirement.
- **Workflow steps** (IR `Step` nodes from `flow:`) project into `### Steps` as numbered items.
- **Constraints** (IR `Constraint` nodes) project into `### Constraints` as bulleted items in the MVP.
- **Constraint strength** affects wording and prominence. `invariant` constraints should be rendered as strongest non-negotiable constraints, `required` constraints as mandatory rules, and `preferred` constraints as guidance that yields to stronger constraints.
- **Constraint polarity** affects phrasing. `polarity: require` renders as a positive obligation; `polarity: avoid` renders as a prohibition.
- **Output contracts** (IR `OutputContract` nodes and `return` contracts) project into `## Output`.
- **Conditional logic** (`if` in source) is flattened into prose instructions in the target section for the contained role. The compiled output does not use code-like branching syntax.

The role name should not be changed to match a Markdown section. The same `Constraint` role can produce different wording based on strength and polarity.

## Authoring Constructs Compile Away

Compiled output is fully self-contained. No authoring machinery survives into the emitted file:

- **Imports** resolve and inline. If a skill uses an imported instruction such as `repo_tools.unrelated_edits`, the compiled output contains the expanded instruction text. No import paths, module references, or library names appear.
- **Text references** resolve and inline. A bare name like `preserve_existing_patterns` becomes its full text content in the compiled output.
- **Generated definitions** resolve and inline. The `generated definition` metadata (`summary:`, the `generated` marker) is stripped. Only the instruction content appears.
- **No provenance markers.** The compiled output does not contain comments like `<!-- expanded from repo_tools.unrelated_edits -->`. Clean output only.

### Only Used Imports Are Inlined

The compiler inlines only imported declarations that are actually used in the skill. Unused imports are dead code and are excluded from compiled output. The compiled file is shaped by what the skill uses, not by what the source file imports.

**Source auto-fix for unused imports.** The compiler should automatically remove unused import declarations from the source `.glyph.md` file, similar to how modern language toolchains strip unused imports. This is a source-to-source fix that happens before or during compilation — not a silent omission. The exact pipeline stage for this auto-fix (pre-compilation lint, part of the repair pass, or a dedicated import-pruning pass) is an open question, but the behavior is: if you import something you don't use, the compiler removes it from your source file.

## Formatting Rules

These rules keep compiled output reliably parseable by current coding agents:

1. **One instruction per list item.** No run-on multi-sentence bullets. Each numbered step or bulleted constraint is a single, clear instruction.
2. **Numbered lists for `### Steps`** (order matters). **Bulleted lists for `### Constraints`** (order usually does not matter).
3. **No hard line-wrapping mid-sentence.** Each list item is a single unwrapped line. Agents handle long lines better than soft-wrapped prose that looks like multiple items.
4. **Single blank line between sections and sub-sections.** No double blank lines, no trailing whitespace.
5. **No inline HTML or special formatting.** Compiled output uses only standard Markdown: headings, lists, bold, code spans. No HTML tags, no custom directives.

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

- **Effect vocabulary**: The `## Effects` section content depends on the finalized effect keywords and their human-readable expansions.
- **IR role taxonomy**: The projection from IR roles, constraint strength, and constraint polarity is defined in [ir-roles.md](ir-roles.md).
- **Source syntax**: The compiled output shape is independent of source syntax decisions, since output is a projection of the IR.

## Open Questions

- Whether target-specific renderers should add `### Guidance` for `Constraint(strength: preferred)`, or whether preferred constraints should stay merged with Constraints.
- The exact wording and prominence rules for `Constraint(strength: invariant)`.
- Whether the compiler should emit a `## Effects` section with "None" content vs. omitting it entirely for pure skills. Current decision: omit entirely.
