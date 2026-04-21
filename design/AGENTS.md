# Glyph — Design

This folder is the working source of truth for Glyph system design — flat because this **is** the main design (not a per-topic subfolder). The repository [README](../README.md) describes the project at a high level; design decisions are captured here.

## Document Roles

### Foundations (strongest statements)
- [principles.md](principles.md) — stable design principles that shape all downstream decisions
- [boundaries.md](boundaries.md) — hard conceptual boundaries that keep the system from drifting

### Language Shape (active decisions)
- [authoring-surface.md](authoring-surface.md) — human-facing `.glyph.md` source forms, MVP top-level declarations, one-file-to-one same-basename `.md` output, exported text/blocks, path-based imports, semantic shortcuts, inline instructions, and duck-typed authoring
- [data-flow-and-calls.md](data-flow-and-calls.md) — how skills, private blocks, exported blocks, calls, parameters, local bindings, returns, duck typing, closure, and effects pass values through Glyph
- [effects.md](effects.md) — MVP effect vocabulary (8 keywords), `effects:` clause syntax, inference and propagation semantics, `none` handling, compiled-output surfacing, and extension policy
- [ir-roles.md](ir-roles.md) — MVP instruction role taxonomy: `InputContract`, `Step`, `Constraint`, `Context`, `OutputContract`, with constraint strength and polarity attributes
- [values-and-literals.md](values-and-literals.md) — MVP primitive values: strings (inline and block, no interpolation), numbers (int and float with lossless coercion), booleans, `none`, identifier rules (case-normalized, no shadowing), and name resolution
- [comments.md](comments.md) — MVP comment syntax: `//` line comments, no block comments, Markdown-safe, stripped from compiled output, preserved by repair
- [compiled-output.md](compiled-output.md) — compiled `.md` file shape: YAML frontmatter (`name`, `description`), fixed section order (Effects, Inputs, Instructions, Output, When To Use), H3 sub-sections for Steps/Constraints, formatting rules, import inlining, and authoring-construct erasure
- [llm-repair-pass.md](llm-repair-pass.md) — source-to-source LLM repair for invalid Glyph files before deterministic IR compilation
- [specialization-and-inheritance.md](specialization-and-inheritance.md) — post-MVP compile-time specialization for expert agents using `agent`, `abstract agent`, slots, overrides, appends, prepends, locked constraints, and optional traits

## How To Read These Docs

- Start with `principles.md` and `boundaries.md`.
- Then read `authoring-surface.md` for the current source-language direction.
- Read `data-flow-and-calls.md` for value passing, function-like calls, and explicit data flow.
- Read `effects.md` for the MVP effect vocabulary, syntax, inference semantics, and extension policy.
- Read `ir-roles.md` for the closed MVP instruction role set and constraint strength/polarity model.
- Read `values-and-literals.md` for primitive values, identifier rules, and name resolution.
- Read `compiled-output.md` for the compiled `.md` file shape, section ordering, formatting rules, and how authoring constructs are erased.
- Read `llm-repair-pass.md` for how invalid but readable source is repaired before compilation.
- Read `specialization-and-inheritance.md` for post-MVP inheritance-like reuse through explicit specialization rather than general class inheritance.
- The rest of the spec will be rebuilt from scratch around these decisions.

## Current Posture

- `principles.md` and `boundaries.md` are the strongest statements in the folder.
- `authoring-surface.md` captures the first concrete language-shape decisions for the rebuilt spec, including `.glyph.md` source modules, one-file-to-one same-basename `.md` output, path-based imports, exported text/blocks, and the five MVP top-level declarations: `import`, `text`, `export block`, `block`, and `skill`.
- `data-flow-and-calls.md` captures the first concrete contract for parameters, calls, local variables, return values, exported-block closure, and effects.
- `effects.md` captures the MVP effect vocabulary (8 keywords), `effects:` clause syntax, compiler inference and propagation, and additive extension policy.
- `ir-roles.md` captures the closed MVP role set for instruction intent, keeps effects separate from roles, and defines constraint strength/polarity plus conservative repair inference for `always`.
- `values-and-literals.md` captures the MVP primitive value surface: strings, numbers, booleans, `none`, identifier normalization, no-shadowing rule, and name resolution.
- `compiled-output.md` captures the compiled `.md` file shape: YAML frontmatter (`name`, `description`), fixed section order (Effects → Inputs → Instructions → Output → When To Use), H3 sub-sections (`### Steps`, `### Constraints`), formatting rules, import inlining with source auto-fix for unused imports, and complete authoring-construct erasure.
- `llm-repair-pass.md` captures the source-preserving repair contract for compiler-blocking issues.
- `specialization-and-inheritance.md` captures a post-MVP reuse model for expert-agent variants.
- Earlier first-pass drafts (language core, semantics, types & effects, constraints, IR, compiler, output, validation, gap checklist) were removed and will be rebuilt.

## Relationship to Research

Design decisions build on top of the [agent-skill-dsl research](../research/agent-skill-dsl/). Consolidated wiki pages from research are promoted here as individual design files once they stabilise into decisions.
