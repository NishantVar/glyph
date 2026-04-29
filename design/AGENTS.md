# Glyph — Design

**Do not read files in `archive/`.** That directory contains superseded proposals kept only for historical reference. The active design is entirely in the files listed below.

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
- [ir-and-semantics.md](ir-and-semantics.md) — IR roles (`InputContract`, `Step`, `Constraint`, `OutputContract`), constraint keywords (`require`/`avoid`/`must`, 4-form model with `soft`/`hard` strength), effects (9 keywords, propagation, validation), section vocabulary

### Modules
- [imports.md](imports.md) — path-based import resolution, importable vs. private declarations, cycle rejection, effect propagation across boundaries
- [stdlib.md](stdlib.md) — standard library: MVP contains three entries (`subagent`, `send`, `load`), the `Agent` compiler-known type, UFCS method-call syntax, the `spawns_agent` effect, and the uniform synthetic-body projection model

### IR
- [ir-schema.md](ir-schema.md) — **canonical IR node schema**: every node type (`Skill`, `Block`, `ExportBlock`, `Call`, `Constraint`, `InlineInstruction`, `InstructionRef`, `Branch`, `Return`), enums (`Role`, `Strength`, `Polarity`, `EffectKeyword`, `TypeTag`, `Value`), resolved IR shape, node identifier spec
- [ir-json-schema.md](ir-json-schema.md) — **canonical IR JSON serialization**: top-level envelope, per-node-kind JSON shapes, enum casing (all snake_case), Expression/Value unions, versioning policy, worked example. Contract for both `--emit-ir` (produces) and `validate-output` (consumes).

### Compilation
- [pipeline.md](pipeline.md) — **canonical compiler pipeline**: 7 phases (Parse, Analyze, Repair, Lower, Validate, Expand, Emit), Safety Sandwich, multi-file order, cacheability
- [diagnostics.md](diagnostics.md) — structured diagnostic shape, classification tiers (`error`/`repairable`/`warning`), ID scheme, representative catalog
- [repair.md](repair.md) — LLM repair pass, generated definitions (text and block), comment syntax, intent potency, idempotence
- [expand.md](expand.md) — Expand pass Step 2 (LLM reshaping) and Phase 6b validation gate: input schema, output contract, role-preservation check, retry / deterministic-fallback / hard-fail policy, non-idempotence
- [compiled-output.md](compiled-output.md) — compiled Markdown shape: YAML frontmatter, `## Parameters` (conditional), `## Instructions` (`### Steps` + `### Constraints`), parameterless compilation model

### CLI
- [cli.md](cli.md) — **v0 CLI surface**: subcommands (`compile`, `check`, `fmt`, `validate-output`), flags (`--emit-ir`, `--out-dir`, `--format`), exit codes (0/1/2/3 agent-oriented), diagnostic channel discipline, multi-file behavior, deferred features

### Agent
- [agent-skill.md](agent-skill.md) — **agent skill design**: workflow state machine, repair guidance, constraint conflict scan, Step 2 prose reshaping, `validate-output` subcommand (Phase 6b), IR JSON schema reference

### Meta
- [todo.md](todo.md) — deferred design items, open questions, and post-MVP feature tracking

### MVP
- [build-foundation.md](build-foundation.md) — **Rust implementation foundation**: two-crate workspace, hand-rolled parser, span/arena types, sync-only architecture, error/diagnostic channels, CLI contract, agent workflow, dependency inventory
- [mvp-acceptance.md](mvp-acceptance.md) — walking skeleton (`update_docs.glyph.md`), test corpus structure, 5-skill multi-file acceptance project, 75 compiler-scope + 11 agent-scope diagnostic IDs, exit criteria

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
