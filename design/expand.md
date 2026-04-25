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
- invent sections beyond `## Instructions` with its `### Steps` and `### Constraints` sub-sections (see `compiled-output.md`);
- invent new `{param}` references that do not correspond to declared parameters (declared parameter references must be preserved);
- change effects, types, or the call graph;
- re-materialize content that was already prose after Step 1 (inline strings and resolved `text` references pass through untouched);
- serve as a second repair loop; it has no access to diagnostics and no ability to rewrite source.

If Step 2 cannot cleanly reshape a node, that is a Phase 6b failure (see §4), not an opportunity for Step 2 to be more creative.

## 3. Input / Output Contract

### 3.1 Input schema

Step 2 receives a **resolved IR** — the same IR shape produced by Phase 4 (Lower) and validated by Phase 5 (Validate), but with bare names and inline strings already resolved by Step 1. Parameter references (`{param}`) are preserved as named slots. The LLM does **not** see the original source file, the authoring-level declarations, or any unresolved names.

Specifically, Step 2 receives:

- the full validated IR of the skill (all nodes, not one node at a time) in a serialized form — JSON or an equivalent structured encoding;
- for every `Call` node: `{ target_name, resolved_body_text, site_modifier?, role, effects, scoped_constraints, position }`, where `resolved_body_text` is the post-Step-1 body with `{param}` references preserved as named slots, and `scoped_constraints` is the list of constraints declared on the called block (see §3.2 Scoped Constraint Inlining);
- for every top-level `Constraint` node: `{ resolved_text, strength, polarity }` (text may contain `{param}` references);
- for every `InlineInstruction` node: `{ text, role }` (typically passes through);
- for every `Branch` node: `{ condition_text, then_body, elif_branches, else_body }` with every sub-body already resolved;
- for the `Return` expression: its resolved text plus a flag indicating it must fold into the final Step;
- skill-level metadata: `name`, `description` (if present), `effects` (as a list), the ordered position of each node in `flow:`, and the parameter list (names, types, and defaults — used for generating the `## Parameters` section).

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

**Strength and polarity wording.** The same wording rules as top-level constraints apply to scoped constraints. The deterministic-fallback table in §5.2 also applies (see §5.2 for fallback projection of scoped constraints).

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
2. **At most two H3 sub-sections**, both under `## Instructions`: `### Steps` and `### Constraints`. `### Constraints` is omitted when the IR has no constraints. `### Steps` is omitted only for pure constraint-only skills.
3. **Role preservation** (1-to-1):
   - Every `Step` node (and every `Call`, `InlineInstruction`, `Branch`, `Return` that projects to a Step per `compiled-output.md`) must produce exactly one numbered list item under `### Steps`, in the same order as the IR.
   - Every `Constraint` node must produce exactly one bulleted list item under `### Constraints`. Order is not required to match the IR.
   - The `Return` expression must fold into the last `### Steps` item, not produce a separate item or section.
4. **No invented content.** Step 2 may reshape wording but must not add new steps, new constraints, new sections, or commentary.
5. **Bounded length.** Each step is **one instruction-sized paragraph** — at most three sentences, typically one or two. Each constraint is **one sentence**. This is the floor and ceiling; longer nodes indicate Step 2 is inventing content, shorter nodes indicate Step 2 is stripping content.
6. **Parameter references preserved.** `{param}` references from the resolved IR must survive into the output unchanged. Step 2 must not substitute, remove, or rename them. Step 2 must not invent new `{param}` references for names not in the skill's parameter list.
7. **No authoring artifacts.** No `generated` markers, no `with` modifier text, no import paths, no IR field names. `{param}` references for declared parameters are the only authoring-adjacent syntax that survives.
8. **Standard Markdown only.** Headings, numbered lists, bulleted lists, inline emphasis. No HTML, no tables, no code blocks inside steps.

The frontmatter (`name`, `description`, `effects`) is **not** produced by Step 2. It is assembled deterministically by Phase 7 (Emit) from skill-level IR metadata. The `## Parameters` section is assembled by Step 2: it generates a brief description for each parameter from the parameter's name, type, usage context, and default value. Step 2 also produces the `## Instructions` section body.

## 4. Phase 6b: Validation Gate

Phase 6b is the deterministic check that runs between Step 2 and Emit. It is architecturally analogous to Phase 5 (Validate): a pure pass/fail gate that owns correctness for the work the LLM just did. No LLM involvement.

### 4.1 What 6b Verifies

For the Markdown returned by Step 2:

- **Section shape.**
  - At most two H2 sections: `## Parameters` (conditional) and `## Instructions` (always present).
  - No other H2 sections exist.
  - Every H3 under `## Instructions` is either `### Steps` or `### Constraints`. No other H3s.
  - At least one of `### Steps` or `### Constraints` is present (per `compiled-output.md`).

- **Role preservation (1-to-1 count).**
  - The number of numbered items under `### Steps` equals the count of Step-projecting IR nodes (every IR node whose role projects to `Step`, with conditional `Branch` nodes counting as one Step per flattened branch group and the `Return` folded into the last Step rather than producing an additional one).
  - The number of bulleted items under `### Constraints` equals the count of **top-level** `Constraint` IR nodes. Scoped constraints carried on `Call` nodes (§3.2) are projected into the inlined Step prose, not counted here.
  - Ordering under `### Steps` matches the IR's `flow:` order.

- **Parameter reference validity.**
  - Every `{...}` reference in any list item must correspond to a declared parameter in the skill's `InputContract`. Invented parameter references are rejected.
  - Every parameter from the `InputContract` that appears in the Step 1 resolved IR must still appear as a `{param}` reference in the output (Step 2 must not silently drop them).
  - No `with` modifier string from any `Call` node appears verbatim in the output (it should have been consumed, not quoted).

- **Parameters section shape.**
  - If the skill has parameters, `## Parameters` must be present and contain exactly one bulleted item per `InputContract` parameter.
  - Each item must include the parameter name in bold and a brief description. Default values, if declared, must be listed.
  - If the skill has no parameters, `## Parameters` must not be present.

- **Content shape.**
  - Each `### Steps` item is at most three sentences.
  - Each `### Constraints` item is a single sentence.
  - Markdown parses cleanly.

- **Frontmatter non-interference.** If Step 2 returned anything resembling YAML frontmatter (a `---` block at the top), 6b rejects the output. Frontmatter assembly is Emit's job.

### 4.2 Diagnostic Shape

6b failures are reported as diagnostics using the schema in `diagnostics.md`. IDs follow the namespace convention with the `expand` segment reserved for this phase:

| ID | Classification | Trigger |
|---|---|---|
| `G::expand::extra-h2` | error | Step 2 emitted an H2 other than `## Instructions` |
| `G::expand::missing-instructions` | error | Step 2 did not emit `## Instructions` |
| `G::expand::extra-h3` | error | Step 2 emitted an H3 beyond `### Steps` / `### Constraints` |
| `G::expand::step-count-mismatch` | error | Number of `### Steps` items does not match Step-projecting IR node count |
| `G::expand::constraint-count-mismatch` | error | Number of `### Constraints` items does not match `Constraint` node count |
| `G::expand::step-order-mismatch` | error | Step order diverges from `flow:` order |
| `G::expand::invented-param-ref` | error | `{...}` reference does not match any declared parameter |
| `G::expand::dropped-param-ref` | error | A parameter reference from Step 1 output was silently removed by Step 2 |
| `G::expand::modifier-leaked` | error | `with` modifier string appears verbatim in output |
| `G::expand::params-section-mismatch` | error | `## Parameters` item count does not match `InputContract` parameter count |
| `G::expand::params-section-missing` | error | Skill has parameters but `## Parameters` section is absent |
| `G::expand::params-section-spurious` | error | Skill has no parameters but `## Parameters` section is present |
| `G::expand::step-too-long` | error | A step exceeds three sentences |
| `G::expand::constraint-multi-sentence` | error | A constraint is more than one sentence |
| `G::expand::frontmatter-returned` | error | Step 2 returned YAML frontmatter |
| `G::expand::malformed-markdown` | error | Output does not parse as Markdown |

All 6b diagnostics are classified `error`, not `repairable`. Phase 3 Repair operates on source; 6b failures are a Step 2 output problem and are handled by the retry / fallback policy in §5, not by re-running Repair.

### 4.3 What 6b Does Not Check

- **Semantic faithfulness of wording.** 6b does not verify that the prose the LLM produced for a step actually reflects the IR node's meaning. That is out of scope for a deterministic gate. The Safety Sandwich bounds the LLM structurally, not semantically; semantic drift is mitigated by the one-sentence rule for generated bodies (`repair.md` §5) and by the resolved-body text flowing in as Step 2's primary input.
- **Effect correctness.** Effects are fixed in Phase 4 and validated in Phase 5. Step 2 cannot touch them.
- **Style.** Tone, formality, and clarity are not checked. Only the structural contract is enforced.

## 5. Failure Policy

Step 2 is not infallible. Phase 6b failures are handled by a three-tier policy, in order.

### 5.1 Retry (tier 1)

On the first 6b failure, Step 2 is re-invoked with the same resolved IR and an additional **violation summary** describing what 6b rejected — e.g., "the previous attempt produced 5 steps but the IR has 6 Step nodes; Step 3 was missing." The violation summary is structured feedback, not free-form prose.

**Retry budget: 2 retries** (so up to 3 Step 2 attempts total per invocation). The budget is small deliberately: Step 2 operates on already-valid IR, so a correct output should be within reach; repeated failure indicates the LLM is consistently misreading the input, and more retries will not help.

If any retry passes 6b, the pipeline continues to Emit. The earlier failed attempts are discarded.

### 5.2 Deterministic Fallback (tier 2)

If all retries fail 6b, the pipeline does **not** abort. It falls back to a **deterministic projection** of the resolved IR. This projection is a minimal, mechanical rendering of the Step 1 output directly into Markdown:

- The `## Parameters` section is assembled mechanically: each parameter → one bulleted item with name in bold, type (if annotated), and default (if declared). No LLM-generated description.
- Each Step-projecting IR node → one numbered item using its `resolved_body_text` verbatim, with `{param}` references preserved (with light punctuation cleanup: capitalize first letter, ensure trailing period).
- Each top-level `Constraint` node → one bulleted item shaped by a fixed lookup table:
  - `(soft, require)`: `"Do <resolved text>."`
  - `(soft, avoid)`: `"Do not <resolved text>."`
  - `(hard, require)`: `"Always <resolved text>."`
  - `(hard, avoid)`: `"Never <resolved text>."`
- Scoped constraints on `Call` nodes (§3.2) are projected as a localized framing sentence appended to the call's inlined Step using the same lookup table — e.g., a `(hard, avoid)` scoped constraint with text "unrelated edits" becomes `" Never unrelated edits."` appended to the step's resolved body. They are not added to `### Constraints`.
- `with` modifiers are ignored in fallback (no LLM to apply them).
- Return folding is approximated by appending `"Return the result."` to the last Step.

The fallback output is stilted — that is the point. It trades prose quality for determinism and guarantees 6b will pass (the projection is defined to satisfy the 1-to-1 mapping by construction). The user-visible result is a compiled file that is less polished than a successful Step 2 run, but still correct and usable.

The fallback also emits a `G::expand::fell-back-to-projection` diagnostic at `warning` classification, surfaced to the author via the compiler CLI but not embedded in the compiled output.

### 5.3 Hard Failure (tier 3)

The invocation aborts only when the deterministic fallback itself cannot be produced. In practice this means the input IR is malformed in a way Phase 5 should have caught — a structural bug in the compiler. The pipeline emits a `G::expand::fallback-failed` diagnostic at `error` classification, does not write a `.md` file, and returns a non-zero exit status. This path should never fire for a Phase-5-validated IR; if it does, it is a compiler bug, not a user error.

### 5.4 User-Visible Behavior Summary

| Path | User sees |
|---|---|
| Step 2 passes 6b on attempt 1 | Normal compiled `.md`, no warnings |
| Step 2 passes 6b after 1–2 retries | Normal compiled `.md`, no warnings (retries are invisible) |
| All retries fail, fallback succeeds | Compiled `.md` with stilted wording, plus a `fell-back-to-projection` warning on stderr |
| Fallback fails | No `.md` written, `fallback-failed` error on stderr, non-zero exit |

## 6. Partial Failure

A **partial failure** is an imagined scenario where Step 2 returns Markdown in which some list items are well-formed and others are malformed — e.g., 5 of 6 Steps render cleanly and the 6th contains a `{param}` placeholder.

**MVP policy: all-or-nothing per invocation.** Mixed output is never emitted. Phase 6b treats any 6b violation as a full Step 2 failure; it does not attempt to salvage the well-formed items.

Rationale:

- **The Safety Sandwich collapses otherwise.** If 6b accepts partial outputs, every 6b pass-with-patches is a point where the LLM-produced content has leaked past the deterministic boundary. The boundary exists precisely to prevent that.
- **No principled repair target.** If 6b tries to fix only the broken items, it would need a per-node LLM retry with only one node's resolved IR visible — the opposite of the whole-skill prompting model Step 2 relies on. The re-prompted node would lose its surrounding context and likely produce worse prose than a full retry.
- **Diagnostics stay clean.** One invocation produces one output or one diagnostic path, never a mix.
- **Fallback already exists.** The deterministic fallback (§5.2) is the correct "partial failure is unacceptable, produce something" mechanism. It is deterministic by construction and covers the edge cases a partial-repair mechanism would try to cover.

Compiler authors who are tempted to add a per-node repair path should instead consider whether Step 1 is doing too little (the LLM should not need so much latitude) or whether the retry budget in §5.1 is too small.

## 7. Determinism and Reproducibility

Step 2 is **not idempotent** across model versions, prompt variations, or even repeated runs with the same model at temperature > 0. Two compilations of the same skill may produce two compiled `.md` files whose prose differs word-for-word. This is an honest limitation of any LLM pass.

What *is* guaranteed:

- **Structural idempotence.** Every Step 2 output that passes 6b has the same structural shape: the same H2, the same H3s, the same number of Steps in the same order, the same number of Constraints. Word-level prose is not stable; IR-level projection is.
- **Semantic content bounds.** The resolved body text going into Step 2 is identical across runs (Step 1 is fully deterministic). The LLM may reshape wording but cannot invent content that was not in the resolved IR, because 6b enforces the 1-to-1 role mapping.
- **Fallback is fully deterministic.** If Step 2 repeatedly fails, the §5.2 fallback produces byte-for-byte identical output every run.

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
  input_contract: [ Param(name: "scope", type: String) ]
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
- Parameter metadata → `[ { name: "scope", type: String, default: none } ]`.

After Step 1, every bare name and inline string has concrete content. `{scope}` references are preserved as named slots for the consuming LLM. This is the input to Step 2.

### 8.3 Step 2 Input (conceptual serialization)

Step 2's prompt contains the resolved IR, the output template, and the formatting rules. Conceptually:

```text
You are producing the `## Parameters` and `## Instructions` sections of a compiled skill file.

Parameters:
  - name: "scope", type: String, default: none

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
  - Emit `## Parameters` with one bulleted item per parameter (name, description, default).
  - Emit `## Instructions` with `### Steps` and `### Constraints`.
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
- **scope**: Area of codebase to focus on

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
- Section shape: one `## Parameters`, one `## Instructions`, two H3s (`### Steps`, `### Constraints`). Pass.
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
- **IR and semantics** (`ir-and-semantics.md`): the node shapes Step 2 consumes (`InputContract`, `Step`, `Constraint`, `OutputContract`), strength/polarity model, effect vocabulary.
- **Compiled output** (`compiled-output.md`): the output shape Step 2 must produce — `## Parameters` (conditional), `## Instructions` with `### Steps` + `### Constraints`, formatting rules. YAML frontmatter is assembled by Emit.
- **Diagnostics** (`diagnostics.md`): the diagnostic schema and ID convention used by Phase 6b.
- **Foundations** (`foundations.md`): #18 (deterministic passes own correctness — the Safety Sandwich), #11 (reliability beats elegance — justifies the deterministic fallback in §5.2), #33 (novice learnability — motivates `with` modifier as the only call-site specialization mechanism).
