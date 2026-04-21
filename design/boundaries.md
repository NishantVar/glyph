# Glyph Boundaries

This document records hard conceptual boundaries for Glyph so future design work does not drift away from the core system.

## Boundaries

1. **Glyph is not a prompt template system.**
   Glyph should model structure, flow, constraints, and contracts as language elements rather than as string interpolation wrapped around prose.

2. **Glyph is a language with a compiler, not a runtime.**
   Glyph's center of gravity is skill definition, analysis, transformation, visualization, and compilation. It should not grow into a long-lived agent execution runtime as its primary identity.

3. **Glyph is authoring-first, not orchestration-first.**
   Glyph should optimize the quality of individual skill definitions and their compiled output. Coordination features may exist around Glyph in the future, but orchestration should remain secondary to the language and compiler.

4. **Glyph should target agents broadly, with special care for current coding agents.**
   Compiled skills should be consumable by general-purpose agents rather than tied to one narrow execution environment. At the same time, the design should stay grounded in the needs and failure modes of today's coding agents.

5. **Glyph source is `.glyph.md`; MVP output is same-basename `.md`.**
   A Glyph source module should live in a `.glyph.md` file. The compiler may pass through a typed IR or JSON representation, but the MVP output is exactly one Markdown file per source file, formed by replacing `.glyph.md` with `.md`.

6. **Text reuse is not prompt templating.**
   Named text blocks, imported instruction libraries, semantic shortcuts, and inline quoted instructions are authoring conveniences that compile into structured IR nodes. They should not turn Glyph into arbitrary string interpolation around prose.

7. **LLM-assisted expansion is not language semantics.**
   A short instruction name may be expanded with LLM help when local, imported, and standard-library resolution fail, but in the MVP that expansion happens through the repair pass by materializing a stable generated definition. The result must be validated before output generation. The source language should not depend on unbounded runtime interpretation.

8. **Optional role markers should not dominate the source.**
   Explicit role and constraint markers may be useful disambiguators, but Glyph should not force authors to label every instruction manually when the compiler can infer the role, constraint strength, and constraint polarity cleanly. The MVP role vocabulary is defined in `ir-roles.md`.

9. **LLM repair is not semantic rewriting.**
   The LLM repair pass may fix invalid Glyph source by adding minimal syntax, declarations, annotations, or stable generated shorthand definitions, but it must preserve author-facing shorthand and readability. It should not inline shorthand as full prose at use sites, compile the skill directly, or strengthen weak intent into stronger behavior without evidence. On unchanged inputs, repair should be idempotent.

10. **Only explicit exports are importable.**
   Ordinary `block`s and non-exported `text` declarations are private to their source module. A block must be declared as `export block` before another `.glyph.md` file may import it, and exported blocks must be self-contained. A text declaration must be declared as `export text` before another file may import it.

11. **MVP imports are local-path based.**
   MVP imports should resolve through explicit file paths to `.glyph.md` source modules. Package-style, registry-backed, or versioned imports are future product features, not MVP requirements.

12. **Specialization is not unrestricted class inheritance.**
   Reuse for expert agents and similar definitions should happen through `abstract agent` bases, concrete `agent` definitions, declared slots, deterministic merge order, explicit compile-time `override`, `append`, and `prepend` operations, and version-aware base-agent contracts. Glyph should not rely on runtime inheritance, hidden method dispatch, or arbitrary replacement of inherited behavior. Locked inherited constraints must remain visible and preserved in the flattened agent definition.
