# Glyph Foundations

Design principles and hard boundaries for Glyph, consolidated into a single reference card.
Each item is 1-2 sentences; detailed rules live in the linked design docs.

---

## Identity

1. **Glyph is a language with a compiler, not a runtime.** Its center of gravity is skill definition, analysis, transformation, visualization, and compilation -- not long-lived agent execution.

2. **Glyph is not a prompt template system.** Structure, flow, constraints, and contracts are language elements, not string interpolation wrapped around prose.

3. **Glyph is authoring-first, not orchestration-first.** The language optimizes individual skill quality; coordination features are secondary.

4. **Glyph targets agents broadly, with special care for current coding agents.** Compiled skills should be consumable by general-purpose agents, grounded in today's coding-agent failure modes.

## Readability and Authoring

5. **Easy readability is a primary product goal.** Skills should read like small structured programs, not prose prompts; hierarchy, flow, and constraints should be obvious at a glance.

6. **Easy maintenance is a primary product goal.** Authors should be able to name, export, import, and shorthand repeated instruction text without copying prose blocks through every skill.

7. **Use Python-like readability, not Python runtime semantics.** Borrow indentation, low punctuation, and duck-typed ergonomics; do not inherit arbitrary runtime execution or hidden side effects.

8. **Optional markers should not dominate the source.** Explicit role and constraint markers are available for disambiguation, but the compiler should infer role, strength, and polarity when it can (see `ir-and-semantics.md`).

## Source vs. Compiled Output

9. **Authoring and execution are separate.** Source Glyph exists for humans; compiled output exists for agents. The compiler handles flattening, expansion, defaults, and target-specific generation.

10. **Compiled output is parameterized, with tiered self-containment.** Compilation is parameterless — `glyph compile skill.glyph` produces one `.md` file per skill, with parameters as named `{param}` slots resolved by the consuming LLM at runtime. Simple skills are fully self-contained in one file. Complex skills may reference separately compiled procedure files for imported blocks that are large, conditional, or shared across skills — the compiler decides the projection tier (inline, same-file procedure section, or external procedure file) based on callee complexity, conditionality, and reuse. See `compiled-output.md` for the three-tier projection model.

11. **Reliability beats elegance in compiled output.** Compiled agent instructions favor explicitness and followability over compression.

## Language Core

12. **The core language is intentionally small.** MVP top-level declarations: `import`, `const`, `export block`, `block`, `skill`. Interior primitives: `flow`, `call`, `if`, constraints, `return`. See `language-surface.md`.

13. **The source language is forgiving; the IR is strict.** Authors may omit annotations and use duck-typed values; compilation normalizes permissive source into an explicit, typed IR.

14. **Instruction roles are inferred by default.** The compiler infers role, strength, and polarity from context; explicit markers are available for disambiguation (see `ir-and-semantics.md`).

15. **Text reuse is not prompt templating.** Named text, imported libraries, and semantic shortcuts compile into structured IR nodes, not arbitrary string interpolation (see `language-surface.md`).

## IR and Semantics

16. **The IR is the real semantic contract.** Parsing, analysis, normalization, visualization, and validation operate on a typed IR, not prompt text. See `ir-and-semantics.md`.

17. **Invalid states should be unrepresentable in the IR where possible.** The compiler uses types, enums, and structured forms to rule out malformed programs before output generation.

18. **Deterministic compiler passes own correctness.** Parsing, typing, normalization, and validation are deterministic; any LLM-assisted step runs inside those boundaries and is checked afterward.

## Constraints and Effects

19. **Constraints are first-class and phase-aware.** Constraints are represented explicitly in the language and IR with strength (`soft`/`hard`) and polarity (`require`/`avoid`), not buried in prose (see `ir-and-semantics.md`).

20. **Data flow and effects must be explicit.** Skills and blocks declare inputs, outputs, and effects; hidden ambient context is minimized (see `ir-and-semantics.md`, `data-flow.md`).

21. **Function-like calls pass values explicitly.** Blocks and primitives accept parameters, return values, and bind locals; the IR normalizes every call into explicit target, arguments, output, type, and effects (see `data-flow.md`).

## Control Flow

22. **Control flow is finite and analyzable.** Glyph prefers bounded iteration and explicit branching over unrestricted recursion or highly dynamic execution (see `data-flow.md`).

## Modularity and Reuse

23. **Only explicit exports are importable.** Ordinary `block`s and non-exported `const` are private; `export block` and `export const` are required for cross-file reuse, and exported blocks must be self-contained (see `imports.md`).

24. **The abstraction hierarchy is `skill` > `export block` > private `block`.** A `skill` is the compiled public unit; an `export block` is the importable reusable unit; a private `block` is internal structure (see `language-surface.md`).

25. **Self-containment means declared dependencies, not total isolation.** A unit is self-contained if its behavior is determined by declared inputs, local bindings, explicit imports, declared constraints, outputs, and effects -- it can be analyzed and tested independently.

26. **MVP imports are local-path based.** Package-style, registry-backed, or versioned imports are future work (see `imports.md`).

27. **Specialization is deferred.** Skill inheritance and reuse beyond imports are post-MVP (see `todo.md`).

## LLM Repair

28. **LLM repair is source-preserving, compiler-bounded, and idempotent.** Repair adds minimal syntax to make invalid source compile; it preserves shorthand, names, ordering, and readability. See `repair.md`.

29. **LLM-assisted expansion is not language semantics.** Bare-name expansion via LLM happens through the repair pass by materializing stable generated definitions; the source language does not depend on unbounded runtime interpretation (see `repair.md`).

30. **Repair respects intent potency.** Repair may clarify existing intent but must not strengthen weak intent into a hard requirement without evidence (see `repair.md`).

## Visualizability and Tooling

31. **Visualizability is a language constraint.** Every skill must support source view, graph/workflow view, and compiled-output view; constructs that cannot be represented across all three likely do not belong in the early language.

32. **Tooling and testing are part of language design.** Every feature must justify how it is parsed, type-checked, visualized, transformed, and tested; features difficult to validate are too vague for the core language.

## Learnability

33. **Novice learnability is a first-principles goal.** A new author should be able to write a useful skill using a small kernel: `skill`, `require`/`avoid`/`must`, `flow:`, quoted inline strings, calls with parentheses, and the `with` modifier. Every other construct (blocks, named constants, types, effects, imports) must be discoverable later or inferred by the compiler and repair pass; the novice surface must not require learning them up front.
