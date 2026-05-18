# CallBodyShape Span Emission — Closing the `with` Modifier Drop

**Status:** draft (rev 2, post-review)
**Date:** 2026-05-18
**Phase:** 6 / Step 2 (Expand)
**Related ADRs:** [[docs/adr/0016-llm-reshape-no-deterministic-fallback]], [[docs/adr/0018-phase-6b-structural-only-gate]]
**Related docs:** [[docs/architecture/expand]] §3.5, [[llm_expand_pass]] §1.1–§1.2, [[docs/reference/diagnostics]]

## 1. Problem

A `with "…"` site modifier on a Call was silently dropped from the compiled Markdown. The reproduction: a Branch whose then-arm Call had `site_modifier: "name each construct and show it beside the instruction it creates"` rendered identically to the otherwise-arm Call — both produced the literal `"Follow the build-walkthrough procedure."`. The modifier's intent never reached the agent-facing artifact.

The IR is correct: `with` parses and lowers into `IrCall.site_modifier` faithfully. The gap is in Phase 6 Step 2 (the deterministic emitter):

- `SpanKind::CallBodyShape` and `SpanPayload { site_modifier, resolved_body, … }` are defined in `crates/glyph-core/src/emit/scaffold.rs`.
- `crates/glyph-core/src/emit/stub_fill.rs:24` knows how to fill a `CallBodyShape` span (verbatim `resolved_body`, with modifier deliberately ignored — documented in `expand.md` §3.5).
- **No emit site ever pushes a `CallBodyShape` span.** Every Call emission path pushes a literal-template string. The seven sites are:
  - `scaffold.rs` top-level: tier 1 (~L1037), tier 2 (~L1060), tier 3 (~L1071), stdlib/bound unresolved (`bound_name.is_some()`, ~L1086).
  - `branch.rs::emit_lettered_substeps` in-arm: tier 1 (L300–318), tier 2 (L319–327), tier 3 (L328–336).

Because no span is emitted, the modifier is structurally invisible to the fill layer — even a fully-wired LLM expand pass would never see it. LLM-grade `local_refs` cross-references (`llm_expand_pass.md` §1.2) are silently degraded to bare substitution in the same way.

## 2. Goals & Non-Goals

**Goals.**

1. Plumb every Call emission path (all seven sites above) through a `CallBodyShape` span when LLM judgment is required.
2. Make the failure mode loud: when the stub filler is asked to fill a `CallBodyShape` span it cannot, abort compilation with a specific diagnostic. No silent drop. No deterministic fallback that produces clunkier prose (per [[docs/adr/0016-llm-reshape-no-deterministic-fallback]]).
3. Preserve existing behavior for Calls that need no LLM judgment, so today's snapshots and test corpus are unaffected except in the cases that are actually buggy today.

**Non-goals.**

- **Scoped constraints are out of scope.** `IrCall` has no `scoped_constraints` field today (`emit_ir.rs` hardcodes `"scoped_constraints": []`). Lowering callee constraints into the call site, serializing the field, and exercising it end-to-end is a separate, larger piece of work tracked as a follow-up in §7. The CallBodyShape span this spec emits does **not** carry scoped constraints, and the triviality predicate does not check them.
- Wiring the actual LLM filler. This spec covers the deterministic-emitter + stub side only.
- Phase 6b semantic checks that the LLM's woven prose faithfully reflects modifier intent (out of scope per [[docs/adr/0018-phase-6b-structural-only-gate]]). See §3.9 for how Phase 6b complements this work.
- Any change to Step 1 resolution, the `with` parse path, or the IR shape (beyond extending `SpanPayload`, which is internal to the emit module).

## 3. Design

### 3.1 Posture (locked)

**Loud failure**, per ADR-0016. When LLM judgment is needed and no LLM is wired, the build aborts with a structural diagnostic. The user re-runs once the LLM is wired (or removes the `with` modifier that requires it). No `.md` is written.

### 3.2 Scope (locked)

The fix covers **all three Call projection tiers** (tier 1 inline, tier 2 same_file_procedure, tier 3 external_file) plus the **stdlib/bound unresolved** path, in **both positions** (top-level under `## Steps` and lettered sub-steps inside a Branch arm). The CallBodyShape span's responsibilities for this spec are **two**: `site_modifier` weaving and LLM-grade `local_refs` cross-reference resolution. Scoped constraints are explicitly deferred to a follow-up (§7).

### 3.3 Triviality predicate

A Call is **trivial** (no LLM needed) when:

```rust
fn call_needs_llm_fill(c: &IrCall) -> bool {
    c.site_modifier.is_some() || !c.local_refs.is_empty()
}
```

Non-empty `local_refs` is treated as non-trivial because `llm_expand_pass.md` §1.2 requires a natural-language cross-reference (e.g. "the diagnosis from your earlier analysis"), which the deterministic `substitute_local_refs_in` bare substitution does not produce. (This widens the loud-failure surface beyond `with`, but matches the architecture's stated contract.)

Scoped constraints are not part of the predicate. When that responsibility is added, the predicate gains an `|| !c.scoped_constraints.is_empty()` clause and the follow-up spec re-introduces the corresponding test row.

### 3.4 Emit-site changes

Each of the seven Call emission sites adopts the same pattern: keep the existing literal path under the triviality predicate, otherwise emit a `CallBodyShape` span.

**Span boundaries.** The `CallBodyShape` span owns **only the call-body prose**. The following remain deterministic literals **outside** the span, in their current emit order:

- Numbered list prefix (`{idx}.` at top level) and lettered prefix (`{letter}.` in arms).
- The naming sentence trailing (`Refer to this result as …` / `Refer to this agent as …`) appended via `naming_sentence_for_call` + `append_sentence`.
- The return-fold suffix (`, and return that as your result.` / the §9.3 return-prose paragraph) emitted by `templates::append_return_sentence` for the Identifier-form Output Contract.
- The procedure-section anchor and ordering side-effect (`procedure_seen.insert(...)`, `procedure_order.push(...)`).

The LLM (when wired) writes only the prose that replaces the literal anchor — *"Follow the X procedure below."*, *"Load and follow the procedure in `path`."*, the resolved inline body, or *"Call `target`."*. The deterministic emitter still owns surrounding structure.

**Pseudocode — representative site (tier 2 same_file_procedure, top-level, currently `scaffold.rs:1058–1068`):**

```rust
IrNode::Call(c) if c.projection_tier == Some(2) => {
    s.push_literal(format!("{}. ", idx + 1));
    let kebab = c.target.replace('_', "-");
    let anchor = format!("Follow the {} procedure below.", kebab);
    if call_needs_llm_fill(c) {
        s.push_span(SpanRef {
            id: s.next_span_id(),
            kind: SpanKind::CallBodyShape,
            ir_node: c.node_id,
            payload: SpanPayload {
                target_name: Some(c.target.clone()),
                projection_tier: Some(2),
                site_modifier: c.site_modifier.clone(),
                resolved_body: Some(anchor),
                local_refs: c.local_refs.iter().cloned().collect(),
                ..SpanPayload::default()
            },
        });
    } else {
        s.push_literal(anchor);
    }
    if let Some(naming) = naming_sentence_for_call(c) {
        // Naming sentence trails the body — deterministic, outside the span.
        s.push_literal(format!(" {}", naming));
    }
    s.push_literal("\n");
    if procedure_seen.insert(c.target.clone()) {
        procedure_order.push(c.target.clone());
    }
}
```

Equivalent changes at the other six sites with site-specific anchors:

| Site | Position | Anchor when trivial / payload.resolved_body when non-trivial |
|---|---|---|
| tier 1 (inline) | top-level | `c.resolved_body` (with §9.1 producer-naming and §9.3 return-fold rules applied **outside** the span) |
| tier 2 (same_file_procedure) | top-level | `"Follow the {kebab} procedure below."` |
| tier 3 (external_file) | top-level | `templates::external_file_step(path)` |
| stdlib/bound (`bound_name.is_some()`) | top-level | `format!("Call \`{}\`.", c.target)` |
| tier 1 (inline) | in-arm | substituted resolved body |
| tier 2 (same_file_procedure) | in-arm | `"Follow the {kebab} procedure."` |
| tier 3 (external_file) | in-arm | `templates::external_file_step(path)` |

**Tier-1 final-call handling.** Today's top-level tier-1 path (`scaffold.rs:1020–1056`) has specialized handling for: (a) the final Step folding in the Identifier-form return suffix, (b) the producer naming sentence trailing, (c) the empty-body + return-only case. All of this remains deterministic. The span replaces only the body text — the return-fold concatenation in `templates::append_return_sentence` runs against the span's resolved-body anchor when trivial, and runs against the *literal anchor string* (not the LLM's filled prose) at scaffold-build time, with the LLM's prose merged in afterward by `merger.rs`. The merger must therefore preserve the return-suffix as a post-span literal chunk; this is already its existing contract.

**Empty body + `with` modifier.** A tier-1 Call whose `resolved_body` is empty but carries a `with` modifier is non-trivial → span emitted → stub hard-fails. (The LLM, when wired, would author the body from the modifier alone.) The span payload's `resolved_body` is `Some("")` rather than `None`, so consumers can distinguish "empty body, has modifier" from "no body field at all."

### 3.5 SpanPayload extension

`crates/glyph-core/src/emit/scaffold.rs` ~L302. Add three fields:

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
    pub target_name: Option<String>,
    pub projection_tier: Option<u8>,
    pub local_refs: Vec<LocalRefPayload>,
}

#[derive(Clone, Debug)]
pub struct LocalRefPayload {
    pub name: String,
    pub producer_node: NodeId,
}
```

`target_name` and `projection_tier` let the LLM-side filler (when wired) know the kind of Call being expanded so it can shape prose around the correct anchor and naming convention. They're cheap and Option-typed so existing span constructions (`ParamDescription`, `BranchCondition`) stay source-compatible. `local_refs` is the LLM-grade cross-reference vector.

Scoped constraints intentionally absent — see §7.

### 3.6 Stub-fill behavior change

`crates/glyph-core/src/emit/stub_fill.rs`. The `CallBodyShape` arm changes from infallible verbatim pass-through to a hard-fail when called. Because under §3.4 a `CallBodyShape` span is only ever emitted when `call_needs_llm_fill` is true, the stub's posture is "any emitted CallBodyShape means LLM was required."

```rust
#[derive(Clone, Debug)]
pub struct StubFillError {
    pub ir_node: NodeId,
    pub target_name: Option<String>,
    pub has_modifier: bool,
    pub has_local_refs: bool,
}

pub fn fill(scaffold: &Scaffold) -> Result<HashMap<SpanId, String>, Vec<StubFillError>> {
    let mut out = HashMap::new();
    let mut errors = Vec::new();
    for chunk in &scaffold.chunks {
        if let Chunk::Span(span) = chunk {
            match span.kind {
                SpanKind::ParamDescription => { out.insert(span.id, String::new()); }
                SpanKind::BranchCondition => {
                    // unchanged
                }
                SpanKind::CallBodyShape => {
                    errors.push(StubFillError {
                        ir_node: span.ir_node,
                        target_name: span.payload.target_name.clone(),
                        has_modifier: span.payload.site_modifier.is_some(),
                        has_local_refs: !span.payload.local_refs.is_empty(),
                    });
                }
            }
        }
    }
    if errors.is_empty() { Ok(out) } else { Err(errors) }
}
```

The caller (`compile_markdown` / `compile_to_md` in `expand.rs` / `scaffold.rs`) propagates the error, converts each `StubFillError` into a `G::expand::llm-required-for-call` diagnostic per §3.7, and aborts the compile before the merger runs. No `.md` is written.

### 3.7 Diagnostic

- **ID:** `G::expand::llm-required-for-call`
- **Phase:** Step 2 fill-time (pre-6b). Fires in the fill layer before merge; **not** a Phase 6b structural diagnostic. See §3.9 for the relationship.
- **Classification:** `error`. Not `repairable` — Phase 3 Repair operates on source, and this is a build configuration / filler-wiring issue.
- **Registered in:**
  - `docs/reference/diagnostics.md` — the public catalog. This is the contract-bearing location.
  - A new subsection in `docs/architecture/expand.md` (see §3.8) — internal rationale.
- **Message template** (one diagnostic per failing Call, reasons in deterministic order — `with modifier` first, then `local-ref cross-references`):
  > `` Call to `{target}` (IR node {node_id}) requires LLM-grade expansion because it has a {reasons}; this compiler build is using the stub filler. Enable the LLM expand filler, or remove the {remediation}. ``
  >
  > `{reasons}` example: *"with modifier and local-ref cross-references"* / *"with modifier"* / *"local-ref cross-references"*.
  > `{remediation}` example: *"with modifier / rewrite the local reference"* / *"with modifier"* / *"local reference"*.

- **Concrete example (matches the reviewer's preferred wording):**
  > `` Call to `inspect_failure` (IR node n3) requires LLM-grade expansion because it has a with modifier and local-ref cross-references; this compiler build is using the stub filler. Enable the LLM expand filler, or remove the with modifier / rewrite the local reference. ``

- **No `.md` file written.** Matches the §5.6 "validation failure persists" row of `expand.md`.

### 3.8 Documentation updates

1. **`docs/reference/diagnostics.md`.** Register `G::expand::llm-required-for-call` in the public diagnostic catalog with classification `error`, the trigger from §3.7, and an `expand` namespace placement. This is the contract-bearing change.

2. **`docs/architecture/expand.md`.** Two edits:
   - **§3.5 SpanKind table, `CallBodyShape` row, "Stub behavior today" cell.** Replace:
     - Before: *"Verbatim resolved body — modifier and scoped constraints currently ignored."*
     - After: *"Spans are emitted only when `site_modifier` or `local_refs` are non-empty; the stub hard-fails with `G::expand::llm-required-for-call`. Trivial Calls do not emit a span and render via the deterministic literal template. Scoped-constraint weaving is deferred (see [[todo/expand-todos]])."*
   - **New subsection §4.x or §3.x** (placement TBD by the architecture doc owner): "Step 2 fill-time diagnostics." Document that `G::expand::llm-required-for-call` is a pre-6b, fill-layer diagnostic emitted before the merger runs, distinct from §4.2's 6b structural catalog. List the single ID. Cross-reference Phase 6b's complementary structural checks (`modifier-leaked`, `unresolved-local-ref`).

3. **`llm_expand_pass.md` preamble.** Add a one-line note: the stub filler no longer silently elides `with` modifiers or LLM-grade local-ref cross-references — it refuses with a structural diagnostic until the LLM expand pass is wired.

4. **`todo/expand-todos.md`.** Add a follow-up item: *"Lower callee constraints into `IrCall.scoped_constraints`, serialize via `emit_ir.rs`, extend the CallBodyShape triviality predicate and the stub-fill `StubFillError` to cover scoped constraints. Reuses the span-emission machinery from this spec."*

### 3.9 Relationship to Phase 6b

Phase 6b is **not** the layer this diagnostic lives in, and 6b is **not** asked to gain new semantic responsibilities. This work is the pre-6b complement:

| Layer | Owns | Catches |
|---|---|---|
| Step 2 fill (this spec) | The decision to refuse silent modifier drop | Stub-cannot-fill cases → `G::expand::llm-required-for-call` |
| Phase 6b structural (existing) | The structural projection from IR to Markdown | `G::expand::modifier-leaked`, `G::expand::unresolved-local-ref`, count/order/parity checks |

When the LLM filler is eventually wired, this diagnostic stops firing on well-formed inputs, and Phase 6b's existing structural catalog continues to enforce that the LLM's prose faithfully consumes modifiers (no verbatim leak) and resolves local refs (no `{name}` token survives). Semantic-quality checks remain explicitly out of scope per ADR-0018.

## 4. Behavior matrix

| Call shape | Today | After fix |
|---|---|---|
| No modifier, no local refs | Deterministic literal | **Unchanged** — same literal, no span |
| `with "…"`, no local refs | Deterministic literal (modifier silently dropped — **bug**) | `CallBodyShape` span emitted; stub hard-fails with `G::expand::llm-required-for-call` |
| `local_refs` non-empty, no modifier | Bare `substitute_local_refs_in` substitution | Span emitted; stub hard-fails |
| `with "…"` + `local_refs` non-empty | Both silently degraded | Span emitted with both payloads; stub hard-fails listing both reasons |
| Empty body + `with "…"` (tier 1) | Empty step text (modifier dropped) | Span emitted with `resolved_body: Some("")` and modifier; stub hard-fails |
| Scoped-constraint Call | Constraints silently dropped | **Still silent today** — explicit follow-up (§7). Not regressed by this spec; tracked separately. |

## 5. Affected files

```
crates/glyph-core/src/emit/scaffold.rs
  - Extend SpanPayload (target_name, projection_tier, local_refs) per §3.5.
  - Add call_needs_llm_fill helper per §3.3.
  - Replace tier 1/2/3/stdlib literal emission (~L1037–L1091) with span-when-needed.

crates/glyph-core/src/emit/branch.rs
  - Replace tier 1/2/3 in-arm literal emission (L300–L336) with span-when-needed.

crates/glyph-core/src/emit/stub_fill.rs
  - Change fill() signature to Result<HashMap, Vec<StubFillError>>.
  - Define StubFillError per §3.6.

crates/glyph-core/src/emit/merger.rs
  - Plumb Result up; update test fixtures.

crates/glyph-core/src/expand.rs (compile_markdown / compile_to_md)
  - Propagate stub_fill Err into diagnostics; emit one G::expand::llm-required-for-call per failing span; abort before merger runs; do not write .md.

crates/glyph-core/src/diagnostic.rs (or wherever IDs live)
  - Register G::expand::llm-required-for-call.

docs/reference/diagnostics.md
  - Register the diagnostic ID with trigger text (§3.8 item 1).

docs/architecture/expand.md
  - §3.5 stub-behavior cell update; new "Step 2 fill-time diagnostics" subsection (§3.8 item 2).

llm_expand_pass.md
  - Preamble note on refusal semantics (§3.8 item 3).

todo/expand-todos.md
  - Add scoped-constraints follow-up entry (§3.8 item 4).
```

## 6. Test plan

The matrix below covers seven emit sites × responsibility combinations, plus regression coverage for the deterministic paths that are intentionally unchanged.

### 6.1 Regression: trivial Calls unchanged

Per-site regression tests (one per emit path), each asserting the exact rendered Markdown is byte-identical to today's output:

- T1 top-level inline Call with no modifier and no local_refs → still emits resolved body inline.
- T2 top-level same_file_procedure Call → still emits `"N. Follow the {kebab} procedure below."`.
- T3 top-level external_file Call → still emits `templates::external_file_step(path)`.
- Stdlib/bound top-level Call with `bound_name.is_some()` and no modifier → still emits `"N. Call \`target\`."`.
- T1 in-arm inline Call → still emits substituted resolved body.
- T2 in-arm same_file_procedure → still emits `"   X. Follow the {kebab} procedure."`.
- T3 in-arm external_file → still emits external_file_step.

These do not rely on snapshot-passes-through; each is an explicit assert against the rendered string. Branching inside formatting-sensitive emit code (per reviewer item 11) warrants targeted coverage.

### 6.2 New: hard-fail on `with` modifier (per site)

One test per emit site, each:
- Builds a skill exercising the site with a non-empty `site_modifier`.
- Asserts compile aborts with exactly one `G::expand::llm-required-for-call` diagnostic naming the correct IR node id.
- Asserts no `.md` file is written.
- Asserts the diagnostic message includes the target name, IR node id, `"with modifier"` reason, and the remediation hint.

Seven sites → seven tests. The flow_assign with-modifier corpus fixture and any multi-file fix/review fixtures that today use `with` are updated to expect hard-failure (or moved to a dedicated `expected-failure` corpus directory if one exists).

### 6.3 New: hard-fail on `local_refs` (per site, where the site can host local_refs)

Tier 1 paths (both top-level and in-arm) accept local_refs. Two tests, each asserting hard-fail with the `"local-ref cross-references"` reason.

### 6.4 New: combined modifier + local_refs

One test where a single Call has both. Asserts:
- One diagnostic per failing Call (not two).
- The reasons substring is exactly `"with modifier and local-ref cross-references"` — deterministic order, with-first.
- The remediation hint includes both parts.

### 6.5 New: multiple failing spans

One test with a skill containing two distinct Calls each requiring LLM fill. Asserts:
- Two `G::expand::llm-required-for-call` diagnostics emitted.
- Each names the correct IR node id.
- Stable ordering (by IR node id ascending).

### 6.6 New: no output file on failure

Existing CI helpers assert exit non-zero and stderr carries the diagnostic; explicitly assert `compiled.md` is absent on disk after the failing compile (matches `expand.md` §5.6).

### 6.7 New: span boundaries — naming and return-fold stay outside span

Two assertions tested via deterministic-emit-only inspection of the scaffold chunks:
- For a tier-1 top-level Call with a `with` modifier as the final Step with Identifier-form return: the scaffold contains `[Literal("N. "), Span(CallBodyShape), Literal(", and return that as your result.\n")]`. The return-fold literal is **not** inside the span.
- For a tier-2 top-level Call with a `with` modifier and a `bound_name`: the scaffold contains `[Literal("N. "), Span(CallBodyShape), Literal(" Refer to this result as <n>.\n")]`. The naming sentence is **not** inside the span.

### 6.8 IR-node-id stability

Each new test asserts the diagnostic names the failing Call by its stable IR node id (`n0`, `n1`, …) consistent with `expand.md` §3.1.

### 6.9 Excluded (deferred)

- Scoped-constraint Calls: no test in this spec. The follow-up spec (§7) will add coverage when `IrCall.scoped_constraints` is introduced.

## 7. Follow-up work

- **Scoped constraints.** Lower callee constraints into a new `IrCall.scoped_constraints` field; serialize via `emit_ir.rs` (today hardcoded to `[]`); extend the §3.3 triviality predicate to `|| !c.scoped_constraints.is_empty()`; extend `SpanPayload` and `StubFillError` accordingly; reuse the span-emission machinery this spec introduces. Tracked in `todo/expand-todos.md` per §3.8 item 4.
- **LLM filler wiring.** The actual LLM call that fills `CallBodyShape` spans is tracked separately. Once wired, this spec's diagnostic stops firing on well-formed inputs and Phase 6b's existing `modifier-leaked` / `unresolved-local-ref` checks take over enforcement.

## 8. Open questions

- **Architecture doc owner placement decision** for the new "Step 2 fill-time diagnostics" subsection (§3.8 item 2). I've left placement TBD between a new §3.x and a new §4.x.
- **Exact wording** of the deterministic reason and remediation phrases (§3.7) can be tightened during implementation; the example matches the reviewer's preferred shape.
