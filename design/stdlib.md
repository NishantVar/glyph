# Glyph Standard Library

This document defines the MVP standard library for Glyph: what ships with the compiler, how it is distributed and resolved, and how it interacts with the rest of the language.

## Design Posture

The MVP stdlib is **minimal by intent**. Glyph is an authoring language, not a runtime — most reusable instruction patterns are better expressed as user-authored `export block` and `export text` declarations in project libraries. The stdlib exists only for primitives that require compiler-known types or effects that cannot be expressed in user code.

For MVP, the stdlib contains **one entry**: `subagent`.

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
        researcher = subagent("Investigate " + scope)
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

## The `Agent` Type

`Agent` is a **compiler-known primitive type**, joining `String`, `Int`, `Float`, `Bool`, and `None` in the type vocabulary defined by `types.md`.

| Value kind | Type name |
|---|---|
| Subagent handle | `Agent` |

### Semantics

An `Agent` value is a handle representing a spawned subagent. It carries identity — the compiled output uses the binding name to refer to the agent across steps.

Unlike other primitive types, `Agent` is not a literal. There is no agent literal syntax. The only way to obtain an `Agent` value is by calling `subagent()`.

### Type Checking

`Agent` participates in nominal matching at call boundaries, like all other types (`types.md`). If a block declares a parameter as `: Agent`, passing a `String` is a compile error. If the annotation is omitted, no check is performed.

## The `spawns_agent` Effect

`spawns_agent` is a new effect keyword added to the MVP vocabulary, extending the set from 8 to 9 keywords.

| Keyword | Meaning |
|---|---|
| `spawns_agent` | Spawns a subagent to perform delegated work. |

### Propagation

`spawns_agent` propagates through the call graph like all other effects (`ir-and-semantics.md`). If a block calls `subagent()`, its inferred effect set includes `spawns_agent`. If the block explicitly declares `effects:`, the declared set must include `spawns_agent` or the compiler emits an error.

### Relationship to Other Effects

`spawns_agent` is orthogonal to existing effects. A skill that spawns a subagent may also read files, run commands, etc. The spawned agent's own effects are not propagated into the caller's effect set — the caller only declares that it spawns an agent, not what that agent does.

## Distribution and Resolution

### Import Path

The stdlib is distributed as a `.glyph.md` file shipped with the compiler at a well-known path. It is imported using the `@glyph/std` namespace:

```glyph
import "@glyph/std" { subagent }
```

This follows the same pattern as the `@glyph/prefs` namespace sketched in `preferences.md` and `todo.md`. The `@glyph/` prefix is reserved for compiler-shipped modules.

### Explicit Import Required

Stdlib entries are **not auto-available**. They must be explicitly imported like any other exported declaration. This keeps the name resolution order simple and avoids magic names.

The name resolution order in `values-and-names.md` lists "standard-library entry" as step 3. For MVP, this step resolves names from `@glyph/std` imports — it does not inject names without an import statement.

### No Special Resolution

Stdlib imports follow the same resolution rules as all other imports (`imports.md`): selective import, whole-module import with alias, no-shadowing, collision diagnostics. The only difference is the `@glyph/` path prefix, which the compiler resolves to the shipped stdlib file location rather than a relative file path.

## Interaction With Repair

The repair pass receives stdlib entries as part of its input (`repair.md` §3). For MVP, this means repair knows about `subagent`. If an author writes `subagent(task)` without importing it, repair may add the missing `import "@glyph/std" { subagent }` statement rather than generating a `generated text` definition — stdlib resolution takes priority over repair materialization.

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
- **Inter-agent communication.** Message passing, shared state, or channel primitives between agents. Deferred until multi-agent orchestration model is designed.
- **Stdlib expansion.** Additional entries (common constraint texts, utility blocks) may be added post-MVP if patterns emerge from real usage. The stdlib should grow conservatively.
- **`@glyph/` namespace resolution.** The exact mechanism for resolving `@glyph/` prefixed imports to compiler-shipped files (environment variable, compiler config, hardcoded path) is an implementation detail deferred to the compiler build.
