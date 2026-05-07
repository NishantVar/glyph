---
name: expand
description: Use during a /glyph:compile run when the compiler has produced a scaffolded Markdown file with marked prose spans plus the resolved IR side-map. Read both, fill each marked span with prose generated from the matching IR node, and write the expanded Markdown back to scaffold_path.
---

## Parameters

- **scaffold_path**. Required.
- **resolved_ir**. Required.

## Instructions

### Context

- **invocation-shape-is-one-scaffold-with-resolved-ir**

  The LLM receives one scaffolded Markdown document plus the resolved IR side-map for the same skill in a single prompt, and returns one expanded Markdown document with every marked prose span filled.

- **deterministic-emitter-owns-structure**

  The deterministic emitter owns section headings, Step numbering, sub-step lettering, the `If`/`Otherwise` arm structure, the `## Parameters` bullet structure, every `### Constraints` bullet, the external-file Step template, and the OutputContract Identifier return-fold suffix. The LLM's role is to fill prose into existing slots, never to regenerate, paraphrase, or restructure them.

- **full-ir-visibility-enables-calibration**

  The LLM sees the full resolved IR for the skill in a single prompt. That visibility is for calibrating prose so consecutive Steps read naturally — not for reordering, merging, or splitting them.

- **per-step-length-budget**

  A non-conditional Step body and each Branch sub-step is at most three sentences, typically one or two. A parameter description is a single short clause that complements the deterministic name/type/default fragment, not a paragraph.

### Steps

1. For each Call node carrying a `site_modifier` (the `with "..."` clause), weave the modifier's intent into the Step's prose. The literal modifier string must never appear verbatim in the output.
2. For each Call node whose `scoped_constraints` array is non-empty, either fold each constraint into the Step's prose or prepend a localized framing sentence that scopes the constraint to the inlined region. Apply the standard strength and polarity wording. Never emit a scoped constraint as a `### Constraints` bullet — those are top-level only.
3. For each `{name}` token in a Step's resolved body whose name appears in the Call's `local_refs` array, replace the token with a natural-language cross-reference to the producing step (for example, `the diagnosis from your earlier analysis` or `the diagnosis identified in step 1`). The literal `{name}` token must not survive in the output.
4. For an OutputContract whose `form` is the `Description` variant, paraphrase the description into a Step-shaped sentence and fold it into the final Step's prose. The angle-bracket-quoted token, the surrounding angle brackets, and the verbatim quoted text must all be absent from the output. The `Identifier` variant is handled deterministically — leave its scaffold span untouched.
5. For each Branch whose `condition` is a code-shaped expression, convert the expression into natural-language prose suitable for an `If <prose>:` arm header. Use `applies_descriptions` from the IR side-map for any embedded `BLOCKNAME.applies()` sub-expressions and weave them into the larger condition prose. Pure-`applies()` Branches and the `Otherwise:` arm header are emitted deterministically — leave their scaffold spans untouched.
6. For each Param in the skill's InputContract, generate a brief description from the parameter's name, type, default value, and how it is referenced in the body. Fill only the prose slot inside the deterministically-scaffolded `## Parameters` bullet — the bold name, the type fragment, and the `(default: ...)` or `(required)` trailer are not your responsibility.
7. Use the visibility you have over the full resolved IR to calibrate prose so consecutive Steps read as a single connected workflow rather than isolated sentences. Adjust wording without violating any preservation, length, or no-invention constraint. Produce: the scaffolded compiled file with every marked prose span filled, as a single Markdown text blob.

### Constraints

- Edit only the prose inside marked spans the deterministic emitter laid down. Do not add, merge, split, or reorder Steps, sub-steps, constraints, sections, or commentary relative to the IR's flow order.
- You must preserve every deterministic structure the emitter laid down: section headings, numbered Step ordering, sub-step lettering, `### Context` bullets, InlineInstruction and InstructionRef text, pure-`applies()` decision-frame headers, the `If <condition>:` and `Otherwise:` arm structure, the `## Parameters` bullet structure, every `### Constraints` bullet, the locked external-file Step template, and the OutputContract Identifier return-fold suffix. If any look malformed, do not edit them — that is a deterministic-emitter bug, surfaced by Phase 6b.
- You must preserve every `{param}` token whose name matches a declared InputContract parameter verbatim into the output. If Step 1's resolved body contained a parameter slot, re-introduce it in the output prose — silent dropping is forbidden.
- You must never quote a `with` modifier string verbatim in the compiled output. Modifiers are consumed by being woven into prose, never echoed.
- You must never let any `{name}` token from a Call's `local_refs` array survive in the output. Every local-binding reference must be resolved to a natural-language cross-reference to the producing step.
- You must never introduce a `{name}` token for any name that is not declared in the skill's InputContract. Inventing parameter references is forbidden.
- You must never let `generated` markers, import paths, IR field names, IR node IDs, raw condition expressions, output-target tokens, surrounding angle brackets, YAML frontmatter, JSON, IR, or commentary appear in the compiled output. The output channel is Markdown text only.
- You must never use HTML, tables, or fenced code blocks inside a Step body or sub-step body. Inline emphasis is fine; structural Markdown is the deterministic emitter's job.

