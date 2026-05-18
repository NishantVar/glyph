# CallBodyShape Span Emission — Closing the `with` Modifier Drop

**Status:** draft
**Date:** 2026-05-18
**Phase:** 6 / Step 2 (Expand)
**Related ADRs:** [[0016-llm-reshape-no-deterministic-fallback]]
**Related docs:** [[docs/architecture/expand]] §3.5, [[llm_expand_pass]] §1.1–§1.2

## 1. Problem

A `with "…"` site modifier on a Call was silently dropped from the compiled Markdown. The reproduction: a Branch whose then-arm Call had `site_modifier: "name each construct and show it beside the instruction it creates"` rendered identically to the otherwise-arm Call — both produced the literal `"Follow the build-walkthrough procedure."`. The modifier's intent never reached the agent-facing artifact.

The IR is correct: `with` parses and lowers into `IrCall.site_modifier` faithfully. The gap is in Phase 6 Step 2 (the deterministic emitter):

- `SpanKind::CallBodyShape` and `SpanPayload { site_modifier, resolved_body, … }` are defined in `crates/glyph-core/src/emit/scaffold.rs`.
- `crates/glyph-core/src/emit/stub_fill.rs:24` knows how to fill a `CallBodyShape` span (verbatim `resolved_body`, with modifier deliberately ignored — documented in `expand.md` §3.5).
- **No emit site ever pushes a `CallBodyShape` span.** Every Call emission path pushes a literal-template string:
  - `scaffold.rs` top-level: tier 1 (~L1037), tier 2 (~L1060), tier 3 (~L1071), stdlib/subagent (~L1086).
  - `branch.rs::emit_lettered_substeps` in-arm: tier 1 (L300–318), tier 2 (L319–327), tier 3 (L328–336).

Because no span is emitted, the modifier is structurally invisible to the fill layer — even a fully-wired LLM expand pass would never see it. Scoped constraints (`expand.md` §3.2) and the LLM-grade local-ref cross-references (`llm_expand_pass.md` §1.2) are silently dropped the same way.

## 2. Goals & Non-Goals

**Goals.**

1. Plumb every Call emission path through a `CallBodyShape` span when LLM judgment is required.
2. Make the failure mode loud: when the stub filler is asked to fill a `CallBodyShape` span it cannot, abort compilation with a specific diagnostic. No silent drop. No deterministic fallback that produces clunkier prose (per ADR-0016).
3. Preserve existing behavior for Calls that need no LLM judgment, so today's snapshots and test corpus are unaffected except in the cases that are actually buggy today.

**Non-goals.**

- Wiring the actual LLM filler. This spec covers the deterministic-emitter + stub side only.
- Phase 6b semantic checks that the LLM's woven prose faithfully reflects modifier intent.
- Any change to Step 1 resolution, the `with` parse path, or the IR shape (beyond extending `SpanPayload`).
- A deterministic fallback that produces "good enough" prose without an LLM.

## 3. Design

### 3.1 Posture (locked)

**Loud failure**, per ADR-0016. When LLM judgment is needed and no LLM is wired, the build aborts with a structural diagnostic. The user re-runs once the LLM is wired (or removes the `with` modifier / scoped constraint that requires it). No `.md` is written.

### 3.2 Scope (locked)

The fix covers **all three Call projection tiers** (tier 1 inline, tier 2 same_file_procedure, tier 3 external_file) in **both positions** (top-level under `## Steps` and lettered sub-steps inside a Branch arm), and all three `CallBodyShape` responsibilities: `site_modifier`, `scoped_constraints`, and LLM-grade `local_refs`.

### 3.3 Triviality predicate

A Call is **trivial** (no LLM needed) when:

```rust
c.site_modifier.is_none()
    && c.scoped_constraints.is_empty()
    && c.local_refs.is_empty()
```

Non-empty `local_refs` is treated as non-trivial because `llm_expand_pass.md` §1.2 requires a natural-language cross-reference ("the diagnosis from your earlier analysis"), which the deterministic `substitute_local_refs_in` bare substitution does not produce. (This widens the loud-failure surface beyond `with`, but matches the architecture's stated contract.)

### 3.4 Emit-site changes

Each of the six Call emission sites adopts the same pattern: keep the existing literal path under the triviality predicate, otherwise emit a `CallBodyShape` span.

Pseudocode (one representative site — tier 2 same_file_procedure, top-level, currently `scaffold.rs:1058–1067`):

```rust
IrNode::Call(c) if c.projection_tier == Some(2) => {
    s.push_literal(format!("{}. ", idx + 1));
    if call_needs_llm_fill(c) {
        s.push_span(SpanRef {
            id: s.next_span_id(),
            kind: SpanKind::CallBodyShape,
            ir_node: c.node_id,
            payload: SpanPayload {
                site_modifier: c.site_modifier.clone(),
                resolved_body: Some(format!(
                    "Follow the {} procedure below.",
                    c.target.replace('_', "-")
                )),
                scoped_constraints: c.scoped_constraints.clone(),
                local_refs: c.local_refs.clone(),
                ..SpanPayload::default()
            },
        });
    } else {
        let kebab = c.target.replace('_', "-");
        s.push_literal(format!("Follow the {} procedure below.", kebab));
    }
    // Naming sentence trailing + newline stay outside the span — deterministic.
    if let Some(naming) = naming_sentence_for_call(c) {
        s.push_literal(format!(" {}", naming));
    }
    s.push_literal("\n");
    if procedure_seen.insert(c.target.clone()) {
        procedure_order.push(c.target.clone());
    }
}
```

Equivalent changes at the other five sites (different anchor sentence per tier). The naming sentence (`Refer to this result as …`) and the numbered/lettered list prefix remain deterministic — `expand.md` §3.5 puts them outside the span.

### 3.5 SpanPayload extension

`crates/glyph-core/src/emit/scaffold.rs` ~L302. Add two fields:

```rust
#[derive(Clone, Debug, Default)]
pub struct SpanPayload {
    pub site_modifier: Option<String>,
    pub resolved_body: Option<String>,
    pub condition_expression: Option<String>,
    pub resolved_predicates: Option<BTreeMap<String, String>>,
    pub classification: Option<crate::condition::ConditionClassification>,
    pub predicate_shape: BranchPredicateShape,
    pub param_name: Option<String>,
    pub param_type: Option<String>,
    pub param_default: Option<String>,
    // New:
    pub scoped_constraints: Vec<ScopedConstraintPayload>,
    pub local_refs: Vec<LocalRefPayload>,
}

#[derive(Clone, Debug)]
pub struct ScopedConstraintPayload {
    pub text: String,
    pub strength: ConstraintStrength,
    pub polarity: ConstraintPolarity,
}

#[derive(Clone, Debug)]
pub struct LocalRefPayload {
    pub name: String,
    pub producer_node: NodeId,
}
```

Both new vectors default to empty so existing span constructions (`ParamDescription`, `BranchCondition`) remain source-compatible.

### 3.6 Stub-fill behavior change

`crates/glyph-core/src/emit/stub_fill.rs`. The `CallBodyShape` arm changes from infallible verbatim pass-through to a hard-fail when called. Because under §3.4 a `CallBodyShape` span is only ever emitted when `call_needs_llm_fill` is true, the stub's posture is "any emitted CallBodyShape means LLM was required."

```rust
pub fn fill(scaffold: &Scaffold) -> Result<HashMap<SpanId, String>, Vec<StubFillError>> {
    let mut out = HashMap::new();
    let mut errors = Vec::new();
    for chunk in &scaffold.chunks {
        if let Chunk::Span(span) = chunk {
            match span.kind {
                SpanKind::ParamDescription => { out.insert(span.id, String::new()); }
                SpanKind::BranchCondition => { /* unchanged */ }
                SpanKind::CallBodyShape => {
                    errors.push(StubFillError::LlmRequiredForCall {
                        ir_node: span.ir_node,
                        has_modifier: span.payload.site_modifier.is_some(),
                        has_scoped_constraints: !span.payload.scoped_constraints.is_empty(),
                        has_local_refs: !span.payload.local_refs.is_empty(),
                    });
                }
            }
        }
    }
    if errors.is_empty() { Ok(out) } else { Err(errors) }
}
```

### 3.7 Pipeline plumbing & diagnostic

The caller of `stub_fill::fill` (in `compile_markdown` / `compile_to_md`, `crates/glyph-core/src/expand.rs` and `crates/glyph-core/src/emit/scaffold.rs::compile_to_md`) propagates the error and emits one diagnostic per failing span:

- **ID:** `G::expand::llm-required-for-call`
- **Classification:** `error` (not `repairable` — Phase 3 Repair operates on source; this is a Step 2 fill issue, handled per `expand.md` §5).
- **Message template:**
  > `Call at IR node {n} requires LLM-grade expansion ({reasons}); no LLM filler is wired. Either wire the LLM expand pass or remove the {…}.`
  >
  > `{reasons}` is a comma-joined subset of `with modifier`, `scoped constraints`, `local-ref cross-references`.
- **No `.md` file written.** Matches the §5.6 user-visible behavior summary for "validation failure persists."

Add one row to the §4.2 diagnostic catalog in `docs/architecture/expand.md`:

| ID | Classification | Trigger |
|---|---|---|
| `G::expand::llm-required-for-call` | error | Stub filler encountered a `CallBodyShape` span requiring LLM judgment and no LLM is wired. |

### 3.8 Documentation updates

Three small edits.

1. **`docs/architecture/expand.md` §3.5**, the `CallBodyShape` row of the SpanKind table. Replace the "Stub behavior today" cell:
   - Before: *"Verbatim resolved body — modifier and scoped constraints currently ignored."*
   - After: *"Spans are emitted only when `site_modifier`, `scoped_constraints`, or `local_refs` are non-empty; the stub hard-fails with `G::expand::llm-required-for-call`. Trivial Calls do not emit a span and render via the deterministic literal template."*

2. **`docs/architecture/expand.md` §4.2 diagnostic table.** Insert the row from §3.7 above.

3. **`llm_expand_pass.md`** preamble. Add a one-line note: the stub filler no longer silently elides `with` modifiers, scoped constraints, or local-ref cross-references — it refuses with a structural diagnostic until the LLM expand pass is wired.

## 4. Behavior matrix

| Call shape | Today | After fix |
|---|---|---|
| No modifier, no scoped constraints, no local refs | Deterministic literal | **Unchanged** — same literal, no span |
| `with "…"`, no scoped constraints, no local refs | Deterministic literal (modifier silently dropped — **bug**) | `CallBodyShape` span emitted; stub hard-fails with `G::expand::llm-required-for-call` |
| Scoped constraints non-empty | Deterministic literal (constraints silently dropped) | Span emitted; stub hard-fails |
| `local_refs` non-empty, no modifier | Bare `substitute_local_refs_in` substitution | Span emitted; stub hard-fails |
| Any combination of the above | Various silent drops | Span emitted with all three payloads; stub hard-fails listing each reason |

## 5. Affected files

```
crates/glyph-core/src/emit/scaffold.rs
  - Extend SpanPayload (§3.5).
  - Add call_needs_llm_fill helper.
  - Replace tier 1/2/3/stdlib literal emission (~L1037–L1091) with span-when-needed.
crates/glyph-core/src/emit/branch.rs
  - Replace tier 1/2/3 in-arm literal emission (L300–L336) with span-when-needed.
crates/glyph-core/src/emit/stub_fill.rs
  - Change fill() signature to Result, add StubFillError, hard-fail on CallBodyShape.
crates/glyph-core/src/emit/merger.rs
  - Plumb Result up; update tests.
crates/glyph-core/src/expand.rs
  - Propagate stub_fill error into diagnostics; emit G::expand::llm-required-for-call.
crates/glyph-core/src/diagnostic.rs (or wherever IDs live)
  - Register the new diagnostic ID.
docs/architecture/expand.md
  - §3.5 stub-behavior cell; §4.2 diagnostic table.
llm_expand_pass.md
  - Preamble note on refusal semantics.
```

## 6. Test plan

1. **Regression — trivial Calls unaffected.** Existing snapshot suite passes unchanged: every snapshot that does not exercise `with`, scoped constraints, or local refs renders byte-identically.
2. **New — modifier hard-fails.** A test skill with `with "…"` on a tier-2 in-arm Call compiles to `G::expand::llm-required-for-call`, no `.md` written, exit non-zero.
3. **New — same for tier 1 / tier 3, top-level and in-arm.** Six matrix entries.
4. **New — scoped-constraint hard-fails.** A Call to a block with a scoped `Constraint` emits the diagnostic.
5. **New — local_refs hard-fails.** A skill with a local binding referenced inside a Call body emits the diagnostic.
6. **New — combined responsibilities.** One Call with all three non-empty surfaces all three reasons in the diagnostic message.
7. **IR-node-id stability.** The diagnostic names the failing Call by stable IR node id (`n4`, …) consistent with `expand.md` §3.1.

## 7. Open questions / deferred work

- **Concrete diagnostic message wording.** §3.7 has a template; final wording can be tightened during implementation.
- **Whether `local_refs.is_empty()` should be the triviality bar, or a stricter predicate** (e.g., "local_refs that the LLM would phrase non-trivially"). Picked the strict bar per the locked "all three responsibilities" scope; can be loosened later if it proves too noisy.
- **Wiring the actual LLM filler** is out of scope here; tracked separately in [[todo/expand-todos]].
