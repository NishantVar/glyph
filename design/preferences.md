# Glyph Preferences

This document defines how Glyph handles user and project preferences — stable configurable values like terminal multiplexer, communication style, validation strictness, preferred tools, or project conventions.

## Design Posture

Preferences in Glyph are **ordinary exported constants**, not a dedicated language feature. There is no `pref(...)` call syntax, no `reads_prefs` effect, and no ambient lookup. A preference is a named value — a `const` declaration marked `export` — that callers bring into scope with a normal `import`.

This keeps the language surface small and makes preferences indistinguishable from any other imported constant at the syntax, resolution, and effect level.

## Declaring a Preference

A preference is declared with `export const` and the appropriate literal value:

```glyph
export const tone = "concise"

export const terminal_mux = "tmux"

export const validation_strictness = 2

export const default_temperature = 0.7
```

Rules:

- MVP pref values are string, integer, or float literals. The compiler infers the value kind from the literal on the right side.
- **A default value is mandatory.** Every preference declaration must include the `= <literal>` assignment. An `export const tone` without a right-hand side is a parse error. This ensures every preference has a known fallback — both for the current compile-time inlining model and for any future runtime-slot mechanism.
- The literal on the right-hand side is the **final value** in MVP. An override mechanism (project config file, CLI flags, env vars) is deferred; see [[todo]].
- Any `.glyph` file may declare preferences. A dedicated prefs file is conventional, not required.

## Importing a Preference

Preferences are imported like any other exported name, using the import forms defined in [[language-surface]] and the resolution rules in [[imports]]:

```glyph
import "./prefs.glyph" { tone, terminal_mux, validation_strictness }

skill write_summary()
    flow:
        render(tone=tone, strictness=validation_strictness)
```

Or with a whole-module alias:

```glyph
import "./prefs.glyph" as prefs

skill write_summary()
    flow:
        render(tone=prefs.tone)
```

At compile time the preference value is inlined into the compiled Markdown, identical to any other imported `const` value.

Preferences may also be used directly as **parameter defaults** on `skill` and `export block` declarations (per [[language-surface]] §3.8). For example:

```glyph
import "./prefs.glyph" { default_temperature }

skill summarize(temperature: Temperature = default_temperature)
    flow:
        ...
```

The default is resolved at compile time and the resolved literal value appears in the compiled `## Parameters` section. When the prefs library's value changes, every skill that defaults to it picks up the new value on the next compile — single source of truth.

## Standard Prefs Library

The compiler ships a default prefs file so any project can import a baseline pref set without defining its own:

```glyph
import "@glyph/prefs" { tone, terminal_mux }
```

The exact import scheme, the standard pref set, and how it composes with user-defined prefs are TODOs; see [[todo]].

## Effects

Importing or reading a preference does **not** contribute any effect. A `reads_prefs` effect was considered and rejected: preferences are ordinary compile-time constants, and treating them specially would conflate configuration with runtime side effects.

An `export block` that reads a preference does so through an explicit import. The preference appears as a declared dependency, not hidden ambient context, so closure (see [[data-flow]]) is preserved automatically.

## Library File Semantics

A prefs file like `prefs.glyph` is a library file under the rules in [[language-surface]] §File-Level Rules. It has zero `skill` declarations and only `export const` declarations. Under the library emission model:

- **It emits zero `.md` files.** Constants are always inlined into consumers at compile time — they never meet a tier threshold for standalone procedure files.
- **It compiles successfully.** Zero output is not an error. The file contributes names and values to consumers through the validated IR.
- **It satisfies the "at least one export" rule.** Every declaration in a prefs file is already `export`.

## Recompilation On Preference Change

Preference values are inlined at compile time. If a preference value changes, every skill that imports the prefs file (directly or transitively) must be recompiled. The compiler's caching guarantees that changing a prefs source file invalidates all consumers.

Runtime injection of preference values is not part of MVP. A future Glyph-aware loader or hook could substitute preference values before the agent reads the compiled skill.

## What Preferences Are Not

- **Not a call.** There is no `pref("key")` form. Preferences are plain identifiers.
- **Not ambient.** A skill that depends on a preference must import it explicitly.
- **Not typed by a magic system.** Preferences carry the value kind inferred from their literal (string, integer, float). Callers can declare matching parameter types per [[types]].
- **Not mutable at runtime.** The compiled Markdown contains the resolved value. Mutation happens only by editing source and recompiling.

## Interaction With Other Design Areas

- **Declaration headers** ([[language-surface]]): Value-binding declaration grammar for `const` and its `export` variant is defined there.
- **Import resolution** ([[imports]]): Pref imports follow the standard path and selective-import rules; no special resolution is needed.
- **Effects** ([[ir-and-semantics]]): Pref reads contribute no effects. The 9-keyword MVP effect vocabulary is unchanged.
- **Data flow** ([[data-flow]]): The "Global Preferences" section there defers to this document.
- **Todo** ([[todo]]): Pref override mechanism and standard prefs file details are tracked as deferred items.

## Deferred

- Project-level or user-level override mechanism (config file, CLI flags, env vars).
- **Preferences as runtime parameter slots.** Instead of inlining preference values at compile time, the compiler could promote imported prefs to entries in the consuming skill's `## Parameters` section with `{pref_name}` slots in Step/Constraint prose and the declared literal as the default value. This preserves the parameterless compilation model (one stable `.md` per source file) while letting the consuming LLM resolve prefs from user context at runtime — identical to how skill parameters already work. Trade-offs: (1) a skill importing N prefs gains N extra parameters, potentially cluttering `## Parameters`; (2) prefs bubble up through every intermediate caller in the call graph; (3) it blurs the distinction between operational inputs (e.g., `scope`) and behavioral configuration (e.g., `tone`). The mandatory default value rule (see §Declaring a Preference) ensures every pref has a fallback regardless of which model is active.
- Standard prefs library contents, import scheme (`@glyph/prefs` or equivalent), and composition rules with user-defined prefs.
- `bool` preferences (blocked on adding `bool` literal support in `const`).
- Structured preferences (objects, lists) — blocked on collection types.
- Runtime preference injection.
