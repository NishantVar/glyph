# LLM Expand Pass — Responsibilities

> **Architecture context:** The deterministic emitter (per `design/expand.md` §3 and `design/compiled-output.md`) produces a Markdown *scaffold* with typed spans. This document specifies what the LLM does **inside those spans**. The LLM never produces deterministic structure — it only fills span content. Span kinds (`ParamDescription`, `DescriptionReturnFold`, `BranchCondition`, `CallBodyShape`) and the per-kind contract are summarized in `design/expand.md` §3.5; the IR fields the LLM consumes are tracked in `design/todo.md` §Expand — LLM Span Fill.

This file enumerates **only** what the LLM is responsible for in Glyph's Expand pass (Step 2). Everything else in the compiled output is produced by the deterministic emitter and the LLM must not regenerate, paraphrase, or restructure it. The LLM's role is to fill specific text spans inside an already-scaffolded Markdown document.

For the architectural framing (Safety Sandwich, retry policy, validation gate), see `design/expand.md`. For the deterministic emitter's contract, see `design/expand.md` §3 and §3.5, and `design/compiled-output.md`.

Format below: **"If the LLM sees X, it'll do Y."** Each item is a span-level instruction the consuming model must follow when handed a scaffolded compiled file with marked spans to fill.

---

## 1. Per-node prose generation

### 1.1 Call body prose

1. **If the LLM sees a `Call` node with a `site_modifier` (the `with "…"` clause)** → it'll weave the modifier's intent into the Step's prose. The literal modifier string must not appear verbatim in the output; its meaning is folded into the Step's wording.

2. **If the LLM sees a `Call` node with non-empty `scoped_constraints`** → it'll either fold each constraint into the Step's prose or add a localized framing sentence that scopes the constraint to the inlined region. It applies the strength/polarity wording rules but does **not** emit the constraint as a `### Constraints` bullet (those are scoped, not top-level).

### 1.2 Local binding references

3. **If the LLM sees a `{name}` token in a Step's resolved body whose name appears in the Call's `local_refs` array** → it'll replace the token with a natural-language cross-reference to the producing step (e.g., "the diagnosis from your earlier analysis" or "the diagnosis identified in step 1"). The literal `{name}` token must not survive into the output.

### 1.3 Output contract

4. **If the LLM sees an `OutputContract` whose `form == Description("…")`** → it'll paraphrase the description into a Step-shaped sentence and fold it into the final Step's prose. The literal `<"…">` token, the surrounding angle brackets, and the verbatim quoted text must all be absent from the output.

   *(Note: the `Identifier` form is handled deterministically by appending a fixed return-fold suffix — the LLM is not responsible for it.)*

### 1.4 Branch conditions

5. **If the LLM sees a `Branch` whose `condition` is a code-shaped expression** (e.g., `x > 5 and not is_dry_run`) → it'll convert the expression into natural-language prose suitable for an `If <prose>:` arm header. It uses `applies_descriptions` from the IR side-map for any `BLOCKNAME.applies()` sub-expressions and weaves them into the larger condition.

   *(Note: pure-`applies()` Branches and the `Otherwise:` arm header are emitted deterministically per `design/expand.md` §3.3 — the LLM is only responsible for converting non-pure-`applies()` condition expressions.)*

### 1.5 Parameter descriptions

6. **If the LLM sees a `Param` in the skill's `InputContract`** → it'll generate a brief description for that parameter from its name, type, default value, and how it is referenced in the skill body. The description fills the prose slot inside the deterministically-scaffolded `## Parameters` bullet (the bold name, type, and `(default: …)` / `(required)` trailer are not its job).

---

## 2. Whole-skill calibration

7. **If the LLM sees the full resolved IR for the skill in one prompt** → it'll calibrate the prose it writes so that consecutive Steps read naturally as a sequence. Wording for one Step may be adjusted to flow with the Step before or after it, provided no rule in §3 below is violated.

---

## 3. Discipline within LLM-written spans

### 3.1 Preservation

8. **If the LLM sees a `{param}` reference in a resolved body and the name matches a declared `InputContract` parameter** → it'll preserve the token verbatim in the output. It will not substitute the parameter's value, rename it, or remove it.

9. **If the LLM is shaping a Step's body and the resolved body from Step 1 contained a `{param}` reference** → it'll re-introduce that reference in the output. Silently dropping a parameter reference is forbidden.

### 3.2 No invention

10. **If the LLM is tempted to write `{name}` for any name not declared in the skill's `InputContract`** → it won't. Inventing parameter references is forbidden.

11. **If the LLM is tempted to add a Step, sub-step, constraint, section, or commentary that does not correspond to an IR node** → it won't. The deterministic emitter has already laid out all sections, all numbered items, and all bullets; the LLM only fills prose into existing slots.

12. **If the LLM is tempted to reorder, merge, or split Steps relative to the IR's flow order** → it won't.

### 3.3 No leaks

13. **If the LLM is tempted to quote a `with` modifier string verbatim in the output** → it won't. The modifier is consumed by being woven into prose, never echoed.

14. **If the LLM is tempted to leave any `local_ref` `{name}` token in the output** → it won't. Every `local_ref` must be resolved to a natural-language cross-reference.

15. **If the LLM is tempted to leave authoring artifacts in the output** (`generated` markers, import paths, IR field names, IR node IDs, the `<"…">` token, raw condition expressions, etc.) → it won't.

### 3.4 Length and form

16. **If the LLM is writing a non-conditional Step's body** → it'll keep it to at most three sentences (typically one or two), per the Phase 6b `step-too-long` check.

17. **If the LLM is writing a sub-step within a Branch projection** → it'll keep it to at most three sentences.

18. **If the LLM is writing a parameter description** → it'll keep it to a single short clause that complements the deterministic name/type/default fragment, not a paragraph.

19. **If the LLM is tempted to use HTML, tables, or fenced code blocks inside a Step's body** → it won't. Standard Markdown only — inline emphasis is fine; structural Markdown is the deterministic emitter's job.

### 3.5 Output channel

20. **If the LLM is tempted to emit YAML frontmatter, JSON, IR, or commentary** → it won't. Its output is Markdown text intended to be substituted into specific spans of the scaffolded compiled file.

---

## 4. What the LLM is not responsible for

For reference (so the LLM does not duplicate or override deterministic work):

- Section headings (`## Parameters`, `## Instructions`, `### Context`, `### Steps`, `### Constraints`, `### Procedure: <name>`).
- Numbering of `### Steps` items and lettering (`a.`, `b.`, `c.`) of Branch sub-steps.
- `### Context` bullets (already prose post-Step 1).
- `InlineInstruction.text` and `InstructionRef.resolved_text` (pass through unchanged).
- `same_file_procedure` H3 names (kebab-case derivation from the callee identifier).
- The pure-`applies()` decision-frame header sentences (per `design/expand.md` §3.3).
- The `If <condition>:` / `Otherwise:` arm header structure.
- `## Parameters` bullet structure (bold name, type, default/required trailer).
- All `### Constraints` bullets (rendered from a fixed four-form `(strength × polarity)` template per `design/expand.md` §3.3 / `design/compiled-output.md` §Constraint Rendering).
- The `external_file` Step prose: locked template `Load and follow the procedure in \`{procedure_path}\`.`.
- The `OutputContract.Identifier` return-fold suffix: locked append `, and return that as your result.`.

If any of these appear malformed in the scaffold the LLM is editing, the LLM should not "fix" them — that is a deterministic-emitter bug, surfaced through Phase 6b, and the LLM has no authority to alter them.
