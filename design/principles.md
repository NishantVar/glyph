# Glyph Design Principles

This document records the current design principles for Glyph.
Together with the rest of `design/`, it is part of the working source of truth for system design.

## Principles

1. **Easy readability is a primary product goal.**
   Skills should read like small structured programs rather than prose prompts. Hierarchy, flow, and constraints should be obvious at a glance.

2. **Easy maintenance is a primary product goal.**
   Authors should be able to name repeated instruction text, explicitly export reusable text or blocks, import local source modules, and use concise shorthand without copying large prose blocks through every skill.

3. **The source language should be forgiving, while the IR should be strict.**
   Authors may omit many annotations, use duck-typed values, and rely on names whose expansion is resolved later. Compilation must normalize that permissive source into an explicit, typed intermediate representation before agent-facing output is generated.

4. **Use Python-like readability and duck-typed ergonomics, not Python runtime semantics.**
   Glyph should borrow indentation, low punctuation, simple bindings, and structural compatibility from Python. It should not inherit arbitrary runtime execution, hidden side effects, or unconstrained dynamic behavior.

5. **Instruction roles should be inferred by default.**
   Authors should be able to write bare instruction names or inline text without always adding explicit role markers. Bare instruction names are allowed in the MVP. The compiler should infer the right role, constraint strength, and constraint polarity from context, imported metadata, and expansion content, while explicit markers remain available for disambiguation. The MVP role vocabulary is defined in `ir-roles.md`.

6. **LLM repair should be source-preserving, compiler-bounded, and idempotent.**
   LLM repair is part of the MVP. When forgiving source fails to compile, an LLM may repair the Glyph source by adding the minimum syntax needed for deterministic compilation. It should preserve shorthand, names, ordering, and readability. Undefined bare names may get stable generated definitions during the repair pass, but repair should not inline expanded prose at use sites or reinterpret the skill. Repair must follow intent potency: it may clarify existing intent, but it must not strengthen weak intent into a hard requirement without evidence. Running repair twice on unchanged inputs should produce no further changes.

7. **Keep the core language intentionally small.**
   The MVP source surface should start with a small set of base declaration kinds: `import`, value-binding kinds (`text`, `int`, `float`), `block`, and `skill`. `export` is a visibility modifier on value-binding and block kinds (`export text`, `export int`, `export float`, `export block`) and `generated` is a repair-authorship modifier on `text` (`generated text`). Inside those declarations, Glyph should stay focused on constrained primitives such as `flow`, `call`, `if`, constraints, and `return`. `for_each` is deferred beyond the MVP. Additional declaration kinds (`bool`, `agent`, etc.) may be added later only with strong justification.

8. **Authoring and execution must remain separate.**
   Source Glyph exists for humans. Compiled output exists for agents. The language should express intent and structure, while the compiler handles flattening, expansion, default resolution, and target-specific instruction generation.

9. **The IR is the real semantic contract.**
   Glyph should compile into a typed intermediate representation before producing agent-facing output. Parsing, analysis, normalization, visualization, and validation should operate on the IR rather than on prompt text.

10. **Constraints must be first-class and phase-aware.**
   Constraints should eventually be represented explicitly in the language and IR rather than buried in prose. Some constraints are statically checkable, some guide runtime behavior, some are validated after execution, and some have preferred strength rather than required strength. The MVP role model defines constraint strength and polarity; the exact domain constraint vocabulary and syntax can still be designed separately.

11. **Deterministic compiler passes should own correctness.**
   Parsing, typing, normalization, and validation should be deterministic. Any LLM-assisted expansion should run inside those boundaries and be checked afterward by deterministic validation.

12. **Data flow and effects must be explicit.**
   Skills and blocks should declare their inputs, outputs, and meaningful effects. Hidden ambient context should be minimized because it makes behavior harder to analyze, test, and visualize. The MVP effect vocabulary should be coarse and extensible rather than exhaustive.

13. **Function-like calls should pass values explicitly.**
    Blocks and primitive operations may accept parameters, return values, and bind local variables. Source-level duck typing is allowed, but the IR should normalize every call into explicit target, arguments, output binding, type, and effects.

14. **Reuse should prefer explicit specialization over unrestricted inheritance.**
    Expert agents and similar reusable behavior should be specialized through `abstract agent` bases, concrete `agent` definitions, named slots, explicit `override`/`append`/`prepend` operations, deterministic slot merge order, and compile-time flattening. Derived agents should only change declared extension points. Locked inherited constraints must compile into preserved constraint identities, and derived agents should track compatible base-agent versions or fingerprints.

15. **MVP compiled output should be a single same-basename Markdown file.**
    Source files should use the `.glyph.md` extension. In the MVP, each source file compiles to exactly one Markdown agent-instruction file by replacing the `.glyph.md` suffix with `.md`; for example, `skill.glyph.md` compiles to `skill.md`, and `x.glyph.md` compiles to `x.md`. A typed IR or JSON form may exist between source and output, but it is an internal compiler contract rather than the main user-facing artifact.

16. **Reliability beats elegance in compiled output.**
   Source Glyph can be concise and readable, but compiled agent instructions should favor explicitness, clarity, and followability over elegance or compression.

17. **Visualizability is a language constraint.**
    Skills should support at least three coherent views: source/code view, graph or workflow view, and compiled agent-output view. If a construct cannot be represented clearly across those views, it likely does not belong in the early language.

18. **Control flow should remain finite and analyzable.**
    Glyph should prefer bounded iteration and explicit branching over unrestricted recursion or highly dynamic execution. The language should stay easy to inspect statically.

19. **Importable modularity requires closed exported blocks.**
    Ordinary `block`s are private implementation details. Only `export block`s may be imported by other files, and every exported block must be self-contained: its behavior must be determined by declared inputs, local bindings, explicit imports, declared constraints, declared outputs, and declared effects.

20. **The primary abstraction boundary is `skill`, then `export block`, then private `block`.**
    A `skill` is the public unit that compiles into agent-facing Markdown. An `export block` is the importable reusable unit and must be closed. A private `block` is the internal unit of structure, reuse, and testing inside a file. Primitive `call`s can represent lower-level capabilities or runtime operations.

21. **Invalid states should be unrepresentable in the IR where possible.**
    The source may be permissive for authors, but the compiler should use types, enums, and structured forms to rule out malformed programs before output generation instead of relying on informal conventions or prompt wording.

22. **Tooling and testing must be part of the language design.**
    Every feature should justify how it is parsed, type-checked, visualized, transformed, and tested. If a feature is difficult to validate or explain, it is likely too vague for the core language.

## Boundary For Self-Containment

The right boundary for "self-contained" is not that every function or block must run meaningfully in complete isolation.

A unit is self-contained enough if:

- Its behavior is determined by declared inputs, local bindings, explicit imports, declared constraints, declared outputs, and declared effects.
- It can be analyzed independently.
- It can be tested in isolation with supplied inputs and stubbed dependencies where necessary.

Pure or near-pure helper blocks may be truly standalone. Effectful exported blocks can still be self-contained if they declare the effects they rely on or perform. Private blocks do not need to be importable or standalone, but hidden ambient context should still be minimized.
