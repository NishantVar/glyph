# Glyph Standard Library

This document defines the MVP standard library for Glyph: what ships with the compiler, how it is distributed and resolved, and how it interacts with the rest of the language.

## Design Posture

The MVP stdlib is **minimal by intent**. Glyph is an authoring language, not a runtime — most reusable instruction patterns are better expressed as user-authored `export block` and `export const` declarations in project libraries. The stdlib exists only for primitives that require compiler-known types or effects that cannot be expressed in user code.

For MVP, the stdlib contains **three entries**: `subagent`, `send`, and `load`. The first two are author-facing; `load` is compiler-internal (see §The `load` Primitive). There is no concurrency primitive — multiple `subagent(...)` calls in source compile to multiple "Spawn a subagent..." instructions, and the consuming agent decides whether to dispatch them concurrently or sequentially. Concurrency as a guaranteed language feature is deferred.

## Projection Model: Uniform Synthetic Body

Stdlib calls project to compiled output using the **same `resolved_body_text` mechanism** as user-defined blocks. There is no special-casing. Each stdlib entry has a compiler-provided **synthetic body template** that Step 1 (deterministic resolution) attaches to the `ResolvedCall` node. Step 2 (LLM reshaping) then treats it identically to any other Call — reshaping the body, applying `with` modifiers, and preserving `{param}` slots.

The synthetic body template may reference two kinds of tokens:

- **Parameter slots** (`{task}`, `{message}`) — resolved from `Call.args`, same as user-defined blocks.
- **Call-site context** (`Call.output.name`, `Call.args.agent.name`) — resolved from existing fields on the `Call` IR node. These are not `{param}` slots in the source sense — they are concrete values that Step 1 reads from the IR and writes directly into the body text.

This uniform approach scales naturally: adding a future stdlib entry (e.g., `await(agent)`) means defining one synthetic body template, not a new projection code path. See each primitive's §Projection Rule for its specific template.

## The `subagent` Primitive

### Declaration

```glyph
export block subagent(task) -> Agent
    effects: spawns_agent

    flow:
        "Spawn a new subagent to perform the given task."
        return none
```

### Purpose

`subagent` spawns a new agent to handle a delegated task. The caller receives an `Agent`-typed handle that identifies the spawned agent for future reference.

```glyph
import "@glyph/std" { subagent }

skill investigate(scope = ".")
    effects: spawns_agent

    flow:
        researcher = subagent(scope) with "investigate this area"
        return researcher
```

### Compiled Output

A `subagent` call compiles to a prose instruction in the `### Steps` section of the compiled Markdown:

```md
1. Spawn a subagent to investigate the given scope. Refer to this agent as "researcher."
```

The compiled instruction includes the bound name so the agent knows how to reference the subagent in subsequent steps.

### Projection Rule

`subagent` uses the same `resolved_body_text` mechanism as user-defined blocks — no special-casing. Step 1 (deterministic resolution) constructs the synthetic body by reading existing fields on the `Call` IR node:

- `{task}` ← from `Call.args.task`. May be a string literal or contain `{param}` slots (preserved as-is for runtime resolution).
- Agent reference name ← from `Call.output.name` (the binding, e.g., `researcher` in `researcher = subagent(...)`). Omitted if the call has no binding.

**Synthetic body template:**

```
"Spawn a subagent to: {task}. Refer to this agent as \"{output.name}\"."
```

If there is no binding (bare `subagent(scope)` without `x = ...`), the template truncates to:

```
"Spawn a subagent to: {task}."
```

**`with` modifier:** Allowed. Reshapes the synthetic body via Step 2 like any other call. Example: `subagent(scope) with "investigate auth boundaries"` produces a body that Step 2 weaves together — e.g., "Spawn a subagent to investigate auth boundaries in {scope}. Refer to this agent as 'researcher.'"

**`scoped_constraints`:** Empty. `subagent` declares no constraints.

**Effect contribution:** `{ spawns_agent }`, propagated via normal call-graph inference.

### Parameters

- `task` — a description of what the subagent should do.

### Return Type

`Agent` — a compiler-known type. See the Agent Type section below.

## The `send` Primitive

### Declaration

```glyph
export block send(agent: Agent, message)
    effects: spawns_agent

    flow:
        "Send a follow-up message to the given agent."
        return none
```

### Purpose

`send` delivers a follow-up message to a running subagent. The first parameter is the `Agent` handle obtained from a prior `subagent()` call. Because the first parameter is typed `Agent`, UFCS applies: authors write `agent.send(message)` and the compiler desugars it to `send(agent, message)`.

```glyph
import "@glyph/std" { subagent, send }

skill investigate(scope = ".")
    effects: spawns_agent

    flow:
        researcher = subagent(scope) with "investigate this area"
        researcher.send("Now check the edge cases around token expiry.")
        return researcher
```

### Compiled Output

A `send` call compiles to a prose instruction in `### Steps`:

```md
2. Send the researcher this follow-up: "Now check the edge cases around token expiry."
```

### Projection Rule

`send` uses the same `resolved_body_text` mechanism as `subagent` and user-defined blocks. Step 1 constructs the synthetic body by reading existing fields on the `Call` IR node:

- Agent reference name ← from `Call.args.agent`, which is a `BindingRef`. Step 1 uses the binding's name (e.g., `researcher`).
- `{message}` ← from `Call.args.message`. May be a string literal or contain `{param}` slots.

**Synthetic body template:**

```
"Send {args.agent.name} the following: {message}."
```

**`with` modifier:** Allowed, though uncommon. Would shape the tone or framing of the send instruction via Step 2.

**`scoped_constraints`:** Empty. `send` declares no constraints.

**Effect contribution:** `{ spawns_agent }`, propagated via normal call-graph inference.

### Parameters

- `agent` (`: Agent`) — the target subagent, obtained from a prior `subagent()` call.
- `message` — the follow-up instruction to send.

### Return Type

`send` is a side-effecting operation with no meaningful return value (omits `->`).

## The `load` Primitive

### Declaration

```glyph
export block load(path: FilePath)
    effects: reads_files

    flow:
        "Load and follow the instructions in the given file."
        return none
```

### Purpose

`load` is a **compiler-internal primitive** — authors do not write `load()` calls directly. The compiler emits `load` instructions in compiled output when it selects the external-file projection tier for an imported block call (see `compiled-output.md` §Three-Tier Block Projection).

When the compiler determines that an imported `export block` should be projected as an external file (because it is conditional or shared across skills), it:

1. Compiles the export block to a standalone procedure `.md` file.
2. Replaces the inlined Call expansion in the referencing skill's `### Steps` with a prose instruction that directs the consuming agent to load and follow the procedure file.

The `load` primitive exists in the stdlib to provide a consistent effect signature (`reads_files`) and to participate in effect propagation. Skills that reference external procedure files carry `reads_files` in their inferred effect set because of the transitive `load` call.

### Compiled Output

A `load` reference compiles to a prose instruction in `### Steps`:

```md
2. If the files have security concerns, load and follow the procedure in
   `review_tools/review-code.md`, focusing on security vulnerabilities.
```

The file path is a relative path from the compiled output directory to the procedure file.

### Parameters

- `path` (`FilePath`) — relative path to the compiled procedure file.

### Return Type

`load` directs the agent to follow instructions in another file; it does not produce a meaningful return value (omits `->`).

### Not Author-Facing

Unlike `subagent` and `send`, `load` is not imported by authors. It has no source-level syntax. The compiler uses it internally when selecting the external-file projection tier. Authors control which blocks are imported; the compiler decides whether those imports inline, become same-file procedures, or become external file references.

## The `Agent` Type

`Agent` is a **compiler-known type**. It is the only non-domain type that the compiler treats specially, alongside the internal value kinds (string, integer, float, boolean, none) defined by `types.md`.

| Value kind | Type name |
|---|---|
| Subagent handle | `Agent` |

### Semantics

An `Agent` value is a handle representing a spawned subagent. It carries identity — the compiled output uses the binding name to refer to the agent across steps.

Unlike other primitive types, `Agent` is not a literal. There is no agent literal syntax. The only way to obtain an `Agent` value is by calling `subagent()`.

`Agent` is the receiver type for `send` via UFCS (`data-flow.md`): `researcher.send(msg)` desugars to `send(researcher, msg)`. This is not special method dispatch — it is the general UFCS rule applied to a stdlib function whose first parameter is typed `Agent`. UFCS is pure syntactic sugar in a single namespace with no method dispatch; see `values-and-names.md` §UFCS Name Resolution for the canonical rule.

### Type Checking

`Agent` participates in nominal matching at call boundaries, like all other types (`types.md`). If a block declares a parameter as `: Agent`, passing a `String` is a compile error. If the annotation is omitted, no check is performed.

### Agent Value Lifecycle

An `Agent` value behaves like any other typed value once obtained from `subagent(...)`:

- **Bindings.** `researcher = subagent(scope)` binds the `Agent` handle to the name `researcher`. Subsequent flow statements may reference `researcher` until the binding's scope ends. Branch scoping (`data-flow.md` §Local Bindings And Mutation) applies — an `Agent` bound inside an `if`/`elif`/`else` branch is visible only within that branch.
- **Passing as an argument.** An `Agent` may be passed to any block that declares an `Agent` parameter — same-file `block`, `export block`, or `send` (via UFCS or positional). Passing an `Agent` where a non-`Agent` annotation is declared is a nominal-mismatch error (`G::analyze::nominal-mismatch`).
- **Returning from a block or skill.** A `block` or `export block` may declare `-> Agent` and `return researcher` from its body. The returned handle refers to the same spawned subagent; it is not a copy or a fresh spawn. Returning an `Agent` from a `skill` is legal — the skill's `OutputContract` records the type. **Return folding for `Agent`-typed values:** when `return <agent_binding>` folds into the final Step, the prose says the agent itself is the result (e.g., "Your result is the researcher agent spawned above — the caller may continue sending it instructions."), **not** that the agent's output is the result. `return researcher` means you are returning the handle, not the researcher's findings. If the author intends to return what the agent produced, they should use an explicit inline string (e.g., `return "Report the researcher's findings as your result."`).
- **Across branches.** An `Agent` bound at flow top-level remains visible inside subsequent branches, but an `Agent` bound inside one branch is not visible in a sibling branch. To use the same handle across branches, bind it before the conditional.
- **No literal form.** There is no `Agent` literal. The only way to introduce a new `Agent` value is `subagent(...)`. A user-defined block that declares `-> Agent` must obtain its return value transitively from a `subagent` call (directly, through an imported callee, or through a parameter of type `Agent`).
- **No identity equality, no termination primitive.** MVP has no `==` operator, no `if researcher == other_agent:` form, and no explicit "wait for completion" or "kill agent" primitive. An `Agent` is opaque — it can only be created (`subagent`), referenced by binding name in compiled prose, and passed to `send`. Identity-based comparison and lifecycle control are deferred (see §Deferred).

## The `spawns_agent` Effect

> **Gated: `--enable-effects` (default: off).** The effect declarations on stdlib entries (`spawns_agent`, `reads_files`) and the propagation rules below are inactive unless `--enable-effects` is passed. When the gate is off, stdlib entries still function normally — only the effect metadata is suppressed.

`spawns_agent` is a new effect keyword added to the MVP vocabulary, extending the set from 8 to 9 keywords.

| Keyword | Meaning |
|---|---|
| `spawns_agent` | Spawns or interacts with subagents. |

Both stdlib primitives (`subagent`, `send`) carry the `spawns_agent` effect. There is no separate effect for messaging vs. spawning — `spawns_agent` covers all subagent interaction.

### Propagation

`spawns_agent` propagates through the call graph like all other effects (`ir-and-semantics.md`). If a block calls `subagent()` or `send()`, the compiler adds `spawns_agent` to the block's **inferred** effect set — stdlib calls contribute to inferred effects via their synthetic-body projection (§Projection Model: Uniform Synthetic Body), exactly the same way user-defined block calls do. If the declaration omits `effects:` entirely, Phase 3a auto-adds the inferred set (including `spawns_agent`) and emits `G::repair::inferred-effects` (warning, informational). If the declaration explicitly lists effects but omits `spawns_agent`, the compiler emits `G::analyze::effects-under-declared` (error). If the declared set includes keywords not in the inferred set, the compiler emits `G::analyze::effects-over-declared` (warning). See `ir-and-semantics.md` §Effects for the full infer-when-omitted / validate-when-declared policy.

### Relationship to Other Effects

`spawns_agent` is orthogonal to existing effects. A skill that spawns a subagent may also read files, run commands, etc. The spawned agent's own effects are not propagated into the caller's effect set — the caller only declares that it spawns an agent, not what that agent does. See `ir-and-semantics.md` §Effect Boundaries At Subagent Spawns for the full reasoning and worked example.

## Distribution and Resolution

### Import Path

For MVP, the stdlib is **compiler-embedded** and `@glyph/` is a **reserved virtual prefix**, not a filesystem path. The three stdlib entry signatures, their effects, and the `Agent` type are baked into `glyph-core` as in-memory synthetic definitions. There is no on-disk file, no install path, and no filesystem lookup for any `@glyph/*` import. The import syntax mirrors file-based imports:

```glyph
import "@glyph/std" { subagent }
```

Resolution behaviour for the `@glyph/` namespace:

- `@glyph/std` resolves to the in-memory synthetic definitions for `subagent`, `send`, and `load` (the latter is compiler-internal — see §The `load` Primitive).
- Any other `@glyph/*` path (e.g., `@glyph/foo`, `@glyph/std/extra`) fires `G::imports::unknown-stdlib-module` (error). The MVP recognises exactly one virtual module under this prefix.
- The `@glyph/` prefix is reserved for compiler-shipped modules and never collides with filesystem paths: a real on-disk file named `@glyph` is not consulted.

This follows the same pattern as the `@glyph/prefs` namespace sketched in `preferences.md` and `todo.md`. Post-MVP, when the stdlib grows beyond a handful of entries, the compiler may migrate to real `.glyph.md` files shipped at a well-known path. The import syntax stays the same — only the resolution mechanism changes.

### Explicit Import Required

Stdlib entries are **not auto-available**. They must be explicitly imported like any other exported declaration. This keeps the name resolution order simple and avoids magic names.

The name resolution order in `values-and-names.md` lists "standard-library entry" as step 3. For MVP, this step resolves names from `@glyph/std` imports — it does not inject names without an import statement.

### No Special Resolution

Stdlib imports follow the same resolution rules as all other imports (`imports.md`): selective import, whole-module import with alias, no-shadowing, collision diagnostics. The only difference is the `@glyph/` path prefix, which the compiler resolves internally rather than to a relative file path.

## Interaction With Repair

Stdlib names resolve during Phase 2 (Analyze) through normal name resolution — if the author has imported them, they resolve; if not, Analyze emits a `G::analyze::stdlib-missing-import` diagnostic (repairable). Repair may then add the missing `import "@glyph/std" { ... }` statement. **Stdlib names never trigger `generated const` or `generated block` materialization** because either they resolve (import present) or their diagnostic is specifically `stdlib-missing-import` (not `undefined-name`/`undefined-call`). Misuse of a resolved stdlib name (wrong argument count, bare-name reference to a block, missing effect declaration) is a normal compile error, not a repair target.

## Interaction With Closure

`subagent` is an imported `export block`. An `export block` that calls `subagent` satisfies its closure requirement through the explicit import, like any other imported dependency (`data-flow.md`).

## Interaction With Compiled Output

The `import "@glyph/std" { subagent }` statement compiles away like all imports (`compiled-output.md`). No stdlib references, import paths, or module names appear in the compiled Markdown.

## Cross-References

- **Types** (`types.md`): `Agent` joins the primitive type vocabulary.
- **Effects** (`ir-and-semantics.md`): `spawns_agent` extends the effect keyword set.
- **Imports** (`imports.md`): `@glyph/std` follows standard import semantics with a compiler-resolved path prefix.
- **Repair** (`repair.md`): Repair prefers stdlib resolution over `generated const` materialization.
- **Compiled output** (`compiled-output.md`): Stdlib imports compile away completely.
- **Preferences** (`preferences.md`): `@glyph/prefs` and `@glyph/std` share the `@glyph/` namespace convention.

## Deferred

- **Named agent invocation.** Post-MVP skill inheritance (see `todo.md`) may allow defining named agents that can be imported and called directly. The `subagent` primitive covers anonymous delegation for MVP.
- **Shared state between agents.** Shared memory, channels, or structured data exchange between agents. MVP agents communicate via `send` (prose messages) only.
- **Agent completion / await.** An explicit mechanism to block until a subagent finishes and retrieve its result. For MVP, completion is implicit in prose: subsequent steps that reference an agent's output naturally imply the agent has finished.
- **Stdlib expansion.** Additional entries (common constraint texts, utility blocks) may be added post-MVP if patterns emerge from real usage. The stdlib should grow conservatively.
- **`@glyph/` namespace resolution.** For MVP, stdlib is compiler-embedded (see §Distribution and Resolution). Post-MVP, if the stdlib migrates to real `.glyph.md` files, the resolution mechanism (environment variable, compiler config, well-known path) becomes an implementation decision.
- **General collection types.** Lists, sets, maps, and other collections are not part of MVP. A general collection type system is deferred.
- **Concurrency primitive.** A guaranteed-concurrent spawn primitive (e.g. a future `parallel`) is deferred. MVP relies on the consuming agent's discretion when multiple `subagent(...)` calls appear.
