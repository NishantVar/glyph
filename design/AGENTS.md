# Glyph — Design

This folder is the working source of truth for Glyph system design. The repository [README](../README.md) describes the project at a high level; design decisions are captured here.

## Documents

### Foundations
- [foundations.md](foundations.md) — stable design principles and hard conceptual boundaries (reference card)

### Language
- [language-surface.md](language-surface.md) — source syntax: declarations (`skill`, `block`, `export block`, `text`, `int`, `float`, `import` and their `export`/`generated` variants), header grammar, indentation, sub-section syntax, authoring model, source-to-IR pipeline
- [values-and-names.md](values-and-names.md) — primitive values, identifiers, reserved keywords, name resolution, no-shadowing rule
- [data-flow.md](data-flow.md) — parameters, bindings, calls, arguments, control flow (`if`/`elif`/`else`), return semantics, closure and scope
- [types.md](types.md) — semantic type labels, primitives (`String`, `Int`, `Float`, `Bool`, `None`), named domain types, nominal matching
- [preferences.md](preferences.md) — preferences as ordinary exported constants (`export text`/`export int`/`export float`), no `pref(...)` call, no special effect, compile-time resolution

### IR and Semantics
- [ir-and-semantics.md](ir-and-semantics.md) — IR roles (`InputContract`, `Step`, `Constraint`, `Context`, `OutputContract`), constraint strength/polarity, effects (8 keywords, propagation, validation), section vocabulary

### Modules
- [imports.md](imports.md) — path-based import resolution, importable vs. private declarations, cycle rejection, effect propagation across boundaries

### Compilation
- [repair.md](repair.md) — LLM repair pass, generated definitions, comment syntax, intent potency, idempotence
- [compiled-output.md](compiled-output.md) — compiled Markdown shape: YAML frontmatter, fixed section order, formatting, authoring-construct erasure

### Post-MVP
- [specialization.md](specialization.md) — compile-time specialization for expert agents: `agent`, `abstract agent`, slots, override/append/prepend, locked constraints

## Reading Order

1. **foundations.md** — philosophy and hard limits at a glance
2. **language-surface.md** — what Glyph source looks like
3. **values-and-names.md** — the value and naming system
4. **data-flow.md** — how data moves through skills and blocks
5. **types.md** — the type system
6. **preferences.md** — how user/project preferences work
7. **ir-and-semantics.md** — what the compiler produces internally
8. **imports.md** — multi-file composition
9. **repair.md** — how invalid source gets fixed before compilation
10. **compiled-output.md** — what the final Markdown looks like
11. **specialization.md** — future reuse model (post-MVP)

## Relationship to Research

Design decisions build on top of the [agent-skill-dsl research](../research/agent-skill-dsl/). Consolidated wiki pages from research are promoted here as individual design files once they stabilise into decisions.
