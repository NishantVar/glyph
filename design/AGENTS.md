# Glyph — Design

**Do not read files in `archive/`.** That directory contains superseded proposals kept only for historical reference. The active design is entirely in the files listed below.

This folder is the working source of truth for Glyph system design. The repository [README](../README.md) describes the project at a high level; design decisions are captured here.

## Documents

### Foundations
- [foundations.md](foundations.md) — stable design principles and hard conceptual boundaries (reference card)
- [foundations-first-principles.md](foundations-first-principles.md) — candidate reduction of the foundations into a smaller set of bedrock principles centered on novice learnability and intent-first authoring

### Language
- [language-surface.md](language-surface.md) — source syntax: declarations (`skill`, `block`, `export block`, `text`, `int`, `float`, `import` and their `export`/`generated` variants), header grammar, indentation, sub-section syntax, authoring model, source-to-IR pipeline
- [values-and-names.md](values-and-names.md) — primitive values, identifiers, reserved keywords, name resolution, no-shadowing rule
- [data-flow.md](data-flow.md) — parameters, bindings, calls, arguments, control flow (`if`/`elif`/`else`), return semantics, closure and scope
- [types.md](types.md) — semantic type labels, primitives (`String`, `Int`, `Float`, `Bool`, `None`), named domain types, nominal matching
- [preferences.md](preferences.md) — preferences as ordinary exported constants (`export text`/`export int`/`export float`), no `pref(...)` call, no special effect, compile-time resolution

### IR and Semantics
- [ir-and-semantics.md](ir-and-semantics.md) — IR roles (`InputContract`, `Step`, `Constraint`, `OutputContract`), constraint strength/polarity, effects (9 keywords, propagation, validation), section vocabulary

### Modules
- [imports.md](imports.md) — path-based import resolution, importable vs. private declarations, cycle rejection, effect propagation across boundaries
- [stdlib.md](stdlib.md) — standard library: MVP contains one entry (`subagent`), the `Agent` compiler-known type, and the `spawns_agent` effect

### Compilation
- [pipeline.md](pipeline.md) — **canonical compiler pipeline**: 7 phases (Parse, Analyze, Repair, Lower, Validate, Expand, Emit), Safety Sandwich, multi-file order, cacheability
- [diagnostics.md](diagnostics.md) — structured diagnostic shape, classification tiers (`error`/`repairable`/`warning`), ID scheme, representative catalog
- [repair.md](repair.md) — LLM repair pass, generated definitions (text and block), comment syntax, intent potency, idempotence
- [compiled-output.md](compiled-output.md) — compiled Markdown shape: YAML frontmatter, `## Instructions` (`### Steps` + `### Constraints`), per-invocation model

## Reading Order

1. **foundations.md** — philosophy and hard limits at a glance
2. **language-surface.md** — what Glyph source looks like
3. **values-and-names.md** — the value and naming system
4. **data-flow.md** — how data moves through skills and blocks
5. **types.md** — the type system
6. **preferences.md** — how user/project preferences work
7. **ir-and-semantics.md** — what the compiler produces internally
8. **imports.md** — multi-file composition
9. **stdlib.md** — what ships with the compiler
10. **repair.md** — how invalid source gets fixed before compilation
11. **pipeline.md** — the canonical compiler pipeline end-to-end
12. **diagnostics.md** — the diagnostic contract between Analyze and Repair
13. **compiled-output.md** — what the final Markdown looks like

## Relationship to Research

Design decisions build on top of the [agent-skill-dsl research](../research/agent-skill-dsl/). Consolidated wiki pages from research are promoted here as individual design files once they stabilise into decisions.
