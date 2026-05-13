# Glyph — Design

This folder holds **language and product design** for Glyph. The audience is a language/product designer asking *"What can a Glyph author write? What does it mean? Why is the language shaped this way?"* — not someone working on the compiler internals.

If you need to know how the compiler is built, see [`../docs/`](../docs/) instead:

- [`../docs/reference/`](../docs/reference/) — stable contracts (CLI, diagnostics, IR JSON, compiled-output shape, MVP acceptance)
- [`../docs/architecture/`](../docs/architecture/) — durable maintainer architecture (compiler pipeline, repair, expand, IR schema, IR semantics, LSP, tree-sitter, agent-skill, walking-skeleton, diagnostics rationale)
- [`../docs/adr/`](../docs/adr/) — decision records for non-obvious implementation choices
- [`../todo/`](../todo/) — bugs, implementation TODOs, migration chores

**Do not read files in `archive/`.** That directory contains superseded proposals kept only for historical reference. The active design is entirely in the files listed below.

## Documents

### Foundations
- [[foundations]] — stable design principles and hard conceptual boundaries (reference card)
- [[primitives]] — five semantic primitives that all Glyph source forms decompose into: instruction, constraint, context, interface, and binding

### Language
- [[language-surface]] — source syntax: declarations (`skill`, `block`, `export block`, `const`, `import` and their `export`/`generated` variants), header grammar, indentation, sub-section syntax, authoring model
- [[values-and-names]] — primitive values, identifiers, reserved keywords, name resolution, no-shadowing rule
- [[data-flow]] — parameters, bindings, calls, arguments, control flow (`if`/`elif`/`else`), return semantics, closure and scope
- [[types]] — semantic domain types (no author-facing primitives), implicit type declaration, nominal matching
- [[preferences]] — preferences as ordinary exported constants (`export const`), no `pref(...)` call, no special effect, compile-time resolution
- [[capabilities]] — post-MVP design for capability-based composition: skills own main flow; capabilities provide named operations, contracts, and policy without implicit top-level execution

### Semantics
- [[ir-and-semantics]] — author-visible IR roles (`InputContract`, `Step`, `Constraint`, `Context`, `OutputContract`), constraint keywords (`require`/`avoid`/`must`, 4-form model with `soft`/`hard` strength), `context` marker, effects (9 keywords, propagation, validation), section vocabulary

### Modules
- [[imports]] — path-based import resolution, importable vs. private declarations, cycle rejection, effect propagation across boundaries
- [[stdlib]] — standard library: MVP contains three entries (`subagent`, `send`, `load`), the `Agent` compiler-known type, UFCS method-call syntax, the `spawns_agent` effect, and the uniform synthetic-body projection model

### Author-Facing Compiler Behavior
- [[design/repair]] — what Repair may change in source, what it must preserve, how generated definitions appear, idempotence as a contract. *(Maintainer-facing repair internals live in [`../docs/architecture/repair.md`](../docs/architecture/repair.md).)*
- [[design/expand]] — what authors can rely on about the Expand pass: role-preservation, non-idempotence. *(Validation algorithm and retry mechanics live in [`../docs/architecture/expand.md`](../docs/architecture/expand.md).)*
- [[design/compiled-output]] — compiled Markdown shape: YAML frontmatter, `## Parameters` (conditional), peer-level body H2s (`## Context` + `## Steps` + `## Constraints`), parameterless compilation model. *(The downstream-consumer contract is mirrored in [`../docs/reference/compiled-output.md`](../docs/reference/compiled-output.md).)*

### CLI & Editor (user-facing surface)
- [[design/cli]] — high-level CLI design and the agent-oriented exit-code mental model. *(The exact subcommand/flag/exit-code contract lives in [`../docs/reference/cli.md`](../docs/reference/cli.md).)*
- [[glyph-lsp]] — what an author/editor user sees from the language server: diagnostics, go-to-def, configuration. *(LSP implementation architecture lives in [`../docs/architecture/lsp.md`](../docs/architecture/lsp.md).)*

### Meta / Forward-Looking
- [[todo]] — deferred design items, open author-facing questions, post-MVP feature tracking
- [[todo_evolution]] — priority list for evolving Glyph into a contract-centered agent language
- [[user-facing-todo]] — post-MVP author-facing language ideas, including `goal:` and richer `output:` contracts

## Reading Order

1. **foundations.md** — philosophy and hard limits at a glance
2. **language-surface.md** — what Glyph source looks like
3. **values-and-names.md** — the value and naming system
4. **data-flow.md** — how data moves through skills and blocks
5. **types.md** — the type system
6. **preferences.md** — how user/project preferences work
7. **ir-and-semantics.md** — author-visible IR roles, constraints, and effects
8. **imports.md** — multi-file composition
9. **stdlib.md** — what ships with the compiler
10. **repair.md** — what Repair may change about author source
11. **compiled-output.md** — what the final Markdown looks like

## Relationship to Research

Design decisions build on top of the [agent-skill-dsl research](../research/agent-skill-dsl/). Consolidated wiki pages from research are promoted here as individual design files once they stabilise into decisions.
