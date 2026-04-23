# Glyph IR Roles, Constraints, Effects, and Section Vocabulary

This document is the single authoritative source for Glyph's MVP IR structure, constraint model, effect vocabulary, and section-to-IR mapping.

## 1. IR Roles

The MVP instruction role set is **closed**:

| Role | Meaning |
|------|---------|
| `InputContract` | What must be provided at invocation time, or what an input must mean for the unit to be valid. Defines the caller/callee boundary — differs from `Context` (which informs but is not required) and from `Constraint` (which governs behavior, not inputs). |
| `Step` | An ordered action in the workflow. Inside `flow:`, bare calls default to `Step`. A step may carry effect annotations, but effects are not roles. |
| `Constraint` | A behavioral rule governing how work is performed. Hard positive rules, prohibitions, and preferences are all constraints with different strength/polarity attributes — they do not become separate roles. |
| `Context` | Non-normative information the agent needs to interpret the task. Not an obligation. Repair must not silently turn context into required behavior. If text could be either context or a constraint, the compiler should request clarification rather than defaulting to `Context`. |
| `OutputContract` | What the final result, return value, or report should contain or satisfy. Describes the result boundary, not a workflow action (`Step`) or a process rule (`Constraint`). |

Activation/routing rules, preconditions, failure policies, and effects are **not** MVP instruction roles. They are either separate IR structures or deferred design areas.

### Why This Set

- **Input-first, not output-first.** Roles classify author intent. Markdown sections are target-specific projections and should not determine the semantic taxonomy.
- **One `Constraint` role.** Strength and polarity are attributes, not separate roles. This keeps the taxonomy small while preserving the semantics needed for repair, compilation, and visualization.
- **Effects stay separate.** A role answers "what kind of intent is this instruction?" An effect answers "what external capability or side effect does this unit perform?" Conflating them would force a call like `inspect_repo(scope)` to be both `Step` and `Effect`. Effects remain annotations on skills, blocks, calls, and steps.

### Non-Roles (Deferred)

- **Activation** — when a skill should be selected. Routing metadata, not execution intent.
- **Preconditions** — related to input contracts but may eventually deserve their own construct. For MVP, invocation requirements belong under `InputContract`.
- **Failure policy** — what to do when assumptions fail. Deferred; simple conditional behavior uses constraints or workflow structure.

### Projection Guidance

Projection from IR to compiled Markdown is target-specific. General guidance:

- `Step` controls ordered workflow rendering.
- `Constraint` controls behavioral rule rendering; strength and polarity influence wording, prominence, and protection against demotion.
- `InputContract` controls input/assumption rendering.
- `Context` controls informational context rendering.
- `OutputContract` controls final-result or return-contract rendering.

The IR preserves role, strength, and polarity even if a target currently renders several distinctions near each other.

## 2. Constraints

### Strength x Polarity Model

Every `Constraint` IR node carries two structured attributes:

```text
Constraint {
  strength: invariant | required | preferred
  polarity: require | avoid
}
```

**Strength:**

- `invariant` — must always be preserved; strongest contract.
- `required` — must be followed for this skill or block.
- `preferred` — should be followed when compatible with stronger constraints.

**Polarity:**

- `require` — positive obligation: do this.
- `avoid` — negative obligation: do not do this.

### Source Marker Table

| Source marker | IR mapping |
|---------------|------------|
| `require` | `Constraint(strength: required, polarity: require)` |
| `avoid` | `Constraint(strength: required, polarity: avoid)` |
| `prefer` | `Constraint(strength: preferred, polarity: require)` |
| `always` | `Constraint(strength: invariant, polarity: require)` |
| `always avoid` | `Constraint(strength: invariant, polarity: avoid)` |
| `prefer avoid` | `Constraint(strength: preferred, polarity: avoid)` |

`always` modifies strength. `avoid` modifies polarity. This allows the source to stay readable without multiplying IR roles.

Other source markers:

| Marker | IR mapping |
|--------|------------|
| `input` | `InputContract` |
| `output` | `OutputContract` |
| `flow` | contains `Step` nodes |

`context` may be available as an author-facing disambiguator but is not part of the everyday recommended marker set. Most `Context` nodes come from clearly non-normative inline text or repaired/intermediate source.

### Marker-Plus-Concept Form

The canonical source form is marker-plus-concept. Authors may write compact compound names such as `avoid_unrelated_edits`, but repair normalizes them to explicit marker form such as `avoid unrelated_edits` and emits a notification.

### Body-Level Constraint Normalization

Authors may write constraint markers directly at body level without a `constraints:` wrapper:

```glyph
skill fix_bug(scope)
    require preserve_existing_patterns
    avoid unrelated_edits
    flow:
        ...
```

The compiler normalizes body-level markers into a `constraints:` section as a source-to-source rewrite (part of the repair/formatting pass). Both forms produce identical IR. The canonical source form always uses the `constraints:` section.

### Inference And Repair

Authors should be able to write terse source. The compiler infers roles, strength, and polarity where possible, and the repair pass materializes the minimal explicit marker back into source when confidence is high.

Evidence order:

1. Explicit marker in source.
2. Metadata from same-file `text` or block declarations.
3. Metadata from imported or standard-library declarations.
4. Position and structure (e.g., `flow:` implies `Step`).
5. Compound-name cues (`avoid_*`, `prefer_*`, `always_*`, `never_*`, `must_never_*`) — repaired to marker-plus-concept form.
6. LLM repair-generated definitions.
7. Diagnostic if role, strength, or polarity remains ambiguous.

`require`, `avoid`, and `prefer` may be inferred during repair when evidence is clear. `always` must be inferred conservatively — only when the source already carries invariant-level intent (trusted metadata, strong wording like `always_*`, `never_*`, `must_never_*`). A plain `avoid_*` cue repairs to required avoidance, not invariant. `always` should stay rare; it is not just a more emphatic `require`.

## 3. Effects

### MVP Keywords

Eight `verb_noun` snake_case effect keywords:

| Keyword | Meaning |
|---------|---------|
| `none` | No meaningful effects. Pure or near-pure computation. |
| `reads_files` | Inspects files, repository contents, source code, logs, or other local file-system artifacts. |
| `reads_env` | Reads environment variables, system state, git metadata, or project configuration that is not file content. |
| `writes_files` | Creates or modifies files such as source code, configuration, or data files. |
| `runs_commands` | Invokes shell commands, test runners, formatters, linters, package managers, or similar tools. |
| `uses_network` | Accesses web resources, downloads packages, calls remote APIs, or contacts external services. |
| `asks_user` | Pauses execution to request human input, approval, or clarification. |
| `creates_artifacts` | Produces durable outputs (reports, generated assets, compiled Markdown, archives). Distinct from `writes_files`: artifact creation is the skill's purpose, not a side-effect file edit. |

### Syntax

The `effects:` clause may appear on `skill`, `block`, and `export block` declarations. Two forms; the compiler normalizes both to the same IR:

```glyph
// Inline (preferred for short lists)
effects: reads_files, runs_commands

// Indented list (preferred for longer lists)
effects:
    - reads_files
    - writes_files
    - runs_commands
```

### `none` Semantics

- Omitting `effects:` entirely is equivalent to `effects: none`.
- Writing `effects: none` explicitly is allowed for documentation.
- `none` must not appear alongside other keywords. `effects: none, reads_files` is a compile error.

### Propagation

The compiler infers effects by walking the call graph:

- Each primitive call or block call contributes its declared or inferred effects.
- A block's inferred effect set is the **union** of its own direct effects and the effects of every block it calls.
- Skills, exported blocks, and private blocks all participate in inference.
- There is no effect subtraction or masking in the MVP.
- Effect sets are unordered; the compiler may sort them alphabetically or by declaration order.

### Author Declaration And Validation

Authors may optionally declare `effects:` for readability. When declared, the compiler validates that the **declared set is a superset of the inferred set**. If the declared set is smaller than inferred, that is a compile error (the declaration is lying about what the block does).

Import contracts are satisfied through the compiler's output: the IR and compiled Markdown always contain the full inferred effect set regardless of whether the author wrote `effects:`.

### Effects Are Not Instruction Roles

A role classifies author intent. An effect classifies capabilities or side effects. A step in a flow is `Step` with effect annotations — it is not simultaneously an `Effect` role. Effects remain separate annotations on skills, blocks, calls, and steps.

### Extension Policy

- New keywords may be added (e.g., `reads_database`, `sends_messages`).
- Existing keywords are never renamed or removed once stabilized.
- Old skills are unaffected; their import contracts remain valid.
- No namespacing in MVP. If the flat namespace becomes crowded, namespacing may be added as a backwards-compatible extension.
- New effects follow the `verb_noun` snake_case convention.

## 4. Section Vocabulary

### The Six MVP Sub-Section Headers

Six colon-terminated headers are available inside `skill`, `block`, and `export block` bodies:

| Section | Spelling | Content |
|---------|----------|---------|
| `effects:` | plural | Effect keywords (see section 3) |
| `constraints:` | plural | Constraint markers: `require`, `avoid`, `prefer`, `always` + concept |
| `inputs:` | plural | `input` marker statements — InputContract detail beyond header params |
| `outputs:` | plural | `output` marker statements — OutputContract detail beyond `return` |
| `flow:` | singular | Ordered steps: calls, bindings, `return`, `if`, bare names, inline strings |
| `when_to_use:` | snake_case phrase | Trigger guidance for skill routing |

**Spelling convention:** all headers use snake_case. Plural for set-like sections. Singular for the workflow container. Multi-word phrase for `when_to_use:`.

### No `context:` Section

`Context` is an IR role but has no dedicated section header. Context is non-normative information that authors place inline — as quoted strings, bare informational text, or with the `context` disambiguator. The compiler classifies clearly non-normative text as `Context` IR nodes without a separate section.

### Section Content Rules

**`inputs:`** — adds InputContract detail beyond header parameters. Header params define names and types; `inputs:` adds semantic descriptions, availability assumptions, or contract prose. Not a duplicate of header parameters. When present, InputContract nodes merge with header param information in compiled output. Omitted when parameters are self-explanatory.

**`outputs:`** — adds OutputContract detail beyond `return`. `return` in `flow:` defines what value is produced; `outputs:` describes what the output should contain or satisfy. Not a duplicate of `return`. Omitted when the return value is self-explanatory.

**`flow:`** — the ordered workflow section. All content defaults to the `Step` IR role unless explicit syntax or resolved metadata says otherwise. The only section that contains ordered, sequential content.

**`when_to_use:`** — trigger guidance for skill routing, beyond what fits in `description`. Available only on `skill` declarations.

### Mandatory / Optional Per Declaration Kind

| Section | `skill` | `block` | `export block` |
|---------|---------|---------|----------------|
| `effects:` | Optional | Optional | Optional (validated against inference) |
| `constraints:` | Optional | Optional | Optional |
| `inputs:` | Optional | Optional | Optional |
| `outputs:` | Optional | Optional | Optional |
| `flow:` | Required (unless instruction-only) | Optional | Expected (needs explicit `return`) |
| `when_to_use:` | Optional | N/A | N/A |

A `skill` body must contain at least `constraints:` or `flow:` (or both). An empty skill body is a compile error. An `export block` must have an explicit `return` path, which in practice means it will have `flow:`. `when_to_use:` is restricted to `skill` because trigger guidance is routing metadata for the compiled entrypoint.

### Recommended Source Order

Source order is free — the compiler reorders to the fixed compiled-output order. Recommended convention:

1. `effects:`
2. `constraints:`
3. `inputs:` (if used)
4. `flow:`
5. `outputs:` (if used)
6. `when_to_use:` (if used)

The compiler's source normalization pass enforces this order when rewriting.

## Open Questions

- Whether the compiler should warn on over-declared effects (declared broader than inferred). Not an error but may indicate stale annotations.
- Whether effect annotations on individual calls within a flow are useful for MVP or should be deferred.
- How standard-library primitives declare their effect signatures.
- Whether `constraints:` should allow mixing strengths or group by strength.
- Whether source normalization should also sort constraints by strength within `constraints:`.
- `context:` as a future section header if inline context proves insufficient.
