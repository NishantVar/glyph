# Glyph Expand Pass (Phase 6) — Architecture

Audience: compiler maintainers. The author-facing contract (what Expand changes in compiled output, role preservation as a 1-to-1 guarantee, non-idempotence as a property authors should plan around) lives in [[design/expand]]. This document covers Step 2 reshape mechanics, the Phase 6b validation algorithm, retry/fallback policy, internal scaffold + span IR, and the structural diagnostic catalog. It expands the short treatment in [[compiler-pipeline]] Phase 6 at the fidelity of [[docs/architecture/repair]].

Expand has two sub-steps:

- **Step 1 (deterministic resolution)** — mechanical resolution of bare names, inline strings, and parameter metadata into the IR. Parameters are preserved as `{param}` slots, not substituted. No LLM. Fully specified in [[compiler-pipeline]] Phase 6 and [[docs/reference/compiled-output]].
- **Step 2 (scaffold-with-spans + LLM span fill)** — the deterministic emitter walks the resolved IR and produces a typed Markdown *scaffold* with named spans; an LLM (when wired) fills the spans. Today's stub filler is the only filler shipped; the LLM-side contract is enumerated in [[llm_expand_pass]]. This document focuses here.
- **Phase 6b (structural validation)** — deterministic check that the merged scaffold + span fills faithfully project the input IR. Runs between Step 2 and Phase 7 (Emit). For scaffolded portions, most 6b checks are satisfied by construction; 6b retains them as defense in depth (see §4).

The ordering Step 1 → Step 2 → 6b is the closing half of the **Safety Sandwich** ([[foundations]] #18): every LLM pass is framed by deterministic work on both sides. Step 1 hands the LLM a fully-resolved IR; Phase 6b checks that the LLM's Markdown still matches the IR structurally before Emit gets it.

## 1. Purpose

Step 2 exists to turn structured IR content into readable agent instructions. After Step 1, every node already carries resolved content (bare names inlined, parameter references preserved as `{param}` slots). If Emit ran directly on the Step 1 output, the result would be correct but stilted — a sequence of short declarative sentences with no shaping, no application of `with` modifiers, no constraint wording calibrated to strength and polarity keywords, and no return folding. Step 2 is the pass that makes the output *useful*, not just *correct*.

Step 2 is split into two layers:

1. **Deterministic emitter (always runs).** Walks the resolved IR and produces a *scaffold-with-spans* — a `Scaffold { chunks: Vec<Chunk> }` value where every chunk is either a literal Markdown string or a typed `Span` placeholder. The scaffold owns all deterministic structure: section headers, list numbering, constraint rendering, return-fold suffixes (Identifier form), pure-`applies()` Branch projection, and the external-file Step template.
2. **Span fill (LLM when wired; stub today).** Each `SpanKind` carries the IR context the filler needs (see §3.5 Deterministic Emitter Responsibilities and [[llm_expand_pass]] for the per-kind contract). The merger substitutes fills into the scaffold to produce the final Markdown. The scaffold is **internal** — it is not exposed via `--emit-ir`.

Step 2 is also where `with` modifiers are applied. The modifier is the only call-site specialization mechanism in MVP ([[compiler-pipeline]] Phase 6), and its application is an LLM task by design: the modifier is natural language, the body it reshapes is natural language, and the output must be natural language. The deterministic emitter exposes the modifier through a `CallBodyShape` span; the LLM weaves it into the Step's prose.

## 2. Non-Goals

Step 2 must not:

- repair invalid source or add missing definitions (that is Phase 3 Repair, [[docs/architecture/repair]]);
- introduce new IR nodes, new calls, new constraints, or new steps;
- reinterpret or reorder the skill's workflow;
- invent sections beyond the canonical peer-level H2s `## Parameters`, `## Context`, `## Steps`, `## Constraints` (see [[docs/reference/compiled-output]]);
- invent new `{param}` references that do not correspond to declared parameters (declared parameter references must be preserved), or fail to resolve `local_ref` slots into natural-language prose;
- change effects, types, or the call graph;
- re-materialize content that was already prose after Step 1 (inline strings and resolved `const` references pass through untouched);
- serve as a second repair loop; it has no access to diagnostics and no ability to rewrite source.

If Step 2 cannot cleanly reshape a node, that is a Phase 6b failure (see §4), not an opportunity for Step 2 to be more creative.

## 3. Input / Output Contract

### 3.1 Input schema

Step 2 receives a **resolved IR** — the same IR shape produced by Phase 4 (Lower) and validated by Phase 5 (Validate), but with bare names and inline strings already resolved by Step 1. Parameter references (`{param}`) are preserved as named slots. The LLM does **not** see the original source file, the authoring-level declarations, or any unresolved names.

Specifically, Step 2 receives:

- the full validated IR of the skill (all nodes, not one node at a time) in a serialized form — JSON or an equivalent structured encoding;
- for every `Call` node: `{ target_name, resolved_body_text, local_refs, site_modifier?, role, effects, scoped_constraints, position }`, where `resolved_body_text` is the post-Step-1 body with `{param}` references preserved as named slots and `{local}` references preserved as literal `{name}` tokens. The `local_refs` array (see [[ir-schema]] §Resolved IR) lists each local-binding slot by name and producing node ID; Step 2 cross-references this array to identify which `{name}` tokens are local bindings that must be resolved into natural-language prose. `scoped_constraints` is the list of constraints declared on the called block (see §3.2 Scoped Constraint Inlining);
- for every top-level `Constraint` node: `{ resolved_text, strength, polarity }` (text may contain `{param}` references);
- for every `InlineInstruction` node: `{ text, role }` (typically passes through);
- for every `Branch` node: `{ condition_text, then_body, elif_branches, else_body, resolved_predicates? }` with every sub-body already resolved. The optional `resolved_predicates: {predicate_token → resolved_string}` side-map is populated by Step 1 whenever the Branch's own `condition_text` or any `elif_branches[*].condition_text` contains a predicate-form token (see [[ir-and-semantics]] §Predicates). Step 2 uses the side-map to choose the projection form: when every arm's condition is *purely* one or more predicate-form tokens combined by `or` (or each `if`/`elif` arm is guarded by a single predicate token), Step 2 emits a "decide which applies" prose frame keyed by the resolved predicate strings; for mixed conditions (e.g., `complex_change_required and not is_dry_run`), Step 2 inlines the resolved predicate into the larger condition prose via a `BranchCondition` span (e.g., "If the requested change requires regenerating multi-line prose and this is not a dry run, ...");
- for the `Return` expression: its resolved text plus a flag indicating it must fold into the final Step;
- skill-level metadata: `name`, `description` (if present), `effects` (as a list), the ordered position of each node in `flow:`, and the parameter list (names, types, and defaults if declared — used for generating the `## Parameters` section, where parameters without defaults render with a `(required)` marker per [[docs/reference/compiled-output]]).
- a **stable, file-local IR node ID** (e.g., `n0`, `n1`, …) on every node, assigned by Lower ([[compiler-pipeline]] Phase 4). The IDs are opaque, never appear in compiled output, and are not echoed back by Step 2 (the output contract in §3.4 is Markdown only). They exist so that Phase 6b's count + ordering checks (§4.1) and any internal diagnostic referring to "the IR node Step 2 failed to project" can name a specific node unambiguously across runs and across the parse-then-re-parse boundary inside Repair.

**Scaffold-with-spans, not whole-document prompting.** The deterministic emitter walks the resolved IR exactly once per compile and emits a `Scaffold` whose `Span` chunks are the only LLM-visible surface. The LLM is invoked **per span**, not per skill: each span carries the IR context the filler needs (resolved body text, site modifier, condition expression, applies-descriptions side-map, parameter metadata) in its `SpanPayload`. Sibling-node context is provided by reading neighboring literal chunks of the scaffold when required by a span kind.

This is deliberate:

- Most of the compiled file is structurally fixed (section headers, list numbering, constraint wording, return-fold suffixes for the Identifier form, pure-`applies()` Branch projection). The deterministic emitter owns that structure; the LLM never produces it.
- The LLM is load-bearing only where natural-language judgement is needed — `with` modifier weaving, `Description`-form return folds, mixed-condition Branch headers, parameter descriptions. Each of these has a dedicated `SpanKind` (see §3.5).
- Span-level retry is feasible: a failed `BranchCondition` span can be re-prompted in isolation without re-flowing the rest of the skill (see §5.3).

Step 1's output is the primary thing Step 2 reshapes — the resolved body text flows into the `CallBodyShape` span unchanged. The deterministic emitter is responsible for placing the span in the correct list-item slot; the LLM is responsible only for the prose inside.

The scaffold is not exposed via `--emit-ir` and is not stable across compiler versions; it is an internal value between the deterministic emitter and the fill site (see [[compiler-pipeline]] Phase 6 and the in-tree `glyph-core::emit` module inventory).

### 3.2 Scoped Constraint Inlining

A `block` (or `export block`) called from within a flow may itself declare constraints. Per [[data-flow]] §Constraint Scoping, these constraints **do not** propagate to the caller's top-level `## Constraints`. They stay scoped to the inlined region of the call.

**IR shape.** When Lower resolves a call, the resolved-call node carries the callee's declared constraints attached as the `scoped_constraints` field. Each entry is `{ resolved_text, strength, polarity }`, identical in shape to a top-level `Constraint` node, but flagged as scoped to this call.

**Step 2 behavior.** For every `Call` node whose `scoped_constraints` is non-empty, Step 2 must weave the constraint into the prose of the inlined step(s) produced from that call's body. Two acceptable projections are permitted:

- **Inline weaving.** Fold the constraint into the step's prose itself. Example: a call whose body is "Review the changes" with scoped constraint `(hard, avoid: unrelated_edits)` becomes a Step like "Review the changes, never touching files outside the declared scope."
- **Localized framing sentence.** Prepend or append a sentence that scopes the constraint to the inlined region. Example: "While reviewing, do not modify unrelated files. Review the changes." Use this when the constraint cannot be cleanly folded into a single sentence.

Step 2 picks per call based on what reads naturally. Multiple scoped constraints on one call may be combined into a single framing sentence or distributed across the inlined steps.

**Strength and polarity wording.** The same wording rules as top-level constraints apply to scoped constraints — `hard` renders with strongest non-negotiable wording, `soft` renders with standard wording, `require` renders as a positive obligation, `avoid` renders as a prohibition (per [[docs/reference/compiled-output]] §Constraint Rendering).

**Output placement.** Scoped constraints **never** appear as items in `## Constraints`. The caller's `## Constraints` section lists only the caller's own top-level `Constraint` IR nodes. Phase 6b enforces this (see §4.1).

**Why this is Step 2's job.** Scoped constraints are call-site contextual: their wording depends on the surrounding step's prose, the strength/polarity, and the position in the flow. Mechanical folding produces awkward output. The whole-skill prompting model (§3.1) already gives Step 2 the visibility needed to weave gracefully.

### 3.3 Pure-Predicate Branch Projection

The "decide which applies" prose frame mentioned in §3.1 is not a single sentence — it is a small family of phrasings keyed to the IR shape of the `Branch`. Step 1 populates `resolved_predicates` whenever any arm condition contains a predicate-form token; Step 2 (or the deterministic emitter, since this projection is mechanical) selects the framing per the table below.

A `Branch` qualifies for **pure-predicate projection** when **every** `if`/`elif` arm's condition is purely one or more predicate-form tokens (`predicate_applies`, `predicate_const`, or `predicate_literal`) combined by `or` only. The presence of an `else` arm does not disqualify it. Any other condition shape (e.g., `complex_change_required and not is_dry_run`, mixing a predicate with a boolean token) falls back to the mixed-condition path described in §3.1, which inlines the resolved predicate into the larger condition prose via a `BranchCondition` span.

All three predicate forms produce the same projection frames — the difference is only in how Step 1 resolves the predicate text:

- `predicate_applies` → resolved from `resolved_predicates["block_name.applies()"]`
- `predicate_const` → resolved from `resolved_predicates["const_name"]`
- `predicate_literal` → the literal's inner text used directly (no map lookup)

| IR shape | Frame |
|---|---|
| Single arm: one predicate token, no `elif`, no `else` | `Decide whether <resolved predicate text> applies and, if so:` followed by lettered sub-steps for the arm body. |
| Multiple predicate arms, no `else` | `Decide which of the following applies and follow only that path:` (verbatim, per [[docs/reference/compiled-output]] §Predicate-Driven Branch Projection) followed by lettered sub-steps, each prefixed `If <resolved predicate text>:`. |
| Multiple predicate arms with `else` | Same opening sentence as above, with the `else` arm's lettered sub-step prefixed `Otherwise:` instead of `If <predicate>:`. |

**Worked example — single-arm pure-predicate const (from `predicate_const_single_arm.glyph`):**

Source:
```glyph
const complex_change_required = "the requested change requires regenerating multi-line prose …"

flow:
    if complex_change_required:
        recommend_full_compile()
```

Compiled output (deterministic):
```md
N. Decide whether the requested change requires regenerating multi-line prose … applies and, if so:
   a. Stop and recommend running `/glyph:compile` instead — incremental edit cannot regenerate prose.
```

**Worked example — inline literal predicate (from `predicate_inline_literal.glyph`):**

Source:
```glyph
flow:
    if "the user has explicitly opted out of compile-on-save":
        skip_compile()
```

Compiled output:
```md
N. Decide whether the user has explicitly opted out of compile-on-save applies and, if so:
   a. Skip compilation and continue without changes.
```

Two scenarios that look related but are governed by different rules:

- **Two independent `if` statements** (e.g., `if a.applies(): … if b.applies(): …` written as separate flow statements) are **two separate `Branch` IR nodes**. Each projects to its own top-level numbered Step independently. Both arms can fire because they are not in the same Branch.
- **Branches nested inside arms** stop at one level (per §4.1 sub-step counting). A nested `Branch` inside an outer arm flattens into the parent sub-step's prose. In practice Repair §4.9 auto-extracts nested branches into `generated block` declarations, so the projection rules above typically apply only to top-level Branch nodes.

Phase 6b validates the resulting structure via the same count + ordering checks in §4.1; the framing sentences themselves are not checked for verbatim match (a future structural check could be added if drift becomes a problem — see [[expand-todos]]).

### 3.4 Output contract

Step 2 must return **Markdown only** — specifically, the body of the compiled file below the frontmatter. It must not return JSON, IR, explanations, or commentary. The expected shape is:

```md
## Steps

1. <expanded step 1>
2. <expanded step 2>
...

## Constraints

- <expanded constraint 1>
- <expanded constraint 2>
...
```

The output must preserve the following structural invariants:

1. **Body sections sit at peer H2.** Allowed H2 names are `## Parameters`, `## Context`, `## Steps`, `## Constraints` (Phase 3 will extend the catalogue with freeform headings). No `## Instructions` wrapper is emitted.
2. **Body H2s are conditional.** `## Context` is omitted when the IR has no context. `## Constraints` is omitted when the IR has no constraints. `## Steps` is omitted only for pure constraint-only skills. `### Procedure: <name>` sections appear at H3 — nested under whichever body H2 came last — only for calls with `same_file_procedure` projection.
3. **Role preservation** (1-to-1):
   - Every top-level `Step` node (and every top-level `Call`, `InlineInstruction`, `InstructionRef` that projects to a Step per [[docs/reference/compiled-output]]) must produce exactly one top-level numbered list item under `## Steps`, in the same order as the IR.
   - Every top-level `Branch` node must produce exactly one top-level numbered list item under `## Steps`, containing lettered sub-steps per arm (see [[docs/reference/compiled-output]] §Constraint Rendering). Each arm is introduced by a condition header (`If <condition>:` for `if`/`elif`, `Otherwise:` for `else`), and each Step-projecting node inside the arm produces a lettered sub-step (`a.`, `b.`, `c.`). Letters reset per arm.
   - Every `Constraint` node must produce exactly one bulleted list item under `## Constraints`. Order is not required to match the IR.
   - The `Return` expression must fold into the last `## Steps` item (or the last sub-step of the final arm, if the last Step is a Branch), not produce a separate item or section.
   - An `OutputContract` from `return <name>` or `return <"description">` must fold into natural output prose. The literal `<name>` token (identifier form) and the literal `<"…">` token, surrounding angle brackets, or bare quoted description (descriptive form) must not survive in the compiled Markdown.
   - Every Call with `projection_mode: same_file_procedure` must additionally produce one `### Procedure: <name>` section with numbered items matching the callee's flow node count. The referencing Step includes the procedure name in its prose.
   - Calls with `projection_mode: external_file` produce one Step whose prose includes the file path from `procedure_path`. No `### Procedure:` section is emitted for external projections.
4. **No invented content.** Step 2 may reshape wording but must not add new steps, new sub-steps, new constraints, new sections, or commentary.
5. **Bounded length (guideline, not enforced).** For non-conditional Steps, each step typically reads as **one instruction-sized paragraph** — one to three sentences. For conditional Steps (Branch projections), each **sub-step** is similarly compact. Constraints typically read as one sentence, though some normative rules carry a brief justification clause. These are authorial guidelines for the Step 2 reshaping pass; Phase 6b no longer fails the build on length, since author content (long inline strings, multi-sentence imported constraint consts) is faithfully preserved through Expand and surfacing it as a hard error penalises the author rather than catching LLM drift. Structural drift in Expand is caught by the count, ordering, parity, and reference checks instead.
6. **Parameter references preserved.** `{param}` references from the resolved IR must survive into the output unchanged. Step 2 must not substitute, remove, or rename them. Step 2 must not invent new `{param}` references for names not in the skill's parameter list.
6b. **Local binding references resolved.** `local_ref` slots (e.g., `{diagnosis}` where `diagnosis` is a local binding, not a declared parameter) must be resolved by Step 2 into natural-language cross-references in the prose. They must **not** survive as literal `{name}` tokens in the output — the consuming LLM already produced the referenced value in a prior step. For example, `{diagnosis}` might become "the diagnosis from your earlier analysis" or "the diagnosis identified in step 1." Step 2 uses the local's name and the producing step's position/content to generate a clear cross-reference.
6c. **Output target tokens resolved.** Output targets (`OutputContract.form`, per [[ir-schema]] §OutputContract) must be described in prose. The literal source token must not survive anywhere in the compiled Markdown:
   - When `form == Identifier(name)` (from `return <name>`), the literal `<name>` token (angle brackets and identifier) must be absent.
   - When `form == Description(text)` (from `return <"…">`), the literal `<"…">` token, the surrounding angle brackets, and the verbatim quoted `text` content must all be absent — Step 2 paraphrases the description into a Step-shaped sentence rather than pasting it.
   Both leak shapes are flagged by the same diagnostic, `G::expand::output-target-leak` (the validator's textual scan is form-agnostic; see §4.1 and the diagnostic table in §4.2).
7. **No authoring artifacts.** No `generated` markers, no `with` modifier text, no import paths, no IR field names. `{param}` references for declared parameters are the only authoring-adjacent syntax that survives. Local binding references are fully resolved into prose.
8. **Standard Markdown only.** Headings, numbered lists, bulleted lists, inline emphasis. No HTML, no tables, no code blocks inside steps.

The frontmatter (`name`, `description`, `effects`) is **not** produced by Step 2. It is assembled deterministically by Phase 7 (Emit) from skill-level IR metadata. The `## Parameters` section is assembled by Step 2: the deterministic emitter scaffolds each bullet (`- **name** (default: …)` / `(required)` trailer) and emits a `ParamDescription` span where the LLM (when wired) fills the description. Step 2 also produces the body sections (`## Context`, `## Steps`, `## Constraints`) at peer H2 level.

### 3.5 Deterministic Emitter Responsibilities

The deterministic emitter owns all structure that does not require natural-language judgement. The LLM, when wired, fills only the typed spans listed below. Today's stub filler preserves observable behavior for span kinds where the deterministic fallback reads acceptably (see [[llm_expand_pass]] for the per-kind LLM contract).

**Owned by the deterministic emitter (no span emitted):**

- Section shape: peer-level `## Parameters`, `## Context`, `## Steps`, `## Constraints` H2s, plus the H3 `### Procedure: <name>` (nested under whichever body H2 came last).
- Numbered Step list, lettered sub-step list (with letter reset per Branch arm), and bulleted `## Constraints` list.
- Constraint rendering — the four-form lock (`hard avoid`, `soft avoid`, `hard require`, `soft require`) per [[docs/reference/compiled-output]] §Constraint Rendering.
- `OutputContract.Identifier` return fold — the locked suffix `, and return that as your result.` (or the standalone form `Return <name> as your result.` for return-only skills/procedures), with `<name>` snake_case → space-separated by the shared `kebab_case` / `snake_to_words` helpers.
- Pure-predicate Branch projection — all three sub-cases from §3.3 (single-arm `Decide whether <resolved predicate text> applies and, if so:`; multi-arm `Decide which of the following applies and follow only that path:` with `If <resolved predicate text>:` arm headers; `Otherwise:` else-arm header). All three predicate forms (`.applies()`, string-const, inline literal) use the same framing; the emitter reads the resolved predicate text from `resolved_predicates` (for `.applies()` and const forms) or directly from the condition string (for inline literals).
- External-file Call Step template — `Load and follow the procedure in \`{procedure_path}\`.`.
- `## Parameters` bullet scaffolding — bold name and `(default: …)` / `(required)` trailer.
- Procedure section ordering (by first reference from `## Steps`) and procedure-name kebab-casing.

**Filled by spans (LLM when wired; stub today):**

| `SpanKind` | What the LLM fills | Stub behavior today | Cross-reference |
|---|---|---|---|
| `ParamDescription` | A brief description of the parameter from its name, type, default, and usage context. | Empty string — bullet renders as `- **name** (required)` / `(default: …)`. | [[llm_expand_pass]] §1.5 |
| `DescriptionReturnFold` | A Step-shaped paraphrase of the `OutputContract.Description` text, folded into the final Step. | Verbatim description text slotted into the locked Description-suffix wrapper. | [[llm_expand_pass]] §1.3 |
| `BranchCondition` | Natural-language prose for a mixed-condition `if`/`elif` arm header (e.g., `complex_change_required and not is_dry_run` → `If the requested change requires regenerating multi-line prose and this is not a dry run:`). The span payload includes the condition source string, the `resolved_predicates` map for predicate-token substitution, and the `condition_kinds` classification list so the LLM knows which tokens are predicates and which are booleans. | Verbatim condition expression slotted into `If <expr>:`. | [[llm_expand_pass]] §1.4 |
| `CallBodyShape` | Step prose that weaves the `with` modifier, scoped constraints, and local-binding cross-references into the resolved body. | Spans are emitted only when `site_modifier` or `local_refs` are non-empty; the stub hard-fails with `G::expand::llm-required-for-call` and the lib-level callers convert that into `CompileOutcome::Diagnostics`, suppressing the `.md` write. Trivial Calls do not emit a span and render via the deterministic literal template. Scoped-constraint weaving is deferred (see [`todo/expand-todos`](../../todo/expand-todos.md)). | [[llm_expand_pass]] §1.1, §1.2 |

The scaffold-with-spans IR (`Scaffold`, `Chunk`, `SpanRef`, `SpanKind`, `SpanPayload`) is internal to the `glyph-core::emit` module. It is not exposed via `--emit-ir` and is not stable across compiler versions.

### Step 2 fill-time diagnostics

The fill layer (`crates/glyph-core/src/emit/stub_fill.rs`) can refuse to fill a span before the merger runs. These diagnostics are distinct from §4.2's Phase 6b structural catalog — they fire **before** any `.md` text is produced. The single ID today:

| ID | Trigger |
|---|---|
| `G::expand::llm-required-for-call` | A `CallBodyShape` span is emitted (because the Call has a `with` modifier or non-empty `local_refs`) and the build is using the stub filler instead of the LLM filler. |

Relationship to Phase 6b: this diagnostic catches the *configuration / filler-wiring* failure that would otherwise silently elide modifier intent or LLM-grade local-ref cross-references. Phase 6b's complementary structural checks (`G::expand::modifier-leaked`, `G::expand::unresolved-local-ref`) catch the *content* failure when the LLM filler runs but produces non-conforming prose.

## 4. Phase 6b: Validation Gate

Phase 6b is the deterministic check that runs between Step 2 and Emit. It is architecturally analogous to Phase 5 (Validate): a pure pass/fail gate that owns correctness for the work the LLM just did. No LLM involvement.

### 4.1 What 6b Verifies

For the Markdown returned by Step 2:

- **Section shape.**
  - H2 sections sit at peer level. Allowed H2 names in Phase 1: `## Parameters` (conditional), `## Context` (conditional), `## Steps` (conditional only for pure constraint-only skills), `## Constraints` (conditional). Phase 3 will extend the catalogue with freeform headings.
  - No `## Instructions` wrapper is emitted.
  - At least one of `## Steps` or `## Constraints` is present (per [[docs/reference/compiled-output]]).
  - H2 ordering (canonical default): `## Parameters` first (if present), then `## Context`, then `## Steps`, then `## Constraints`. `### Procedure: <name>` sections appear at H3 — nested under whichever body H2 came last — in order of first reference from `## Steps`.

- **Role preservation (1-to-1 count).**
  - **Top-level Step count.** The number of top-level numbered items under `## Steps` equals:
    ```
    (count of top-level FlowNodes whose role is Step)
    + (count of top-level Branch nodes × 1)
    - (1 if the flow ends with a Return that folds into the last Step)
    ```
    Each top-level `Branch` node contributes exactly **1** to the top-level count, regardless of how many arms it has or how many statements each arm contains. `Return` folds into the preceding Step and does not produce its own numbered item.
  - **Per-Branch sub-step count.** Each `Branch` projects to a single numbered Step with lettered sub-steps per arm ([[docs/reference/compiled-output]] §Constraint Rendering). For each arm, the count of lettered sub-steps (`a.`, `b.`, `c.`, resetting per arm) equals the count of Step-projecting nodes in that arm's body. `Constraint` nodes inside an arm do **not** receive their own letter — they inline into adjacent sub-step prose. **Nested `Branch` recursion stops at one level.** A `Branch` nested inside an outer `Branch`'s arm contributes exactly **1** to that arm's sub-step count (it does not re-expand into n sub-steps per its own arms) and flattens into prose within its parent sub-step. In practice, Repair §4.9 auto-extracts nested branches into `generated block` declarations before Phase 6b, so the validator typically sees a `Call` to the extracted block rather than a literal nested `Branch`; this counting rule is defensive for cases where extraction does not run. The `## Steps` count formula at the top of this section — `(Step nodes) + (Branch nodes × 1) − (Return folds)` — is consistent with this rule. The agent-side counter described in [[agent-skill]] §`validate-output` uses the same rule.
  - The number of bulleted items under `## Constraints` equals the count of **top-level** `Constraint` IR nodes — i.e., entries in `Skill.constraints` / `Block.constraints` / `ExportBlock.constraints` after Lower's flow-top-level hoisting ([[ir-and-semantics]] §Flow-Level Constraint Markers). Two categories are excluded from this count and instead projected into Step prose: (a) **scoped constraints** carried on `Call` nodes (§3.2), and (b) **branch-scoped `Constraint` flow nodes** that remain inside `Branch` bodies after Lower (these inline into the conditional Step's sub-step prose, per [[docs/reference/compiled-output]] §Constraint Rendering).
  - Ordering under `## Steps` matches the IR's `flow:` order.

- **Procedure section validation** (for `same_file_procedure` projections).
  - One `### Procedure: <name>` section per unique callee with `projection_mode: same_file_procedure`.
  - Procedure name in H3 matches the callee name (kebab-case).
  - Numbered items in each procedure section equal the callee's flow node count.
  - If the callee declares body-level constraint markers OR body-level `context` markers, a preamble of standalone paragraphs exists between the H3 heading and the numbered step list. Preamble paragraphs are blank-line separated from each other AND from the step list, and **do not** count toward the per-procedure numbered-item count above. The locked preamble shape (4-form constraint template; `**<kebab-name>:** <text>` for name-ref `context`; `**Context:** <text>` for inline-string `context`) is contracted at [[docs/reference/compiled-output]] §Procedure Preamble (Tier 2 and Tier 3); rationale lives in [[0025-context-preamble-format]].
  - Every Step that references a procedure uses the procedure's name in its prose.
  - Reference count: the number of Steps referencing procedure X matches the number of Call nodes targeting X with `same_file_procedure` projection.
  - No duplicate procedure names.
  - Procedure sections (H3) appear after the body H2s (`## Context`, `## Steps`, `## Constraints`), nested under whichever body H2 came last, ordered by first reference.

- **External file reference validation** (for `external_file` projections).
  - Every Call node with `projection_mode: external_file` produces a Step whose prose includes the procedure file path from `procedure_path`.
  - The file path in the Step matches the `procedure_path` on the `ResolvedCall` node.
  - No `### Procedure:` section is emitted for external-file projections.

- **Parameter reference validity.**
  - Every `{...}` reference in any list item must correspond to a declared parameter in the skill's `InputContract`. Invented parameter references are rejected.
  - Every parameter from the `InputContract` that appears in the Step 1 resolved IR must still appear as a `{param}` reference in the output (Step 2 must not silently drop them).
  - No `local_ref` slots survive as literal `{name}` tokens in the output. Phase 6b iterates each Call's `local_refs` array ([[ir-schema]] §Resolved IR) and checks that the corresponding `{name}` token no longer appears literally in the compiled Markdown. A surviving local-ref is `G::expand::unresolved-local-ref` (error).
  - No `with` modifier string from any `Call` node appears verbatim in the output (it should have been consumed, not quoted).

- **Parameters section shape.**
  - If the skill has parameters, `## Parameters` must be present and contain exactly one bulleted item per `InputContract` parameter.
  - Each item must include the parameter name in bold and a brief description. Each item must end with either a `(default: <value>)` trailer (when the parameter has a default) or a `(required)` trailer (when it does not). Skill parameters use both forms; export-block parameters always carry a default per [[language-surface]] §3.8.
  - If the skill has no parameters, `## Parameters` must not be present.

- **Markdown parses cleanly.** No structural malformation in the output.

- **Frontmatter non-interference.** If Step 2 returned anything resembling YAML frontmatter (a `---` block at the top), 6b rejects the output. Frontmatter assembly is Emit's job.

### 4.2 Diagnostic Shape

6b failures are reported as diagnostics using the schema in [[docs/reference/diagnostics]]. IDs follow the namespace convention with the `expand` segment reserved for this phase:

| ID | Classification | Trigger |
|---|---|---|
| `G::expand::extra-h2` | error | Step 2 emitted an H2 outside the Phase 1 catalogue (`## Parameters`, `## Context`, `## Steps`, `## Constraints`). Phase 3 extends the catalogue with freeform headings. |
| `G::expand::missing-instructions` | error | RETIRED post-Phase-1. Reserved for forward-compat; no longer emitted because the `## Instructions` wrapper is gone — its role is now covered by `extra-h2` and the body H2 count checks. |
| `G::expand::extra-h3` | error | RETIRED post-Phase-1. Reserved for forward-compat; with body sections now at H2, the only legal H3 is `### Procedure: <name>` (which has its own dedicated diagnostics). |
| `G::expand::step-count-mismatch` | error | Number of top-level `## Steps` items does not match expected top-level Step count (see §4.1 count formula) |
| `G::expand::substep-count-mismatch` | error | Number of lettered sub-steps in a Branch's arm does not match the count of Step-projecting nodes in that arm's IR body |
| `G::expand::constraint-count-mismatch` | error | Number of `## Constraints` items does not match `Constraint` node count |
| `G::expand::context-count-mismatch` | error | Number of `## Context` items does not match the IR's top-level `context` array length on the skill/block |
| `G::expand::step-order-mismatch` | error | Step order diverges from `flow:` order |
| `G::expand::invented-param-ref` | error | `{...}` reference does not match any declared parameter |
| `G::expand::dropped-param-ref` | error | A parameter reference from Step 1 output was silently removed by Step 2 |
| `G::expand::unresolved-local-ref` | error | A `local_ref` slot survived as a literal `{name}` token — Step 2 failed to resolve it into prose |
| `G::expand::output-target-leak` | error | An output target literal survived in compiled Markdown — either `<name>` (identifier form) or `<"…">` / its quoted description text (descriptive form). The same diagnostic ID covers both forms; the `OutputContract.form` field on the violating contract distinguishes which leak shape was checked. |
| `G::expand::modifier-leaked` | error | `with` modifier string appears verbatim in output |
| `G::expand::params-section-mismatch` | error | `## Parameters` item count does not match `InputContract` parameter count |
| `G::expand::params-section-missing` | error | Skill has parameters but `## Parameters` section is absent |
| `G::expand::params-section-spurious` | error | Skill has no parameters but `## Parameters` section is present |
| `G::expand::frontmatter-returned` | error | Step 2 returned YAML frontmatter |
| `G::expand::malformed-markdown` | error | Output does not parse as Markdown |
| `G::expand::procedure-count-mismatch` | error | Number of `### Procedure:` sections does not match count of `same_file_procedure` projection calls |
| `G::expand::procedure-name-mismatch` | error | Procedure H3 name does not match any callee with `same_file_procedure` projection |
| `G::expand::procedure-step-count-mismatch` | error | Numbered items in a procedure section do not match the callee's flow node count |
| `G::expand::procedure-ref-missing` | error | A `same_file_procedure` Call produced no procedure reference in its Step prose |
| `G::expand::procedure-ref-dangling` | error | Step references a procedure name that has no matching `### Procedure:` section |
| `G::expand::procedure-duplicate` | error | Same procedure name appears in two or more `### Procedure:` sections |
| `G::expand::procedure-order` | error | `### Procedure:` sections are not ordered by first reference from `## Steps` |

All 6b diagnostics are classified `error`, not `repairable`. Phase 3 Repair operates on source; 6b failures are a Step 2 output problem and are handled by the retry / fallback policy in §5, not by re-running Repair.

### 4.3 What 6b Does Not Check

- **Semantic faithfulness of wording.** 6b does not verify that the prose the LLM produced for a step actually reflects the IR node's meaning. That is out of scope for a deterministic gate. The Safety Sandwich bounds the LLM structurally, not semantically; semantic drift is mitigated by the single-string rule for generated bodies ([[docs/architecture/repair]] §5) and by the resolved-body text flowing in as Step 2's primary input.
- **Effect correctness.** Effects are fixed in Phase 4 and validated in Phase 5. Step 2 cannot touch them.
- **Style.** Tone, formality, and clarity are not checked. Only the structural contract is enforced.
- **Markdown well-formedness via a real Markdown parser.** 6b's lightweight structural checks (§4.1) plus `G::expand::malformed-markdown` are sufficient for MVP. A full parser-based well-formedness pass beyond these checks is **deferred** — see [[expand-todos]].
- **No-embedded-HTML scan.** A scan rejecting raw HTML in Step 2's output is **deferred** — false-positive risk on legitimate constraint prose mentioning HTML tags is real, and consuming LLMs treat the file as text. See [[expand-todos]].

## 5. Failure Policy

Step 2 is not infallible. The deterministic emitter cannot fail — its output is a function of the IR — so the failure surface is the span-fill layer. Per-span retry is the unit of failure handling: when a span fill is rejected by the merger or by Phase 6b, the deterministic structure is preserved and only the failing span IDs are retried in isolation. Step 2 either succeeds (after at most two retries per failure mode, see §5.5) or hard-fails; the user re-runs.

There is **no deterministic fallback for span content** that requires natural-language judgement: the `with` modifier is the construct that makes a mechanical projection structurally low-quality (its weaving into prose has no good deterministic phrasing — see [[compiler-pipeline]] §Phase 6 and the discussion in [[foundations]] #18 of where the LLM is load-bearing). Maintaining a second filler that is always uglier than the primary path would erode trust in the abstraction every time it triggered. Span kinds whose stub fill reads acceptably today (`BranchCondition` verbatim slotting, `DescriptionReturnFold` verbatim slotting) are documented as such in §3.5; they are not architectural fallbacks — they are explicit, span-scoped behaviors the stub filler exposes until the LLM pass lands.

### 5.1 Transient Failure (network or 5xx)

Retry up to 3 times with exponential backoff. After exhaustion, emit `G::expand::llm-unavailable` (error) and abort compilation. No `.md` file is written. The user re-runs.

### 5.2 Malformed Output (does not match expected shape)

Up to **two retries** with progressively richer feedback. Each retry's prompt includes:

1. **The original prompt** — resolved IR, output template, formatting rules (unchanged).
2. **The model's previous failed output** — verbatim, clearly labeled as "your previous attempt."
3. **A structured violation report** — naming the failure (e.g., "the previous attempt did not parse as Markdown" or "the previous attempt contained a YAML frontmatter block; frontmatter is assembled by Emit, not Step 2") and pointing to where in the previous output the violation appeared.
4. **An edit directive** — "Edit your previous output to fix these violations rather than starting from scratch."

This gives the LLM the structural target, its own draft, and a precise pointer to what's wrong, so the retry can converge by editing rather than regenerating.

If both retries also fail, emit the specific 6b structural diagnostic that fired (e.g., `G::expand::malformed-markdown` or `G::expand::frontmatter-returned`) and abort. No `.md` file is written.

### 5.3 Phase 6b Validation Failure (structural rejection)

Up to **two retries per failing span** with the same info-rich feedback model as §5.2 — explicitly **revise-with-feedback**, not clean-slate regeneration. The deterministic scaffold is preserved; only the spans whose fills produced the 6b violation are retried. Each retry reads the previous attempt's span fill and fixes the specific 6b violations rather than re-projecting the IR from scratch. Each retry's prompt includes:

1. **The original prompt.**
2. **The model's previous failed output** — verbatim.
3. **The specific 6b violation report** — naming the diagnostic ID(s) (per §4.2), the failing nodes by **stable IR node ID** (§3.1), and where in the previous output the violation appeared. Examples:
   - "the previous attempt produced 5 steps but the IR has 6 Step-projecting nodes; node `n3` (Call to `identify_root_cause`) was missing from `## Steps`."
   - "the previous attempt invented a `{ctx}` reference in Step 4; `ctx` is not declared in the skill's `InputContract`."
4. **An edit directive** — "Edit your previous output to fix these violations rather than starting from scratch."

If both retries also fail 6b, emit the specific 6b diagnostic that fired (the catalog in §4.2) and abort. **The last failed `foo.md` is left on disk** so the user can read the failed prose to diagnose; the agent does not silently revert to the mechanical compiler-emitted `foo.md`. The 6b diagnostics are surfaced on stderr.

**Determinism of retries.** Because each retry's prompt differs from the original (it includes the previous failed output and the violation report), retries are inherently non-idempotent across runs — even at temperature 0, the trajectory diverges. This does not change §7's existing non-idempotence claim; it just makes the source of divergence on the failure path explicit.

**Cache interaction.** Failed attempts are **never cached**. Only the final successful Step 2 output enters the cache. The cache key is `(skill IR, prompt template, model id)`; retry history is invisible to the cache layer ([[compiler-pipeline]] §Cacheability).

### 5.4 Quality

Semantic wrongness — output that passes 6b but reshapes prose in a way the author would object to — is not detected by the compiler. The Safety Sandwich bounds the LLM structurally, not semantically ([[foundations]] #18). The mitigations are the single-string rule for generated bodies ([[docs/architecture/repair]] §5), the resolved-body text flowing into Step 2 unchanged, and the Phase 6b slot/role/count enforcement.

### 5.5 Retry Budget Constants

| Path | Budget |
|---|---|
| Transient (network/5xx) | 3 retries with exponential backoff |
| Malformed output | 2 retries with info-rich feedback (original prompt + previous failed output + violation report + edit directive) |
| Phase 6b validation failure | 2 retries **per failing span** with info-rich feedback (original prompt + previous failed span fill + 6b violation report by IR node ID + edit directive). The deterministic structure is preserved across retries. |

These numbers are compiler-config values, not hardcoded constants.

### 5.6 User-Visible Behavior Summary

| Path | User sees |
|---|---|
| Step 2 passes 6b on attempt 1 | Normal compiled `.md`, no warnings |
| Step 2 passes 6b on retry | Normal compiled `.md`, no warnings (the retry is invisible) |
| Transient failure persists | No `.md` written, `G::expand::llm-unavailable` on stderr, non-zero exit |
| Malformed output persists | No `.md` written, the specific 6b diagnostic on stderr, non-zero exit |
| Validation failure persists | No `.md` written, the specific 6b diagnostic on stderr, non-zero exit |

## 6. Partial Failure

A **partial failure** is an imagined scenario where Step 2 returns Markdown in which some list items are well-formed and others are malformed — e.g., 5 of 6 Steps render cleanly and the 6th contains a `{param}` placeholder.

**MVP policy: all-or-nothing per invocation.** Mixed output is never emitted. Phase 6b treats any 6b violation as a full Step 2 failure; it does not attempt to salvage the well-formed items.

Rationale:

- **The Safety Sandwich collapses otherwise.** If 6b accepts partial outputs, every 6b pass-with-patches is a point where the LLM-produced content has leaked past the deterministic boundary. The boundary exists precisely to prevent that.
- **No principled repair target.** If 6b tries to fix only the broken items, it would need a per-node LLM retry with only one node's resolved IR visible — the opposite of the whole-skill prompting model Step 2 relies on. The re-prompted node would lose its surrounding context and likely produce worse prose than a full retry.
- **Diagnostics stay clean.** One invocation produces one output or one diagnostic path, never a mix.

Compiler authors who are tempted to add a per-node repair path should instead consider whether Step 1 is doing too little (the LLM should not need so much latitude) or whether the retry budget in §5.5 is too small.

## 7. Determinism and Reproducibility

Step 2 is **not idempotent** across model versions, prompt variations, or even repeated runs with the same model at temperature > 0. Two compilations of the same skill may produce two compiled `.md` files whose prose differs word-for-word. This is an honest limitation of any LLM pass.

What *is* guaranteed:

- **Structural idempotence.** Every Step 2 output that passes 6b has the same structural shape: the same H2, the same H3s, the same number of Steps in the same order, the same number of Constraints. Word-level prose is not stable; IR-level projection is.
- **Semantic content bounds.** The resolved body text going into Step 2 is identical across runs (Step 1 is fully deterministic). The LLM may reshape wording but cannot invent content that was not in the resolved IR, because 6b enforces the 1-to-1 role mapping.
- **Hard-fail is deterministic.** When Step 2 cannot produce a 6b-passing output within the retry budget (§5.5), the compiler aborts deterministically with the specific 6b diagnostic. There is no fallback emitter; failure is loud, not silent.

**Contrast with Repair's idempotence** ([[docs/architecture/repair]] §4.5): Repair is idempotent because its detection mechanism is name resolution — if every name already resolves, there is nothing for Repair to do, so no LLM is invoked on the second run. Step 2 has no such no-op path: every invocation produces prose from structured IR, and that production is an LLM call every time. Repair makes the *source* stable; Step 2 makes the *output* structurally stable but not textually stable.

Practical implications for tooling:

- Build caching ([[compiler-pipeline]] §Cacheability) must include Step 2's prose output in the cache entry if byte-stable output is required across runs.
- Diff-based CI that compares compiled `.md` across commits will see prose churn from Step 2 even when the source is unchanged. Teams that need stable compiled outputs should cache them alongside the source, not recompile on every build.
- Snapshot tests should assert structural shape (counts, ordering, frontmatter) rather than byte-identical prose.

## 8. Worked Example

Using the `fix_bug` skill from [[docs/reference/compiled-output]] §Complete Example.

### 8.1 Post-Lower IR (schematic)

After Phases 1–5, the IR for `fix_bug` looks roughly like:

```text
Skill {
  name: "fix_bug"
  description: "Debug and fix a bug in the codebase with minimal, targeted changes."
  input_contract: [ Param(name: "scope", type: String, default: ".") ]
  effects: [ reads_files, writes_files, runs_commands ]
  constraints: [
    Constraint(strength: soft, polarity: avoid, resolved_text: "Making changes outside {scope}."),
    Constraint(strength: soft, polarity: require, resolved_text: "Follow the repository's existing patterns before introducing new abstractions.")
  ]
  flow: [
    Call(target: "inspect_failure", args: {area: <scope>}, body: "Inspect the failure in {area} and identify what is failing.", site_modifier: "focus on auth boundaries"),
    Call(target: "identify_root_cause", args: {}, body: "Identify the root cause of the issue."),
    InlineInstruction(text: "Don't propose a fix until you've confirmed the root cause."),
    Call(target: "patch_minimally", args: {}, body: "Apply the smallest change that fixes the issue."),
    InstructionRef(name: "validate_before_success", resolved_text: "Validate that the fix works before reporting success."),
    Return(expr: Call(target: "summarize_changes", args: {}, body: "Summarize what was changed and why."))
  ]
}
```

### 8.2 Step 1 Output for `fix_bug`

Step 1 resolves bare names and inline strings, preserving `{param}` references as named slots:

- `Call(inspect_failure)` → `resolved_body_text: "Inspect the failure in {scope} and identify what is failing."`, `site_modifier` preserved as `"focus on auth boundaries"`. The `{area}` parameter in the generated block mapped to `scope` at the call site, so it becomes `{scope}`.
- `Call(identify_root_cause)` → `resolved_body_text: "Identify the root cause of the issue."` (no params).
- `InlineInstruction` → unchanged.
- `Call(patch_minimally)` → `resolved_body_text: "Apply the smallest change that fixes the issue."`
- `InstructionRef(validate_before_success)` → `resolved_text: "Validate that the fix works before reporting success."`
- `Return(Call(summarize_changes))` → `resolved_body_text: "Summarize what was changed and why."`, flagged as the return.
- Constraints → unchanged (their text is already concrete).
- Parameter metadata → `[ { name: "scope", type: String, default: "." } ]`.

After Step 1, every bare name and inline string has concrete content. `{scope}` references are preserved as named slots for the consuming LLM. This is the input to Step 2.

### 8.3 Step 2 Input (conceptual serialization)

Step 2's prompt contains the resolved IR, the output template, and the formatting rules. Conceptually:

```text
You are producing the `## Parameters`, `## Context`, `## Steps`, and `## Constraints` sections of a compiled skill file. All four are peer H2s (there is no `## Instructions` wrapper).

Parameters:
  - name: "scope", type: String, default: "."

Input IR (resolved):

Flow (in order):
  1. Call: body = "Inspect the failure in {scope} and identify what is failing."
           with = "focus on auth boundaries"
           role = Step
  2. Call: body = "Identify the root cause of the issue."
           role = Step
  3. InlineInstruction: text = "Don't propose a fix until you've confirmed the root cause."
           role = Step
  4. Call: body = "Apply the smallest change that fixes the issue."
           role = Step
  5. InstructionRef: text = "Validate that the fix works before reporting success."
           role = Step
  6. Return(Call): body = "Summarize what was changed and why."
           role = Step, fold_return = true

Constraints (unordered):
  - strength=soft, polarity=avoid, text="Making changes outside {scope}."
  - strength=soft, polarity=require, text="Follow the repository's existing patterns before introducing new abstractions."

Rules:
  - Emit `## Parameters` with one bulleted item per parameter (name, description, and either `(default: <value>)` or `(required)` per `compiled-output.md` §`## Parameters`).
  - Emit body sections as peer H2s: `## Context` (if any), `## Steps`, `## Constraints` (if any). No `## Instructions` wrapper.
  - Every flow item -> exactly one numbered Step, in order.
  - Every constraint -> exactly one bulleted Constraint.
  - The final Step must end with a sentence summarizing the return value.
  - Apply `with` modifiers when present; do not quote them.
  - Preserve {param} references — do not substitute or remove them.
  - No frontmatter, no authoring artifacts.
```

### 8.4 Step 2 Output

```md
## Parameters
- **scope**: Area of codebase to focus on (default: ".")

## Steps

1. Inspect the failure in {scope}, focusing on auth boundaries and permission checks. Identify what is failing and whether any auth-related logic is involved.
2. Identify the root cause of the issue.
3. Don't propose a fix until you've confirmed the root cause.
4. Apply the smallest change that fixes the issue.
5. Validate that the fix works before reporting success.
6. Summarize what was changed and why, and return that as your result.

## Constraints

- Do not make changes outside {scope}.
- Follow the repository's existing patterns before introducing new abstractions.
```

### 8.5 Phase 6b Check (passing)

6b walks the output against the IR:

- Parameters section: skill has 1 parameter (`scope`) → `## Parameters` has 1 bulleted item. Pass.
- Section shape: peer-level H2s `## Parameters`, `## Steps`, `## Constraints` (no `## Context` because the IR has none). Pass.
- Step count: 6 IR flow nodes → 6 numbered items. Pass.
- Step order: matches IR flow order. Pass.
- Return folding: the 6th Step ends with "and return that as your result." Pass.
- Constraint count: 2 IR constraints → 2 bulleted items. Pass.
- Parameter reference validity: `{scope}` appears in Steps and Constraints and matches the declared parameter. No invented references. Pass.
- "focus on auth boundaries" modifier was applied to Step 1 but not quoted verbatim. Pass.
- Content shape: each Step is ≤ 3 sentences, each Constraint is 1 sentence. Pass.
- Frontmatter: none present (correct — Emit will add it). Pass.

6b emits no diagnostics. The output proceeds to Phase 7 (Emit), which prepends the YAML frontmatter from skill metadata and writes `fix_bug.md`.

## 9. Cross-References

- **Author-facing contract** ([[design/expand]]): the short product-level statement of what Expand changes and the role-preservation guarantee an author may rely on.
- **Pipeline architecture** ([[compiler-pipeline]] §Phase 6): canonical description of Expand's two-step model. This document refines Step 2 and adds Phase 6b.
- **Repair architecture** ([[docs/architecture/repair]]): contrast — Repair is source-to-source, idempotent, and driven by diagnostics; Step 2 is IR-to-Markdown, not idempotent, and driven by the resolved IR itself.
- **IR and semantics** ([[ir-and-semantics]]): the node shapes Step 2 consumes (`InputContract`, `Step`, `Constraint`, `Context`, `OutputContract`), strength/polarity model, effect vocabulary.
- **Compiled output** ([[design/compiled-output]]): the output shape Step 2 must produce — peer-level H2s `## Parameters` (conditional), `## Context` (conditional), `## Steps`, `## Constraints` (conditional), formatting rules. YAML frontmatter is assembled by Emit.
- **Diagnostics** ([[docs/reference/diagnostics]]): the diagnostic schema and ID convention used by Phase 6b.
- **CLI surface for the external Phase 6b runner** ([[docs/reference/cli]] §`glyph validate-output`): exit codes and IO contract.
- **Foundations** ([[foundations]]): #18 (deterministic passes own correctness — the Safety Sandwich), #33 (novice learnability — motivates `with` modifier as the only call-site specialization mechanism, which in turn motivates the no-fallback posture in §5).
- **ADRs:** [[0016-llm-reshape-no-deterministic-fallback]], [[0018-phase-6b-structural-only-gate]].
