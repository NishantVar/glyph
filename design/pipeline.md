# Glyph Compiler Pipeline

This document is the single authoritative reference for the Glyph compiler pipeline: its phases, their ordering, inputs, outputs, and the Safety Sandwich pattern that bounds LLM-assisted passes with deterministic checks.

This supersedes the 5-pass diagram in `README.md`, the 9-step list in `language-surface.md` §5, and the prose summary in `foundations.md` #18. Those documents should reference this file for the canonical pipeline.

## Overview

The Glyph compiler has **seven phases** in two LLM-bounded stages:

```
Source (.glyph.md)
  → 1. Parse           (deterministic)
  → 2. Analyze         (deterministic)
  → 3. Repair          [LLM, bounded loop]
  → 4. Lower           (deterministic)
  → 5. Validate        (deterministic)
  → 6. Expand          [LLM, per-invocation]
  → 7. Emit            (deterministic)
Output (.md)
```

Phases 1-5 operate on source and produce a validated IR. They run once per source file and their output is cacheable. Phases 6-7 take the validated IR plus concrete invocation arguments and produce the compiled Markdown. They run fresh for each invocation with different arguments.

## Safety Sandwich

Every LLM-assisted phase is bounded by deterministic phases that check its work:

```
Deterministic [1. Parse + 2. Analyze]
  → LLM [3. Repair]
  → Deterministic [re-run 1+2, then 4. Lower + 5. Validate]
  → LLM [6. Expand]
  → Deterministic [7. Emit]
```

This is the "Safety Sandwich" pattern referenced in `foundations.md` #18: deterministic compiler passes own correctness; any LLM-assisted step runs inside those boundaries and is checked afterward.

## Phase 1: Parse (deterministic)

**Input:** Raw `.glyph.md` source text (one or more files).

**Output:** Loose source AST per file + import dependency DAG across files.

Parse turns raw text into structure without understanding meaning.

- Reads the `.glyph.md` file and tracks indentation levels (4-space units per `language-surface.md`).
- Rejects tabs and mixed indentation as hard errors.
- Identifies top-level declarations: `skill`, `block`, `export block`, `text`, `export text`, `int`, `float`, `import`, `generated text`, `generated block` (and their `export` variants where valid).
- Identifies declaration headers: name, parameters, return types.
- Identifies sub-section headers: `description:`, `flow:`, `effects:`, `constraints:`.
- Identifies body content: bare names, inline strings, calls with arguments, `with` modifiers on calls, `if`/`elif`/`else`, `return`, constraint markers (`require`, `avoid`, `prefer`, `must`).
- Handles paired delimiters for line continuation: `()`, `{}`, `"""`.
- Strips comments (`//`) but preserves their positions for repair.
- Produces a loose AST — names are unresolved, types are not checked, roles are not assigned. Purely structural.

**Multi-file:** When compiling multiple files, Parse reads every `import` path, resolves it to a file, and builds a directed acyclic graph (DAG) of file dependencies. Cycles are rejected here with a diagnostic naming the full cycle path (per `imports.md` §5). The DAG determines compilation order: leaves (files with no imports) compile first.

**What Parse does not do:** No name resolution, no type checking, no understanding of semantics. `avoid_unrelated_edits` is just an identifier at this point.

## Phase 2: Analyze (deterministic)

**Input:** Loose source AST from Phase 1.

**Output:** Annotated AST with inferred metadata + structured diagnostics.

Analyze tries to understand the source as deeply as it can using deterministic rules alone. Where it cannot figure something out, it emits a structured diagnostic.

- **Name resolution.** For every name in the source, checks in order: (1) same-file parameter or local binding, (2) same-file `text`/`int`/`float` declaration, (3) selectively imported name, (4) qualified name via whole-module import, (5) standard library entry (see `stdlib.md`). Unresolved names are marked and a repairable diagnostic is emitted.

- **Role inference.** For every instruction, determines its IR role (`InputContract`, `Step`, `Constraint`, `OutputContract` — the four MVP roles per `ir-and-semantics.md`). Uses the evidence chain: explicit marker → metadata from declarations → metadata from imports/stdlib → position (e.g., inside `flow:` implies `Step`) → compound-name cues (`avoid_*` implies Constraint). Emits a repairable diagnostic when role is ambiguous.

- **Constraint attribute inference.** For instructions identified as constraints, determines strength (`invariant`, `required`, `preferred`) and polarity (`require`, `avoid`) per the marker table in `ir-and-semantics.md` §2. Emits a diagnostic when ambiguous.

- **Type inference.** Traces types through the call graph. Where types cannot be inferred and are not needed for a call-boundary check, marks as untyped (acceptable in MVP per `types.md`).

- **Effect inference.** Walks the call graph and unions effects per `ir-and-semantics.md` §3. Checks that any author-declared `effects:` is a superset of the inferred set.

- **Closure checking.** For `export block` declarations, verifies closure requirements per `data-flow.md`.

- **Diagnostic classification.** Every issue is tagged:
  - `error` — hard stop, cannot continue (e.g., circular import, fundamentally broken syntax).
  - `repairable` — the LLM repair pass can likely fix this (e.g., unresolved bare name, ambiguous role, missing type annotation).
  - `warning` — non-blocking observation (e.g., over-declared effects).

**What Analyze does not do:** Does not change the source. Does not generate prose. Does not build the IR. Read-only analysis that produces diagnostics.

## Phase 3: Repair (LLM + deterministic, bounded loop)

**Input:** Original source + annotated AST + structured diagnostics from Phase 2.

**Output:** Repaired `.glyph.md` source written back to the file.

Repair makes the source valid so it can compile. It is not just a safety net — it is the **primary content generation mechanism for novice authors** (`foundations.md` #33, `repair.md` §1). A novice using only the kernel surface writes source that contains many undefined names and calls; repair materializes definitions for them.

Repair has two sub-steps that run together:

### 3a. Deterministic source rewrites (always run, no LLM)

Mechanical transformations where the correct fix is unambiguous:

- Body-level constraint hoisting: if `require foo` appears at body level without a `constraints:` wrapper, wrap it into a `constraints:` section (per `ir-and-semantics.md` §2).
- Duplicate import merging: two imports from the same file merge into one statement (per `imports.md` §6).
- Unused import removal: imported names not referenced anywhere in the file are removed (per `imports.md` §7).
- Source section reordering: sections within a declaration body are reordered to the recommended convention (per `ir-and-semantics.md` §4).

### 3b. LLM-assisted repair (only runs if repairable diagnostics remain after 3a)

The LLM receives the source, the remaining diagnostics, and the compiler's rules. It makes the smallest changes needed:

- **Unresolved bare names** (no parens at use site): generates a `generated text` definition — a single one-sentence expansion — and appends it to the end of the file.
- **Unresolved parens-calls** (parentheses at use site): generates a `generated block` definition — a single one-sentence body — and appends it to the end of the file. Parameters are inferred from the call site.
- **Ambiguous roles:** adds the minimal marker needed (e.g., adds `avoid` in front of an ambiguous instruction).
- **Missing type annotations:** adds `: Type` where inference failed and the compiler requires it.
- **Missing `description:`:** generates a one-line summary from the skill name and body content, adds it as a `description:` sub-section in the source.
- **Missing `effects:`:** infers effects from the call graph and adds an `effects:` sub-section.
- **Broken indentation or delimiters:** fixes structural issues that Parse flagged.
- **Marker addition when inference succeeds:** materializes the smallest explicit marker back into source when role/strength/polarity confidence is high.

Generated definitions follow the one-sentence rule: bodies stay close to the name's meaning and minimize drift from author intent. Full rules in `repair.md` §5.

### The repair loop

```
repeat (max 3 iterations):
    run 3a (deterministic rewrites)
    if repairable diagnostics remain after 3a:
        run 3b (LLM repair)
    re-run Phase 1 (Parse) on the modified source
    re-run Phase 2 (Analyze) on the re-parsed AST
    if zero repairable diagnostics: accept, exit loop
if still has repairable diagnostics after 3 iterations:
    fail with the remaining diagnostics for author to fix manually
```

**Why max 3:** Iteration 1 handles the bulk. Iteration 2 catches cascading issues — e.g., a generated definition introduces a new name that needs role inference, or splitting a compound name at a use site reveals a new unresolved concept name. Iteration 3 is the safety margin. If 3 rounds cannot converge, the source has a deeper problem that requires human intervention.

**Idempotence:** Running repair on already-valid source produces zero changes. The mechanism is name resolution: if every name resolves and every role is determined, there are no repairable diagnostics, so neither 3a nor 3b runs. See `repair.md` §4.5.

**What Repair does not do:** Does not produce compiled output. Does not expand shorthand into agent-facing prose at use sites. Does not restructure the skill's workflow. Does not add behavior the author did not imply (per `repair.md` §4.4, intent potency). Makes the source *valid*, not *polished*.

## Phase 4: Lower (deterministic)

**Input:** Valid, repaired source AST (passes Parse + Analyze with zero errors and zero repairable diagnostics).

**Output:** Typed IR — the strict internal representation that all later phases operate on.

Lower converts the human-friendly source AST into the strict IR. Every shortcut is resolved into its explicit form.

- **Named arguments only.** Positional call arguments are mapped to parameter names by declaration order. The IR contains only named arguments (per `data-flow.md`).
- **Flat calls only.** Nested calls like `validate(make_plan(ctx))` are desugared into sequential calls with compiler-generated temporary bindings (per `data-flow.md`).
- **Defaults filled.** Omitted optional parameters get their default values inserted.
- **Callee resolution.** Every call target is resolved to its full declaration — same-file block, imported export block, or standard library primitive.
- **Effect recording.** Every `Call` node gets its inferred effect set attached.
- **`with` modifier recording.** The `with` modifier string from each call site is stored on the `Call` IR node as `site_modifier` (per `data-flow.md`).
- **IR node construction.** Every source element becomes an explicit typed IR node:
  - `Call { target, args, output, return_type, effects, site_modifier }`
  - `InstructionRef { name, resolved_text, role, constraint_attrs }`
  - `InlineInstruction { text, role }`
  - `Return { value }`
  - `Branch { condition, then_body, elif_branches, else_body }`
  - `PropertyAccess { object, property }`
- **Section assignment.** Every IR node is assigned to its output location based on its role: `Step` → Steps, `Constraint` → Constraints, `InputContract` → folded into Steps at expand time, `OutputContract` → folded into final Step.

**What Lower does not do:** Does not generate prose. Does not validate correctness (that is Phase 5). Does not touch the source file. One-way transformation from source world to IR world.

## Phase 5: Validate (deterministic)

**Input:** Typed IR from Phase 4.

**Output:** The same IR (unchanged) if valid, or hard errors if not.

Validate is the final correctness gate before any LLM touches the IR.

- **Completeness.** Every name resolves to a definition. Every call has a matching declaration. Every type is assigned (or explicitly untyped, which is allowed in MVP).
- **Type matching.** At every call boundary, if both sides have type annotations, the names must match per `types.md` (nominal matching). If either side is untyped, no check.
- **Effect validation.** If the author declared `effects:`, the declared set must be a superset of the inferred set. Otherwise compile error (per `ir-and-semantics.md` §3).
- **Return path completeness.** Every `export block` must have an explicit `return` on every code path. Private blocks and skills may implicitly return `none` (per `data-flow.md`).
- **Skill body non-empty.** A `skill` must have at least `constraints:` or `flow:` (or both). Empty skill body is an error (per `ir-and-semantics.md` §4).
- **Effect `none` exclusivity.** `effects: none, reads_files` is an error (per `ir-and-semantics.md` §3).
- **Constraint well-formedness.** Constraint attributes have valid strength and polarity.

**What Validate does not do:** Does not change the IR. Does not generate output. Pure pass/fail gate.

## Phase 6: Expand (LLM, per-invocation)

**Input:** Validated IR from Phase 5 + concrete argument values for the skill's parameters.

**Output:** Expanded IR — every node carries its final agent-facing prose.

Expand is where the structured IR becomes readable agent instructions. Repair made the source *valid*. Expand makes the output *useful*.

Expand is **per-invocation**: it receives concrete argument values (e.g., `scope = "auth"`) and produces compiled Markdown in which every parameter has been resolved into prose. Different argument sets produce different compiled files. The `.glyph.md` source is the reusable artifact; the compiled `.md` is a specialization for one use (per `compiled-output.md`).

**What Expand does:**

- **Step expansion.** Each `Step` node gets a full prose instruction sentence.
  - A call like `inspect_failure(scope)` with `scope = "auth"` expands into prose that mentions "the auth module" — concrete, no variable references.
  - A `with` modifier on the call (e.g., `with "focus on auth boundaries"`) shapes the expanded wording. The modifier is consumed and does not appear in output.
  - A bare name reference like `validate_before_success` that resolves to `generated text` uses that text as-is (deterministic, no LLM needed).
  - An inline string like `"Don't propose a fix until you've confirmed the root cause."` is used as-is (deterministic).

- **Constraint expansion.** Each `Constraint` node gets wording shaped by its strength and polarity.
  - `Constraint(strength: required, polarity: avoid)` renders as a prohibition: "Do not make changes outside the requested scope."
  - `Constraint(strength: invariant, polarity: require)` renders with strongest possible wording.
  - `Constraint(strength: preferred, polarity: require)` renders with softer wording ("When possible, ..." or "Prefer ...").

- **Effect set finalization.** The full inferred effect set is prepared for frontmatter output as a YAML list.

- **Description generation.** If `description:` is still missing after repair (unlikely but possible), generates one from the skill name and IR body.

- **Return folding.** The `return` expression becomes the closing sentence of the final numbered step (per `compiled-output.md`). No separate output section.

- **Conditional flattening.** `if`/`elif`/`else` branches are turned into prose conditional instructions within steps.

- **Parameter resolution.** Every parameter is resolved to its concrete value. No `{param}` placeholders survive. A surviving placeholder is a compile error (per `compiled-output.md`).

**What is deterministic vs. LLM in Expand:**

Not everything needs the LLM:
- Bare names that resolve to full prose `text` or `generated text` → used as-is (deterministic).
- Inline strings → used as-is (deterministic).
- Effect keyword lists → passed through (deterministic).

The LLM handles:
- Call-node expansion into natural-language step instructions.
- `with` modifier consumption and wording specialization.
- Constraint rewording based on strength and polarity.
- Parameter value weaving into step prose.
- Conditional flattening into prose.
- Return folding into the final step.

**What Expand does not do:** Does not change the source file. Does not alter the IR's structure (roles, types, effects, call graph). Only adds prose content to existing nodes.

## Phase 7: Emit (deterministic)

**Input:** Expanded IR from Phase 6.

**Output:** Compiled `.md` file.

Emit is pure formatting. The IR has all the content; Emit arranges it into the fixed Markdown template defined by `compiled-output.md`.

- **YAML frontmatter.** Three fields:
  - `name` — from the skill declaration name.
  - `description` — from the skill's `description:` sub-section (generated by Repair or Expand if omitted).
  - `effects` — YAML flow-sequence list of the full inferred effect set. Field omitted entirely when effects are `none` or empty.

- **`## Instructions`** — the single H2 section. Always emitted. Contains:
  - `### Steps` — numbered list, one expanded instruction per item. Order matters. The final item includes the return-summary sentence. Conditional: omitted only for pure constraint-only skills.
  - `### Constraints` — bulleted list, one expanded constraint per item. Order usually does not matter. Conditional: omitted when there are no constraints.
  - At least one of `### Steps` or `### Constraints` must be present.

- **Authoring construct erasure.** All imports, text references, `generated text`/`generated block` markers, comments, module paths, `with` modifiers, parameter names — everything from the authoring world is gone. The compiled file is fully self-contained (per `compiled-output.md`).

- **Formatting rules** (per `compiled-output.md`):
  1. One instruction per list item (except the final Step, which may include the return sentence).
  2. Numbered lists for Steps, bulleted lists for Constraints.
  3. No hard line-wrapping mid-sentence.
  4. Single blank line between sections.
  5. Standard Markdown only.

- **File output.** Same-basename `.md`. E.g., `fix_bug.glyph.md` → `fix_bug.md`.

**What Emit does not do:** No LLM involvement. No content generation. No decisions about what to say. If Expand did its job, Emit is trivial.

## Multi-File Compilation Order

When compiling multiple `.glyph.md` files that import each other:

1. **Phase 1 (Parse)** builds the import dependency DAG across all files and topological-sorts it. Cycles are rejected as hard errors.

2. **Leaves compile first.** Files with no imports go through the full pipeline first.

3. **Dependency readiness.** An importing file cannot enter Phase 2 (Analyze) until the imported file has passed Phase 5 (Validate). The importer needs the dependency's validated IR for name resolution, type matching, and effect propagation.

4. **Expand/Emit can parallelize.** A dependency's Phases 6-7 (Expand + Emit) can run in parallel with the importer's Phases 1-5, since importers only need the validated IR, not the compiled output.

5. **Multi-file repair.** Repair may edit files other than the current one when diagnostics require it (per `repair.md` §9). If this changes a dependency, that dependency must re-run from Phase 1. The same max-3-iteration bound applies across the whole build. If multi-file repair does not converge in 3 rounds, fail with diagnostics.

## Visualization

Visualization is not a pipeline phase. It is a **separate output path** that branches off after Phase 5 (Validate):

```
                              ┌──→ 6. Expand → 7. Emit → compiled .md
5. Validate ──→ validated IR ─┤
                              └──→ Graph renderer → visual output
```

The graph renderer reads the validated IR and projects it as a data-flow graph: parameters are entry nodes, calls are operation nodes, bindings are value edges, returns are exit nodes, effects are annotations on call nodes, branches are decision nodes (per `data-flow.md`).

The graph renderer does not need Expand's prose. It works with the structural IR directly. The output format (JSON, DOT, Mermaid, etc.) is a tooling decision, not a pipeline decision. The pipeline's only obligation is that the validated IR is a clean, well-structured format that supports graph projection (`foundations.md` #31).

## Reconciliation

This document reconciles three prior descriptions:

| README (5-pass) | language-surface.md §5 (9-step) | This document (7-phase) |
|---|---|---|
| Parse | 1. Parse | **1. Parse** |
| Analyze | 2. Diagnose | **2. Analyze** |
| — | 3. Repair | **3. Repair** (loop includes re-parse + re-analyze) |
| — | 4. Re-parse | (inside Repair loop) |
| Transform | 5. Resolve, 6. Infer, 7. Normalize, 8. Type | **4. Lower** |
| Expand [LLM] | (not in the 9-step list) | **6. Expand** (per-invocation, distinct from Repair) |
| Validate | 9. Validate | **5. Validate** |
| Output | (implicit) | **7. Emit** |

Key clarifications this reconciliation makes:

- The README's "Expand [LLM]" is **Phase 6 (Expand)**, the per-invocation pass that turns IR into agent-facing prose. It is distinct from the README's implied single-LLM-pass model — there are two LLM passes (Repair and Expand), each bounded by deterministic checks.
- The 9-step list's steps 5-8 (Resolve, Infer, Normalize, Type) all happen inside **Phase 4 (Lower)** as sub-operations of the source-to-IR conversion.
- The 9-step list does not mention an LLM expand pass; this document adds it as Phase 6.
- The README does not mention repair as a separate pass; this document makes it explicit as Phase 3 with a bounded loop.

## Source-To-Source vs. IR-Only Transforms

| Transform | Phase | Touches `.glyph.md`? |
|---|---|---|
| Body-level constraint → `constraints:` section | 3a | Yes |
| Unused import removal | 3a | Yes |
| Duplicate import merging | 3a | Yes |
| Section reorder to convention | 3a | Yes |
| `generated text` materialization | 3b | Yes |
| `generated block` materialization | 3b | Yes |
| Missing `description:` generation | 3b | Yes |
| Missing `effects:` generation | 3b | Yes |
| Role/constraint marker addition | 3b | Yes |
| Positional → named args | 4 | No (IR only) |
| Nested call desugaring | 4 | No (IR only) |
| Default value filling | 4 | No (IR only) |
| Effect propagation (union) | 4 | No (IR only) |
| `with` modifier recording | 4 | No (IR only) |
| Parameter resolution to concrete values | 6 | No (in-memory) |
| `with` modifier consumption | 6 | No (in-memory) |
| Return folding into final step | 6 | No (in-memory) |
| Conditional flattening to prose | 6 | No (in-memory) |

## Cacheability

| Phases | Cacheable? | Key |
|---|---|---|
| 1-5 (Parse through Validate) | Yes | Source file content hash + import content hashes |
| 6-7 (Expand + Emit) | Per-invocation | Source hash + concrete argument values |

Phases 1-5 produce a validated IR that does not depend on invocation arguments. If the source file and its imports have not changed, the validated IR can be reused across invocations. Phases 6-7 must run fresh whenever arguments change, since the compiled output is a specialization for those specific values.

Incremental compilation and build caching are **deferred** from MVP (per `todo.md`). The pipeline design supports them by separating argument-independent phases (1-5) from argument-dependent phases (6-7), but the MVP compiler may re-run all phases on every invocation.

## Cross-References

- **Foundations:** `foundations.md` — #18 (deterministic passes own correctness), #33 (novice learnability).
- **Source syntax:** `language-surface.md` — declaration forms, indentation, sub-sections. §5 lists the 9-step pipeline that this document supersedes.
- **IR and roles:** `ir-and-semantics.md` — four MVP roles, constraint model, effects, section vocabulary.
- **Repair:** `repair.md` — full repair rules, generated definitions (text and block), one-sentence rule, idempotence, intent potency.
- **Data flow:** `data-flow.md` — parameters, calls, `with` modifier, control flow, return semantics, closure.
- **Compiled output:** `compiled-output.md` — frontmatter shape, `## Instructions` structure, projection rules, per-invocation model.
- **Imports:** `imports.md` — path resolution, cycle rejection, effect propagation, multi-file compilation order.
- **Types:** `types.md` — nominal matching at call boundaries.
- **Standard library:** `stdlib.md` — MVP stdlib entries and their effect signatures.
