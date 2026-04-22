# Glyph Section Vocabulary

This document defines the canonical set of sub-section headers inside `skill`, `block`, and `export block` bodies. It covers spelling, mandatory/optional rules per declaration kind, source ordering, normalization, and the mapping from source sections to compiled output.

## Status

Builds on:

- `block-structure.md` — defines colon-terminated sub-section syntax, indentation, and short/long forms
- `declaration-headers.md` — defines header-line syntax including parameters and return types
- `compiled-output.md` — defines the fixed compiled section order
- `ir-roles.md` — defines the MVP instruction role taxonomy
- `effects.md` — defines effect keywords and `effects:` clause syntax

## MVP Sub-Section Headers

Six colon-terminated sub-section headers are available inside declaration bodies:

| Section | Spelling | Content |
|---------|----------|---------|
| `effects:` | plural | Effect keywords per `effects.md` |
| `constraints:` | plural | Constraint markers: `require`, `avoid`, `prefer`, `always` + concept |
| `inputs:` | plural | `input` marker statements describing InputContract beyond header params |
| `outputs:` | plural | `output` marker statements describing OutputContract beyond `return` |
| `flow:` | singular | Ordered steps: calls, bindings, `return`, `if`, bare names, inline strings |
| `when_to_use:` | snake_case phrase | Trigger guidance for skill routing |

### Spelling Convention

All section headers use snake_case. Plural for set-like sections (`effects:`, `constraints:`, `inputs:`, `outputs:`). Singular for the workflow container (`flow:`). Multi-word phrase for `when_to_use:`.

### No `context:` Section

`Context` is an MVP IR role (`ir-roles.md`), but it does not have a dedicated section header. Context is non-normative information that authors typically place inline — as quoted strings, bare informational text, or with the `context` disambiguator — alongside other body content. The compiler classifies clearly non-normative text as `Context` IR nodes without requiring a separate section.

## Section Content

### `effects:`

Effect keywords from the MVP vocabulary (`effects.md`). Two forms per `block-structure.md`:

```glyph
// Short form
effects: reads_files, runs_commands

// Long form
effects:
    - reads_files
    - writes_files
    - runs_commands
```

Omitting `effects:` entirely is equivalent to `effects: none`. When declared explicitly, the compiler validates the declared set is a superset of the inferred set.

### `constraints:`

Constraint markers using the strength/polarity vocabulary from `ir-roles.md`:

```glyph
constraints:
    require preserve_existing_patterns
    avoid unrelated_edits
    prefer simple_solution
    always preserve_user_data
    always avoid exposing_secrets
    prefer avoid broad_refactors
```

Each line inside `constraints:` is a marker-plus-concept statement. The marker sets IR strength and polarity:

- `require` — `Constraint(strength: required, polarity: require)`
- `avoid` — `Constraint(strength: required, polarity: avoid)`
- `prefer` — `Constraint(strength: preferred, polarity: require)`
- `always` — `Constraint(strength: invariant, polarity: require)`
- `always avoid` — `Constraint(strength: invariant, polarity: avoid)`
- `prefer avoid` — `Constraint(strength: preferred, polarity: avoid)`

#### Body-Level Constraint Normalization

Authors may write constraint markers directly at body level without a `constraints:` section wrapper:

```glyph
skill fix_bug(scope)
    require preserve_existing_patterns
    avoid unrelated_edits

    flow:
        ...
```

The compiler normalizes body-level constraint markers into a `constraints:` section in the source file. This is a source-to-source normalization, similar to how compound names like `avoid_unrelated_edits` are repaired to `avoid unrelated_edits` (`ir-roles.md`). The canonical source form always uses the `constraints:` section:

```glyph
skill fix_bug(scope)
    effects: reads_files, writes_files, runs_commands

    constraints:
        require preserve_existing_patterns
        avoid unrelated_edits

    flow:
        ...
```

Both forms produce identical IR. The normalization runs as part of the repair/formatting pass and rewrites the source file.

### `inputs:`

`input` marker statements that add InputContract detail beyond what the header parameters declare. Header parameters define names and types; `inputs:` adds semantic descriptions, availability assumptions, or contract prose.

```glyph
skill fix_bug(scope)
    inputs:
        input scope identifies the target file or module
        input failing_log is available

    flow:
        ...
```

`inputs:` is not a duplicate of header parameters. The compiler projects header params into `## Inputs` in compiled output regardless of whether `inputs:` exists. When `inputs:` is present, its InputContract nodes merge with header param information in the compiled `## Inputs` section.

If a skill's parameters are self-explanatory from their names and types, `inputs:` is omitted.

### `outputs:`

`output` marker statements that describe OutputContract detail beyond what `return` provides. `return` in `flow:` defines what value is produced; `outputs:` describes what the output should contain or satisfy.

```glyph
skill fix_bug(scope)
    outputs:
        output explain the root cause
        output list file paths modified

    flow:
        ...
        return summarize_changes()
```

`outputs:` is not a duplicate of `return`. The compiler projects `return` type and `outputs:` OutputContract nodes together into `## Output` in compiled output.

If a skill's return value is self-explanatory, `outputs:` is omitted.

### `flow:`

The ordered workflow section. Contains calls, local bindings, `return` statements, `if` branches (MVP), bare instruction names, and inline quoted strings. All content inside `flow:` defaults to the `Step` IR role unless explicit syntax or resolved metadata says otherwise (`ir-roles.md`).

```glyph
flow:
    ctx = inspect_repo(scope)
    plan = make_plan(ctx, risk)
    apply_changes(plan)
    validate(plan)
    return summarize(plan)
```

`flow:` is the only section that contains ordered, sequential content. Compiled output projects `flow:` steps into `### Steps` as numbered items under `## Instructions`.

### `when_to_use:`

Trigger guidance for skill routing. Describes when an agent should select this skill, beyond what fits in the frontmatter `description`.

```glyph
when_to_use:
    - "The user reports a test failure or unexpected behavior."
    - "A CI pipeline fails and the user asks for help debugging."
    - "The user says 'fix' or 'debug' in relation to existing code."
```

Available only on `skill` declarations. Compiled output projects `when_to_use:` into `## When To Use`. Omitted when `description` is sufficient.

## Mandatory vs. Optional Per Declaration Kind

| Section | `skill` | `block` | `export block` |
|---------|---------|---------|----------------|
| `effects:` | Optional | Optional | Optional (compiler validates against inference) |
| `constraints:` | Optional | Optional | Optional |
| `inputs:` | Optional | Optional | Optional |
| `outputs:` | Optional | Optional | Optional |
| `flow:` | Required (unless instruction-only) | Optional | Expected (needs explicit `return`) |
| `when_to_use:` | Optional | Not available | Not available |

A `skill` body must contain at least a `constraints:` section or a `flow:` section (or both). An empty skill body is a compile error.

An `export block` must have an explicit `return` path (`data-flow-and-calls.md`), which in practice means it will have a `flow:` section. The `effects:` clause is optional on `export block` because the compiler infers effects, but declared effects are validated against inference.

`when_to_use:` is restricted to `skill` because trigger guidance is routing metadata for the compiled skill entrypoint, not for helper blocks.

## Recommended Source Order

Source section order is free — the compiler reorders content to the fixed compiled-output order regardless of source arrangement. However, the following convention is recommended for readability:

1. `effects:`
2. `constraints:`
3. `inputs:` (if used)
4. `flow:`
5. `outputs:` (if used)
6. `when_to_use:` (if used)

This convention mirrors the compiled output order (Effects -> Inputs -> Instructions -> Output -> When To Use), with `constraints:` placed early because constraints frame how the workflow should be executed.

The compiler's source normalization pass enforces this order when rewriting the source file. Authors may write sections in any order; the normalized source will follow this convention.

## Source-To-Compiled-Output Mapping

| Source form | Compiled section | Reference |
|-------------|-----------------|-----------|
| `effects:` | `## Effects` | `compiled-output.md` §Effects |
| Header params + `inputs:` + body `input` markers | `## Inputs` | `compiled-output.md` §Inputs |
| Body `context` markers + inline non-normative text | `## Inputs` (as informational context) | `compiled-output.md` §Inputs, `ir-roles.md` §Context |
| `flow:` steps | `### Steps` under `## Instructions` | `compiled-output.md` §Instructions |
| `constraints:` content | `### Constraints` under `## Instructions` | `compiled-output.md` §Instructions |
| `return` in flow + `outputs:` + body `output` markers | `## Output` | `compiled-output.md` §Output |
| `when_to_use:` | `## When To Use` | `compiled-output.md` §When To Use |

Context nodes project into `## Inputs` as informational context or assumptions. The wording must not turn context into a requirement (`compiled-output.md` projection rules).

Constraint strength and polarity affect compiled wording and prominence per `ir-roles.md` and `compiled-output.md` projection rules.

## Complete Example

```glyph
import "./repo_tools.glyph.md" { unrelated_edits, preserve_existing_patterns }

skill fix_bug(scope)
    effects: reads_files, writes_files, runs_commands

    constraints:
        require preserve_existing_patterns
        avoid unrelated_edits
        prefer simple_solution

    inputs:
        input scope identifies the target file, module, or bug description

    flow:
        inspect_failure(scope)
        identify_root_cause()
        patch_minimally()
        validate_before_success
        return summarize_changes()

    outputs:
        output explain the root cause of the bug
        output list all file paths modified
```

Compiles to:

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

- `scope` — the target file, module, or bug description

## Instructions

### Steps

1. Inspect the failure within the given scope and reproduce it.
2. Identify the root cause before proposing or applying a fix.
3. Patch minimally — change only what is necessary to fix the bug.
4. Run validation to confirm the fix works before reporting success.

### Constraints

- Follow the repository's existing patterns, helper APIs, naming, and file organization before introducing a new abstraction or style.
- Do not make unrelated edits outside the requested scope.
- Prefer the simplest solution that addresses the bug.

## Output

- The root cause of the bug.
- A summary of changes made, including all file paths modified.
```

## Deferred

- Exact content grammar inside each section body (depends on call-site syntax and type vocabulary)
- Whether `constraints:` section allows mixing strengths or should group by strength
- `for_each` inside `flow:` (post-MVP per principle 7)
- Whether source normalization should also sort constraints by strength within `constraints:`
- `context:` as a future section header if inline context proves insufficient
