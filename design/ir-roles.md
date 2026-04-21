# Glyph IR Roles

This document defines the MVP IR role taxonomy for Glyph instructions.

The role taxonomy is input-first: it classifies what kind of intent the author
is expressing, not where the instruction happens to appear in compiled Markdown.
Compiled output may project several roles into the same visible section, but the
IR should preserve the distinctions needed for repair, validation, visualization,
and future compilation targets.

## Decision

The MVP instruction role set is closed:

- `InputContract`
- `Step`
- `Constraint`
- `Context`
- `OutputContract`

Effects, activation/routing rules, preconditions, and failure policies are not
MVP instruction roles. They are either separate IR structures or deferred design
areas.

## Decision Rationale

The role set is intentionally smaller than the number of ways an instruction can
be rendered. Roles classify author intent; attributes and metadata capture other
dimensions.

- **Input-first, not output-first.** Roles describe what the author is telling
  the agent. Markdown sections are target-specific projections and should not
  determine the semantic taxonomy.
- **Constraints stay one role.** Hard positive rules, prohibitions, and
  preferences are all behavioral constraints. Splitting them into separate roles
  would encode strength and polarity into the role name and make the taxonomy
  larger without adding semantics.
- **Strength is an attribute.** `invariant`, `required`, and `preferred` should
  affect repair potency, wording, prominence, and future specialization rules
  without creating new roles.
- **Polarity is an attribute.** Positive obligations and prohibitions need
  different phrasing and contradiction checks, but both are constraints.
- **Effects stay separate.** Effects describe capabilities or side effects, such
  as reading files or running commands. A step can have effects; an effect is not
  a competing instruction role.
- **Repair makes terse source explicit.** Authors can write concise shorthand,
  and repair should add `require`, `avoid`, or `prefer` back into source when
  evidence is clear, before strict IR compilation. `always` is conservative
  because it means invariant strength.

## Role Semantics

### `InputContract`

An `InputContract` states what must be provided to the skill or block at
invocation time, or what an input must mean for the unit to be valid.

Examples:

```glyph
input scope identifies the target package
input failing_log is available
```

`InputContract` differs from `Context` because it is required for invocation.
`Context` informs behavior; an input contract defines the caller/callee boundary.

`InputContract` differs from `Constraint` because it governs whether the unit has
the required inputs to begin, not how the agent should behave while doing the
work.

### `Step`

A `Step` is an ordered action in the workflow.

Examples:

```glyph
flow:
    inspect_failure(scope)
    identify_root_cause()
    patch_minimally()
```

Inside `flow:`, bare calls default to `Step` unless explicit syntax or resolved
metadata says otherwise. A step may have effects such as `reads_files` or
`runs_commands`, but those effects are annotations on the step, not roles.

### `Constraint`

A `Constraint` governs behavior while performing the skill or block.

The MVP keeps one constraint role and represents strength and polarity as
structured attributes:

```text
Constraint {
  strength: invariant | required | preferred
  polarity: require | avoid
}
```

Strength means:

- `invariant` - must always be preserved; strongest contract.
- `required` - must be followed for this skill or block.
- `preferred` - should be followed when compatible with stronger constraints.

Polarity means:

- `require` - positive obligation: do this.
- `avoid` - negative obligation: do not do this.

Examples:

```glyph
always preserve_user_data
always avoid exposing_secrets

require validate_before_success
avoid unrelated_edits

prefer simple_solution
prefer avoid broad_refactors
```

Normalized examples:

```text
Constraint(strength: invariant, polarity: require, text: preserve_user_data)
Constraint(strength: invariant, polarity: avoid, text: exposing_secrets)
Constraint(strength: required, polarity: require, text: validate_before_success)
Constraint(strength: required, polarity: avoid, text: unrelated_edits)
Constraint(strength: preferred, polarity: require, text: simple_solution)
Constraint(strength: preferred, polarity: avoid, text: broad_refactors)
```

Hard positive rules, prohibitions, and preferences do not become separate roles.
They are all constraints with different attributes. This keeps the role set small
while preserving the semantics needed for repair and compilation.

### `Context`

`Context` provides non-normative information the agent needs to interpret the
task.

Examples:

```glyph
"The repo uses pnpm."
"Generated files live under dist/."
```

`Context` is intentionally separate from `Constraint`. Context may affect how an
agent reasons, but it is not itself an obligation. Repair must not silently turn
context into required behavior.

`context` may exist as an author-facing disambiguator, but authors should not
need to write it most of the time. They are more likely to write inline
informational text, comments, or other lightweight source forms. The compiler may
classify clearly non-normative facts as `Context` in the IR or in a
repaired/intermediate source form. If a quoted or bare instruction could be
either context or a constraint, the compiler should request clarification rather
than using `Context` as a fallback bucket.

### `OutputContract`

An `OutputContract` states what the final result, return value, or report should
contain or satisfy.

Examples:

```glyph
output explain tradeoffs
output mention validation status
return summarize_changes()
```

`OutputContract` differs from `Step` because it describes the result boundary,
not an action in the workflow. It differs from `Constraint` because it governs
the deliverable rather than the process.

## Source Markers

The MVP source language should make common roles obvious while still allowing
terse authoring and repair-assisted normalization.

Recommended source markers:

```text
input     -> InputContract
output    -> OutputContract
flow      -> contains Step nodes

always    -> Constraint(strength: invariant, polarity: require)
require   -> Constraint(strength: required, polarity: require)
avoid     -> Constraint(strength: required, polarity: avoid)
prefer    -> Constraint(strength: preferred, polarity: require)
```

Composed constraint markers:

```text
always avoid  -> Constraint(strength: invariant, polarity: avoid)
prefer avoid  -> Constraint(strength: preferred, polarity: avoid)
```

`always` modifies strength. `avoid` modifies polarity. This allows the source to
stay readable without multiplying IR roles.

`context` may be available as an author-facing disambiguator, but it is not part
of the everyday recommended marker set. Most `Context` nodes should come from
clearly non-normative inline text or repaired/intermediate source.

Marker-plus-concept form is the canonical source form. Authors may write compact
compound names such as `avoid_unrelated_edits`, but repair should normalize them
to explicit marker form such as `avoid unrelated_edits` and emit a notification.
The purpose is to keep source easy to write while making the final source
deterministic and visibly structured.

## Inference And Repair

Authors should be able to write terse source. The compiler should infer roles,
strength, and polarity where possible, and the repair pass should materialize the
minimal explicit marker back into source when confidence is high. Strict IR
compilation should operate on the repaired explicit source rather than relying
on silent role guesses.

Recommended evidence order:

1. Explicit marker in source.
2. Metadata from same-file `text` or block declarations.
3. Metadata from imported or standard-library declarations.
4. Position and structure, such as `flow:` implying `Step`.
5. Compound-name cues, such as `avoid_*`, `prefer_*`, `always_*`, `never_*`, or
   `must_never_*`, which should be repaired to marker-plus-concept form when
   confidence is high.
6. LLM repair-generated definitions.
7. Diagnostic if the role, strength, or polarity remains ambiguous.

`require`, `avoid`, and `prefer` may be inferred during repair when evidence is
clear.

`always` must be inferred conservatively because it means invariant strength.
Repair may add `always` only when the source already carries invariant-level
intent, such as trusted metadata or strong wording like `always_*`, `never_*`, or
`must_never_*`. A plain `avoid_*` cue should normally repair to required
avoidance, not invariant avoidance.

`always` should stay rare. It is not just a more emphatic `require`; overuse
would make compiled output noisy and future specialization rules too rigid.

Example:

```glyph
skill fix_bug(scope)
    preserve_existing_patterns
    unrelated_edits
    simple_solutions

    flow:
        inspect_failure(scope)
        patch_minimally()
```

May repair to:

```glyph
skill fix_bug(scope)
    require preserve_existing_patterns
    avoid unrelated_edits
    prefer simple_solutions

    flow:
        inspect_failure(scope)
        patch_minimally()
```

The exact spelling of markers may still evolve, but the MVP canonical shape is
marker-plus-concept. The semantic requirement is that repaired source exposes
role, strength, and polarity explicitly enough for deterministic IR compilation.

## Non-Roles

### Effects

Effects are not instruction roles.

A role answers: what kind of author intent is this instruction?

An effect answers: what external capability or side effect does this unit
perform or require?

Example:

```glyph
flow:
    inspect_repo(scope)
```

Normalizes conceptually to:

```text
Instruction {
  role: Step,
  effects: [reads_files]
}
```

If effects were roles, a call such as `inspect_repo(scope)` would need to be both
`Step` and `Effect`, which would make the role taxonomy do two jobs. Effects
remain separate annotations on skills, blocks, calls, and steps.

### Activation

Activation or trigger guidance answers when a skill should be selected. That is
routing metadata, not execution intent. It is deferred beyond the MVP role set.

### Preconditions

Preconditions are related to input contracts but may eventually deserve their
own construct. For the MVP, invocation requirements belong under
`InputContract`. A later design may split required inputs from state
preconditions.

### Failure Policy

Failure policy describes what to do when assumptions fail, inputs are missing,
validation fails, or tools cannot run. It is deferred beyond the MVP role set.
Simple conditional behavior can be represented with constraints or workflow
structure until failure policy is designed directly.

## Projection Guidance

Projection from IR to final Markdown is target-specific. The role set should not
be renamed to match any single target's visible sections.

General guidance:

- `Step` controls ordered workflow rendering.
- `Constraint` controls behavioral rule rendering, with strength and polarity
  influencing wording, prominence, repetition, and future protection against
  demotion.
- `InputContract` controls input/assumption rendering.
- `Context` controls informational context rendering.
- `OutputContract` controls final-result or return-contract rendering.

The IR should preserve role, strength, and polarity even if a target currently
renders several of those distinctions near each other.
