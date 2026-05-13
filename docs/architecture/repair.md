# Repair Pass — Implementation Architecture

Maintainer-facing reference for the Repair pass: phase mechanics, algorithms, LLM prompts, retry policy, diagnostic plumbing, and the deterministic/LLM-assisted boundary.

The author-facing contract — what Repair may change, what it must preserve, idempotence as a guarantee, the repair/error boundary — lives at [[design/repair]]. This document does not repeat those rules; it describes how they are implemented.

## 1. Pipeline Position

Repair is Phase 3 of the compiler pipeline ([[compiler-pipeline]]). It sits between Analyze (Phase 2) and Lower (Phase 4):

```text
Phase 1 Parse   -> Phase 2 Analyze
                -> Phase 3 Repair        (this document)
                -> Phase 4 Lower         -> Phase 5 Validate
                -> Phase 6 Expand        -> Phase 7 Emit
```

Repair only runs when Phase 2 emits `repairable` diagnostics, or when Phase 3c's constraint-conflict scan is triggered by constraint count alone.

## 2. Inputs and Outputs

### Input

- Original Glyph source.
- Structured diagnostics from Phase 1 and Phase 2.
- Known local declarations, imports, and standard-library entries.
- Partial AST when parsing succeeded far enough — including `extra_subsections` recovery slots populated by the duplicate-sub-section recovery shape ([[language-surface]] §2.5). Phase 3a's deterministic merge (§4.4) consumes these to satisfy `G::parse::duplicate-subsection`. If Phase 3a is disabled or the merge cannot proceed, Analyze surfaces `G::analyze::unmerged-duplicate-subsection` (error) before Repair runs.
- Compiler rules for valid syntax, role and constraint markers, type annotations, and declaration forms.

The LLM repairs against diagnostics — it does not free-form guess from scratch.

### Output

- Repaired Glyph source written back to disk after each accepted iteration.
- A concise change list.
- Any unresolved questions or diagnostics that still need author input.
- A confidence level or repair status.

Phases 1, 2, 4, and 5 re-run on the repaired source. Repair is accepted only if the deterministic compiler accepts the rewrite.

## 3. Sub-Phase Structure

Phase 3 has three sub-steps. Their boundaries are load-bearing:

- **3a — Deterministic auto-fixes.** No LLM. Driven by Phase 1 / Phase 2 diagnostics with known mechanical fixes.
- **3b — LLM-assisted repairs.** One LLM call per file per iteration. Driven by remaining `repairable` diagnostics.
- **3c — Constraint conflict scan.** Independent of Phase 2 diagnostics. Triggered by constraint count (`>= 2` constraints on any declaration).

### 3.1 Phase 3a — Deterministic Auto-Fixes

Phase 3a operates in two strata mirroring `glyph fmt` ([[docs/reference/cli]] §`glyph fmt`):

**Stratum 1 — pre-Parse text-level rewrites.** Run on raw source. May turn previously-rejecting source into source Phase 1 can accept.

- Tab -> 4 spaces, mixed-indent normalization (#106).
- Legacy `-> None` strip on declaration headers (#114). Identifier-boundary semantics; case-insensitive on `none`. The value keyword `none` in `return none`, `effects: none`, etc. is preserved untouched. Triggered by `G::parse::none-as-return-type`.

**Stratum 2 — post-Parse AST-level rewrites.** Require a successful Phase 1. If Phase 1 fails after stratum 1, only stratum-1 fixes are written and the parse diagnostic surfaces to subsequent phases; stratum-2 rewrites are skipped.

- Duplicate sub-section merge (#109) — see §4.4.
- Duplicate import collapse (#107).
- Unused import removal (#108).
- Stdlib auto-import (#110, `G::analyze::stdlib-missing-import`).
- Const-in-flow parens-add (#111, `G::analyze::const-in-flow`).
- Effects auto-insert (#112, gated on `--enable-effects`; emits `G::repair::inferred-effects` warning).
- Placeholder return rewrite (#113, `G::analyze::placeholder-string-return`) — bifurcates on placeholder shape:
  - Identifier-shaped `return "<current_branch>"` -> `return <current_branch>`.
  - Non-identifier-shaped `return "<root cause analysis ...>"` -> `return <"root cause analysis ...">` with `<` / `>` stripped and `"` / `\` escaped per [[values-and-names]] §Inline Strings.
  - Empty `return "<>"` is not repaired (would produce malformed `<"">`).

### 3.2 Phase 3b — LLM-Assisted Repairs

Driven by `repairable` diagnostics surviving Phase 3a: undefined names, ambiguous roles, missing returns, nested-branch extraction, missing description, etc.

**Granularity: one call per file per iteration.** The full file source and all repairable diagnostics for that file go in a single prompt. Repair is not invoked per-diagnostic and does not stream diagnostics; the LLM produces one rewritten file in one call.

Rationale:

- Glyph files are small by design; whole-file context fits comfortably in modern LLM context windows.
- Single-call repair eliminates merge complexity (two per-diagnostic repairs that both want to add an import would otherwise need a separate merge step).
- The call is cacheable per-file by `(post-rewrite-file-hash, diagnostics-hash, repair-model-version)`.

**Hoisting constraint.** Repair-generated markers must respect the four-case hoisting rule ([[language-surface]] §4.2a). Generated markers materialize at body-level (case 1) or `flow:`-top (case 2) only — never inside an existing named section (`context:`, `constraints:`, `flow:`, or any freeform colon-keyword section). Injecting a marker inside a section body would silently move it into a different output H2 destination.

**Cross-file scope.** Repair only edits the current file. If a diagnostic requires changes to another file (e.g., importing a non-exported block), Phase 3b emits a non-repairable diagnostic for the author. Repair does not add new `import` statements and does not modify an imported file's declarations. Cross-file repair is post-MVP (see [[repair-todos]]).

**Imports as resolution targets only.** Repair resolves against existing imports — an unresolved bare name matching an already-imported declaration prefers that resolution over generating a new definition (per the idempotence detection in [[design/repair]] §5). What Repair never does is *modify the import set*: no new `import`, no alias change, no selective-vs-whole-module switch. The post-repair import block is byte-identical to the pre-repair version unless a stratum-2 deterministic rewrite triggered.

### 3.3 Phase 3c — Constraint Conflict Scan

Runs once per declaration with **>= 2** entries in its `constraints:` set. Operates on `Skill.constraints`, `Block.constraints`, and `ExportBlock.constraints`. Does **not** scan across scopes — a callee's `scoped_constraints` (carried on `Call` nodes per [[docs/architecture/expand]] §3.2) are intentional composition, not conflict candidates.

**Mechanism (LLM-assisted, structured output):**

1. **Input.** The constraint set for one declaration: each entry as `{ id, resolved_text, strength, polarity }`. Identifiers are **declaration-local constraint indices** from the annotated AST (`c0`, `c1`, …, assigned in source order), not IR node IDs — Lower has not run yet at Phase 3c time. See [[ir-schema]] §Node Identifiers.
2. **Prompt.** Structured judgment: for each pair `(A, B)`, classify as `contradiction` (mutually exclusive), `tension` (in friction but both reasonable to hold; agent balances at runtime), or `none`.
3. **Output (structured JSON).** `{ conflicts: [{ pair: [id_A, id_B], type: "contradiction" | "tension" | "none", explanation: "..." }, ...] }`. The model addresses every pair; pairs classified `none` may be omitted.
4. **Compiler handling:**
   - All `none` / empty conflicts -> no diagnostic, source proceeds to Lower.
   - At least one `tension` -> emit `G::repair::constraint-tension` (warning, non-blocking) per tension pair. Build proceeds; both constraints survive into compiled output. The warning carries the LLM's `explanation` as a hint.
   - At least one `contradiction` -> emit `G::repair::constraint-contradiction` (error) per contradiction pair. Compilation fails. The author must edit one of the two.

**Why hard-error on contradiction (not auto-fix).** Picking which constraint wins is a semantic judgment the author should make. Auto-dropping erodes trust in compiler-preserved authoring intent. 3b generates *new* content from missing references; 3c proposing to *delete* authored content would be a categorically higher-stakes action.

**Why warning on tension (not auto-resolve).** Tension is often deliberate ("be thorough" + "be efficient"). The agent balances at runtime; the warning lets the author know the friction is visible. Tension does not invalidate compiled output — both constraints render in `## Constraints`.

**Retry policy.** Same info-rich pattern as Expand Step 2 ([[docs/architecture/expand]] §5). Up to **2 retries** if the LLM output is malformed (not valid JSON, doesn't address every pair, references an ID not in the input set, returns a `type` outside the three-value enum). Each retry includes the original prompt, the previous failed output, a structured violation report, and an edit directive. After two failed retries, emit `G::repair::constraint-scan-malformed` (error) and abort.

**Idempotence.** Phase 3c does not modify source — it only emits diagnostics. Re-running Repair on the same source produces the same constraint set, the same prompt, and (modulo model non-determinism, same caveat as 3b) the same verdict.

**Cost.** One LLM call per declaration with >= 2 constraints. Skills/blocks with 0 or 1 constraints incur no Phase 3c call. The prompt is small (constraint texts only, no IR graph or surrounding flow).

## 4. Notable Algorithms

### 4.1 Nested Branch Auto-Extraction (Phase 3b)

When a `Branch` appears inside another `Branch`'s arm body, the compiled output supports only one level of structured sub-steps ([[docs/reference/compiled-output]] §Constraint Rendering). Repair auto-extracts the inner branch into a `generated block`.

1. **Detection.** Analyze (Phase 2) detects a nested `Branch` and emits `G::analyze::nested-branch` (repairable).
2. **Extraction.** The LLM extracts the inner `Branch` and its arm contents into a new `generated block`. The LLM names the block based on intent and surrounding context.
3. **Closure capture.** Bindings or parameters from the outer scope referenced by the inner branch become parameters of the extracted block. This is mini closure analysis: the LLM (guided by the diagnostic's related spans) identifies outer-scope names appearing inside the inner branch bodies and adds them as parameters.
4. **Call replacement.** The inner `Branch` is replaced with a call to the new `generated block`, passing the captured bindings as arguments.
5. **Notification.** Repair emits `G::repair::branch-extracted` (warning) informing the author.

**Idempotence.** After extraction, the inner `Branch` no longer exists — re-running Analyze finds no nested branch, so no further extraction occurs.

### 4.2 Compound Names

Compound names like `avoid_unrelated_edits` are valid identifiers and are **not** forcibly split into marker-plus-concept form. Both `avoid_unrelated_edits` and `avoid unrelated_edits` are accepted authoring styles.

When a compound name resolves to a declaration, the compiler infers role, strength, and polarity from the declaration's text content, with the name prefix (`avoid_*`, `must_*`) as supporting evidence. No splitting or renaming.

When a compound name is unresolved, Repair generates a `generated const` under the full compound name with the full semantics baked into the text body:

```glyph
generated const avoid_unrelated_edits = "Do not make changes outside the requested scope."
```

The definition carries the polarity in its text. No splitting, no renaming.

### 4.3 Condition-Position Predicate Generation

An undefined bare name in an `if` / `elif` condition routes to `generated const` (not `generated block`). The repair LLM receives:

- The undefined name (strong signal — names like `complex_change_required` are nearly self-describing).
- 3-5 surrounding flow statements before and after the conditional.
- Sibling `if` / `elif` arms in the same Branch (if any), with their resolved predicate text.
- The enclosing skill or block's `description:` and name.
- The enclosing skill's `context:` entries (if any).

**Output.** A single clause following the predicate canonical form: lowercase first word, no trailing period, 1-2 sentences typical, hard cap ~50 words. Same form as constraint text in [[docs/reference/compiled-output]] §Constraint Rendering. The generated clause should read naturally as an "if X" condition header — e.g., "the requested change requires regenerating multi-line prose" rather than "Returns true when the requested change requires regenerating multi-line prose."

**Failure.** If the LLM produces an empty or malformed string, Repair emits `G::repair::predicate-generation-failed` (error, non-repairable). The author must add the `const` manually.

**Inferred strings with `{name}` slots.** If the generated string contains a `{name}` slot, it is stripped before storing (predicates are consulted as-is, not rendered through parameter slots).

### 4.4 Duplicate Sub-Section Merge (Phase 3a)

When a `skill`, `block`, or `export block` body contains the same sub-section keyword (`description:`, `context:`, `constraints:`, `effects:`, `flow:`) more than once, the parser is permissive: the first occurrence populates the canonical singleton field and every later occurrence lands in `extra_subsections` ([[language-surface]] §2.5, [[tree-sitter]] §2.1). Phase 3a's deterministic post-Parse stratum merges these duplicates back into a single sub-section in source. The pass is purely textual — it splices source spans; it does not re-emit from IR — so author formatting inside each body is preserved verbatim.

**Trigger.** `G::parse::duplicate-subsection` (repairable). The merge runs whenever `extra_subsections` is non-empty for any declaration AST node.

**Mechanism.**

1. Order the duplicate occurrences by source position (first, second, …).
2. The **first** occurrence is the *anchor*: its header line, indentation, and body span are kept in place.
3. For each later occurrence (in source order), append its body content under the anchor's body, preserving the anchor's body indentation. The duplicate's header line is removed.
4. Concatenation rules differ by sub-section kind:
   - `description:` — concatenate body text with a single blank line between bodies.
   - `context:`, `constraints:`, `effects:` — concatenate entries (one per line) in source order; do not deduplicate (deduplication is a separate concern owned by Lower).
   - `flow:` — append later statements after the anchor's last statement, preserving statement order across the duplicates.

**Comment placement.** Comments are first-class authored content and must not be deleted, moved, or rewritten. Three sub-rules govern how comment trivia attached to a duplicate occurrence are placed when its header line is removed:

- **(a) Whole-line comments inside the second body are preserved verbatim.** Whole-line `//` comments between body entries of a duplicate occurrence move with their entries into the merged body, retaining their relative position to the entries that follow them.
- **(b) A trailing comment on the second header becomes a new whole-line comment at the merge boundary.** When a duplicate occurrence's header line carries a trailing `// …` comment, the merge converts it into a whole-line comment placed at the boundary in the merged body — immediately before the first entry contributed by that duplicate, at the body's indentation. The comment text is unchanged.
- **(c) Whole-line comments between the first body and the second header are preserved at the boundary.** Any whole-line `//` comments between the end of the anchor's body and the start of the duplicate's header land at the merge boundary as whole-line comments at the merged body's indentation, in their original source order, immediately before the entries contributed by the duplicate.

**Idempotence.** After a successful merge, `extra_subsections` is empty and the body contains a single occurrence of each sub-section kind. Re-running Phase 3a finds nothing to merge.

**Failure mode.** If Phase 3a is disabled (`--no-repair`, `glyph fmt --check`) or the merge cannot complete (e.g., the duplicate's body cannot be located in source), `extra_subsections` survives into Analyze and surfaces as `G::analyze::unmerged-duplicate-subsection` (error).

### 4.5 No Overwrite Of Existing Declarations

Repair never silently overwrites, deletes, or renames an existing declaration to make room for a newly-generated one, **except** when an author-written declaration supersedes a `generated` one (No-Shadowing Rule). The remaining collision cases hard-fail with `G::analyze::name-collision`:

- **Author-written vs. author-written.** Two hand-written declarations share a name. Always a hard error.
- **Generated vs. generated.** Two different unresolved use sites would produce the same generated name. Always a hard error.

The author resolves manually. For these non-supersession cases the LLM cannot infer which definition the author intended, so a hard-fail is the only safe rule.

## 5. Validation Loop

Repair is iterative but bounded:

1. Run deterministic compiler stages.
2. If diagnostics are repairable, run the LLM repair pass.
3. Re-run deterministic compiler stages.
4. Accept repaired source only if it compiles.
5. If diagnostics remain after a bounded number of attempts, stop and return the unresolved issues.

The LLM repair pass is never treated as proof of correctness. The deterministic compiler remains the authority.

## 6. Retry and Failure Policy

Three failure modes, each with its own policy. The numbers below are compiler-config values, not hardcoded constants.

**Transient failure (network or 5xx).** Retry up to **3 times** with exponential backoff. After exhaustion, emit `G::repair::llm-unavailable` and abort compilation. The user re-runs.

**Invalid Glyph output.** A single LLM call. If the rewritten file does not parse (Phase 1 fails on the LLM's output), emit `G::repair::output-invalid` (which captures the LLM's output for inspection) and abort. **No retry.** A self-correction prompt for syntactic errors is not part of the contract; in practice an LLM that produces non-parseable Glyph once is unlikely to self-correct on a second prompt. The source on disk is left untouched.

**No convergence.** The repair loop in Phase 3 caps at **3 iterations**. If repairable diagnostics remain after the third iteration, emit `G::repair::no-convergence` with the residual diagnostics attached, surface them to the author on stderr, and abort. Whatever partial repairs succeeded in earlier iterations remain in the source file (Repair writes back after each accepted iteration).

**Quality.** Semantic wrongness — a rewrite that parses, validates, and converges but does not match author intent — is not detected by the compiler. The mitigation is the per-generation warning (`G::repair::generated-const` / `G::repair::generated-block`) plus author review of generated definitions. This is a social contract, not an automated check.

## 7. Multi-File Repair (MVP Scope)

MVP: repair only edits the current file. All repairs — generated definitions, marker additions, indentation fixes, section reordering — are local to the file being compiled. If a diagnostic requires changes to another file (e.g., an imported block is not exported), Repair emits a non-repairable diagnostic.

This restriction eliminates cross-file trigger propagation: one file's repair cannot force another file to re-run from Phase 1. Each file's repair loop is self-contained.

**Generated bodies do not introduce cross-file dependencies.** A `generated block` body is a single instruction string with `{param}` slots. It is not a `flow:` block and cannot contain calls into other declarations — neither same-file nor imported. This sidesteps the question of whether a repair-generated body could legitimately reference an imported callee: by construction, it never does. If the author's intent requires composing imported callees, the right surface is a hand-written `block` or `export block`, not a generated definition.

**Post-MVP.** Cross-file repair and auto-import discovery (adding imports to files the author did not reference) are deferred. See [[repair-todos]].

## 8. Argument-Agnosticism Invariant

Repair is **argument-agnostic**. It operates on authored source without any concrete argument values. (Since compilation is parameterless, no phase receives concrete argument values — parameters appear as `{param}` slots in compiled output, resolved by the consuming LLM at runtime.) The invariant holds for three structural reasons:

1. **Nominal-only types.** The MVP type system ([[types]]) uses opaque name tags with no union types, generics, or conditional types. No type can narrow based on a concrete argument value, so no type diagnostic is hidden from Repair by the absence of arguments.
2. **Branch conditions are structural, not evaluated.** `if`/`elif`/`else` blocks are checked exhaustively — Repair resolves names and assigns roles in every branch regardless of the condition. Conditions are preserved as text through Lower and flattened into prose by Expand; no phase evaluates them.
3. **Topological compilation order.** An importing file cannot enter Phase 2 until the imported file has passed Phase 5 ([[compiler-pipeline]] §Multi-File Compilation Order). Repair always sees dependencies in post-repair, post-validate form.

This invariant enables the cache-key-by-post-repair-source-hash strategy ([[compiler-pipeline]] §Cacheability): Phases 1-5 produce a validated IR independent of invocation arguments.

**Post-MVP consideration.** If the type system gains union types, structural narrowing, or value-dependent type features, this invariant must be re-examined.

## 9. Determinism, Caching, And CI

Repair is LLM-driven and not byte-deterministic. The compiler accepts this by design: Repair is the primary content-generation mechanism for novice authors, and forcing determinism would either gut its capability or require seeding/temperature controls that don't transfer across model versions.

**CI mode.** The compiler is fully deterministic and does not run Phase 3 itself. If `repairable` diagnostics exist after Phase 2, the compiler exits with code 2 — it is the agent's responsibility to perform LLM repair and re-invoke. In CI (where no agent runs), exit code 2 is a build failure, which enforces the "commit post-repair source" workflow and guarantees deterministic builds. See [[docs/reference/cli]] for exit code semantics.

**Cache implications.** The cache key is the post-repair source hash ([[compiler-pipeline]] §Cacheability). After the author commits, the on-disk source IS the post-repair source, so cache keys are stable across machines compiling the same committed file. The non-determinism is a one-time cost paid at authoring time, not at build time.

**Hostile case: un-repaired source committed.** If an author commits source with `repairable` diagnostics and CI runs the compiler directly (no agent), exit code 2 fails the build. If CI runs the agent skill, the agent repairs — but may produce different post-repair source on each machine, yielding different compiled `.md`. The recommended CI configuration is to run the compiler directly (no agent).

**Step 2 (Expand) non-determinism is separate.** Expand Step 2's LLM reshaping is also non-deterministic but is bounded by Phase 6b's role-preservation gate ([[docs/architecture/expand]] §4). There is no deterministic fallback emitter for Step 2 — it either passes 6b (after at most two retries) or hard-fails ([[docs/architecture/expand]] §5). The cache strategy at `compiler-pipeline.md:522` allows reusing Step 2 output when source has not changed.

## 10. Diagnostic Plumbing

The diagnostic shape and classification tiers are defined in [[docs/reference/diagnostics]]. Diagnostic IDs referenced by Repair (representative, not exhaustive):

| ID | Tier | Phase | Notes |
|---|---|---|---|
| `G::parse::duplicate-subsection` | repairable | 3a | Triggers §4.4 merge. |
| `G::parse::none-as-return-type` | repairable | 3a (stratum 1) | Strips legacy `-> None`. |
| `G::analyze::unmerged-duplicate-subsection` | error | post-Repair | Phase 3a disabled or merge failed. |
| `G::analyze::stdlib-missing-import` | repairable | 3a | Stdlib auto-import. |
| `G::analyze::const-in-flow` | repairable | 3a | Parens-add for bare names in `flow:`. |
| `G::analyze::missing-effects` | repairable | 3a | Effects auto-insert. |
| `G::analyze::missing-description` | repairable | 3b | LLM generates `description:` body. |
| `G::analyze::placeholder-string-return` | repairable | 3a | Rewrites `return "<...>"`. |
| `G::analyze::nested-branch` | repairable | 3b | Triggers §4.1 extraction. |
| `G::analyze::name-collision` | error | — | Author-vs-author or generated-vs-generated. |
| `G::repair::inferred-effects` | warning | 3a | Informational after effects insert. |
| `G::repair::branch-extracted` | warning | 3b | Informational after §4.1. |
| `G::repair::generated-const` | warning | 3b | Per-generation notification. |
| `G::repair::generated-block` | warning | 3b | Per-generation notification. |
| `G::repair::constraint-tension` | warning | 3c | Non-blocking. |
| `G::repair::constraint-contradiction` | error | 3c | Blocking. |
| `G::repair::constraint-scan-malformed` | error | 3c | After 2 retries. |
| `G::repair::predicate-generation-failed` | error | 3b | LLM produced empty/malformed predicate. |
| `G::repair::llm-unavailable` | error | any 3b/3c | After 3 transient retries. |
| `G::repair::output-invalid` | error | 3b | Rewrite doesn't parse. |
| `G::repair::no-convergence` | error | 3 (loop) | After 3 iterations. |

The full catalog is built out as the compiler is implemented ([[docs/reference/diagnostics]]).
