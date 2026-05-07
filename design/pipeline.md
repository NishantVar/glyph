# Glyph Compiler Pipeline

This document is the single authoritative reference for the Glyph compiler pipeline: its phases, their ordering, inputs, outputs, and the Safety Sandwich pattern that bounds LLM-assisted passes with deterministic checks.

This supersedes the 5-pass diagram in `README.md`, the 9-step list in `language-surface.md` §5, and the prose summary in `foundations.md` #18. Those documents should reference this file for the canonical pipeline.

## Overview

The Glyph compiler has **seven phases** in two LLM-bounded stages:

```
Source (.glyph)
  → 1. Parse           (deterministic)
  → 2. Analyze         (deterministic)
  → 3. Repair          [LLM, bounded loop]
  → 4. Lower           (deterministic)
  → 5. Validate        (deterministic)
  → 6. Expand          [deterministic + LLM]
  → 7. Emit            (deterministic)
Output (.md)
```

All seven phases operate once per source file. Phases 1-5 produce a validated IR; Phases 6-7 take the validated IR and produce the compiled Markdown. Compilation is parameterless — parameters appear in the compiled output as named slots that the consuming LLM resolves from context at runtime.

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

**Input:** Raw `.glyph` source text (one or more files).

**Output:** Loose source AST per file + import dependency DAG across files.

Parse turns raw text into structure without understanding meaning.

- Reads the `.glyph` file and tracks indentation levels (4-space units per `language-surface.md`).
- Flags tabs and mixed indentation as repairable diagnostics (repair may auto-fix to 4-space indentation).
- Identifies top-level declarations: `skill`, `block`, `export block`, `const`, `export const`, `import`, `generated const`, `generated block` (and their `export` variants where valid).
- Identifies declaration headers: name, parameters, return types.
- Identifies sub-section headers: `description:`, `flow:`, `effects:`, `constraints:`.
- Identifies body content: bare names, inline strings, calls with arguments (including UFCS calls like `x.foo(args)`), `with` modifiers on calls, `if`/`elif`/`else`, `return`, constraint markers (`require`, `avoid`, `must`).
- Validates the shape of the block trigger predicate `BLOCKNAME.applies(...)` when it appears in an `if` / `elif` condition (per `ir-and-semantics.md` §Block Trigger Predicate, `data-flow.md` §Condition Expressions): emits `G::parse::applies-no-parens` (error) if `applies` appears without `()`, and `G::parse::applies-with-args` (error) if any arguments are passed. Resolution of the receiver to a block (vs. a non-block) is left to Phase 2.
- Handles paired delimiters for line continuation: `()`, `{}`, `"""`.
- Strips comments (`//`) but preserves their positions for repair.
- Produces a loose AST — names are unresolved, types are not checked, roles are not assigned. Purely structural.

**Multi-file:** When compiling multiple files, Parse reads every `import` path, resolves it to a file, and builds a directed acyclic graph (DAG) of file dependencies. Cycles are rejected here with a diagnostic naming the full cycle path (per `imports.md` §5). The DAG determines compilation order: leaves (files with no imports) compile first.

**Bail at first parse error.** Within a single file, Phase 1 stops at the first parse error and emits exactly one diagnostic. There is no error recovery, no skip-to-next-declaration, and no multi-error collection across the file. The agent (Phase 3) sees one diagnostic per Phase 1 invocation; subsequent parse problems surface in the next compile after repair. The reasoning: Glyph source is small, repair is cheap to re-invoke, and a noisy multi-error parse output frequently misleads repair when later "errors" are merely cascade artifacts of the first.

**Pre-Parse stratum exemption.** The pre-Parse text-rewrite stratum (`cli.md` §`glyph fmt`) — tab → 4-space conversion and mixed-indentation fixes — is **not** subject to bail-at-first because it operates at the lexer/source-text level before any AST exists. Those repairs are batched: every tab line and every mixed-indent line in the file is normalised in a single pre-Parse pass. Only AST-level parsing follows the one-error-then-stop rule.

**What Parse does not do:** No name resolution, no type checking, no understanding of semantics. `avoid_unrelated_edits` is just an identifier at this point.

## Phase 2: Analyze (deterministic)

**Input:** Loose source AST from Phase 1.

**Output:** Annotated AST with inferred metadata + structured diagnostics.

Analyze tries to understand the source as deeply as it can using deterministic rules alone. Where it cannot figure something out, it emits a structured diagnostic.

- **Name resolution.** For every name in the source, checks in order: (1) same-file parameter or local binding, (2) same-file `const` declaration, (3) selectively imported name, (4) qualified name via whole-module import, (5) standard library entry (see `stdlib.md`). Unresolved names are marked and a repairable diagnostic is emitted.

- **UFCS disambiguation.** For dot-syntax calls (`x.foo(args)`), determines whether the left side is a whole-module import alias (→ qualified callee), a value binding/parameter (→ UFCS call, desugared to `foo(x, args)` in Lower), or a `block` / `export block` declaration (or `module_alias.block_name`) with method `applies` and zero arguments (→ block trigger predicate, not UFCS — kept as a special syntactic form on the Branch's condition string per `ir-and-semantics.md` §Block Trigger Predicate). See `data-flow.md` §UFCS.

- **Block trigger predicate resolution.** For every `BLOCKNAME.applies()` invocation in a Branch condition (top-level or `elif`), Analyze resolves `BLOCKNAME` against the same name-resolution chain as ordinary call targets and validates the kind: a non-block target (e.g., a `const`, parameter, or alias) emits `G::analyze::applies-on-non-block` (error); a block target without a `description:` sub-section emits `G::analyze::applies-on-undescribed-block`, classified `repairable` when the block is in the same file under compilation and `error` when the block is imported (matching `repair.md` §9 single-file scope). Resolution succeeds without rewriting the condition string — the side-map population happens at Expand Step 1.

- **Role inference.** For every instruction, determines its IR role (`InputContract`, `Step`, `Constraint`, `Context`, `OutputContract` — the five MVP roles per `ir-and-semantics.md`). Uses the evidence chain: explicit marker → metadata from declarations → metadata from imports/stdlib → position (e.g., inside `flow:` implies `Step`) → compound-name cues (`avoid_*` implies Constraint). Emits a repairable diagnostic when role is ambiguous.

- **Constraint attribute inference.** For instructions identified as constraints, determines strength (`soft`, `hard`) and polarity (`require`, `avoid`) per the marker table in `ir-and-semantics.md` §2. Emits a diagnostic when ambiguous.

- **Type inference.** Traces types through the call graph. Where types cannot be inferred and are not needed for a call-boundary check, marks as untyped (acceptable in MVP per `types.md`).

- **Effect inference.** *(Gated — requires `--enable-effects`; skipped when the flag is off.)* Walks the call graph and unions effects per `ir-and-semantics.md` §3. Checks that any author-declared `effects:` is a superset of the inferred set.

- **Closure checking.** For `export block` declarations, verifies closure requirements per `data-flow.md`.

- **Diagnostic classification.** Every issue is tagged:
  - `error` — hard stop, cannot continue (e.g., circular import, fundamentally broken syntax).
  - `repairable` — the LLM repair pass can likely fix this (e.g., unresolved bare name, ambiguous role, missing type annotation).
  - `warning` — non-blocking observation (e.g., over-declared effects).

**What Analyze does not do:** Does not change the source. Does not generate prose. Does not build the IR. Read-only analysis that produces diagnostics.

## Phase 3: Repair (LLM + deterministic, bounded loop)

**Input:** Original source + annotated AST + structured diagnostics from Phase 2.

**Output:** Repaired `.glyph` source written back to the file.

Repair makes the source valid so it can compile. It is not just a safety net — it is the **primary content generation mechanism for novice authors** (`foundations.md` #33, `repair.md` §1). A novice using only the kernel surface writes source that contains many undefined names and calls; repair materializes definitions for them.

Repair has three sub-steps:

### 3a. Deterministic source rewrites (always run, no LLM)

Mechanical transformations where the correct fix is unambiguous:

- Unconditional constraint hoisting: constraint markers (`require`/`avoid`/`must`/`must avoid`) that appear at **body level** (directly under a declaration, outside any sub-section) or at **flow top-level** (directly inside `flow:`, not inside a `Branch`) are moved into a `constraints:` section (creating it if needed). This is a source-to-source rewrite — it normalizes the source so that all unconditional constraints are visually grouped in `constraints:`. Constraint markers inside `if`/`elif`/`else` branch bodies are **not** source-rewritten — they are conditional, remain as flow statements, and are handled at IR level by Phase 4 (Lower). See `ir-and-semantics.md` §Body-Level Constraint Normalization and §Flow-Level Constraint Markers. Note: when Phase 3b (LLM repair) adds a constraint marker to resolve an ambiguous role (`G::analyze::ambiguous-role`), the next iteration's 3a pass picks up the newly-marked constraint and hoists it — this is the expected two-iteration cascade.
- Unconditional context hoisting: `context` markers that appear at **body level** or at **flow top-level** are moved into a `context:` section (creating it if needed), parallel to constraint hoisting. Context markers inside `if`/`elif`/`else` branch bodies are not source-rewritten. See `ir-and-semantics.md`.
- Duplicate import merging: two imports from the same file merge into one statement (per `imports.md` §6).
- Unused import removal: imported names not referenced anywhere in the file are removed (per `imports.md` §7).
- Source section reordering: sections within a declaration body are reordered to the recommended convention (per `ir-and-semantics.md` §4).
- Missing `effects:` auto-fill: *(gated — requires `--enable-effects`; skipped when the flag is off)* when a declaration omits `effects:` entirely and the inferred set (from the call graph) is non-empty, 3a inserts an `effects:` sub-section with the inferred set and emits `G::repair::inferred-effects` (warning, informational). This is deterministic — the call graph is fully resolved by Phase 2. Applies uniformly to skills, blocks, and export blocks (per `ir-and-semantics.md` §3).

### 3b. LLM-assisted repair (only runs if repairable diagnostics remain after 3a)

The LLM receives the source, the remaining diagnostics, and the compiler's rules. It makes the smallest changes needed:

- **Unresolved bare names** (no parens at use site): generates a `generated const` definition — a single-string expansion — and appends it to the end of the file.
- **Unresolved parens-calls** (parentheses at use site): generates a `generated block` definition — a single-string body — and appends it to the end of the file. Parameters are inferred from the call site.
- **Ambiguous roles:** adds the minimal marker needed (e.g., adds `avoid` in front of an ambiguous instruction).
- **Missing type annotations:** adds `: Type` where inference failed and the compiler requires it.
- **Missing `description:`:** generates a one-line summary from the skill name and body content, adds it as a `description:` sub-section in the source.
- **Broken indentation or delimiters:** fixes structural issues that Parse flagged.
- **Marker addition when inference succeeds:** materializes the smallest explicit marker back into source when role, strength, and polarity confidence is high.

Generated definitions follow the single-string rule: bodies stay close to the name's meaning and minimize drift from author intent. Full rules in `repair.md` §5.

### 3c. Constraint conflict scan (LLM, runs once after 3a/3b convergence per declaration with ≥2 constraints)

After 3a and 3b have stabilized the source (no remaining `repairable` diagnostics), Phase 3c runs a focused LLM judgment pass over each declaration's `constraints:` set. It does **not** modify source; it emits diagnostics:

- **`G::repair::constraint-tension`** (warning) — two constraints are in friction but both reasonable to hold (e.g., "be thorough" + "be efficient"). Build proceeds; both constraints survive into compiled output.
- **`G::repair::constraint-contradiction`** (error) — two constraints cannot both be satisfied. Compilation fails; the author must edit one. The compiler does not silently drop a constraint.

3c runs once per declaration with ≥2 constraints (skill-level and block-level scanned independently). Cross-scope pairs (callee scoped constraints vs. caller top-level) are intentionally **not** scanned — those compose deliberately. Full rules in `repair.md` §4.10.

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
    hard fail — no .md emitted, non-zero exit, residual diagnostics surfaced to author

# After loop converges, run 3c once per declaration with ≥2 constraints
for each declaration D with len(D.constraints) >= 2:
    run 3c (constraint conflict scan) on D
    if any contradiction diagnostic: hard fail — no .md emitted, non-zero exit
    tension diagnostics emitted as warnings, build proceeds
```

**Hard fail on non-convergence.** If repairable diagnostics remain after 3 iterations, the compiler does not emit a compiled `.md` file. It exits with a non-zero status and surfaces the residual diagnostics on stderr for the author to fix manually. The source file retains whatever partial repairs succeeded in earlier iterations (since Repair writes back to `.glyph` after each accepted iteration). The author sees the residual diagnostics, fixes them, and recompiles. The compiler never emits a compiled file from source that still has `repairable` diagnostics.

**Why max 3:** Iteration 1 handles the bulk. Iteration 2 catches cascading issues — e.g., a generated definition introduces a new name that needs role inference, or splitting a compound name at a use site reveals a new unresolved concept name. Iteration 3 is the safety margin. If 3 rounds cannot converge, the source has a deeper problem that requires human intervention. The bound is a practical limit, not a convergence guarantee — source with deeper dependency chains (e.g., a generated definition that introduces a call to another undefined name, which itself implies a role that requires a third repair) may require author intervention between compiles.

**Idempotence:** Running repair on already-valid source produces zero changes. The mechanism is name resolution: if every name resolves and every role is determined, there are no repairable diagnostics, so neither 3a nor 3b runs. See `repair.md` §4.5.

**What Repair does not do:** Does not produce compiled output. Does not expand shorthand into agent-facing prose at use sites. Does not restructure the skill's workflow. Does not add behavior the author did not imply (per `repair.md` §4.4, intent potency). Makes the source *valid*, not *polished*.

## Phase 4: Lower (deterministic)

**Input:** Valid, repaired source AST (passes Parse + Analyze with zero errors and zero repairable diagnostics).

**Output:** Typed IR — the strict internal representation that all later phases operate on.

Lower converts the human-friendly source AST into the strict IR. Every shortcut is resolved into its explicit form.

- **UFCS desugaring.** UFCS calls like `x.foo(args)` are desugared to `foo(x, args)` — the receiver becomes the first positional argument (per `data-flow.md` §UFCS).
- **Named arguments only.** Positional call arguments (including UFCS receivers) are mapped to parameter names by declaration order. The IR contains only named arguments (per `data-flow.md`).
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
  - `ContextNode { name, resolved_text, role }`
  - `Branch { condition, then_body, elif_branches, else_body }`
  - `PropertyAccess { object, property }`
- **Section assignment.** Every IR node is assigned to its output location based on its role: `Step` → Steps, `Constraint` → Constraints, `Context` → Context section, `InputContract` → folded into Steps at expand time, `OutputContract` → folded into final Step.
- **Body-level and flow-level constraint hoisting.** Constraint markers that appear at body level (outside any sub-section) or as flow statements (per `ir-schema.md` §Flow Nodes) are split by location: a Constraint at body level or flow top-level is hoisted out and appended to the enclosing declaration's `constraints` list (deduplicated by canonical text + polarity + strength); a Constraint inside any `Branch` body (`then_body`, `elif_branches[*].body`, or `else_body`) stays inline in that branch and renders as part of the conditional Step prose at Expand time. This IR-level hoisting is the compiler's internal normalization and runs regardless of whether `glyph fmt` (Phase 3a) already performed the equivalent source-to-source rewrite. See `ir-and-semantics.md` §Body-Level Constraint Normalization, §Flow-Level Constraint Markers, and `compiled-output.md` §Constraint Rendering.
- **Body-level and flow-level context hoisting.** The same hoisting applies to `context` markers: body-level and flow-top-level context markers are hoisted into the declaration's `context` list; branch-scoped context markers stay inline. See `ir-and-semantics.md` for full rules.
- **Stable IR node IDs.** Lower assigns each IR node a stable identifier (e.g., `n0`, `n1`, …) used for Phase 6b structural validation and diagnostic messages. The format, allocation order, scope, stability guarantees, and synthetic-node policy are defined in `ir-schema.md` §Node Identifiers (canonical spec). The ID is opaque, file-local, and never appears in compiled output.

**What Lower does not do:** Does not generate prose. Does not validate correctness (that is Phase 5). Does not touch the source file. One-way transformation from source world to IR world.

## Phase 5: Validate (deterministic)

**Input:** Typed IR from Phase 4.

**Output:** The same IR (unchanged) if valid, or hard errors if not.

Validate is the final correctness gate before any LLM touches the IR.

- **Completeness.** Every name resolves to a definition. Every call has a matching declaration. Every type is assigned (or explicitly untyped, which is allowed in MVP).
- **Type matching.** At every call boundary, if both sides have type annotations, the names must match per `types.md` (nominal matching). If either side is untyped, no check.
- **Effect validation.** If the author declared `effects:`, the declared set must be a superset of the inferred set. Otherwise compile error (per `ir-and-semantics.md` §3).
- **Effect propagation across imports and inlines.** A caller's declared `effects:` must be a superset of every imported callee's declared effects and every inlined private callee's inferred effects. Per `data-flow.md` §Effect Propagation, this is a hard error, not a warning. Repair (Phase 3) may add the missing effect keywords when confidence is high.
- **Closure boundary.** Closure of `export block` is enforced once per file at the export boundary, not transitively across imports. An importer sees only the imported callee's declared contract (parameters, return type, `effects:`, `constraints:`); private declarations in the imported file are invisible to the importer (per `data-flow.md` §Closure Across Imports).
- **Return path completeness.** Every `export block` must have an explicit `return` on every code path. Private blocks and skills may implicitly return `none` (per `data-flow.md`).
- **Skill body non-empty.** A `skill` must have at least `constraints:` or `flow:` (or both). Empty skill body is an error (per `ir-and-semantics.md` §4).
- **Effect `none` exclusivity.** `effects: none, reads_files` is an error (per `ir-and-semantics.md` §3).
- **Constraint well-formedness.** Constraint attributes have valid strength (`soft`/`hard`) and polarity (`require`/`avoid`).

- **Post-Lower IR invariants.** Validate also confirms the IR shape Lower produced is internally consistent. All five are errors and never elided (see `diagnostics.md` Validate phase):
  - **Stable IR node IDs are unique within a file.** Lower assigns these; Validate confirms no collisions (`G::validate::duplicate-node-id`).
  - **Every `Call` node's callee resolves post-Lower.** Sanity check after Lower's UFCS desugaring and branch extraction; closes the call graph (`G::validate::unresolved-callee`).
  - **Branch IR has the shape Phase 6b expects.** Every `Branch` has at least an `if` arm and arm bodies are well-formed (`G::validate::malformed-branch`).
  - **No recursive calls within a file.** The local block-to-block call graph is acyclic (recursion is forbidden in MVP) (`G::validate::recursive-call`).
  - **Every Step-projecting IR node has non-empty body text.** No silently-empty Steps (`G::validate::empty-step`).

**What Validate does not do:** Does not change the IR. Does not generate output. Pure pass/fail gate.

## Phase 6: Expand (deterministic + LLM)

**Input:** Validated IR from Phase 5.

**Output:** Expanded IR — every node carries its final agent-facing prose, with parameter references preserved as `{param}` slots.

Expand is where the structured IR becomes readable agent instructions. Repair made the source *valid*. Expand makes the output *useful*.

Compilation is **parameterless**: Expand does not receive concrete argument values. Parameters appear in the compiled output as named slots (e.g., `{scope}`) listed in a `## Parameters` section with descriptions and optional defaults. The consuming LLM resolves them from user context at runtime. The `.glyph` source is the authoring artifact; the compiled `.md` is a stable, single artifact per source file (per `compiled-output.md`).

### Two-step expansion model

Expand has a strict internal ordering: **deterministic resolution first, then LLM reshaping**. This separation is architecturally important — it produces a concrete intermediate artifact between the two steps that is inspectable, debuggable, and independent of LLM behavior.

#### Step 1: Deterministic resolution (no LLM)

All mechanical substitutions happen first, before any LLM is involved:

- **Parameter reference preservation.** `{param}` placeholders in `generated block` bodies and parameter references in the IR are **not** substituted — they are preserved as named slots for the consuming LLM to resolve at runtime. `"Inspect the failure in {area}"` stays as `"Inspect the failure in {area}"`.
- **Local binding reference tagging.** `{name}` slots that resolve to local bindings (e.g., `{diagnosis}` where `diagnosis = analyze_error(...)`) are tagged as `local_ref` on the resolved IR node. Unlike parameter references, local-ref slots are **not** preserved as literal `{name}` tokens — Step 2 resolves them into natural-language cross-references in the compiled prose. The consuming LLM already produced the referenced value in a prior step and does not need a placeholder for its own output.
- **Parameter metadata assembly.** For each parameter in the skill's `InputContract`, Step 1 collects the name, type annotation (if any), and default value (if any) into a parameter list for the `## Parameters` section.
- **Bare name inlining.** Bare names that resolve to `const` or `generated const` are replaced with their string content as-is.
- **Inline string passthrough.** Inline strings like `"Don't propose a fix until you've confirmed the root cause."` pass through unchanged.
- **Effect keyword passthrough.** The inferred effect set is prepared for frontmatter as a YAML list.
- **Block projection tier assignment.** For each `Call` node targeting a block, Step 1 selects a projection tier (inline, same-file procedure, or external file) based on callee complexity, conditionality, and reuse. The tier is stored on the `ResolvedCall` node as `projection_mode`. For `same_file_procedure` and `external_file` tiers, Step 1 also attaches the callee's resolved flow nodes and constraints to the `ResolvedCall`. See `compiled-output.md` §Three-Tier Block Projection for the heuristic.
- **Block trigger predicate resolution.** For each `Branch` IR node whose `condition` (or any `elif_branches[*].condition`) contains a `BLOCKNAME.applies()` invocation, Step 1 reads the referenced block's resolved `description:` and populates the Branch's `applies_descriptions: {block_name → resolved_description}` side-map (per `ir-schema.md` §Resolved IR `ResolvedBranch`, `ir-json-schema.md` §Branch). The condition string itself is preserved unchanged — only the side-map is added. Step 2 reads the side-map to choose between the pure-applies "decide which applies" form and the mixed-condition inline form (per `compiled-output.md` §Description-Driven Branch Projection).

After Step 1, every IR node has resolved content — bare names and inline strings are concrete, `{param}` references for declared parameters remain as named slots, and `{name}` references to local bindings are tagged as `local_ref` for Step 2 to resolve into prose. An unresolved bare name after this step is a compile error. A `{name}` slot whose name does not resolve to either a declared parameter or a local binding in scope is also a compile error. The result is a **resolved IR** that could theoretically be emitted as-is (it would be correct but stilted).

#### Step 2: Deterministic emitter + LLM span fill (when wired)

Step 2 is split into two layers (see `expand.md` §1 and §3.5):

1. **Deterministic emitter (always runs).** Walks the resolved IR and produces a typed Markdown *scaffold* — `Scaffold { chunks: Vec<Chunk> }` where every chunk is either a literal Markdown string or a typed `Span` placeholder. The scaffold owns all deterministic structure: section headers, list numbering, the locked four-form constraint template, the `Identifier`-form return-fold suffix, pure-`applies()` Branch projection (all three sub-cases per `compiled-output.md` §Description-Driven Branch Projection), and the external-file Call Step template (`Load and follow the procedure in \`{procedure_path}\`.`).
2. **Span fill (LLM when wired; stub today).** Each `SpanKind` (`ParamDescription`, `DescriptionReturnFold`, `BranchCondition`, `CallBodyShape`) carries the IR context the filler needs. The full per-span LLM contract is enumerated in `llm_expand_pass.md`. The merger substitutes fills into the scaffold to produce the final Markdown.

What the LLM (when wired) produces, by `SpanKind`:

- **`CallBodyShape`** — Step prose that weaves the resolved body text, the `site_modifier` (the `with "…"` clause), scoped constraints, and local-binding cross-references. The literal modifier string and `{name}` tokens for local refs must not survive.
- **`BranchCondition`** — natural-language prose for a mixed-condition `if`/`elif` arm header (e.g., `block_x.applies() and not is_dry_run` → `If the user wants a structured plan and this is not a dry run:`). Pure-`applies()` Branches and `Otherwise:` headers are emitted deterministically.
- **`DescriptionReturnFold`** — a Step-shaped paraphrase of `OutputContract.Description("…")` text inside the locked Description-suffix wrapper. The `Identifier` form is folded deterministically.
- **`ParamDescription`** — a brief description of each parameter, slotted into the deterministically-scaffolded `## Parameters` bullet.

**Conditional LLM invocation.** Skills with no spans bypass the LLM entirely — the deterministic emitter produces complete, byte-stable Markdown for those skills. The LLM is invoked per span, not per skill; failed spans retry in isolation (`expand.md` §5.3) without re-flowing the deterministic structure.

**Description generation.** `description:` should always be present after Repair (Phase 3 generates it if the author omitted it). If Repair fails to converge (e.g., `G::analyze::missing-description` persists after 3 iterations), the build hard-fails via `G::repair::no-convergence` — the missing description never reaches Expand. There is no separate `G::validate::missing-description` diagnostic; the Repair convergence check is the safety net.

### How `with` works: the modifier as a reshaping prompt

The `with` modifier is the **only call-site specialization mechanism in MVP**. It lets an author reuse the same block definition across multiple call sites, producing different prose each time without creating separate blocks.

The modifier is a short natural-language prompt that the Expand LLM applies to the resolved body text. It does not change the callee's parameters, effects, constraints, or return type — it only adjusts the wording of the expanded Step.

**Mechanical sequence for a `with`-modified call:**

```
1. Lower (Phase 4) stores the modifier string on the Call IR node as site_modifier.
2. Expand Step 1 preserves {param} references in the callee's body text.
   → Resolved body: "Inspect the failure in {area} and identify what is failing."
   (where {area} maps to the skill-level parameter passed at the call site)
3. Expand Step 2 sends the resolved body + site_modifier to the LLM.
   → LLM prompt (conceptual): "Here is an instruction: 'Inspect the failure in {area}
      and identify what is failing.' Reshape it with this emphasis: 'focus on auth
      boundaries'. Preserve {param} references. Produce a single instruction sentence."
   → LLM output: "Inspect the failure in {area}, focusing on auth boundaries
      and permission checks. Identify what is failing and whether any auth-related
      logic is involved."
```

The same call without `with` would skip the reshaping prompt and produce output closer to the resolved body text.

**Example: same block, three different `with` modifiers.**

Source:

```glyph
skill security_audit(repo = ".")
    flow:
        inspect_failure(repo) with "focus on authentication and authorization"
        inspect_failure(repo) with "focus on input validation and injection vectors"
        inspect_failure(repo) with "focus on secrets and credential handling"
        return summarize_changes()
```

After repair, `inspect_failure` has the same generated definition:

```glyph
generated block inspect_failure(area)
    "Inspect the failure in {area} and identify what is failing."
```

Compiled output:

```md
## Parameters
- **repo**: Repository or service to audit

### Steps

1. Inspect {repo} for authentication and authorization issues. Check session management, role-based access controls, and token validation for weaknesses.
2. Inspect {repo} for input validation and injection vulnerabilities. Check all user-facing inputs, database queries, and command construction for injection vectors.
3. Inspect {repo} for secrets and credential handling. Check for hardcoded keys, unencrypted storage, leaked tokens in logs, and insecure credential rotation practices.
4. Summarize what was found and why, and return that as your result.
```

Each call expands differently because the `with` modifier steers the LLM's reshaping. The resolved body text is identical for all three (`"Inspect the failure in {repo} and identify what is failing."`) — the modifier is what differentiates them.

**Example: `with` on a hand-written block.**

`with` is not limited to generated blocks. It works on any call, including hand-written blocks and imported blocks:

```glyph
export block review_code(files) -> ReviewResult
    effects: reads_files

    flow:
        scan(files)
        check_patterns(files)
        return compile_report(files)
```

Two skills that import and call `review_code` with different emphasis:

```glyph
// In security_review.glyph
skill security_review(scope = ".")
    flow:
        files = gather_files(scope)
        report = review_code(files) with "prioritize security vulnerabilities and unsafe patterns"
        return report

// In performance_review.glyph
skill performance_review(scope = ".")
    flow:
        files = gather_files(scope)
        report = review_code(files) with "prioritize hot paths, unnecessary allocations, and O(n²) patterns"
        return report
```

Same block, same parameters, same effects — but each compiled skill gets review instructions shaped for its specific concern. The `with` modifier is the only thing that differs, and it produces meaningfully different agent behavior.

**Example: call without `with` vs. call with `with`.**

```glyph
skill quick_fix(scope = ".")
    flow:
        inspect_failure(scope)
        return summarize_changes()
```

Compiled output (no `with` modifier):

```md
## Parameters
- **scope**: Area of codebase to focus on (default: ".")

### Steps

1. Inspect the failure in {scope} and identify what is failing.
2. Summarize what was changed and why, and return that as your result.
```

Compare to the earlier `fix_bug` example where the same call carries `with "focus on auth boundaries"`:

```md
### Steps

1. Inspect the failure in {scope}, focusing on auth boundaries and permission checks. Identify what is failing and whether any auth-related logic is involved.
...
```

Without `with`, the output hews closely to the resolved body text. With `with`, the LLM reshapes the prose to emphasize the modifier's concern. The modifier is the author's lever for controlling expansion specificity without touching the block definition.

**What Expand does not do:** Does not change the source file. Does not alter the IR's structure (roles, types, effects, call graph). Only adds prose content to existing nodes.

## Phase 7: Emit (deterministic)

**Input:** Expanded IR from Phase 6.

**Output:** Compiled `.md` file.

Emit is pure formatting. The IR has all the content; Emit arranges it into the fixed Markdown template defined by `compiled-output.md`.

- **YAML frontmatter.** Three fields:
  - `name` — from the skill declaration name.
  - `description` — from the skill's `description:` sub-section (generated by Repair or Expand if omitted).
  - `effects` — YAML flow-sequence list of the full inferred effect set. Field omitted entirely when effects are `none` or empty.

- **`## Parameters`** — emitted when the skill declares parameters. Contains a bulleted list of parameter names, descriptions (generated by Step 2), and either a default value or a `(required)` marker per parameter (see `compiled-output.md` §`## Parameters`). Omitted for parameterless skills.

- **`## Instructions`** — always emitted. Contains:
  - `### Context` — bulleted list, one background item per bullet. Passive framing the agent should understand during execution. Conditional: omitted when no context is declared.
  - `### Steps` — numbered list, one expanded instruction per item. Order matters. The final item includes the return-summary sentence. Conditional: omitted only for pure constraint-only skills.
  - `### Constraints` — bulleted list, one expanded constraint per item. Order usually does not matter. Conditional: omitted when there are no constraints.
  - `### Procedure: <name>` — zero or more procedure sections for blocks projected at the same-file procedure tier. Each contains the callee's expanded flow as a numbered list with an optional constraint preamble. See `compiled-output.md` §Three-Tier Block Projection.
  - At least one of `### Steps` or `### Constraints` must be present. `### Context` is supplementary and does not satisfy this requirement alone.

- **Authoring construct erasure.** All imports, const references, `generated const`/`generated block` markers, comments, module paths, `with` modifiers — everything from the authoring world is gone. Parameter names survive only as `{param}` references in Steps/Constraints and as entries in the `## Parameters` section. The compiled file is self-contained for Tier 1/2 projections; Tier 3 projections retain procedure file paths as runtime references (per `compiled-output.md` §Three-Tier Block Projection).

- **Formatting rules** (per `compiled-output.md`):
  1. One instruction per list item (except the final Step, which may include the return sentence).
  2. Numbered lists for Steps, bulleted lists for Context and Constraints.
  3. No hard line-wrapping mid-sentence.
  4. Single blank line between sections.
  5. Standard Markdown only.

- **File output.** Same-basename `.md` for skills. E.g., `fix_bug.glyph` → `fix_bug.md`. For external-file procedure projections, Emit also writes standalone procedure `.md` files to a subdirectory named after the source file (e.g., `review_tools.glyph` with `export block review_code` → `review_tools/review-code.md`). Procedure files carry `kind: procedure` in frontmatter to distinguish from top-level skills.

- **Atomic rename on disk.** Both compiled artifacts are written through a temp-then-rename pattern: Phase 7 first writes to `foo.md.tmp` and (when `--emit-ir` is set) `foo.ir.json.tmp`, then renames each to its final path only after the entire pipeline succeeds for that file. On hard-fail anywhere in Phases 1–7, the `.tmp` files are deleted and any **prior** `foo.md` / `foo.ir.json` on disk is left untouched. The same rule applies uniformly to compiled Markdown and emitted IR JSON. This is the per-file half of the partial-failure policy (see §Partial Failure Policy below for the multi-file build-level rules).

- **Startup cleanup of stale `.tmp` siblings.** At the very start of Phase 7, before writing any new `.tmp` file, the compiler scans the output paths it is about to write and deletes any pre-existing `.tmp` siblings (`foo.md.tmp`, `foo.ir.json.tmp`, and the `.tmp` companions of any procedure files this build will emit). This handles leftovers from a prior run that crashed, was SIGINT-killed, or was otherwise terminated between writing a `.tmp` and renaming it. There is no lockfile and no separate process supervisor — the sweep is idempotent (deleting a non-existent `.tmp` is a no-op) and self-contained per file. A `.tmp` belonging to a *different* output path (one this build is not about to produce) is left untouched.

- **Library file emission.** A library file (zero `skill` declarations) runs through the same Emit phase. Since there is no skill to project, Emit produces no skill-level `.md` file. However, each `export block` in the library whose expanded prose is >= 150 words (above the Tier 1 inline threshold; see `compiled-output.md` §Three-Tier Block Projection) emits a standalone procedure `.md` file into a subdirectory named after the library source file (e.g., `repo_tools.glyph` with `export block inspect_repo` → `repo_tools/inspect-repo.md`). Export blocks below the threshold and all `export const` values emit nothing — they contribute to consumers only through the validated IR. A library that produces zero `.md` files compiles successfully with no output; this is normal, not an error (see `language-surface.md` §File-Level Rules). Sibling exports within a single library file are visited in **source order** (top-to-bottom as they appear in the `.glyph`); this fixes diagnostic ordering and on-disk write order for reproducibility. Forward references between sibling exports are legal — same-file blocks have no declaration-order requirement (`data-flow.md`).

**What Emit does not do:** No LLM involvement. No content generation. No decisions about what to say. If Expand did its job, Emit is trivial.

## Multi-File Compilation Order

When compiling multiple `.glyph` files that import each other:

1. **Phase 1 (Parse)** builds the import dependency DAG across all files and topological-sorts it. Cycles are rejected as hard errors.

2. **Leaves compile first.** Files with no imports go through the full pipeline first.

3. **Dependency readiness.** An importing file cannot enter Phase 2 (Analyze) until the imported file has passed Phase 5 (Validate). The importer needs the dependency's validated IR for name resolution, type matching, and effect propagation.

4. **Expand/Emit are not parallelised in MVP.** Although a dependency's Phases 6–7 (Expand + Emit) are architecturally independent of an importer's Phases 1–5 — importers only need the dependency's validated IR, not its compiled output — the MVP compiler does not exploit this overlap. Item 7 below makes the build strictly serial across files. The architectural independence is preserved as a post-MVP optimisation note: when parallelism is later introduced, dependency Expand/Emit and importer Parse/Analyze/Lower/Validate can overlap safely.

5. **Repair is per-file only.** Repair only edits the current file (per `repair.md` §9). It does not edit dependencies or add new imports. Each file gets up to 3 repair iterations independently; there is no cross-file trigger propagation. If a file still has repairable diagnostics after its 3 iterations, fail with diagnostics for that file.

6. **Per-file repair iteration accounting.** The compiler is stateless across `glyph compile` invocations — every invocation re-parses every file. The repair iteration counter is owned by the agent and is per-file (see `agent-skill.md` §Iteration Budgets): the agent increments a file's counter only when that file emitted `repairable` diagnostics in the latest invocation. A file that converged in an earlier iteration is skipped on subsequent LLM repair passes even though the compiler still re-processes it on disk. The 3-iteration hard-fail bound is therefore per-file, not per-build.

7. **Strictly serial compilation.** Files compile one at a time, in topological order. The MVP compiler does not parallelize independent files in the DAG — no threadpool, no `rayon`, no async fan-out. This matches the sync-only architecture decision in `build-foundation.md` §A5 (Async Strategy / no async runtime). Parallelism is a post-MVP optimization.

8. **Consumer-side projection-tier word counts.** During a library's Phase 6 Step 1, the resolved expanded prose for each `export block` is computed once and the word count is recorded as a derived field on the validated IR's `ExportBlock` node (in-memory only; not part of the JSON schema — see `ir-schema.md` §Top-Level Compilation Units). Consumers depend on this: when a downstream skill enters its own Phase 6 Step 1, its three-tier projection heuristic reads the imported callee's `resolved_word_count` directly from the in-memory IR. Topological order guarantees the value is computed before any consumer needs it.

9. **Directory-mode scope: every file, no reachability filter.** When the user invokes `glyph compile dir/` (per `cli.md` §`glyph compile`), every `.glyph` file in scope compiles unconditionally, regardless of whether any in-scope skill reaches it through imports. A library file with no in-scope consumer still goes through Phases 1–7 and may produce zero emitted artifacts (which is normal — exit 0). Reachability filtering — pruning the build to only files transitively reachable from a designated root skill — is post-MVP. The DAG ordering above governs which files compile *before* which, not *whether* a file compiles.

### Partial Failure Policy

When some files in a multi-file build fail, the compiler uses a **skip-dependents, leave-stale-`.md`, partial-output** policy:

1. **Skip-dependents.** For each file in topological order: if **all** of its (transitive) imports validated successfully **in this build**, run Phases 1–7 normally. Otherwise mark the file as `skipped-due-to-dep` and do not run any phase on it. The skip emits `G::build::skipped-due-to-failed-import` (warning) naming the failed dependency's file path.

2. **Atomic per-file emission.** `.md` files are written atomically per file at the end of Phase 7. A file either fully succeeds (its `.md` is written or replaced) or its `.md` is not touched. There is no half-written compiled output.

3. **Stale `.md` policy.** If a previous build emitted `b.md` and the current build fails (or skips) `b.glyph`, the existing `b.md` on disk is **left in place** (not deleted). The compiler emits a stderr note: "`b.md` was not regenerated; the on-disk version reflects the previous successful build of `b.glyph` and may be out of sync." Authors who want stale outputs purged must delete them manually; the compiler never deletes a previously emitted `.md` on a failed re-build.

4. **Exit code.** The build exits `0` only if every file in the build set succeeded. If any file failed or was skipped, exit `1`. A build that succeeds for some files and fails for others still produces partial output (the successful files' `.md` are written) but signals failure via the exit code.

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
| Expand [LLM] | (not in the 9-step list) | **6. Expand** (parameterless, distinct from Repair) |
| Validate | 9. Validate | **5. Validate** |
| Output | (implicit) | **7. Emit** |

Key clarifications this reconciliation makes:

- The README's "Expand [LLM]" is **Phase 6 (Expand)**, the pass that turns IR into agent-facing prose with parameter references preserved as `{param}` slots. It is distinct from the README's implied single-LLM-pass model — there are two LLM passes (Repair and Expand), each bounded by deterministic checks.
- The 9-step list's steps 5-8 (Resolve, Infer, Normalize, Type) all happen inside **Phase 4 (Lower)** as sub-operations of the source-to-IR conversion.
- The 9-step list does not mention an LLM expand pass; this document adds it as Phase 6.
- The README does not mention repair as a separate pass; this document makes it explicit as Phase 3 with a bounded loop.

## Source-To-Source vs. IR-Only Transforms

| Transform | Phase | Touches `.glyph`? |
|---|---|---|
| Unconditional constraint → `constraints:` section (body-level + flow-top-level) | 3a | Yes |
| Unconditional context → `context:` section (body-level + flow-top-level) | 3a | Yes |
| Unused import removal | 3a | Yes |
| Duplicate import merging | 3a | Yes |
| Section reorder to convention | 3a | Yes |
| `generated const` materialization | 3b | Yes |
| `generated block` materialization | 3b | Yes |
| Missing `description:` generation | 3b | Yes |
| Missing `effects:` generation | 3b | Yes |
| Role/constraint marker addition | 3b | Yes |
| Positional → named args | 4 | No (IR only) |
| Nested call desugaring | 4 | No (IR only) |
| Default value filling | 4 | No (IR only) |
| Effect propagation (union) | 4 | No (IR only) |
| `with` modifier recording | 4 | No (IR only) |
| Parameter metadata assembly | 6 (Step 1, deterministic) | No (in-memory) |
| Block projection tier assignment | 6 (Step 1, deterministic) | No (in-memory) |
| Bare name / inline string passthrough | 6 (Step 1, deterministic) | No (in-memory) |
| Call-node expansion into prose | 6 (Step 2, LLM) | No (in-memory) |
| `with` modifier reshaping | 6 (Step 2, LLM) | No (in-memory) |
| Constraint rewording | 6 (Step 2, LLM) | No (in-memory) |
| Return folding into final step | 6 (Step 2, LLM) | No (in-memory) |
| Conditional projection to sub-steps | 6 (Step 2, LLM) | No (in-memory) |

## Cacheability

| Phases | Cacheable? | Key |
|---|---|---|
| 1-5 (Parse through Validate) | Yes | Post-repair source file content hash + **transitive** import content hashes |
| 6-7 (Expand + Emit) | Yes | Post-repair source hash + **transitive** import content hashes |

All seven phases produce output that depends only on source content and imports — there is no argument-dependent variation. If the source file and its imports have not changed, the entire pipeline output can be reused.

**Transitive dependency hashes.** The cache key includes the post-repair source hashes of **all transitive dependencies**, not just direct imports. If a library file changes, its procedure `.md` files may change, which means every consumer whose cache key includes that library's hash is stale and must recompile. This is conservative — a library change triggers consumer recompilation even if the change did not affect the specific export the consumer uses. Fine-grained per-export invalidation is a post-MVP optimization.

**Note:** The cache key is the **post-repair** source hash, not the original author-written source. Repair (Phase 3) writes back to the `.glyph` file, so the source on disk after a successful compile already includes all repairs. Subsequent compilations of the same file will find no repairable diagnostics, skip repair, and produce the same validated IR — which matches the cached entry.

**Step 2 non-determinism caveat:** Step 2 (LLM reshaping) is not idempotent across model versions or repeated runs at temperature > 0 (see `expand.md` §7). Byte-stable caching of compiled output requires including the Step 2 output in the cache entry. If the source has not changed and a cached Step 2 output exists, the pipeline may skip Step 2 entirely and reuse the cached prose.

Incremental compilation and build caching are **deferred** from MVP (per `todo.md`). The pipeline design supports them since all phases are argument-independent, but the MVP compiler may re-run all phases on every compilation.

## Cross-References

- **Foundations:** `foundations.md` — #18 (deterministic passes own correctness), #33 (novice learnability).
- **Source syntax:** `language-surface.md` — declaration forms, indentation, sub-sections. §5 lists the 9-step pipeline that this document supersedes.
- **IR and roles:** `ir-and-semantics.md` — five MVP roles, constraint model, effects, section vocabulary.
- **Repair:** `repair.md` — full repair rules, generated definitions (const and block), single-string rule, idempotence, intent potency.
- **Data flow:** `data-flow.md` — parameters, calls, `with` modifier, control flow, return semantics, closure.
- **Compiled output:** `compiled-output.md` — frontmatter shape, `## Parameters` section, `## Instructions` structure, projection rules, parameterless compilation model.
- **Imports:** `imports.md` — path resolution, cycle rejection, effect propagation, multi-file compilation order.
- **Types:** `types.md` — nominal matching at call boundaries.
- **Standard library:** `stdlib.md` — MVP stdlib entries and their effect signatures.
