# Glyph IR Roles, Constraints, Effects, and Section Vocabulary

This document is the single authoritative source for Glyph's MVP IR structure, constraint model, effect vocabulary, and section-to-IR mapping.

## 1. IR Roles

The MVP instruction role set is **closed** to five roles:

| Role | Meaning |
|------|---------|
| `InputContract` | What must be provided at invocation time, or what an input must mean for the unit to be valid. Defines the caller/callee boundary — differs from `Constraint` (which governs behavior, not inputs). |
| `Step` | An ordered action in the workflow. Inside `flow:`, bare calls default to `Step`. A step may carry effect annotations, but effects are not roles. |
| `Constraint` | A behavioral rule governing how work is performed. Positive rules, prohibitions, and their soft/hard variants are all constraints with different strength/polarity attributes — they do not become separate roles. |
| `OutputContract` | What the final result, return value, or report should contain or satisfy. Describes the result boundary, not a workflow action (`Step`) or a process rule (`Constraint`). |
| `Context` | Non-normative informational framing. Background the agent should understand while executing, without directing action or bounding behavior. |

`Context` carries no strength or polarity attributes (unlike `Constraint`). It is purely informational — it frames the agent's understanding without imposing obligations or prohibitions.

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
- `Context` → bulleted items under `### Context`, before `### Steps`. Passive informational text — no strength/polarity wording.
- `Constraint` → bulleted items under `### Constraints`. Strength (`soft`/`hard`) and polarity (`require`/`avoid`) influence wording and prominence.
- `InputContract` → projected into the `## Parameters` section of compiled output (names, descriptions, and either a default value or a `(required)` marker per parameter; see `compiled-output.md` §`## Parameters`). Parameters appear as `{param}` references in Step and Constraint prose, resolved by the consuming LLM at runtime.
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
| `context` | contains `Context` nodes |

`input` and `output` markers are deferred from MVP alongside the `inputs:` / `outputs:` sub-sections. Header parameters cover input definition; `return` covers output.

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
skill fix_bug(scope = ".")
    require preserve_existing_patterns
    avoid unrelated_edits
    flow:
        ...
```

The compiler normalizes body-level markers into a `constraints:` section via two complementary mechanisms: (1) `glyph fmt` (Phase 3a) performs a source-to-source rewrite, moving them into a `constraints:` sub-section for source clarity; (2) Phase 4 (Lower) hoists any remaining body-level constraint AST nodes into the declaration's `constraints` list at IR level, ensuring correct compilation regardless of whether `glyph fmt` was run. Both forms produce identical IR. The canonical source form always uses the `constraints:` section.

#### Flow-Level Constraint Markers

Constraint markers (`require`/`avoid`/`must`/`must avoid`) are also legal as flow statements inside `flow:`, including inside `if`/`elif`/`else` branch bodies. The IR represents them as `Constraint` nodes admissible in the `FlowNode` union (`ir-schema.md` §Flow Nodes). `Context` markers (via the `context` keyword) are similarly admissible as flow nodes, following the same hoisting/branch-scoping rules (see §Body-Level and Flow-Level Context Markers). Lower (Phase 4) splits them by location:

- **Flow top-level** — a constraint marker at the top level of `flow:` (not inside a branch) is **hoisted** out of the flow and appended to the enclosing declaration's `constraints` list, deduplicated against existing entries by canonical text + polarity + strength. Two complementary mechanisms handle this: `glyph fmt` (Phase 3a) performs a source-to-source rewrite, moving flow-top-level constraints into the `constraints:` section for source clarity; Phase 4 (Lower) hoists any remaining flow-top-level `Constraint` nodes at IR level. After hoisting it renders in `### Constraints` like any other top-level constraint.
- **Branch-scoped** — a constraint marker inside an `if`/`elif`/`else` branch body **stays inline** in that branch. Expand renders it as part of the conditional Step prose so the consuming LLM sees that the constraint applies only when that branch is taken (e.g., "If the change touches public APIs, do not break backwards compatibility."). It does not appear in `### Constraints`. See `compiled-output.md` §Constraint Rendering.

By the time Lower completes, all unconditional constraints — whether originally in a `constraints:` section, at body level, or at flow top-level — reside in the declaration's `constraints` list. If `glyph fmt` ran first, the source already reflects this; if not, Lower's IR-level hoisting produces the same result. Branch-scoped markers are the only constraints that remain inside the flow.

### Body-Level and Flow-Level Context Markers

Authors may write `context` markers directly at body level without a `context:` wrapper:

```glyph
skill fix_bug(scope = ".")
    context project_conventions
    context "This codebase uses a monorepo layout."
    flow:
        ...
```

The compiler normalizes body-level `context` markers into the `context:` section via two complementary mechanisms: (1) `glyph fmt` (Phase 3a) performs a source-to-source rewrite, moving them into a `context:` sub-section for source clarity; (2) Phase 4 (Lower) hoists any remaining body-level `context` AST nodes into the declaration's `context` list at IR level, ensuring correct compilation regardless of whether `glyph fmt` was run. Both forms produce identical IR. The canonical source form always uses the `context:` section.

#### Flow-Level Context Markers

`context` markers are also legal as flow statements inside `flow:`, including inside `if`/`elif`/`else` branch bodies. The IR represents them as `Context` nodes admissible in the `FlowNode` union (alongside `Constraint` nodes — see `ir-schema.md` §Flow Nodes). Lower (Phase 4) splits them by location:

- **Flow top-level** — a `context` marker at the top level of `flow:` (not inside a branch) is **hoisted** out of the flow and appended to the enclosing declaration's `context` list, deduplicated against existing entries by canonical text. Two complementary mechanisms handle this: `glyph fmt` (Phase 3a) performs a source-to-source rewrite, moving flow-top-level context markers into the `context:` section for source clarity; Phase 4 (Lower) hoists any remaining flow-top-level `Context` nodes at IR level. After hoisting it renders in `### Context` like any other top-level context entry.
- **Branch-scoped** — a `context` marker inside an `if`/`elif`/`else` branch body **stays inline** in that branch. Expand renders it as part of the conditional Step prose so the consuming LLM sees that the context applies only when that branch is taken. It does not appear in `### Context`.

By the time Lower completes, all unconditional context — whether originally in a `context:` section, at body level, or at flow top-level — resides in the declaration's `context` list. If `glyph fmt` ran first, the source already reflects this; if not, Lower's IR-level hoisting produces the same result. Branch-scoped markers are the only context entries that remain inside the flow.

### Inference And Repair

Authors should be able to write terse source. The compiler infers role, strength, and polarity where possible, and the repair pass materializes the minimal explicit marker back into source when confidence is high.

**Note:** Some forms have fixed roles assigned by grammar — no inference step is involved. Bare calls inside `flow:` are definitionally `Step`. A bare string as a block body shorthand (omitting `flow:`, per `language-surface.md` §3.2) is definitionally `Step`. Content inside `description:` carries no instruction role — it is context metadata. The evidence table below applies only to forms that are genuinely ambiguous.

Evidence order:

1. Explicit marker in source.
2. Metadata from same-file `const` or block declarations.
3. Metadata from imported or standard-library declarations.
4. Position and structure (e.g., inside `flow:` or as a bare block-body string using the single-string shorthand implies `Step`; inside `description:` is context metadata and carries no instruction role).
5. Compound-name cues (`avoid_*`, `must_*`, `never_*`, `must_never_*`) — used as evidence for role/polarity inference; no forced splitting.
6. LLM repair-generated definitions.
7. Diagnostic if role, strength, or polarity remains ambiguous.

`require` and `avoid` may be inferred during repair when evidence is clear. `must` should be inferred conservatively — only when the source already carries hard-strength intent (trusted metadata, strong wording like `must_*`, `never_*`, `must_never_*`). A plain `avoid_*` cue repairs to soft avoidance, not hard. `must` should stay rare; it is not just a more emphatic `require`.

## 3. Effects

> **Gated: `--enable-effects` (default: off).** The entire effects subsystem — parsing, inference, validation, repair auto-fill, and output emission — is disabled unless the `--enable-effects` flag is passed. When the flag is off the parser rejects any `effects:` sub-section with `G::parse::effects-disabled` (error). All design in this section remains the intended target; the gate is temporary until effect inference handles skills without a call graph (see `todo.md`).

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

- Omitting `effects:` entirely means "the compiler should infer effects from the call graph." If the inferred set is non-empty, the compiler auto-adds an `effects:` sub-section during Phase 3a (deterministic repair) and emits a warning-level notification (`G::repair::inferred-effects`). If the inferred set is empty, no `effects:` line is added (the declaration genuinely has no effects).
- Writing `effects: none` explicitly is an **author assertion** that the declaration has no side effects. If the call graph contradicts this (inferred set is non-empty), the compiler emits `G::analyze::effects-under-declared` (error). This is not repairable — the author made a deliberate claim that turned out to be wrong.
- `none` must not appear alongside other keywords. `effects: none, reads_files` is a compile error.

### Propagation

The compiler infers effects by walking the call graph using a **transitive-eager, single-compilation-unit** algorithm. Three propagation rules cover every call, applied unconditionally to every reachable callee — including calls inside `if`/`elif`/`else` branch arms and calls modified by `with`. There is no per-arm reachability analysis; every reachable call contributes.

- **Stdlib-direct.** A call to a standard-library entry contributes that entry's documented effects (see `stdlib.md`).
- **Local-transitive.** A call to a same-file `block` contributes the callee's **inferred** effect set, computed transitively through that callee's own call graph. Same-file `export block` calls follow this rule too — locally we have full visibility.
- **Import-by-declaration.** A call to a callee imported from another file contributes the imported `export block`'s **declared** effect set (the import contract). The importer never re-derives the imported callee's inferred set; it trusts the dependency's declaration as validated by that file's own Validate pass (per `pipeline.md` §Multi-File Compilation Order and `data-flow.md` §Effect Propagation).

A block's inferred effect set is the **union** of its own direct effects and the contributions from every reachable call. Skills, exported blocks, and private blocks all participate in inference. There is no effect subtraction or masking in the MVP. Effect sets are unordered; the compiler may sort them alphabetically or by declaration order.

### Effect Boundaries At Subagent Spawns

The three propagation rules above already produce the correct effect set for skills that spawn subagents — no fourth rule is needed. When a skill calls `subagent(task)`, it calls a stdlib entry whose declared effect is `{ spawns_agent }`. That single keyword propagates to the caller via the Stdlib-direct rule. The *spawned skill* is never a callee in the caller's call graph: it is a runtime artifact selected and executed by the consuming agent, analogous to a subprocess. Its own effect declarations are validated independently when *that* skill is compiled.

Concretely: if skill A spawns a subagent that runs skill B, and skill B declares `effects: writes_files, uses_network`, skill A's inferred effect set does **not** include `writes_files` or `uses_network`. Skill A declares `spawns_agent` and that is the full contract. Skill B's effect surface is validated by skill B's own compilation — the two are independent compilation units with independent effect validation.

This is consistent with the design posture that `spawns_agent` is a self-contained declaration meaning "this skill triggers another execution context" (see `stdlib.md` §The `spawns_agent` Effect). The spawned skill's effects are opaque to the caller for the same reason an imported library's internal private-block effects are opaque to the importer: each compilation unit validates its own contract.

### Projection Tier And Effect Propagation

Projection tier (inline, same-file procedure, external file — see `compiled-output.md` §Three-Tier Block Projection) is a Phase 6 output-layout decision. Effect propagation is resolved in Phases 2 and 5, before projection tiers are assigned. Therefore, **projection tier does not affect effect semantics**: a callee's effect set propagates identically regardless of which tier the compiler later selects for compiled output.

The one addendum: when the compiler selects Tier 3 (external file) for an imported block in Phase 6 Step 1, the compiled output directs the consuming agent to load an external file — a runtime `reads_files` action. If the skill's effect set (resolved in Phases 2/5) does not already include `reads_files`, Phase 6 Step 1 emits an error requiring the author to add it. This is a **post-Phase-5 validation check** specific to Tier 3 selection, not a propagation-time contribution. In practice, most skills that call imported blocks already carry `reads_files` from the callee's own declared effects; the check catches the rare case where the callee has no file-reading effects but the tier selection introduces one. See `compiled-output.md` §External Procedure Files.

### Author Declaration And Validation

**Infer-when-omitted, validate-when-declared.** The compiler computes an **inferred** effect set by walking the call graph and unioning every callee's effect contribution (user-defined blocks per their declared `effects:`, and stdlib calls per their synthetic-body projection — see `stdlib.md` §Projection Model: Uniform Synthetic Body and §Propagation). How the compiler uses that inferred set depends on whether the author wrote an `effects:` line:

- **Omitted entirely.** The author did not write `effects:` at all. Phase 2 (Analyze) emits `G::analyze::missing-effects` (repairable). Phase 3a (deterministic repair) auto-adds an `effects:` sub-section with the inferred set and emits `G::repair::inferred-effects` (warning, informational) so the author knows what was added. This applies uniformly to skills, blocks, and export blocks — there is no declaration-type asymmetry. If the inferred set is empty, no `effects:` line is added and no diagnostic fires (the declaration genuinely has no effects).
- **Declared by the author.** The compiler validates that the **declared set is a superset of the inferred set**. If the declared set is smaller than inferred, that is a compile error (`G::analyze::effects-under-declared`) — the declaration is lying about what the block does. Writing `effects: none` explicitly when the inferred set is non-empty is the same error: `none` is a strict subset claim that contradicts inference. This is not repairable — the author made a deliberate declaration and the compiler will not silently overwrite it.

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

### The Five MVP Sub-Section Headers

Five colon-terminated headers are available inside `skill`, `block`, and `export block` bodies:

| Section | Spelling | Content |
|---------|----------|---------|
| `description:` | singular | One-line summary of when/why to use this skill; compiles to frontmatter `description`. Body is a single quoted string literal (`"..."` or `"""..."""`) or a bare-name reference to a `const` / `export const` declaration |
| `effects:` | plural | Effect keywords (see section 3); compiles to frontmatter `effects` |
| `context:` | singular | Background information the agent should understand while executing. Body contains bare-name references to `const`/`export const` declarations, inline string literals, or `context`-prefixed markers |
| `constraints:` | plural | Constraint markers: `require`, `avoid`, `must` + concept |
| `flow:` | singular | Ordered steps: calls, bindings, `return`, `if`, bare names, inline strings |

`inputs:`, `outputs:`, and `when_to_use:` are deferred from MVP ([todo.md](todo.md)). Header parameters cover input definition; `return` covers output; `description:` covers routing.

**Spelling convention:** all headers use snake_case. Plural for set-like sections. Singular for value and workflow containers.

### `description:` Section

`description:` provides a concise, one-line summary of when and why a skill should be used. It compiles to the `description` field in YAML frontmatter (see `compiled-output.md`), which is the primary trigger for coding agents that select skills.

**Body grammar.** The body is **exactly one quoted string literal** — either an inline `"..."` or a block `"""..."""` — or a **bare name** that resolves to a same-file `const` / `export const` declaration. Concatenation, multiple literals, and arbitrary expressions are forbidden (consistent with the no-string-concatenation foundation in `foundations.md`). For long descriptions, extract to a `const` declaration and reference it by name. Both the short form (content on the same line) and the long form (keyword alone, indented body below) are accepted, per the generic sub-section rule in `language-surface.md` §2.5.

**Parameter slots.** `{name}` parameter references inside the description body are **illegal** and emit `G::parse::param-slot-in-non-instruction-string` (see `values-and-names.md` §No Interpolation). The compiled frontmatter `description` is a literal string, not an instruction with runtime substitutions.

**Singular.** `description:` is set-like neither in source nor in IR — exactly one description per skill. A second `description:` sub-section in the same `skill` body emits `G::parse::duplicate-subsection`, classified **repairable**: Phase 3a's deterministic merge concatenates the duplicate body into the first occurrence (purely textual, no LLM, no contradiction-check — see `repair.md` §4.11). After Phase 3a re-emits the source, only one `description:` is present and the parser accepts the body.

**Availability.** `description:` is available on `skill`, `block`, and `export block` declarations. It remains N/A for value-binding declarations (`const` and its `export`/`generated` variants). On a `skill`, the description compiles to frontmatter and is read by the outer agent that picks the skill. On a `block` or `export block`, the description is the natural-language predicate consulted by `BLOCKNAME.applies()` (see §Block Trigger Predicate below); it does not surface in frontmatter.

```glyph
skill fix_bug(scope = ".")
    description: "Debug and fix a bug in the codebase with minimal, targeted changes."

    flow:
        ...
```

If `description:` is omitted on a `skill`, the compiler generates one from the skill name and body during the LLM repair pass (Phase 3), adding it as a `description:` sub-section in the source. Authors should prefer explicit descriptions for predictable skill routing.

On a `block` / `export block`, `description:` is **optional**. It is required only when the block is referenced via `BLOCKNAME.applies()` somewhere in the build. See §Block Trigger Predicate for required-when-consulted semantics and the cross-file repair limitation.

### `context:` Section

`context:` provides background information the agent should understand during execution — factual framing, domain knowledge, environmental assumptions, or other non-normative content that neither directs action nor bounds behavior.

**Compilation target.** `context:` compiles to `### Context` under `## Instructions` in compiled output, before `### Steps` (see `compiled-output.md`).

**Body grammar.** The body contains **bare-name references** to same-file `const` / `export const` declarations, **inline quoted strings** (`"..."` or `"""..."""`), or **`context`-prefixed markers** that resolve to declarations. Multiple entries are permitted (unlike `description:`, which is singular). Both the short form (content on the same line) and the long form (keyword alone, indented body below) are accepted, per the generic sub-section rule in `language-surface.md` §2.5.

**Parameter slots.** `{name}` parameter references inside `context:` body content are **illegal** and emit `G::parse::param-slot-in-non-instruction-string` (same restriction as `description:`). Context is informational framing, not parameterized instruction prose.

**Availability.** `context:` is available on `skill`, `block`, and `export block` declarations. It remains N/A for value-binding declarations (`const` and its `export`/`generated` variants).

**Optional on all declaration kinds.** `context:` is never required. Omitting it simply means the compiled output has no `### Context` section.

```glyph
skill fix_bug(scope = ".")
    description: "Debug and fix a bug in the codebase with minimal, targeted changes."
    context:
        project_conventions
        "This codebase uses a monorepo layout with per-crate Cargo.toml files."
    flow:
        ...
```

### Block Trigger Predicate

`BLOCKNAME.applies()` is a special syntactic form for description-driven dispatch inside a flow. It evaluates to a `Bool` that the receiving coding agent computes by matching the referenced block's `description:` against current context.

**Surface form.** The receiver must be a same-file `block` / `export block` name, an imported `export block` name, or a single-level qualified callee (`module_alias.block_name`). The method name `applies` and the empty argument list are fixed: `applies(arg)` is a parse error (`G::parse::applies-with-args`); omitting the parens is a parse error (`G::parse::applies-no-parens`). `applies` is reserved in this method-call position and is not a UFCS dispatch — blocks are not first-class values, and there is no `applies(b: Block)` stdlib function.

**Where it is valid.** Only inside an `if` / `elif` condition (directly, or composed with `and` / `or` / `not` per `data-flow.md` §Condition Expressions). It is not a value expression and cannot bind to a variable, appear in `return`, or appear as a call argument in MVP. Authors who need to reuse a predicate factor it as a separate `if` arm, not a binding.

**Required-when-consulted.** A block referenced by `.applies()` must declare `description:`. Resolution behavior:

- **Local block** (declared in the same file as the `.applies()` call) without `description:` → emits `G::analyze::applies-on-undescribed-block` (repairable). Phase 3 Repair generates a description from the block's name, parameters, effects, and flow body, focused on *when this block applies* (the trigger condition), and adds it as a `description:` sub-section.
- **Imported `export block`** without `description:` → emits `G::analyze::applies-on-undescribed-block` as a hard error. Repair only edits the file under compilation; it does not cross file boundaries (`repair.md` §9, `todo.md` §Repair). The author must add `description:` in the foreign source manually.
- **Receiver does not resolve to a block** (e.g., a skill name, parameter, value binding, or unknown name in method position) → emits `G::analyze::applies-on-non-block` (error).

**Optionality otherwise.** A block never consulted via `.applies()` may omit `description:` entirely. Repair does not generate descriptions speculatively for blocks; the metadata is materialized only when a `.applies()` call requires it.

**Metadata, not gate.** A block carrying `description:` remains directly callable by name without consulting its description. `applies()` is opt-in at the call site; the dispatch decision lives in source where the reader can see it.

**Effects.** A `BLOCKNAME.applies()` evaluation contributes no effects to the enclosing declaration. The referenced block's declared effects propagate only via `Call` nodes (i.e., when the block is actually invoked, typically inside an arm body of the same `if`).

**IR representation.** `Branch.condition` remains a String (`ir-schema.md` §Branch). The literal source `block_x.applies()` survives unchanged into the condition text — no new `Expression` variant is introduced. Phase 4 (Lower) recognizes the form during condition tokenization and validates the shape; Expand Step 1 resolves each `block_x.applies()` invocation by reading `block_x`'s `description:` and populating a side map `applies_descriptions: {block_name: resolved_description}` on the Branch node (`ir-schema.md` §Resolved IR). Expand Step 2 reads both the condition string and the side map to render the conditional Step prose (see `compiled-output.md` §Description-Driven Branch Projection).

**Body grammar inheritance.** The body grammar of `description:` on a block is identical to a skill's: exactly one quoted string literal (`"..."` or `"""..."""`), or a bare-name reference to a same-file `const` / `export const` declaration. The same parameter-slot rule (`G::parse::param-slot-in-non-instruction-string`) and singularity rule (`G::parse::duplicate-subsection`) apply.

**Style relief — extract long descriptions.** When a block's `description:` grows long (e.g., trigger phrases, multi-clause "use when" guidance), the bare-name reference form is the recommended pattern: declare a `const` and reference it from `description:`. Block declarations stay tight; trigger prose lives next to other constants as data. Length-based linter nudges are a post-MVP `glyph fmt` / `glyph check` concern (see `todo.md`).

**Composition with regular booleans.** `applies()` calls compose with the existing condition operators (`and`, `or`, `not`, parenthesization). For a condition that is *purely* one or more `applies()` calls combined by `or` (or a chain of `if`/`elif` arms each guarded by a single `applies()` call), Expand Step 2 emits the "decide which applies" prose form. For mixed conditions (e.g., `block_x.applies() and not is_dry_run`), the description inlines into the larger condition prose: "If the user wants a structured plan and this is not a dry run, ...". See `compiled-output.md` §Description-Driven Branch Projection.

### Section Content Rules

**`description:`** — a concise one-line summary. Available on `skill`, `block`, and `export block` declarations.

- On a `skill`, the description summarizes when and why to use this skill, and compiles to frontmatter `description`. If omitted, Repair (Phase 3) generates one from the skill name and body and adds it to the source.
- On a `block` or `export block`, the description names the user-intent or runtime condition under which the block applies. It is consulted only by the trigger predicate `BLOCKNAME.applies()` (see §Block Trigger Predicate); it does **not** appear in compiled output otherwise. **Required when `BLOCKNAME.applies()` is called somewhere reachable**; otherwise optional and treated as documentation only. When the consulting call site is in the same file as the block, a missing description is repairable (`G::analyze::applies-on-undescribed-block` repairable); when the block is imported, a missing description is an error and must be added in the source library directly.

**`effects:`** — declared effect keywords (see section 3). Compiles to frontmatter `effects` as a YAML list. Validated against the inferred effect set.

**`context:`** — background information the agent should understand. Available on `skill`, `block`, and `export block`. Compiles to `### Context` under `## Instructions`, before `### Steps`. Informational framing only — no strength, polarity, or behavioral directives.

**`constraints:`** — constraint markers using the three keywords (`require`, `avoid`, `must`) in four composed forms. Projects to `### Constraints` under `## Instructions`.

**`flow:`** — the ordered workflow section. All content defaults to the `Step` IR role unless explicit syntax or resolved metadata says otherwise. The only section that contains ordered, sequential content. Projects to `### Steps` under `## Instructions`; `return` folds into the final Step. A bare string appearing as a block body shorthand (omitting `flow:`, per `language-surface.md` §3.2) is treated identically and resolves to `Step` — the `flow:` header is not required for the compiler to assign the instruction role.

### Mandatory / Optional Per Declaration Kind

| Section | `skill` | `block` | `export block` |
|---------|---------|---------|----------------|
| `description:` | Optional (generated if omitted) | Optional (Required when consulted via `.applies()`) | Optional (Required when consulted via `.applies()`) |
| `effects:` | Optional | Optional | Optional (validated against inference) |
| `context:` | Optional | Optional | Optional |
| `constraints:` | Optional | Optional | Optional |
| `flow:` | Required (unless instruction-only) | Optional | Expected (needs explicit `return`) |

A `skill` body must contain at least `constraints:` or `flow:` (or both). An empty skill body is a compile error. An `export block` must have an explicit `return` path, which in practice means it will have `flow:`.

### Recommended Source Order

Source order is free — the compiler reorders to the fixed compiled-output order. Recommended convention:

1. `description:` (if used)
2. `effects:`
3. `context:`
4. `constraints:`
5. `flow:`

The compiler's source normalization pass enforces this order when rewriting.

## Open Questions

(None remaining — `context:` has been promoted to a full MVP section header.)
