# Glyph Preferences

This document defines how Glyph handles user and project preferences — stable configurable values like terminal multiplexer, communication style, validation strictness, preferred tools, or project conventions.

## Status

Formalizes decisions captured during Tier 3 work. Builds on `declaration-headers.md` (value-binding declaration kinds), `import-resolution.md` (import semantics), and `effects.md` (effect vocabulary).

## Design Posture

Preferences in Glyph are **ordinary exported constants**, not a dedicated language feature. There is no `pref(...)` call syntax, no `reads_prefs` effect, and no ambient lookup. A preference is a named value — a `text`, `int`, or `float` declaration marked `export` — that callers bring into scope with a normal `import`.

This keeps the language surface small and makes preferences indistinguishable from any other imported constant at the syntax, resolution, and effect level.

## Declaring a Preference

A preference is declared with one of the value-binding declaration kinds and the `export` modifier:

```glyph
export text tone = "concise"

export text terminal_mux = "tmux"

export int validation_strictness = 2

export float default_temperature = 0.7
```

Rules:

- MVP pref values are `String`, `Int`, or `Float` literals. Other types are not supported until the corresponding value-binding declaration is added.
- The literal on the right-hand side is the **final value** in MVP. An override mechanism (project config file, CLI flags, env vars) is deferred; see `todo.md`.
- Any `.glyph.md` file may declare preferences. A dedicated prefs file is conventional, not required.

## Importing a Preference

Preferences are imported like any other exported name, using the import forms defined in `declaration-headers.md` and the resolution rules in `import-resolution.md`:

```glyph
import "./prefs.glyph.md" { tone, terminal_mux, validation_strictness }

skill write_summary()
    flow:
        render(tone=tone, strictness=validation_strictness)
```

Or with a whole-module alias:

```glyph
import "./prefs.glyph.md" as prefs

skill write_summary()
    flow:
        render(tone=prefs.tone)
```

At compile time the preference value is inlined into the compiled Markdown, identical to any other imported `text` / `int` / `float` constant.

## Standard Prefs Library

The compiler ships a default prefs file so any project can import a baseline pref set without defining its own:

```glyph
import "@glyph/prefs" { tone, terminal_mux }
```

The exact import scheme, the standard pref set, and how it composes with user-defined prefs are TODOs; see `todo.md`.

## Effects

Importing or reading a preference does **not** contribute any effect. A `reads_prefs` effect was considered and rejected: preferences are ordinary compile-time constants, and treating them specially would conflate configuration with runtime side effects.

An `export block` that reads a preference does so through an explicit import. The preference appears as a declared dependency, not hidden ambient context, so closure (see `data-flow-and-calls.md`) is preserved automatically.

## Recompilation On Preference Change

Preference values are inlined at compile time. If a preference value changes, affected skills must be recompiled. The compiler may maintain a reverse dependency map from preference source files to compiled outputs to identify which compiled files are stale.

Runtime injection of preference values is not part of MVP. A future Glyph-aware loader or hook could substitute preference values before the agent reads the compiled skill.

## What Preferences Are Not

- **Not a call.** There is no `pref("key")` form. Preferences are plain identifiers.
- **Not ambient.** A skill that depends on a preference must import it explicitly.
- **Not typed by a magic system.** Preferences carry the type of their declaration (`String`, `Int`, `Float`). Callers can declare matching parameter types per `types.md`.
- **Not mutable at runtime.** The compiled Markdown contains the resolved value. Mutation happens only by editing source and recompiling.

## Interaction With Other Design Areas

- **Declaration headers** (`declaration-headers.md`): Value-binding declaration grammar for `text`, `int`, `float` and their `export` variants is defined there.
- **Import resolution** (`import-resolution.md`): Pref imports follow the standard path and selective-import rules; no special resolution is needed.
- **Effects** (`effects.md`): Pref reads contribute no effects. The 8-keyword MVP effect vocabulary is unchanged.
- **Data flow** (`data-flow-and-calls.md`): The "Global Preferences" section there defers to this document.
- **Todo** (`todo.md`): Pref override mechanism and standard prefs file details are tracked as deferred items.

## Deferred

- Project-level or user-level override mechanism (config file, CLI flags, env vars).
- Standard prefs library contents, import scheme (`@glyph/prefs` or equivalent), and composition rules with user-defined prefs.
- `bool` preferences (blocked on adding `bool` as a declaration kind).
- Structured preferences (objects, lists) — blocked on collection types.
- Runtime preference injection.
