# Glyph Expand Pass (Phase 6)

This document is the single authoritative reference for the Expand pass, with specific focus on its LLM sub-pass (**Step 2**) and the deterministic validation gate that follows it (**Phase 6b**). It expands the short treatment in `pipeline.md` Phase 6 at the fidelity of `repair.md`.

Expand has two sub-steps:

- **Step 1 (deterministic resolution)** — mechanical resolution of bare names, inline strings, and parameter metadata into the IR. Parameters are preserved as `{param}` slots, not substituted. No LLM. Fully specified in `pipeline.md` Phase 6 and `compiled-output.md`.
- **Step 2 (LLM reshaping)** — turns the resolved IR into agent-facing prose. This document focuses here.
- **Phase 6b (structural validation)** — deterministic check that Step 2's output faithfully projects the input IR. Runs between Step 2 and Phase 7 (Emit).

The ordering Step 1 → Step 2 → 6b is the closing half of the **Safety Sandwich** (`foundations.md` #18): every LLM pass is framed by deterministic work on both sides. Step 1 hands the LLM a fully-resolved IR; Phase 6b checks that the LLM's Markdown still matches the IR structurally before Emit gets it.

## 1. Purpose

Step 2 exists to turn structured IR content into readable agent instructions. After Step 1, every node already carries resolved content (bare names inlined, parameter references preserved as `{param}` slots). If Emit ran directly on the Step 1 output, the result would be correct but stilted — a sequence of short declarative sentences with no shaping, no application of `with` modifiers, no constraint wording calibrated to strength and polarity keywords, and no return folding. Step 2 is the pass that makes the output *useful*, not just *correct*.

Step 2 is also where `with` modifiers are applied. The modifier is the only call-site specialization mechanism in MVP (`pipeline.md` Phase 6), and its application is an LLM task by design: the modifier is natural language, the body it reshapes is natural language, and the output must be natural language. Step 2 is the single place in the pipeline where this reshaping happens.

## 2. Non-Goals

Step 2 must not:

- repair invalid source or add missing definitions (that is Phase 3 Repair, `repair.md`);
- introduce new IR nodes, new calls, new constraints, or new steps;
- reinterpret or reorder the skill's workflow;
- invent sections beyond `## Instructions` with its `### Context`, `### Steps`, and `### Constraints` sub-sections (see `compiled-output.md`);
- invent new `{param}` references that do not correspond to declared parameters (declared parameter references must be preserved), or fail to resolve `local_ref` slots into natural-language prose;
- change effects, types, or the call graph;
- re-materialize content that was already prose after Step 1 (inline strings and resolved `text` references pass through untouched);
- serve as a second repair loop; it has no access to diagnostics and no ability to rewrite source.

If Step 2 cannot cleanly reshape a node, that is a Phase 6b failure (see §4), not an opportunity for Step 2 to be more creative.

## 3. Input / Output Contract

### 3.1 Input schema

Step 2 receives a **resolved IR** — the same IR shape produced by Phase 4 (Lower) and validated by Phase 5 (Validate), but with bare names and inline strings already resolved by Step 1. Parameter references (`{param}`) are preserved as named slots. The LLM does **not** see the original source file, the authoring-level declarations, or any unresolved names.

Specifically, Step 2 receives:

- the full validated IR of the skill (all nodes, not one node at a time) in a serialized form — JSON or an equivalent structured encoding;
- for every `Call` node: `{ target_name, resolved_body_text, local_refs, site_modifier?, role, effects, scoped_constraints, position }`, where `resolved_body_text` is the post-Step-1 body with `{param}` references preserved as named slots and `{local}` references preserved as literal `{name}` tokens. The `local_refs` array (see `ir-schema.md` §Resolved IR) lists each local-binding slot by name and producing node ID; Step 2 cross-references this array to identify which `{name}` tokens are local bindings that must be resolved into natural-language prose. `scoped_constraints` is the list of constraints declared on the called block (see §3.2 Scoped Constraint Inlining);
- for every top-level `Constraint` node: `{ resolved_text, strength, polarity }` (text may contain `{param}` references);
- for every `InlineInstruction` node: `{ text, role }` (typically passes through);
- for every `Branch` node: `{ condition_text, then_body, elif_branches, else_body, applies_descriptions? }` with every sub-body already resolved. The optional `applies_descriptions: {block_name → resolved_description}` side-map is populated by Step 1 whenever the Branch's own `condition_text` or any `elif_branches[*].condition_text` invokes the block trigger predicate `BLOCKNAME.applies()` (see `ir-and-semantics.md` §Block Trigger Predicate). Step 2 uses the side-map to choose the projection form: when every arm's condition is *purely* one or more `applies()` calls combined by `or` (or each `if`/`elif` arm is guarded by a single `applies()` call), Step 2 emits a "decide which applies" prose frame keyed by the resolved descriptions; for mixed conditions (e.g., `block_x.applies() and not is_dry_run`), Step 2 inlines the resolved description into the larger condition prose (e.g., "If the user wants a structured plan and this is not a dry run, ...");
- for the `Return` expression: its resolved text plus a flag indicating it must fold into the final Step;
- skill-level metadata: `name`, `description` (if present), `effects` (as a list), the ordered position of each node in `flow:`, and the parameter list (names, types, and defaults if declared — used for generating the `## Parameters` section, where parameters without defaults render with a `(required)` marker per `compiled-output.md`).
- a **stable, file-local IR node ID** (e.g., `n0`, `n1`, …) on every node, assigned by Lower (`pipeline.md` Phase 4). The IDs are opaque, never appear in compiled output, and are not echoed back by Step 2 (the output contract in §3.3 is Markdown only). They exist so that Phase 6b's count + ordering checks (§4.1) and any internal diagnostic referring to "the IR node Step 2 failed to project" can name a specific node unambiguously across runs and across the parse-then-re-parse boundary inside Repair.

**Whole-skill prompting, not per-node.** Step 2 is invoked **once per skill compilation**, with the full resolved IR visible in a single prompt. This is deliberate:

- Constraint wording depends on the rest of the skill (a soft constraint reads differently alongside a hard constraint than alongside another soft one).
- `with`-reshaped prose should flow with surrounding steps.
- Return folding requires the LLM to see the final step in context so the closing sentence reads naturally.

Step 1's output is fully visible to Step 2. In fact, the resolved body text *is* the primary thing Step 2 reshapes. Sibling-node context is provided by the whole-skill prompt; the LLM sees what comes before and after every node.

The prompt given to the LLM is structured, not free-form. It contains: the resolved IR block, the target output template (`## Instructions` with its two sub-sections), the formatting rules from `compiled-output.md` §Formatting Rules, and instructions describing exactly which nodes need reshaping and which pass through.

### 3.2 Scoped Constraint Inlining

A `block` (or `export block`) called from within a flow may itself declare constraints. Per `data-flow.md` §Constraint Scoping, these constraints **do not** propagate to the caller's top-level `### Constraints`. They stay scoped to the inlined region of the call.

**IR shape.** When Lower resolves a call, the resolved-call node carries the callee's declared constraints attached as the `scoped_constraints` field. Each entry is `{ resolved_text, strength, polarity }`, identical in shape to a top-level `Constraint` node, but flagged as scoped to this call.

**Step 2 behavior.** For every `Call` node whose `scoped_constraints` is non-empty, Step 2 must weave the constraint into the prose of the inlined step(s) produced from that call's body. Two acceptable projections are permitted:

- **Inline weaving.** Fold the constraint into the step's prose itself. Example: a call whose body is "Review the changes" with scoped constraint `(hard, avoid: unrelated_edits)` becomes a Step like "Review the changes, never touching files outside the declared scope."
- **Localized framing sentence.** Prepend or append a sentence that scopes the constraint to the inlined region. Example: "While reviewing, do not modify unrelated files. Review the changes." Use this when the constraint cannot be cleanly folded into a single sentence.

Step 2 picks per call based on what reads naturally. Multiple scoped constraints on one call may be combined into a single framing sentence or distributed across the inlined steps.

**Strength and polarity wording.** The same wording rules as top-level constraints apply to scoped constraints — `hard` renders with strongest non-negotiable wording, `soft` renders with standard wording, `require` renders as a positive obligation, `avoid` renders as a prohibition (per `compiled-output.md` §Constraint Rendering).

**Output placement.** Scoped constraints **never** appear as items in `### Constraints`. The caller's `### Constraints` section lists only the caller's own top-level `Constraint` IR nodes. Phase 6b enforces this (see §4.1).

**Why this is Step 2's job.** Scoped constraints are call-site contextual: their wording depends on the surrounding step's prose, the strength/polarity, and the position in the flow. Mechanical folding produces awkward output. The whole-skill prompting model (§3.1) already gives Step 2 the visibility needed to weave gracefully.

### 3.3 Output contract

Step 2 must return **Markdown only** — specifically, the body of the compiled file below the frontmatter. It must not return JSON, IR, explanations, or commentary. The expected shape is:

```md
## Instructions

### Steps

1. <expanded step 1>
2. <expanded step 2>
...

### Constraints

- <expanded constraint 1>
- <expanded constraint 2>
...
```

The output must preserve the following structural invariants:

1. **Exactly one `## Instructions` H2.** No other H2 sections.
2. **H3 sub-sections under `## Instructions`** are limited to: `### Context`, `### Steps`, `### Constraints`, and zero or more `### Procedure: <name>` sections. `### Context` is omitted when the IR has no context. `### Constraints` is omitted when the IR has no constraints. `### Steps` is omitted only for pure constraint-only skills. `### Procedure:` sections appear only for calls with `same_file_procedure` projection.
3. **Role preservation** (1-to-1):
   - Every top-level `Step` node (and every top-level `Call`, `InlineInstruction`, `InstructionRef` that projects to a Step per `compiled-output.md`) must produce exactly one top-level numbered list item under `### Steps`, in the same order as the IR.
   - Every top-level `Branch` node must produce exactly one top-level numbered list item under `### Steps`, containing lettered sub-steps per arm (see `compiled-output.md` §Constraint Rendering). Each arm is introduced by a condition header (`If <condition>:` for `if`/`elif`, `Otherwise:` for `else`), and each Step-projecting node inside the arm produces a lettered sub-step (`a.`, `b.`, `c.`). Letters reset per arm.
   - Every `Constraint` node must produce exactly one bulleted list item under `### Constraints`. Order is not required to match the IR.
   - The `Return` expression must fold into the last `### Steps` item (or the last sub-step of the final arm, if the last Step is a Branch), not produce a separate item or section.
   - Every Call with `projection_mode: same_file_procedure` must additionally produce one `### Procedure: <name>` section with numbered items matching the callee's flow node count. The referencing Step includes the procedure name in its prose.
   - Calls with `projection_mode: external_file` produce one Step whose prose includes the file path from `procedure_path`. No `### Procedure:` section is emitted for external projections.
4. **No invented content.** Step 2 may reshape wording but must not add new steps, new sub-steps, new constraints, new sections, or commentary.
5. **Bounded length.** For non-conditional Steps, each step is **one instruction-sized paragraph** — at most three sentences, typically one or two. For conditional Steps (Branch projections), each **sub-step** is at most three sentences. Each constraint is **one sentence**. This is the floor and ceiling; longer items indicate Step 2 is inventing content, shorter items indicate Step 2 is stripping content.
6. **Parameter references preserved.** `{param}` references from the resolved IR must survive into the output unchanged. Step 2 must not substitute, remove, or rename them. Step 2 must not invent new `{param}` references for names not in the skill's parameter list.
6b. **Local binding references resolved.** `local_ref` slots (e.g., `{diagnosis}` where `diagnosis` is a local binding, not a declared parameter) must be resolved by Step 2 into natural-language cross-references in the prose. They must **not** survive as literal `{name}` tokens in the output — the consuming LLM already produced the referenced value in a prior step. For example, `{diagnosis}` might become "the diagnosis from your earlier analysis" or "the diagnosis identified in step 1." Step 2 uses the local's name and the producing step's position/content to generate a clear cross-reference.
7. **No authoring artifacts.** No `generated` markers, no `with` modifier text, no import paths, no IR field names. `{param}` references for declared parameters are the only authoring-adjacent syntax that survives. Local binding references are fully resolved into prose.
8. **Standard Markdown only.** Headings, numbered lists, bulleted lists, inline emphasis. No HTML, no tables, no code blocks inside steps.

The frontmatter (`name`, `description`, `effects`) is **not** produced by Step 2. It is assembled deterministically by Phase 7 (Emit) from skill-level IR metadata. The `## Parameters` section is assembled by Step 2: it generates a brief description for each parameter from the parameter's name, type, usage context, and default value. Step 2 also produces the `## Instructions` section body.

## 4. Phase 6b: Validation Gate

Phase 6b is the deterministic check that runs between Step 2 and Emit. It is architecturally analogous to Phase 5 (Validate): a pure pass/fail gate that owns correctness for the work the LLM just did. No LLM involvement.

### 4.1 What 6b Verifies

For the Markdown returned by Step 2:

- **Section shape.**
  - At most two H2 sections: `## Parameters` (conditional) and `## Instructions` (always present).
  - No other H2 sections exist.
  - H3 sections under `## Instructions` are limited to: `### Context`, `### Steps`, `### Constraints`, and zero or more `### Procedure: <name>` sections. No other H3s.
  - At least one of `### Steps` or `### Constraints` is present (per `compiled-output.md`).
  - H3 ordering: `### Context` first (if present), then `### Steps`, then `### Constraints` (if present), then `### Procedure:` sections (if any) in order of first reference from `### Steps`.

- **Role preservation (1-to-1 count).**
  - **Top-level Step count.** The number of top-level numbered items under `### Steps` equals:
    ```
    (count of top-level FlowNodes whose role is Step)
    + (count of top-level Branch nodes × 1)
    - (1 if the flow ends with a Return that folds into the last Step)
    ```
    Each top-level `Branch` node contributes exactly **1** to the top-level count, regardless of how many arms it has or how many statements each arm contains. `Return` folds into the preceding Step and does not produce its own numbered item.
  - **Per-Branch sub-step count.** Each `Branch` projects to a single numbered Step with lettered sub-steps per arm (`compiled-output.md` §Constraint Rendering). For each arm, the count of lettered sub-steps (`a.`, `b.`, `c.`, resetting per arm) equals the count of Step-projecting nodes in that arm's body. `Constraint` nodes inside an arm do **not** receive their own letter — they inline into adjacent sub-step prose. **Nested `Branch` recursion stops at one level.** A `Branch` nested inside an outer `Branch`'s arm contributes exactly **1** to that arm's sub-step count (it does not re-expand into n sub-steps per its own arms) and flattens into prose within its parent sub-step. In practice, Repair §4.9 auto-extracts nested branches into `generated block` declarations before Phase 6b, so the validator typically sees a `Call` to the extracted block rather than a literal nested `Branch`; this counting rule is defensive for cases where extraction does not run. The `### Steps` count formula at the top of this section — `(Step nodes) + (Branch nodes × 1) − (Return folds)` — is consistent with this rule. The agent-side counter described in `agent-skill.md` §`validate-output` uses the same rule.
  - The number of bulleted items under `### Constraints` equals the count of **top-level** `Constraint` IR nodes — i.e., entries in `Skill.constraints` / `Block.constraints` / `ExportBlock.constraints` after Lower's flow-top-level hoisting (`ir-and-semantics.md` §Flow-Level Constraint Markers). Two categories are excluded from this count and instead projected into Step prose: (a) **scoped constraints** carried on `Call` nodes (§3.2), and (b) **branch-scoped `Constraint` flow nodes** that remain inside `Branch` bodies after Lower (these inline into the conditional Step's sub-step prose, per `compiled-output.md` §Constraint Rendering).
  - Ordering under `### Steps` matches the IR's `flow:` order.

- **Procedure section validation** (for `same_file_procedure` projections).
  - One `### Procedure: <name>` section per unique callee with `projection_mode: same_file_procedure`.
  - Procedure name in H3 matches the callee name (kebab-case).
  - Numbered items in each procedure section equal the callee's flow node count.
  - If the callee declares constraints, a preamble paragraph exists before the numbered list.
  - Every Step that references a procedure uses the procedure's name in its prose.
  - Reference count: the number of Steps referencing procedure X matches the number of Call nodes targeting X with `same_file_procedure` projection.
  - No duplicate procedure names.
  - Procedure sections appear after `### Context`, `### Steps`, and `### Constraints`, ordered by first reference.

- **External file reference validation** (for `external_file` projections).
  - Every Call node with `projection_mode: external_file` produces a Step whose prose includes the procedure file path from `procedure_path`.
  - The file path in the Step matches the `procedure_path` on the `ResolvedCall` node.
  - No `### Procedure:` section is emitted for external-file projections.

- **Parameter reference validity.**
  - Every `{...}` reference in any list item must correspond to a declared parameter in the skill's `InputContract`. Invented parameter references are rejected.
  - Every parameter from the `InputContract` that appears in the Step 1 resolved IR must still appear as a `{param}` reference in the output (Step 2 must not silently drop them).
  - No `local_ref` slots survive as literal `{name}` tokens in the output. Phase 6b iterates each Call's `local_refs` array (`ir-schema.md` §Resolved IR) and checks that the corresponding `{name}` token no longer appears literally in the compiled Markdown. A surviving local-ref is `G::expand::unresolved-local-ref` (error).
  - No `with` modifier string from any `Call` node appears verbatim in the output (it should have been consumed, not quoted).

- **Parameters section shape.**
  - If the skill has parameters, `## Parameters` must be present and contain exactly one bulleted item per `InputContract` parameter.
  - Each item must include the parameter name in bold and a brief description. Each item must end with either a `(default: <value>)` trailer (when the parameter has a default) or a `(required)` trailer (when it does not). Skill parameters use both forms; export-block parameters always carry a default per `language-surface.md` §3.10.
  - If the skill has no parameters, `## Parameters` must not be present.

- **Content shape.**
  - Each non-conditional `### Steps` item is at most three sentences.
  - Each lettered sub-step within a conditional Step (Branch projection) is at most three sentences.
  - Each `### Constraints` item is a single sentence.
  - Markdown parses cleanly.

  **Sentence-counting rule.** The sentence count for an item is computed deterministically, without a tokenizer:
  1. **Strip backtick code spans** from the prose first (any text between matched single backticks). This prevents `.` inside an inline code span from being counted as a boundary.
  2. **A sentence boundary is `.`, `!`, or `?` followed by whitespace or end-of-string.**
  3. **No abbreviation special-casing.** "e.g." counts as a sentence boundary. Authors who do not want this should rewrite the sentence to avoid the abbreviation.

  This rule is agent-implementable in a few lines of code and matches the algorithm specified in `agent-skill.md` §`validate-output`. It is the authoritative sentence-counting algorithm for both the agent-side validator and any future compiler-side implementation of Phase 6b.

- **Frontmatter non-interference.** If Step 2 returned anything resembling YAML frontmatter (a `---` block at the top), 6b rejects the output. Frontmatter assembly is Emit's job.

### 4.2 Diagnostic Shape

6b failures are reported as diagnostics using the schema in `diagnostics.md`. IDs follow the namespace convention with the `expand` segment reserved for this phase:

| ID | Classification | Trigger |
|---|---|---|
| `G::expand::extra-h2` | error | Step 2 emitted an H2 other than `## Instructions` |
| `G::expand::missing-instructions` | error | Step 2 did not emit `## Instructions` |
| `G::expand::extra-h3` | error | Step 2 emitted an H3 not matching `### Context`, `### Steps`, `### Constraints`, or `### Procedure: <name>` |
| `G::expand::step-count-mismatch` | error | Number of top-level `### Steps` items does not match expected top-level Step count (see §4.1 count formula) |
| `G::expand::substep-count-mismatch` | error | Number of lettered sub-steps in a Branch's arm does not match the count of Step-projecting nodes in that arm's IR body |
| `G::expand::constraint-count-mismatch` | error | Number of `### Constraints` items does not match `Constraint` node count |
| `G::expand::step-order-mismatch` | error | Step order diverges from `flow:` order |
| `G::expand::invented-param-ref` | error | `{...}` reference does not match any declared parameter |
| `G::expand::dropped-param-ref` | error | A parameter reference from Step 1 output was silently removed by Step 2 |
| `G::expand::unresolved-local-ref` | error | A `local_ref` slot survived as a literal `{name}` token — Step 2 failed to resolve it into prose |
| `G::expand::modifier-leaked` | error | `with` modifier string appears verbatim in output |
| `G::expand::params-section-mismatch` | error | `## Parameters` item count does not match `InputContract` parameter count |
| `G::expand::params-section-missing` | error | Skill has parameters but `## Parameters` section is absent |
| `G::expand::params-section-spurious` | error | Skill has no parameters but `## Parameters` section is present |
| `G::expand::step-too-long` | error | A non-conditional step exceeds three sentences, or a sub-step within a conditional step exceeds three sentences |
| `G::expand::constraint-multi-sentence` | error | A constraint is more than one sentence |
| `G::expand::frontmatter-returned` | error | Step 2 returned YAML frontmatter |
| `G::expand::malformed-markdown` | error | Output does not parse as Markdown |
| `G::expand::procedure-count-mismatch` | error | Number of `### Procedure:` sections does not match count of `same_file_procedure` projection calls |
| `G::expand::procedure-name-mismatch` | error | Procedure H3 name does not match any callee with `same_file_procedure` projection |
| `G::expand::procedure-step-count-mismatch` | error | Numbered items in a procedure section do not match the callee's flow node count |
| `G::expand::procedure-ref-missing` | error | A `same_file_procedure` Call produced no procedure reference in its Step prose |
| `G::expand::procedure-ref-dangling` | error | Step references a procedure name that has no matching `### Procedure:` section |
| `G::expand::procedure-duplicate` | error | Same procedure name appears in two or more `### Procedure:` sections |
| `G::expand::procedure-order` | error | `### Procedure:` sections are not ordered by first reference from `### Steps` |

All 6b diagnostics are classified `error`, not `repairable`. Phase 3 Repair operates on source; 6b failures are a Step 2 output problem and are handled by the retry / fallback policy in §5, not by re-running Repair.

### 4.3 What 6b Does Not Check

- **Semantic faithfulness of wording.** 6b does not verify that the prose the LLM produced for a step actually reflects the IR node's meaning. That is out of scope for a deterministic gate. The Safety Sandwich bounds the LLM structurally, not semantically; semantic drift is mitigated by the single-string rule for generated bodies (`repair.md` §5) and by the resolved-body text flowing in as Step 2's primary input.
- **Effect correctness.** Effects are fixed in Phase 4 and validated in Phase 5. Step 2 cannot touch them.
- **Style.** Tone, formality, and clarity are not checked. Only the structural contract is enforced.
- **Markdown well-formedness via a real Markdown parser.** 6b's lightweight structural checks (§4.1) plus `G::expand::malformed-markdown` are sufficient for MVP. A full parser-based well-formedness pass beyond these checks is **deferred** — see `todo.md` §Phase 6b Validation.
- **No-embedded-HTML scan.** A scan rejecting raw HTML in Step 2's output is **deferred** — false-positive risk on legitimate constraint prose mentioning HTML tags is real, and consuming LLMs treat the file as text. See `todo.md` §Phase 6b Validation.

## 5. Failure Policy

Step 2 is not infallible. There is **no deterministic fallback emitter**: the `with` modifier is the construct that makes a mechanical projection structurally low-quality (its weaving into prose has no good deterministic phrasing — see `pipeline.md` §Phase 6 and the discussion in `foundations.md` #18 of where the LLM is load-bearing). Maintaining a second emitter that is always uglier than the primary path would erode trust in the abstraction every time it triggered. Step 2 either succeeds (after at most two retries per failure mode, see §5.5) or hard-fails; the user re-runs.

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

Up to **two retries** with the same info-rich feedback model as §5.2 — explicitly **revise-with-feedback**, not clean-slate regeneration. Each retry reads the previous attempt's `foo.md` and fixes the specific 6b violations rather than re-projecting the IR from scratch. Each retry's prompt includes:

1. **The original prompt.**
2. **The model's previous failed output** — verbatim.
3. **The specific 6b violation report** — naming the diagnostic ID(s) (per §4.2), the failing nodes by **stable IR node ID** (§3.1), and where in the previous output the violation appeared. Examples:
   - "the previous attempt produced 5 steps but the IR has 6 Step-projecting nodes; node `n3` (Call to `identify_root_cause`) was missing from `### Steps`."
   - "the previous attempt invented a `{ctx}` reference in Step 4; `ctx` is not declared in the skill's `InputContract`."
4. **An edit directive** — "Edit your previous output to fix these violations rather than starting from scratch."

If both retries also fail 6b, emit the specific 6b diagnostic that fired (the catalog in §4.2) and abort. **The last failed `foo.md` is left on disk** so the user can read the failed prose to diagnose; the agent does not silently revert to the mechanical compiler-emitted `foo.md`. The 6b diagnostics are surfaced on stderr.

**Determinism of retries.** Because each retry's prompt differs from the original (it includes the previous failed output and the violation report), retries are inherently non-idempotent across runs — even at temperature 0, the trajectory diverges. This does not change §7's existing non-idempotence claim; it just makes the source of divergence on the failure path explicit.

**Cache interaction.** Failed attempts are **never cached**. Only the final successful Step 2 output enters the cache. The cache key is `(skill IR, prompt template, model id)`; retry history is invisible to the cache layer (`pipeline.md` §Cacheability).

### 5.4 Quality

Semantic wrongness — output that passes 6b but reshapes prose in a way the author would object to — is not detected by the compiler. The Safety Sandwich bounds the LLM structurally, not semantically (`foundations.md` #18). The mitigations are the single-string rule for generated bodies (`repair.md` §5), the resolved-body text flowing into Step 2 unchanged, and the Phase 6b slot/role/count enforcement.

### 5.5 Retry Budget Constants

| Path | Budget |
|---|---|
| Transient (network/5xx) | 3 retries with exponential backoff |
| Malformed output | 2 retries with info-rich feedback (original prompt + previous failed output + violation report + edit directive) |
| Phase 6b validation failure | 2 retries with info-rich feedback (original prompt + previous failed output + 6b violation report by IR node ID + edit directive) |

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

**Contrast with Repair's idempotence** (`repair.md` §4.5): Repair is idempotent because its detection mechanism is name resolution — if every name already resolves, there is nothing for Repair to do, so no LLM is invoked on the second run. Step 2 has no such no-op path: every invocation produces prose from structured IR, and that production is an LLM call every time. Repair makes the *source* stable; Step 2 makes the *output* structurally stable but not textually stable.

Practical implications for tooling:

- Build caching (`pipeline.md` §Cacheability) must include Step 2's prose output in the cache entry if byte-stable output is required across runs.
- Diff-based CI that compares compiled `.md` across commits will see prose churn from Step 2 even when the source is unchanged. Teams that need stable compiled outputs should cache them alongside the source, not recompile on every build.
- Snapshot tests should assert structural shape (counts, ordering, frontmatter) rather than byte-identical prose.

## 8. Worked Example

Using the `fix_bug` skill from `compiled-output.md` §Complete Example.

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
You are producing the `## Parameters` and `## Instructions` sections of a compiled skill file.

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
  - Emit `## Instructions` with `### Context` (if any), `### Steps`, and `### Constraints`.
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

## Instructions

### Steps

1. Inspect the failure in {scope}, focusing on auth boundaries and permission checks. Identify what is failing and whether any auth-related logic is involved.
2. Identify the root cause of the issue.
3. Don't propose a fix until you've confirmed the root cause.
4. Apply the smallest change that fixes the issue.
5. Validate that the fix works before reporting success.
6. Summarize what was changed and why, and return that as your result.

### Constraints

- Do not make changes outside {scope}.
- Follow the repository's existing patterns before introducing new abstractions.
```

### 8.5 Phase 6b Check (passing)

6b walks the output against the IR:

- Parameters section: skill has 1 parameter (`scope`) → `## Parameters` has 1 bulleted item. Pass.
- Section shape: one `## Parameters`, one `## Instructions`, H3s (`### Context`, `### Steps`, `### Constraints`). Pass.
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

- **Pipeline** (`pipeline.md` §Phase 6): canonical description of Expand's two-step model. This document refines Step 2 and adds Phase 6b.
- **Repair** (`repair.md`): contrast — Repair is source-to-source, idempotent, and driven by diagnostics; Step 2 is IR-to-Markdown, not idempotent, and driven by the resolved IR itself.
- **IR and semantics** (`ir-and-semantics.md`): the node shapes Step 2 consumes (`InputContract`, `Step`, `Constraint`, `Context`, `OutputContract`), strength/polarity model, effect vocabulary.
- **Compiled output** (`compiled-output.md`): the output shape Step 2 must produce — `## Parameters` (conditional), `## Instructions` with `### Context` + `### Steps` + `### Constraints`, formatting rules. YAML frontmatter is assembled by Emit.
- **Diagnostics** (`diagnostics.md`): the diagnostic schema and ID convention used by Phase 6b.
- **Foundations** (`foundations.md`): #18 (deterministic passes own correctness — the Safety Sandwich), #33 (novice learnability — motivates `with` modifier as the only call-site specialization mechanism, which in turn motivates the no-fallback posture in §5).
