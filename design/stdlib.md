# Glyph Standard Library

This document defines the MVP standard library for Glyph: what ships with the compiler, how it is distributed and resolved, and how it interacts with the rest of the language.

## Design Posture

The MVP stdlib is **minimal by intent**. Glyph is an authoring language, not a runtime — most reusable instruction patterns are better expressed as user-authored `export block` and `export text` declarations in project libraries. The stdlib exists only for primitives that require compiler-known types or effects that cannot be expressed in user code.

For MVP, the stdlib contains **two entries**: `subagent` and `send`. There is no concurrency primitive — multiple `subagent(...)` calls in source compile to multiple "Spawn a subagent..." instructions, and the consuming agent decides whether to dispatch them concurrently or sequentially. Concurrency as a guaranteed language feature is deferred.

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

skill investigate(scope)
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

### Parameters

- `task` (`String`) — a description of what the subagent should do.

### Return Type

`Agent` — a compiler-known type. See the Agent Type section below.

## The `send` Primitive

### Declaration

```glyph
export block send(agent: Agent, message) -> None
    effects: spawns_agent

    flow:
        "Send a follow-up message to the given agent."
        return none
```

### Purpose

`send` delivers a follow-up message to a running subagent. The first parameter is the `Agent` handle obtained from a prior `subagent()` call. Because the first parameter is typed `Agent`, UFCS applies: authors write `agent.send(message)` and the compiler desugars it to `send(agent, message)`.

```glyph
import "@glyph/std" { subagent, send }

skill investigate(scope)
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

### Parameters

- `agent` (`Agent`) — the target subagent, obtained from a prior `subagent()` call.
- `message` (`String`) — the follow-up instruction to send.

### Return Type

`None` — `send` is a side-effecting operation with no return value.

## The `Agent` Type

`Agent` is a **compiler-known primitive type**, joining `String`, `Int`, `Float`, `Bool`, and `None` in the type vocabulary defined by `types.md`.

| Value kind | Type name |
|---|---|
| Subagent handle | `Agent` |

### Semantics

An `Agent` value is a handle representing a spawned subagent. It carries identity — the compiled output uses the binding name to refer to the agent across steps.

Unlike other primitive types, `Agent` is not a literal. There is no agent literal syntax. The only way to obtain an `Agent` value is by calling `subagent()`.

`Agent` is the receiver type for `send` via UFCS (`data-flow.md`): `researcher.send(msg)` desugars to `send(researcher, msg)`. This is not special method dispatch — it is the general UFCS rule applied to a stdlib function whose first parameter is typed `Agent`.

### Type Checking

`Agent` participates in nominal matching at call boundaries, like all other types (`types.md`). If a block declares a parameter as `: Agent`, passing a `String` is a compile error. If the annotation is omitted, no check is performed.

## The `spawns_agent` Effect

`spawns_agent` is a new effect keyword added to the MVP vocabulary, extending the set from 8 to 9 keywords.

| Keyword | Meaning |
|---|---|
| `spawns_agent` | Spawns or interacts with subagents. |

Both stdlib primitives (`subagent`, `send`) carry the `spawns_agent` effect. There is no separate effect for messaging vs. spawning — `spawns_agent` covers all subagent interaction.

### Propagation

`spawns_agent` propagates through the call graph like all other effects (`ir-and-semantics.md`). If a block calls `subagent()` or `send()`, its inferred effect set includes `spawns_agent`. If the block explicitly declares `effects:`, the declared set must include `spawns_agent` or the compiler emits an error.

### Relationship to Other Effects

`spawns_agent` is orthogonal to existing effects. A skill that spawns a subagent may also read files, run commands, etc. The spawned agent's own effects are not propagated into the caller's effect set — the caller only declares that it spawns an agent, not what that agent does.

## Distribution and Resolution

### Import Path

For MVP, the stdlib is **compiler-embedded**: the two entry signatures, effects, and the `Agent` type are hardcoded in the compiler. `@glyph/std` is a namespace the compiler recognizes internally, not a file path that resolves to a `.glyph.md` file on disk. The import syntax is the same as file-based imports:

```glyph
import "@glyph/std" { subagent }
```

This follows the same pattern as the `@glyph/prefs` namespace sketched in `preferences.md` and `todo.md`. The `@glyph/` prefix is reserved for compiler-shipped modules.

Post-MVP, when the stdlib grows beyond a handful of entries, the compiler may migrate to real `.glyph.md` files shipped at a well-known path. The import syntax stays the same — only the resolution mechanism changes.

### Explicit Import Required

Stdlib entries are **not auto-available**. They must be explicitly imported like any other exported declaration. This keeps the name resolution order simple and avoids magic names.

The name resolution order in `values-and-names.md` lists "standard-library entry" as step 3. For MVP, this step resolves names from `@glyph/std` imports — it does not inject names without an import statement.

### No Special Resolution

Stdlib imports follow the same resolution rules as all other imports (`imports.md`): selective import, whole-module import with alias, no-shadowing, collision diagnostics. The only difference is the `@glyph/` path prefix, which the compiler resolves internally rather than to a relative file path.

## Interaction With Repair

Stdlib names resolve during Phase 2 (Analyze) through normal name resolution — if the author has imported them, they resolve; if not, Analyze emits a `G::analyze::stdlib-missing-import` diagnostic (repairable). Repair may then add the missing `import "@glyph/std" { ... }` statement. **Stdlib names never trigger `generated text` or `generated block` materialization** because either they resolve (import present) or their diagnostic is specifically `stdlib-missing-import` (not `undefined-name`/`undefined-call`). Misuse of a resolved stdlib name (wrong argument count, bare-name reference to a block, missing effect declaration) is a normal compile error, not a repair target.

## Interaction With Closure

`subagent` is an imported `export block`. An `export block` that calls `subagent` satisfies its closure requirement through the explicit import, like any other imported dependency (`data-flow.md`).

## Interaction With Compiled Output

The `import "@glyph/std" { subagent }` statement compiles away like all imports (`compiled-output.md`). No stdlib references, import paths, or module names appear in the compiled Markdown.

## Cross-References

- **Types** (`types.md`): `Agent` joins the primitive type vocabulary.
- **Effects** (`ir-and-semantics.md`): `spawns_agent` extends the effect keyword set.
- **Imports** (`imports.md`): `@glyph/std` follows standard import semantics with a compiler-resolved path prefix.
- **Repair** (`repair.md`): Repair prefers stdlib resolution over `generated text` materialization.
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
