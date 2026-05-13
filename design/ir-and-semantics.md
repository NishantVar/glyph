# Glyph IR Roles, Constraints, Effects, And Section Vocabulary

This document is the author-facing semantics for Glyph's MVP IR shape,
constraint model, effect vocabulary, and section-to-IR mapping. It describes
what an author can write, what each construct means, and what the agent will
see at the other end.

Maintainer-facing material (Lower-pass hoisting mechanics, the full effect
inference algorithm, role-computation evidence ordering) lives in
[[ir-semantics]]. The IR data shape and JSON encoding
live in [[ir-schema]] and [[ir-json]].

## 1. IR Roles

The MVP instruction role set is **closed** to five roles:

| Role | Meaning |
|------|---------|
| `InputContract` | What must be provided at invocation time, or what an input must mean for the unit to be valid. Defines the caller/callee boundary — differs from `Constraint` (which governs behavior, not inputs). |
| `Step` | An ordered action in the workflow. Inside `flow:`, bare calls default to `Step`. A step may carry effect annotations, but effects are not roles. |
| `Constraint` | A behavioral rule governing how work is performed. Positive rules, prohibitions, and their soft/hard variants are all constraints with different strength/polarity attributes — they do not become separate roles. |
| `OutputContract` | What the final result, return value, or report should contain or satisfy. Describes the result boundary, not a workflow action (`Step`) or a process rule (`Constraint`). |
| `Context` | Non-normative informational framing. Background the agent should understand while executing, without directing action or bounding behavior. |

`Context` carries no strength or polarity attributes (unlike `Constraint`).
It is purely informational — it frames the agent's understanding without
imposing obligations or prohibitions.

Activation/routing rules, preconditions, failure policies, and effects are
**not** MVP instruction roles. They are either separate IR structures or
deferred design areas (see §3 for effects).

### Why This Set

- **Input-first, not output-first.** Roles classify author intent. Markdown
  sections are target-specific projections and should not determine the
  semantic taxonomy.
- **One `Constraint` role.** Strength (`soft`/`hard`) and polarity
  (`require`/`avoid`) are attributes, not separate roles. This keeps the
  taxonomy small while preserving the semantics needed for repair,
  compilation, and visualization.
- **Effects stay separate.** A role answers "what kind of intent is this
  instruction?" An effect answers "what external capability or side effect
  does this unit perform?" Conflating them would force a call like
  `inspect_repo(scope)` to be both `Step` and `Effect`. Effects remain
  annotations on skills, blocks, calls, and steps.

See [[0021-closed-five-role-ir|ADR 0021 — Closed Five-Role IR]]
for the design rationale.

### Non-Roles (Deferred)

- **Activation** — when a skill should be selected. Routing metadata, not
  execution intent.
- **Preconditions** — related to input contracts but may eventually deserve
  their own construct. For MVP, invocation requirements belong under
  `InputContract`.
- **Failure policy** — what to do when assumptions fail. Deferred; simple
  conditional behavior uses constraints or workflow structure.

## 2. Constraints

### Strength And Polarity

Every `Constraint` IR node carries two structured attributes:

```text
Constraint {
  strength: soft | hard
  polarity: require | avoid
}
```

**Strength** (selects the locked rendering template; target agent
compliance is not enforced at runtime):

- `soft` — should be followed; default strength.
- `hard` — must always be followed; strongest contract.

The `(strength, polarity)` tuple selects exactly one of four locked
rendering templates. The canonical templates and the canonical-form rules
for the body text live in [[design/compiled-output]] §Constraint Rendering and
[[GLYPH_LANGUAGE_GUIDE]] §7.2.

**Polarity:**

- `require` — positive obligation: do this.
- `avoid` — negative obligation: do not do this.

Three source keywords compose into four forms.

### Source Marker Table

| Source marker | IR mapping |
|---------------|------------|
| `require` | `Constraint(strength: soft, polarity: require)` |
| `avoid` | `Constraint(strength: soft, polarity: avoid)` |
| `must` | `Constraint(strength: hard, polarity: require)` |
| `must avoid` | `Constraint(strength: hard, polarity: avoid)` |

`must` is a strength modifier — standalone `must X` is shorthand for
`must require X`. `avoid` flips polarity. Three keywords, four forms. See
[[0019-four-form-constraint-model|ADR 0019 — Four-Form Constraint Model]]
for the rationale.

Other source markers:

| Marker | IR mapping |
|--------|------------|
| `flow` | contains `Step` nodes |
| `context` | contains `Context` nodes |

`input` and `output` markers are deferred from MVP alongside the
`inputs:` / `outputs:` sub-sections. Header parameters cover input
definition; `return` covers output.

### Marker-Plus-Concept Form

Two authoring styles are both valid:

- **Marker-plus-concept:** `avoid unrelated_edits` — the marker keyword
  carries polarity, the concept name resolves to a polarity-neutral
  definition.
- **Compound name:** `avoid_unrelated_edits` — the name is a single
  identifier whose definition carries the full semantics (including
  polarity).

There is no forced normalization from one form to the other. If a compound
name resolves to a declaration, the compiler uses it as-is and infers role,
strength, and polarity from the declaration's text content, with the name
prefix (`avoid_*`, `must_*`) as supporting evidence. If a compound name is
unresolved, repair generates a definition under the full compound name with
full semantics baked in — no splitting.

### Source Order: Free Mixing

Inside a `constraints:` section, soft (`require`/`avoid`) and hard
(`must`/`must avoid`) markers may appear in any order. The compiled output
orders constraints independently of source order — strength and polarity
affect wording, not placement (see [[design/compiled-output]]). Authors should
group constraints by topic, not by strength.

### Constraint And Context Markers Outside `constraints:` / `context:`

Authors may write constraint markers (`require`/`avoid`/`must`/`must avoid`)
and `context` markers directly at body level, without a `constraints:` or
`context:` wrapper:

```glyph
skill fix_bug(scope = ".")
    require preserve_existing_patterns
    avoid unrelated_edits
    context project_conventions
    flow:
        ...
```

These markers are also legal as flow statements inside `flow:`, including
inside `if`/`elif`/`else` branch bodies.

**Where they end up in compiled output:**

- A constraint marker at the top of `flow:` (not inside a branch) renders
  in `## Constraints` like any other top-level constraint.
- A constraint marker **inside an `if`/`elif`/`else` branch body** stays
  scoped to that branch. The consuming LLM sees that the constraint
  applies only when the branch is taken (e.g. "If the change touches public
  APIs, do not break backwards compatibility."). It does not appear in
  `## Constraints`.
- `context` markers follow the same rule: top-level renders in
  `## Context`; branch-scoped renders inline in the conditional Step
  prose.

`glyph fmt` preserves source order and marker position — it does not
rewrite body-level or flow-level markers into a `constraints:` / `context:`
sub-section. Both forms produce identical IR.

The internal mechanics that produce this result (Lower-pass hoisting,
deduplication, branch-scoping) are documented in
[[ir-semantics]].

## 3. Effects

> **Gated: `--enable-effects` (default: off).** The entire effects subsystem
> — parsing, inference, validation, repair auto-fill, and output emission —
> is disabled unless the `--enable-effects` flag is passed. When the flag is
> off the parser rejects any `effects:` sub-section. All design in this
> section remains the intended target; the gate is temporary.

Effects answer "what external capability or side effect does this unit
perform?" — they sit alongside roles, not inside them. See
[[0020-fixed-effect-keyword-vocabulary|ADR 0020 — Fixed Effect Keyword Vocabulary]]
for the design rationale.

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
| `spawns_agent` | Spawns a subagent to perform delegated work (see [[stdlib]]). |

### Syntax

The `effects:` clause may appear on `skill`, `block`, and `export block`
declarations. Two forms; both compile identically:

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

- Omitting `effects:` entirely means "the compiler should infer effects
  from the call graph." If the inferred set is non-empty, the compiler
  auto-adds an `effects:` sub-section during repair and surfaces a warning
  so the author knows what was added. If the inferred set is empty, no
  `effects:` line is added (the declaration genuinely has no effects).
- Writing `effects: none` explicitly is an **author assertion** that the
  declaration has no side effects. If the call graph contradicts this
  (inferred set is non-empty), the compiler emits a hard error — the author
  made a deliberate claim that turned out to be wrong. This is not
  repairable.
- `none` must not appear alongside other keywords. `effects: none, reads_files`
  is a compile error.

### Propagation

The compiler infers effects by walking the call graph using a
**transitive-eager, single-compilation-unit** algorithm. Authors only need
to know three propagation rules to predict the inferred set; the rules
apply unconditionally to every reachable callee — including calls inside
`if`/`elif`/`else` branch arms and calls modified by `with`.

- **Stdlib-direct.** A call to a standard-library entry contributes that
  entry's documented effects (see [[stdlib]]).
- **Local-transitive.** A call to a same-file `block` contributes the
  callee's inferred effect set, computed transitively through that
  callee's own call graph. Same-file `export block` calls follow this rule
  too — locally we have full visibility.
- **Import-by-declaration.** A call to a callee imported from another file
  contributes the imported `export block`'s **declared** effect set (the
  import contract). The importer trusts the dependency's declaration; it
  never re-derives the imported callee's inferred set.

A block's inferred effect set is the **union** of its own direct effects
and the contributions from every reachable call. There is no effect
subtraction or masking in the MVP. Effect sets are unordered.

### Effect Boundaries At Subagent Spawns

When a skill calls `subagent(task)`, it calls a stdlib entry whose declared
effect is `{ spawns_agent }`. That single keyword propagates to the caller.
The *spawned skill* is **never** a callee in the caller's call graph: it
is a runtime artifact selected and executed by the consuming agent,
analogous to a subprocess. Its own effect declarations are validated
independently when *that* skill is compiled.

Concretely: if skill A spawns a subagent that runs skill B, and skill B
declares `effects: writes_files, uses_network`, skill A's inferred effect
set does **not** include `writes_files` or `uses_network`. Skill A declares
`spawns_agent` and that is the full contract. The two skills are
independent compilation units with independent effect validation.

This is consistent with the design posture that `spawns_agent` is a
self-contained declaration meaning "this skill triggers another execution
context" (see [[stdlib]] §The `spawns_agent` Effect).

### Author Declaration And Validation

**Infer-when-omitted, validate-when-declared.** The compiler always
computes the inferred effect set. How it uses that set depends on whether
the author wrote an `effects:` line:

- **Omitted entirely.** Deterministic repair auto-adds an `effects:`
  sub-section with the inferred set and surfaces a warning. If the
  inferred set is empty, nothing is added. This applies uniformly to
  skills, blocks, and export blocks.
- **Declared by the author.** The declared set must be a superset of the
  inferred set. If declared is smaller (including writing `effects: none`
  when something is inferred), that is a hard error — the declaration is
  lying about what the block does. Not repairable: the author made a
  deliberate claim and the compiler will not silently overwrite it.

If the declared set is **larger** than inferred (e.g. `effects: reads_files,
runs_commands` when only `reads_files` is inferred), the compiler emits a
warning. Compilation proceeds. Over-declaration is legitimate for
forward-compat or intentional widening of a public contract; the warning
lets the author remove the extra keyword if they are confident it is no
longer needed. Repair never narrows a declared effect set, since that
would silently break import contracts.

The full validation algorithm (Phase numbering, exact diagnostic IDs,
cross-import enforcement details) lives in
[[ir-semantics]].

### Effects Are Not Instruction Roles

A role classifies author intent. An effect classifies capabilities or side
effects. A step in a flow is `Step` with effect annotations — it is not
simultaneously an `Effect` role. Effects remain separate annotations on
skills, blocks, calls, and steps.

### Extension Policy

- New keywords may be added (e.g., `reads_database`, `sends_messages`).
- Existing keywords are never renamed or removed once stabilized.
- Old skills are unaffected; their import contracts remain valid.
- No namespacing in MVP. If the flat namespace becomes crowded, namespacing
  may be added as a backwards-compatible extension.
- New effects follow the `verb_noun` snake_case convention.

### Deferred

- **Per-call effect annotations.** Authors cannot attach an `effects:`
  clause to an individual call site in MVP. Effects are declared only at
  the declaration level (`skill`, `block`, `export block`). Adding this
  later is backwards-compatible.

## 4. Section Vocabulary

### The Five MVP Sub-Section Headers

Five colon-terminated headers are available inside `skill`, `block`, and
`export block` bodies:

| Section | Spelling | Content |
|---------|----------|---------|
| `description:` | singular | One-line summary of when/why to use this skill; compiles to frontmatter `description`. Body is a single quoted string literal (`"..."` or `"""..."""`) or a bare-name reference to a `const` / `export const` declaration |
| `effects:` | plural | Effect keywords (see section 3); compiles to frontmatter `effects` |
| `context:` | singular | Background information the agent should understand while executing. Body contains bare-name references to `const`/`export const` declarations, inline string literals, or `context`-prefixed markers |
| `constraints:` | plural | Constraint markers: `require`, `avoid`, `must` + concept |
| `flow:` | singular | Ordered steps: calls, bindings, `return`, `if`, bare names, inline strings |

`inputs:`, `outputs:`, and `when_to_use:` are deferred from MVP
([[todo]]). Header parameters cover input definition; `return`
covers output; `description:` covers routing.

**Spelling convention:** all headers use snake_case. Plural for set-like
sections. Singular for value and workflow containers.

### `description:` Section

`description:` provides a concise, one-line summary of when and why a
skill should be used. It compiles to the `description` field in YAML
frontmatter (see [[design/compiled-output]]), which is the primary trigger for
coding agents that select skills.

**Body grammar.** The body is **exactly one quoted string literal** —
either an inline `"..."` or a block `"""..."""` — or a **bare name** that
resolves to a same-file `const` / `export const` declaration. Concatenation,
multiple literals, and arbitrary expressions are forbidden (consistent
with the no-string-concatenation foundation in [[foundations]]). For long
descriptions, extract to a `const` declaration and reference it by name.
Both the short form (content on the same line) and the long form
(keyword alone, indented body below) are accepted, per the generic
sub-section rule in [[language-surface]] §2.5.

**Parameter slots.** `{name}` parameter references inside the description
body are **illegal**. The compiled frontmatter `description` is a literal
string, not an instruction with runtime substitutions.

**Singular.** Exactly one description per skill. A second `description:`
sub-section in the same body is automatically merged (textually
concatenated) during repair.

**Availability.** `description:` is available on `skill`, `block`, and
`export block` declarations. It remains N/A for value-binding declarations
(`const` and its `export`/`generated` variants). On a `skill`, the
description compiles to frontmatter and is read by the outer agent that
picks the skill. On a `block` or `export block`, the description is the
natural-language predicate consulted by `BLOCKNAME.applies()` (see §Block
Trigger Predicate below); it does not surface in frontmatter.

```glyph
skill fix_bug(scope = ".")
    description: "Debug and fix a bug in the codebase with minimal, targeted changes."

    flow:
        ...
```

If `description:` is omitted on a `skill`, the compiler generates one from
the skill name and body during repair, adding it as a `description:`
sub-section in the source. Authors should prefer explicit descriptions for
predictable skill routing.

On a `block` / `export block`, `description:` is **optional**. It is
required only when the block is referenced via `BLOCKNAME.applies()`
somewhere in the build. See §Predicates §Block Trigger Predicate for
required-when-consulted semantics.

### `context:` Section

`context:` provides background information the agent should understand
during execution — factual framing, domain knowledge, environmental
assumptions, or other non-normative content that neither directs action
nor bounds behavior.

**Compilation target.** `context:` compiles to a peer-level
`## Context` heading in compiled output, before `## Steps` (see
[[design/compiled-output]]).

**Body grammar.** The body contains **bare-name references** to same-file
`const` / `export const` declarations, **inline quoted strings**
(`"..."` or `"""..."""`), or **`context`-prefixed markers** that resolve
to declarations. Multiple entries are permitted (unlike `description:`,
which is singular). Both the short form (content on the same line) and
the long form (keyword alone, indented body below) are accepted.

**Parameter slots.** `{name}` parameter references inside `context:` body
content are **allowed**. The compiler substitutes parameter values into
context prose during compilation, matching the treatment of `flow:`
strings — context bodies remain informational framing but may carry
parameter-aware copy.

**Availability.** `context:` is available on `skill`, `block`, and
`export block` declarations. It remains N/A for value-binding declarations
(`const` and its `export`/`generated` variants).

**Optional on all declaration kinds.** `context:` is never required.
Omitting it simply means the compiled output has no `## Context` section.

```glyph
skill fix_bug(scope = ".")
    description: "Debug and fix a bug in the codebase with minimal, targeted changes."
    context:
        project_conventions
        "This codebase uses a monorepo layout with per-crate Cargo.toml files."
    flow:
        ...
```

### Predicates

A **predicate** is a natural-language string that the consuming coding
agent evaluates against current context to decide whether a branch arm
applies. Three syntactic forms produce predicates in an `if` / `elif`
condition:

| Form | Example | Resolved value |
|---|---|---|
| Block trigger predicate | `fork_with_plan.applies()` | block's `description:` string |
| String-const predicate | `complex_change_required` | const's string body |
| Inline literal predicate | `"the user has explicitly opted out of compile-on-save"` | the literal itself |

All three forms are semantically equivalent from the agent's perspective —
the agent reads the resolved string and decides. They differ only in
where the string lives in source. Authors choose the form that reads most
clearly at the call site.

Predicates compose with `and`, `or`, `not`, and parenthesization the same
way boolean conditions do. A Branch is **pure-predicate** when every
arm's condition is one or more predicate-form tokens combined by `or`
only. Pure-predicate Branches use the deterministic "decide which applies"
framing (see [[design/compiled-output]] §Predicate-Driven Branch Projection).
Mixed conditions — predicates combined with boolean tokens via `and` or
`not` — go through a separate prose-generation path.

Predicates are only valid in `if` / `elif` condition position. They are
not value expressions and cannot bind to a variable, appear in `return`,
or appear as call arguments.

#### Effects

Predicate evaluation (any form) contributes no effects to the enclosing
declaration. Block declared effects propagate only via `Call` nodes when
the block is actually invoked inside an arm body.

#### Block Trigger Predicate (`.applies()`)

`BLOCKNAME.applies()` evaluates to a predicate by reading the referenced
block's `description:` string. The receiving agent matches this
description against current context.

**Surface form.** The receiver must be a same-file `block` /
`export block` name, an imported `export block` name, or a single-level
qualified callee (`module_alias.block_name`). The method name `applies`
and the empty argument list are fixed: `applies(arg)` is a parse error;
omitting the parens is a parse error. `applies` is reserved in this
method-call position and is not a UFCS dispatch.

**Required-when-consulted.** A block referenced by `.applies()` must
declare `description:`. Resolution behavior:

- **Local block** (declared in the same file as the `.applies()` call)
  without `description:` → repairable. Repair generates a description from
  the block's name, parameters, effects, and flow body, focused on *when
  this block applies*, and adds it as a `description:` sub-section.
- **Imported `export block`** without `description:` → hard error. Repair
  only edits the file under compilation; it does not cross file
  boundaries. The author must add `description:` in the foreign source
  manually.
- **Receiver does not resolve to a block** → error.

**Optionality otherwise.** A block never consulted via `.applies()` may
omit `description:` entirely.

**Metadata, not gate.** A block carrying `description:` remains directly
callable by name without consulting its description. `applies()` is
opt-in at the call site.

**Body grammar.** The body grammar of `description:` on a block is
identical to a skill's: exactly one quoted string literal (`"..."` or
`"""..."""`), or a bare-name reference to a same-file `const` /
`export const` declaration. The same parameter-slot rule and singularity
rule apply.

#### String-Const Predicate

A bare identifier in condition position that resolves to a string-kinded
`const` or `export const` is a string-const predicate. The const's string
body is the predicate. The compiler classifies the condition after name
resolution using the inferred kind of the resolved declaration (see
[[values-and-names]] §Bare-Name Resolution In Condition Position).

An undefined name in condition position is repaired to `generated const`
(not `generated block`) — the same routing as constraint and context
markers. The LLM generates a single-clause predicate string from the
name, surrounding flow context, and the enclosing skill's description.

**No `description:` requirement.** String-const predicates always have a
body (the const RHS); the "required-when-consulted" requirement from
`.applies()` does not apply.

#### Inline Literal Predicate

A quoted string literal in condition position is a self-contained
predicate. The literal text is the predicate string.

**Style guidance.** Inline literals are concise for one-off conditions.
Extract to a named `const` when the predicate is reused, long, or
benefits from a descriptive name.

**Style relief — extract long descriptions.** When a block's
`description:` grows long (e.g., trigger phrases, multi-clause "use
when" guidance), the bare-name reference form is the recommended
pattern: declare a `const` and reference it from `description:`. Block
declarations stay tight; trigger prose lives next to other constants as
data.

### Section Content Rules

**`description:`** — a concise one-line summary. Available on `skill`,
`block`, and `export block` declarations.

- On a `skill`, the description summarizes when and why to use this skill,
  and compiles to frontmatter `description`. If omitted, repair generates
  one from the skill name and body and adds it to the source.
- On a `block` or `export block`, the description names the user-intent
  or runtime condition under which the block applies. It is consulted
  only by the trigger predicate `BLOCKNAME.applies()` (see §Predicates
  §Block Trigger Predicate); it does **not** appear in compiled output
  otherwise. **Required when `BLOCKNAME.applies()` is called somewhere
  reachable**; otherwise optional and treated as documentation only.

**`effects:`** — declared effect keywords (see section 3). Compiles to
frontmatter `effects` as a YAML list. Validated against the inferred
effect set.

**`context:`** — background information the agent should understand.
Available on `skill`, `block`, and `export block`. Compiles to a
peer-level `## Context` heading, before `## Steps`. Informational
framing only — no strength, polarity, or behavioral directives.

**`constraints:`** — constraint markers using the three keywords
(`require`, `avoid`, `must`) in four composed forms. Projects to a
peer-level `## Constraints` heading.

**`flow:`** — the ordered workflow section. All content defaults to the
`Step` IR role unless explicit syntax or resolved metadata says
otherwise. The only section that contains ordered, sequential content.
Projects to a peer-level `## Steps` heading; `return` folds into the
final Step. A bare string appearing as a block body shorthand (omitting
`flow:`, per [[language-surface]] §3.2) is treated identically and
resolves to `Step` — the `flow:` header is not required for the
compiler to assign the instruction role.

### Mandatory / Optional Per Declaration Kind

| Section | `skill` | `block` | `export block` |
|---------|---------|---------|----------------|
| `description:` | Optional (generated if omitted) | Optional (Required when consulted via `.applies()`) | Optional (Required when consulted via `.applies()`) |
| `effects:` | Optional | Optional | Optional (validated against inference) |
| `context:` | Optional | Optional | Optional |
| `constraints:` | Optional | Optional | Optional |
| `flow:` | Required (unless instruction-only) | Optional | Expected (needs explicit `return`) |

A `skill` body must contain at least `constraints:` or `flow:` (or both).
An empty skill body is a compile error. An `export block` must have an
explicit `return` path, which in practice means it will have `flow:`.

### Recommended Source Order

Source order is free — the compiler reorders to the fixed compiled-output
order. Recommended convention:

1. `description:` (if used)
2. `effects:`
3. `context:`
4. `constraints:`
5. `flow:`

The compiler's source normalization pass enforces this order when
rewriting.

### Freeform Sections (Phase 3)

Phase 3 extends the section vocabulary beyond the five built-in
sub-section headers above by allowing **freeform colon-keyword sections**
— authors write `quality:`, `risks:`, `acceptance_criteria:`, etc. and the
section name flows into compiled output as a peer-level `## Heading`
block ([[design/compiled-output]] §Freeform Sections). The closed-role-set rule
in §1 is unchanged: freeform sections are an orthogonal authoring
channel and do not introduce new IR roles.

**What an author can write inside a freeform section.** A freeform
section body accepts the same content kinds that show up elsewhere:
inline string literals, bare-name references to a string-valued `const` /
`export const`, and the five reserved markers (`require`, `avoid`,
`must`, `must avoid`, `context`) followed by an operand.

**Marker semantics.** Inside a freeform section, the reserved marker
words keep their lexical identity — `require`/`avoid`/`must`/`must avoid`
project through the same four-form constraint template as
`## Constraints`, and `context` projects through the bullet-with-bold-label
form. But unlike markers inside `constraints:` / `context:`, freeform-section
markers do **not** hoist into the enclosing declaration's `constraints` /
`context` lists; they stay scoped to their section so the emitter renders
the section as authored under its own `## Heading`.

The IR plumbing for freeform sections (node kinds, host lists, lower /
emit details) lives in [[ir-semantics]] §4 and
[[ir-schema]] §Freeform sections.

## Open Questions

(None remaining — `context:` has been promoted to a full MVP section
header.)
