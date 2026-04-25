# Glyph IR Roles, Constraints, Effects, and Section Vocabulary

This document is the single authoritative source for Glyph's MVP IR structure, constraint model, effect vocabulary, and section-to-IR mapping.

## 1. IR Roles

The MVP instruction role set is **closed** to four roles:

| Role | Meaning |
|------|---------|
| `InputContract` | What must be provided at invocation time, or what an input must mean for the unit to be valid. Defines the caller/callee boundary — differs from `Constraint` (which governs behavior, not inputs). |
| `Step` | An ordered action in the workflow. Inside `flow:`, bare calls default to `Step`. A step may carry effect annotations, but effects are not roles. |
| `Constraint` | A behavioral rule governing how work is performed. Positive rules, prohibitions, and their soft/hard variants are all constraints with different strength/polarity attributes — they do not become separate roles. |
| `OutputContract` | What the final result, return value, or report should contain or satisfy. Describes the result boundary, not a workflow action (`Step`) or a process rule (`Constraint`). |

`Context` (non-normative informational framing) is **deferred from MVP** — see [todo.md](todo.md). With `## Inputs` removed from compiled output there is no clean projection target, and any genuine context can be authored as a Step, a Constraint, or a leading inline sentence inside `flow:`. The `context` keyword stays reserved for this future restoration.

Activation/routing rules, preconditions, failure policies, and effects are **not** MVP instruction roles. They are either separate IR structures or deferred design areas.

### Why This Set

- **Input-first, not output-first.** Roles classify author intent. Markdown sections are target-specific projections and should not determine the semantic taxonomy.
- **One `Constraint` role.** Strength (`soft`/`hard`) and polarity (`require`/`avoid`) are attributes, not separate roles. This keeps the taxonomy small while preserving the semantics needed for repair, compilation, and visualization.
- **Effects stay separate.** A role answers "what kind of intent is this instruction?" An effect answers "what external capability or side effect does this unit perform?" Conflating them would force a call like `inspect_repo(scope)` to be both `Step` and `Effect`. Effects remain annotations on skills, blocks, calls, and steps.

### Non-Roles (Deferred)

- **Activation** — when a skill should be selected. Routing metadata, not execution intent.
- **Preconditions** — related to input contracts but may eventually deserve their own construct. For MVP, invocation requirements belong under `InputContract`.
- **Failure policy** — what to do when assumptions fail. Deferred; simple conditional behavior uses constraints or workflow structure.

### Projection Guidance

Projection from IR to compiled Markdown is target-specific. MVP projection produces YAML frontmatter, a conditional `## Parameters` section, and `## Instructions` (see [compiled-output.md](compiled-output.md)):

- `Step` → numbered list items under `### Steps`. Parameters carried by the step appear as `{param}` references in the compiled prose, resolved by the consuming LLM at runtime.
- `Constraint` → bulleted items under `### Constraints`. Strength (`soft`/`hard`) and polarity (`require`/`avoid`) influence wording and prominence.
- `InputContract` → projected into the `## Parameters` section of compiled output (names, descriptions, optional defaults). Parameters appear as `{param}` references in Step and Constraint prose, resolved by the consuming LLM at runtime.
- `OutputContract` → folded into the final `Step`. The `return` expression becomes the closing sentence of the last numbered step. No dedicated compiled section in MVP.
- Effects → YAML frontmatter `effects` list, not a prose section.

The IR preserves role, strength (`soft`/`hard`), polarity (`require`/`avoid`), and the full `InputContract` / `OutputContract` structure even though MVP compiled output does not project them as separate sections.

## 2. Constraints

### Strength and Polarity

Every `Constraint` IR node carries two structured attributes:

```text
Constraint {
  strength: soft | hard
  polarity: require | avoid
}
```

**Strength** (advisory — affects compiled prose framing, not enforced at runtime; target agent compliance is not guaranteed):

- `soft` — should be followed; default strength.
- `hard` — must always be followed; strongest contract.

**Polarity:**

- `require` — positive obligation: do this.
- `avoid` — negative obligation: do not do this.

Three source keywords compose into four forms:

### Source Marker Table

| Source marker | IR mapping |
|---------------|------------|
| `require` | `Constraint(strength: soft, polarity: require)` |
| `avoid` | `Constraint(strength: soft, polarity: avoid)` |
| `must` | `Constraint(strength: hard, polarity: require)` |
| `must avoid` | `Constraint(strength: hard, polarity: avoid)` |

`must` is a strength modifier — standalone `must X` is shorthand for `must require X`. `avoid` flips polarity. Three keywords, four forms.

Other source markers:

| Marker | IR mapping |
|--------|------------|
| `flow` | contains `Step` nodes |

`input`, `output`, and `context` markers are deferred from MVP alongside the `inputs:` / `outputs:` sub-sections and the `Context` role (see [todo.md](todo.md)). Header parameters cover input definition; `return` covers output.

### Marker-Plus-Concept Form

Two authoring styles are both valid:

- **Marker-plus-concept:** `avoid unrelated_edits` — the marker keyword carries polarity, the concept name resolves to a polarity-neutral definition.
- **Compound name:** `avoid_unrelated_edits` — the name is a single identifier whose definition carries the full semantics (including polarity).

There is no forced normalization from one form to the other. If a compound name resolves to a declaration, the compiler uses it as-is and infers role, strength, and polarity from the declaration's text content, with the name prefix (`avoid_*`, `must_*`) as supporting evidence. If a compound name is unresolved, repair generates a definition under the full compound name with full semantics baked in — no splitting.

### Source Order: Free Mixing

Inside a `constraints:` section, soft (`require`/`avoid`) and hard (`must`/`must avoid`) markers may appear in any order. The parser preserves source order in the IR for visualization and round-tripping; downstream phases do not depend on it. The compiled output orders constraints independently of source order — strength and polarity affect wording, not placement (see `compiled-output.md`). Authors should group constraints by topic, not by strength.

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

Authors should be able to write terse source. The compiler infers role, strength, and polarity where possible, and the repair pass materializes the minimal explicit marker back into source when confidence is high.

Evidence order:

1. Explicit marker in source.
2. Metadata from same-file `text` or block declarations.
3. Metadata from imported or standard-library declarations.
4. Position and structure (e.g., `flow:` implies `Step`).
5. Compound-name cues (`avoid_*`, `must_*`, `never_*`, `must_never_*`) — used as evidence for role/polarity inference; no forced splitting.
6. LLM repair-generated definitions.
7. Diagnostic if role, strength, or polarity remains ambiguous.

`require` and `avoid` may be inferred during repair when evidence is clear. `must` should be inferred conservatively — only when the source already carries hard-strength intent (trusted metadata, strong wording like `must_*`, `never_*`, `must_never_*`). A plain `avoid_*` cue repairs to soft avoidance, not hard. `must` should stay rare; it is not just a more emphatic `require`.

## 3. Effects

### MVP Keywords

Nine `verb_noun` snake_case effect keywords:

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
| `spawns_agent` | Spawns a subagent to perform delegated work (see [stdlib.md](stdlib.md)). |

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

If the declared set is **larger** than inferred (e.g. `effects: reads_files, runs_commands` when only `reads_files` is inferred), the compiler emits a `warning`-tier diagnostic (`G::analyze::effects-over-declared`). Compilation proceeds. Over-declaration is legitimate (forward-compat, intentional widening of a public contract), so it is not an error; the warning lets the author remove the extra keyword if they are confident it is no longer needed. Repair never narrows a declared effect set, since that would silently break import contracts.

**Across imports and inlines.** A caller's declared `effects:` must additionally be a superset of every imported callee's declared effects and every inlined private callee's inferred effects. This is enforced in Phase 5 (Validate) — see `pipeline.md` and `data-flow.md` §Effect Propagation. Effects propagate by *declaration*, not by transitive analysis: the importer sees only the imported callee's declared contract, so the callee's own Validate pass must have already produced a complete declared set.

Import contracts are satisfied through the compiler's output: the IR and compiled Markdown always contain the full inferred effect set regardless of whether the author wrote `effects:`.

### Effects Are Not Instruction Roles

A role classifies author intent. An effect classifies capabilities or side effects. A step in a flow is `Step` with effect annotations — it is not simultaneously an `Effect` role. Effects remain separate annotations on skills, blocks, calls, and steps.

### Extension Policy

- New keywords may be added (e.g., `reads_database`, `sends_messages`).
- Existing keywords are never renamed or removed once stabilized.
- Old skills are unaffected; their import contracts remain valid.
- No namespacing in MVP. If the flat namespace becomes crowded, namespacing may be added as a backwards-compatible extension.
- New effects follow the `verb_noun` snake_case convention.

### Deferred

- **Per-call effect annotations.** Authors cannot attach an `effects:` clause to an individual call site in MVP. Effects are declared only at the declaration level (`skill`, `block`, `export block`); call-site effects are inferred and stored on the `Call` IR node by the compiler, not author-writable. Adding this later is backwards-compatible. The declaration-based model (see §Author Declaration And Validation) is the single source of truth for the MVP.

## 4. Section Vocabulary

### The Four MVP Sub-Section Headers

Four colon-terminated headers are available inside `skill`, `block`, and `export block` bodies:

| Section | Spelling | Content |
|---------|----------|---------|
| `description:` | singular | One-line summary of when/why to use this skill; compiles to frontmatter `description` |
| `effects:` | plural | Effect keywords (see section 3); compiles to frontmatter `effects` |
| `constraints:` | plural | Constraint markers: `require`, `avoid`, `must` + concept |
| `flow:` | singular | Ordered steps: calls, bindings, `return`, `if`, bare names, inline strings |

`inputs:`, `outputs:`, and `when_to_use:` are deferred from MVP ([todo.md](todo.md)). Header parameters cover input definition; `return` covers output; `description:` covers routing.

**Spelling convention:** all headers use snake_case. Plural for set-like sections. Singular for value and workflow containers.

### `description:` Section

`description:` provides a concise, one-line summary of when and why a skill should be used. It compiles to the `description` field in YAML frontmatter (see `compiled-output.md`), which is the primary trigger for coding agents that select skills. Content is a single inline string or bare text.

```glyph
skill fix_bug(scope)
    description: "Debug and fix a bug in the codebase with minimal, targeted changes."

    flow:
        ...
```

If `description:` is omitted, the compiler generates a description from the skill name and body during the LLM repair pass (Phase 3), adding it as a `description:` sub-section in the source. Authors should prefer explicit descriptions for predictable skill routing.

### Section Content Rules

**`description:`** — a concise one-line summary of when and why to use this skill. Compiles to frontmatter `description`. If omitted, Repair (Phase 3) generates one from the skill name and body and adds it to the source. Available only on `skill` declarations.

**`effects:`** — declared effect keywords (see section 3). Compiles to frontmatter `effects` as a YAML list. Validated against the inferred effect set.

**`constraints:`** — constraint markers using the three keywords (`require`, `avoid`, `must`) in four composed forms. Projects to `### Constraints` under `## Instructions`.

**`flow:`** — the ordered workflow section. All content defaults to the `Step` IR role unless explicit syntax or resolved metadata says otherwise. The only section that contains ordered, sequential content. Projects to `### Steps` under `## Instructions`; `return` folds into the final Step.

### Mandatory / Optional Per Declaration Kind

| Section | `skill` | `block` | `export block` |
|---------|---------|---------|----------------|
| `description:` | Optional (generated if omitted) | N/A | N/A |
| `effects:` | Optional | Optional | Optional (validated against inference) |
| `constraints:` | Optional | Optional | Optional |
| `flow:` | Required (unless instruction-only) | Optional | Expected (needs explicit `return`) |

A `skill` body must contain at least `constraints:` or `flow:` (or both). An empty skill body is a compile error. An `export block` must have an explicit `return` path, which in practice means it will have `flow:`.

### Recommended Source Order

Source order is free — the compiler reorders to the fixed compiled-output order. Recommended convention:

1. `description:` (if used)
2. `effects:`
3. `constraints:`
4. `flow:`

The compiler's source normalization pass enforces this order when rewriting.

## Open Questions

- `context:` as a future section header if inline context proves insufficient.
