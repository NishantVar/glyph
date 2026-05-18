# CallBodyShape Span Emission — Closing the `with` Modifier Drop

**Status:** draft (rev 5, post-fourth-pass review)
**Date:** 2026-05-18
**Phase:** 6 / Step 2 (Expand)
**Related ADRs:** [[docs/adr/0016-llm-reshape-no-deterministic-fallback]], [[docs/adr/0018-phase-6b-structural-only-gate]]
**Related docs:** [[docs/architecture/expand]] §3.5, [[llm_expand_pass]] §1.1–§1.2, [[docs/reference/diagnostics]]

## 1. Problem

A `with "…"` site modifier on a Call was silently dropped from the compiled Markdown. The reproduction: a Branch whose then-arm Call had `site_modifier: "name each construct and show it beside the instruction it creates"` rendered identically to the otherwise-arm Call — both produced the literal `"Follow the build-walkthrough procedure."`. The modifier's intent never reached the agent-facing artifact.

The IR is correct: `with` parses and lowers into `IrCall.site_modifier` faithfully. The gap is in Phase 6 Step 2 (the deterministic emitter):

- `SpanKind::CallBodyShape` and `SpanPayload { site_modifier, resolved_body, … }` are defined in `crates/glyph-core/src/emit/scaffold.rs`.
- `crates/glyph-core/src/emit/stub_fill.rs:24` knows how to fill a `CallBodyShape` span (verbatim `resolved_body`, with modifier deliberately ignored — documented in `expand.md` §3.5).
- **No emit site ever pushes a `CallBodyShape` span.** Every Call emission path pushes a literal-template string. The seven sites are **all three tiers in both positions, plus top-level stdlib/bound unresolved** = 3 × 2 + 1 = 7:
  - `scaffold.rs` top-level: tier 1 inline (~L1037), tier 2 same-file procedure (~L1060), tier 3 external file (~L1071), stdlib/bound unresolved (`bound_name.is_some()`, ~L1086).
  - `branch.rs::emit_lettered_substeps` in-arm: tier 1 inline (L300–318), tier 2 same-file procedure (L319–327), tier 3 external file (L328–336).
  - (Stdlib/bound in-arm is structurally impossible today: lettered sub-steps are only emitted under a Branch, and the bound-name path is a top-level skill-Step shape. If branch.rs gains stdlib/bound handling later, that site adopts the same pattern.)

Because no span is emitted, the modifier is structurally invisible to the fill layer — even a fully-wired LLM expand pass would never see it. LLM-grade `local_refs` cross-references (`llm_expand_pass.md` §1.2) are silently degraded to bare substitution in the same way.

## 2. Goals & Non-Goals

**Goals.**

1. Plumb every Call emission path (all seven sites above) through a `CallBodyShape` span when LLM judgment is required.
2. Make the failure mode loud at **production** entry points (`emit::emit` and every caller that produces `CompileOutcome`): when the stub filler is asked to fill a `CallBodyShape` span it cannot, the production emit returns an error variant that the lib-level callers convert into a `CompileOutcome::Diagnostics(bag)`, which `compile_directory_with_layout` already routes to `FileOutcome::Failed` and therefore already skips `atomic_write`. No silent drop. No deterministic fallback that produces clunkier prose (per [[docs/adr/0016-llm-reshape-no-deterministic-fallback]]).
3. Preserve existing behavior for Calls that need no LLM judgment, so today's snapshots and test corpus are unaffected except in the cases that are actually buggy today.

**Non-goals.**

- **Scoped constraints are out of scope.** `IrCall` has no `scoped_constraints` field today (`emit_ir.rs` hardcodes `"scoped_constraints": []`). Lowering callee constraints into the call site, serializing the field, and exercising it end-to-end is a separate, larger piece of work tracked as a follow-up in §7. The CallBodyShape span this spec emits does **not** carry scoped constraints, and the triviality predicate does not check them.
- Wiring the actual LLM filler. This spec covers the deterministic-emitter + stub side only.
- Phase 6b semantic checks that the LLM's woven prose faithfully reflects modifier intent (out of scope per [[docs/adr/0018-phase-6b-structural-only-gate]]). See §3.9 for how Phase 6b complements this work.
- Any change to Step 1 resolution, the `with` parse path, or the IR shape (beyond extending `SpanPayload`, which is internal to the emit module, and the source-span follow-up tracked in §7).

## 3. Design

### 3.1 Posture (locked)

**Loud failure**, per ADR-0016. When LLM judgment is needed and no LLM is wired, the build aborts with a structural diagnostic. The user re-runs once the LLM is wired (or removes the `with` modifier that requires it). No `.md` is written — and this is enforced by the `CompileOutcome::Diagnostics` path in `lib.rs`, which never reaches `atomic_write` (see §3.6).

### 3.2 Scope (locked)

The fix covers **all three Call projection tiers** (tier 1 inline, tier 2 same_file_procedure, tier 3 external_file) **in both positions** (top-level under `## Steps` and lettered sub-steps inside a Branch arm), **plus the top-level stdlib/bound unresolved path** (`bound_name.is_some()`). That is 3 × 2 + 1 = **7 emit sites**. The CallBodyShape span's responsibilities for this spec are **two**: `site_modifier` weaving and LLM-grade `local_refs` cross-reference resolution. Scoped constraints are explicitly deferred to a follow-up (§7).

### 3.3 Triviality predicate

A Call is **trivial** (no LLM needed) when:

```rust
fn call_needs_llm_fill(c: &IrCall) -> bool {
    c.site_modifier.is_some() || !c.local_refs.is_empty()
}
```

Non-empty `local_refs` is treated as non-trivial because `llm_expand_pass.md` §1.2 requires a natural-language cross-reference (e.g. "the diagnosis from your earlier analysis"), which the deterministic `substitute_local_refs_in` bare substitution does not produce. (This widens the loud-failure surface beyond `with`, but matches the architecture's stated contract.)

Scoped constraints are not part of the predicate. When that responsibility is added, the predicate gains an `|| !c.scoped_constraints.is_empty()` clause and the follow-up spec re-introduces the corresponding test row.

**`local_refs` is not gated by tier today.** `populate_local_refs_in_steps` walks every `IrCall.resolved_body` and does not gate on `projection_tier`; tier-2 Calls (and any future tier whose lower-time fills in `resolved_body`) can therefore carry non-empty `local_refs`. The triviality predicate is uniform across all seven sites, and the hard-fail path is uniform too — any non-empty `local_refs` at any site fires the diagnostic. Tests in §6.3 cover tier-1 sites primarily (where the pattern is most common in real corpora) plus a tier-2 case to nail down the uniformity. Tightening the IR side (e.g. clearing `local_refs` on non-inline Calls) is a separate piece of work and is not a precondition for this spec.

### 3.4 Emit-site changes

Each of the seven Call emission sites adopts the same pattern: keep the existing literal path under the triviality predicate, otherwise emit a `CallBodyShape` span. The span owns **only the call-body prose** as one chunk; everything else is a separate `Literal` chunk in the scaffold.

**Span boundaries — chunk layout.** For every site, the scaffold emits an explicit chunk sequence. The literal chunks around the span carry the surrounding structure deterministically:

```
[ Literal("{idx}. ")                     // numbered prefix (top-level) or "   {letter}. " (in-arm)
, Span(CallBodyShape, payload)            // body prose only (span owns the call body)
, Literal(" {naming sentence}")?          // §9.1 producer naming (only when naming_sentence_for_call returns Some)
, Literal("\n")                           // line terminator
]
```

The following remain deterministic literals **outside** the span:

- Numbered list prefix (`{idx}. ` at top level) and lettered prefix (`   {letter}. ` in arms).
- The naming sentence (`Refer to this result as …` / `Refer to this agent as …`) appended via `naming_sentence_for_call` + `append_sentence`. This is the chunk **after** the span when present.
- The trailing `\n`.
- The procedure-section anchor and ordering side-effect (`procedure_seen.insert(...)`, `procedure_order.push(...)`).

The LLM (when wired) writes only the prose that replaces the literal anchor — *"Follow the X procedure below."*, *"Load and follow the procedure in `path`."*, the resolved inline body, or *"Call `target`."*. The deterministic emitter still owns surrounding structure.

**Return-fold mechanism (two different cases; do not conflate).**

`IrCall`'s return contract surfaces in two distinct ways today, and the spec treats them differently:

- **§8.4 Output Contract return sentence** (Identifier-form / Description-form Output Contract on the *final Step* of the skill). `templates::append_return_sentence(body, sent)` strips trailing punctuation from `body` then appends `". {return_sentence}"`. Today's `return_sentence` is computed by `templates::compute_return_sentence` and produces values like `"Produce \`current_branch\`."` or `"Return a list of branch names."` — **not** the wording "`, and return that as your result.`" that earlier revs used as an example.

  Because the punctuation-strip is a *function over the rendered body string*, it cannot be expressed as a fixed post-span `Literal` chunk that runs before fill (the LLM may produce any terminal punctuation, or none). Two options were considered:

  - *(rejected)* Filler-contract: require the LLM filler to emit body prose without terminal punctuation when `payload.has_post_return_fold = true`. Pushes the contract into a place that's hard to enforce and easy to silently violate.
  - *(chosen)* **Post-merge operation.** The scaffold records the computed `return_sentence` on the span payload (new field `post_merge_return_sentence: Option<String>`). The merger, after substituting span-fills back into the chunk stream and producing the final body string for the Call's line, runs `templates::append_return_sentence` against the merged body when this field is `Some`. The naming-sentence post-span Literal chunk, if present, is appended after the return-fold result.

  The chunk layout for a final tier-1 Call with a `with` modifier and an Identifier-form return is therefore:

  ```
  [ Literal("N. ")
  , Span(CallBodyShape, payload { post_merge_return_sentence: Some("Produce `<id>`."), ... })
  , Literal(" Refer to this result as <n>.")?     // naming sentence, only when present
  , Literal("\n")
  ]
  ```

  When the stub filler hard-fails, the merger never runs, so the post-merge return-fold never runs either; the diagnostic alone is the user-facing surface. No `.md` is written. When the LLM filler is later wired, the merger's post-merge step runs against the LLM's prose.

- **§9.3 flow-local return prose** (`Your result is …`). This is emitted **today as a separate numbered Step appended after the flow loop**, not as a suffix on a Call line. It is not touched by this spec, not represented in any span, and not part of any post-span chunk. The chunk-layout description above applies only to per-Call lines.

The merger contract therefore needs **one** small addition: when a `CallBodyShape` span carries `post_merge_return_sentence: Some(sent)`, run `templates::append_return_sentence(merged_body, sent)` to produce the line's body. Everything else is unchanged.

**Tier-1 raw-slot rule (local_refs).** The CallBodyShape span's `payload.resolved_body` for a non-trivial tier-1 Call (both top-level and in-arm) carries the **raw** `c.resolved_body` — `{name}` slots **intact**, not pre-substituted. The LLM filler weaves the cross-reference using `payload.local_refs` (which carries `crate::ir::LocalRef` values, see §3.5) to produce natural-language references like *"the diagnosis from your earlier analysis"* rather than bare names. The trivial tier-1 path retains today's `substitute_local_refs_in` bare-substitution behavior. This is the load-bearing distinction between the trivial and non-trivial tier-1 paths.

**Pseudocode — representative site (tier 2 same_file_procedure, top-level, currently `scaffold.rs:1058–1068`):**

`IrCall` is **not** modified. Match guards continue to switch on the existing `c.projection_tier: Option<u8>` field; the deterministic emitter maps that into a `ProjectionMode` value on the span payload at push time. The IR shape is unchanged (per §2 non-goals).

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
                projection_mode: Some(ProjectionMode::SameFileProcedure),  // mapped from c.projection_tier
                site_modifier: c.site_modifier.clone(),
                resolved_body: Some(anchor),
                local_refs: c.local_refs.clone(),  // crate::ir::LocalRef, see §3.5
                ..SpanPayload::default()
            },
        });
    } else {
        s.push_literal(anchor);
    }
    if let Some(naming) = naming_sentence_for_call(c) {
        s.push_literal(format!(" {}", naming));   // §9.1 producer naming as post-span Literal
    }
    s.push_literal("\n");
    if procedure_seen.insert(c.target.clone()) {
        procedure_order.push(c.target.clone());
    }
}
// Return-fold (§8.4 Output Contract) is handled separately and only at the
// top-level tier-1 final-call site — see "Return-fold mechanism" below. It is
// NOT emitted from this tier-2 pseudocode (tier-2 anchors have never carried
// the return-fold in today's emitter; rev 5 does not add it).
```

A small helper centralises the tier-to-mode mapping so all seven sites use one source of truth:

```rust
// Mirrors the actual emit-site match order: a Call with a bound_name AND a
// projection_tier of 1/2/3 (e.g. a bound user-block) routes through its tier
// path, not the stdlib anchor. StdlibBound is reached only when no tier applies.
fn projection_mode_from(c: &IrCall) -> Option<ProjectionMode> {
    match c.projection_tier {
        Some(1) => Some(ProjectionMode::Inline),
        Some(2) => Some(ProjectionMode::SameFileProcedure),
        Some(3) => Some(ProjectionMode::ExternalFile),
        _ if c.bound_name.is_some() => Some(ProjectionMode::StdlibBound),
        _ => None,
    }
}
```

**Tier-1 pseudocode sketch (top-level and in-arm, non-trivial case):**

```rust
// Match guard: c.projection_tier == Some(1).
// trivial path: existing substitute_local_refs_in flow, unchanged.
// non-trivial path:
s.push_span(SpanRef {
    id: s.next_span_id(),
    kind: SpanKind::CallBodyShape,
    ir_node: c.node_id,
    payload: SpanPayload {
        target_name: Some(c.target.clone()),
        projection_mode: Some(ProjectionMode::Inline),    // mapped from c.projection_tier
        site_modifier: c.site_modifier.clone(),
        resolved_body: c.resolved_body.clone(),           // RAW {name} slots, not pre-substituted
        local_refs: c.local_refs.clone(),                 // crate::ir::LocalRef
        ..SpanPayload::default()
    },
});
```

Equivalent changes at the other six sites with site-specific anchors:

| Site | Position | Anchor when trivial / payload.resolved_body when non-trivial |
|---|---|---|
| tier 1 (inline) | top-level | trivial: substituted `c.resolved_body`. non-trivial: **raw** `c.resolved_body` with `{name}` slots intact (return-fold / naming sentence stay as separate post-span Literal chunks). |
| tier 2 (same_file_procedure) | top-level | `"Follow the {kebab} procedure below."` |
| tier 3 (external_file) | top-level | `templates::external_file_step(path)` |
| stdlib/bound (`bound_name.is_some()`) | top-level | `format!("Call \`{}\`.", c.target)` |
| tier 1 (inline) | in-arm | trivial: substituted `c.resolved_body`. non-trivial: **raw** `c.resolved_body` (same rule as tier-1 top-level). |
| tier 2 (same_file_procedure) | in-arm | `"Follow the {kebab} procedure."` |
| tier 3 (external_file) | in-arm | `templates::external_file_step(path)` |

**Tier-1 final-call handling.** Today's top-level tier-1 path (`scaffold.rs:1020–1056`) has specialized handling for: (a) the final Step folding in the §8.4 Output-Contract return sentence via `templates::append_return_sentence`, (b) the producer naming sentence trailing, (c) the empty-body + return-only case. All of this remains deterministic. The span replaces only the body text. The return sentence is carried on `payload.post_merge_return_sentence` and applied by the merger (see "Return-fold mechanism" below) rather than emitted as a fixed post-span `Literal`, because `append_return_sentence` strips terminal body punctuation before appending — a transformation that cannot run before the body is filled.

**Empty body + `with` modifier.** A tier-1 Call whose `resolved_body` is empty but carries a `with` modifier is non-trivial → span emitted → stub hard-fails. (The LLM, when wired, would author the body from the modifier alone.) The span payload's `resolved_body` is `Some("")` rather than `None`, so consumers can distinguish "empty body, has modifier" from "no body field at all."

### 3.5 SpanPayload extension

`crates/glyph-core/src/emit/scaffold.rs` ~L302. Add a `ProjectionMode` enum to replace the loose `Option<u8>` tier indicator, and add three fields to `SpanPayload`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionMode {
    Inline,                 // tier 1: resolved_body is inlined
    SameFileProcedure,      // tier 2: a separate procedure section in the same file
    ExternalFile,           // tier 3: a separate .md file
    StdlibBound,            // bound_name.is_some() (today: stdlib / Library only)
}

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
    // New for CallBodyShape:
    pub target_name: Option<String>,
    pub projection_mode: Option<ProjectionMode>,
    pub local_refs: Vec<crate::ir::LocalRef>,   // reuse the existing IR type, no new payload struct
    pub post_merge_return_sentence: Option<String>,  // §3.4 return-fold post-merge step
}
```

- `target_name` and `projection_mode` let the LLM-side filler (when wired) know the kind of Call being expanded so it can shape prose around the correct anchor and naming convention. They're cheap and `Option`-typed so existing span constructions (`ParamDescription`, `BranchCondition`) stay source-compatible.
- `local_refs` is the LLM-grade cross-reference vector. **It reuses `crate::ir::LocalRef` directly** — no new `LocalRefPayload` wrapper. The producing pseudocode is simply `c.local_refs.clone()`, with no field translation. If a future change to `LocalRef` needs more fields the span payload picks them up automatically.
- `post_merge_return_sentence` carries the §8.4 Output-Contract return sentence (e.g. `"Produce \`current_branch\`."`) computed via `templates::compute_return_sentence` at scaffold-build time. The merger runs `templates::append_return_sentence(merged_body, sent)` against the span's final rendered body. **Scope is intentionally narrow:** set this field only where the current emitter would have called `templates::append_return_sentence` for a Call — i.e. the top-level tier-1 final-call path in `scaffold.rs:1020–1056`. Tier-2, tier-3, and stdlib/bound anchors do **not** carry the return-fold in today's emitter, and this spec does not add it for them. `None` otherwise. §9.3 flow-local return prose is **not** carried here — it remains a separate post-loop Step (see §3.4).

Scoped constraints intentionally absent — see §7.

### 3.6 Stub-fill and production-path plumbing

Two coupled changes: the stub returns a `Result`, and **`emit::emit` (the production entry point) plus its lib-level callers propagate the failure into `CompileOutcome::Diagnostics`**. The `compile_directory_with_layout` path (`lib.rs:1693`) already routes `CompileOutcome::Diagnostics` to `FileOutcome::Failed` and **never reaches `atomic_write`**, so the loud-failure posture is enforced without new bookkeeping.

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

`crates/glyph-core/src/emit/mod.rs`. The production entry `emit` (`emit/mod.rs:16`) returns `Result<String, Vec<StubFillError>>`:

```rust
pub fn emit(arena: &IrArena, enable_effects: bool) -> Result<String, Vec<StubFillError>> {
    let scaffold = scaffold::build(arena, enable_effects);
    match stub_fill::fill(&scaffold) {
        Ok(fills) => Ok(merger::merge(scaffold, fills).expect("scaffold/fill mismatch is a bug")),
        Err(errors) => Err(errors),
    }
}
```

`crates/glyph-core/src/lib.rs`. The two functions that build `CompileOutcome` from emit output (`compile_source_with_effects` ~L142, `compile_source_with_resolved_imports` ~L2672/2741) wrap the `Err` variant into a fresh diagnostic bag:

```rust
// inside e.g. compile_source_with_effects, where emit::emit is currently called:
let markdown = match emit::emit(&arena, enable_effects) {
    Ok(md) => md,
    Err(mut errors) => {
        // Item 3 enforcement: DiagBag::sorted() sorts by (file, byte_start, id) and falls
        // back to insertion order for ties. All llm-required-for-call diagnostics share
        // the same synthetic offset, so the IR-node-id tiebreaker has to be made real
        // by sorting BEFORE pushing into the bag.
        errors.sort_by_key(|e| e.ir_node.0);
        let mut bag = DiagBag::new();
        let li = LineIndex::new("");                          // synthetic; see §3.7
        let label = source_label_for(file_path);              // file path string, no line/col
        for e in errors {
            let span = Span::new(0, 0, 0);                    // synthetic zero-width at file start
            bag.push(
                Diagnostic::error(
                    "G::expand::llm-required-for-call",
                    format_llm_required_message(&e),          // §3.7
                    SourceSpan::from_byte_span(&label, span, &li),
                ),
                span,
            );
        }
        return Ok(CompileOutcome::Diagnostics(bag));
    }
};
```

(This mirrors the existing fallback at `lib.rs:1726–1747` which already synthesizes diagnostics with a zero-width `Span::new(0, 0, 0)` for pipeline failures that aren't already wired to a structured ID.)

`compile_directory_with_layout` requires **no change**: the existing match-arm at `lib.rs:1693` (`Ok(CompileOutcome::Diagnostics(mut bag))`) routes the new diagnostic to `FileOutcome::Failed`, suppresses `atomic_write`, and propagates `any_failure = true` for non-zero exit.

Test helpers (`compile_markdown`, `compile_to_md`) absorb the new `Result` shape — most call `emit::emit` indirectly through `compile_source_*` and so are unaffected. The two helpers that call `emit::emit` directly (if any survive after the signature change) panic on `Err` with a clear message, since they exist for snapshot tests of happy-path output.

### 3.7 Diagnostic

- **ID:** `G::expand::llm-required-for-call`
- **Phase:** Step 2 fill-time (pre-6b). Fires in the fill layer before merge; **not** a Phase 6b structural diagnostic. See §3.9 for the relationship.
- **Classification:** `error`. Not `repairable` — Phase 3 Repair operates on source, and this is a build configuration / filler-wiring issue.
- **Source span (synthetic).** `IrCall` has no source span field today. Per the existing pattern at `lib.rs:1726–1747`, this spec uses a **synthetic zero-width file-level span** (`Span::new(0, 0, 0)` against an empty `LineIndex`, with the source file path as the `label`). The diagnostic message names the IR node id (`n3`, `n7`, …) so the failing Call is unambiguously identifiable to the user even without precise source coordinates. Surfacing a real source span for `IrCall` is tracked as a follow-up in §7 (it requires threading a span through `IrCall`, parser → lower → IR; out of scope for this spec).
- **Ordering.** `DiagBag::sorted()` sorts by `(file, byte_start, id)` and otherwise relies on insertion order. Because all `G::expand::llm-required-for-call` diagnostics share the same file, the same synthetic byte offset (0), and the same ID, the **IR-node-id tiebreaker is enforced by sorting the `Vec<StubFillError>` by `ir_node.0` before pushing into the bag** (see §3.6 pseudocode). Without that explicit sort the order would track scaffold-visit order, which today happens to be node-id ascending but is not contractually so. Tests in §6.5 assert the sorted output.
- **Registered in:**
  - `docs/reference/diagnostics.md` — the public catalog. This is the contract-bearing location.
  - A new subsection in `docs/architecture/expand.md` (see §3.8) — internal rationale.
- **Message construction.** The reason phrase is **prebuilt deterministically** by the caller before formatting the message — no template-substitution glue, no risk of `"a {empty}"` or grammar bugs:

  ```rust
  fn format_llm_required_message(e: &StubFillError) -> String {
      let reason_phrase = match (e.has_modifier, e.has_local_refs) {
          (true,  false) => "a with modifier",
          (false, true ) => "local-ref cross-references",
          (true,  true ) => "a with modifier and local-ref cross-references",
          (false, false) => unreachable!(
              "StubFillError pushed only when site_modifier or local_refs is non-empty"
          ),
      };
      let remediation = match (e.has_modifier, e.has_local_refs) {
          (true,  false) => "the with modifier",
          (false, true ) => "the local reference",
          (true,  true ) => "the with modifier / rewrite the local reference",
          (false, false) => unreachable!(),
      };
      let target = e.target_name.as_deref().unwrap_or("<unknown>");
      let node = format!("n{}", e.ir_node.0);   // NodeId does not implement Display today; format the inner u32 directly.
      format!(
          "Call to `{target}` (IR node {node}) requires LLM-grade expansion because it has \
           {reason_phrase}; this compiler build is using the stub filler. \
           Enable the LLM expand filler, or remove {remediation}.",
          target = target,
          node = node,
          reason_phrase = reason_phrase,
          remediation = remediation,
      )
  }
  ```

  Reason order is deterministic by construction (with-modifier first, then local-ref cross-references, when both apply).

- **Concrete example (combined case):**
  > `` Call to `inspect_failure` (IR node n3) requires LLM-grade expansion because it has a with modifier and local-ref cross-references; this compiler build is using the stub filler. Enable the LLM expand filler, or remove the with modifier / rewrite the local reference. ``

- **No `.md` file written.** Guaranteed by the existing `CompileOutcome::Diagnostics` branch in `compile_directory_with_layout` — see §3.6.

### 3.8 Documentation updates

1. **`docs/reference/diagnostics.md`.** Register `G::expand::llm-required-for-call` in the public diagnostic catalog with classification `error`, the trigger from §3.7, and an `expand` namespace placement. This is the contract-bearing change.

2. **`docs/architecture/expand.md`.** Two edits:
   - **§3.5 SpanKind table, `CallBodyShape` row, "Stub behavior today" cell.** Replace:
     - Before: *"Verbatim resolved body — modifier and scoped constraints currently ignored."*
     - After: *"Spans are emitted only when `site_modifier` or `local_refs` are non-empty; the stub hard-fails with `G::expand::llm-required-for-call`. Trivial Calls do not emit a span and render via the deterministic literal template. Scoped-constraint weaving is deferred (see [[todo/expand-todos]])."*
   - **New subsection §4.x or §3.x** (placement TBD by the architecture doc owner): "Step 2 fill-time diagnostics." Document that `G::expand::llm-required-for-call` is a pre-6b, fill-layer diagnostic emitted before the merger runs, distinct from §4.2's 6b structural catalog. List the single ID. Cross-reference Phase 6b's complementary structural checks (`modifier-leaked`, `unresolved-local-ref`).

3. **`llm_expand_pass.md` preamble.** Add a one-line note: the stub filler no longer silently elides `with` modifiers or LLM-grade local-ref cross-references — it refuses with a structural diagnostic until the LLM expand pass is wired.

4. **`todo/expand-todos.md`.** Add two follow-up items:
   - *Scoped constraints:* "Lower callee constraints into `IrCall.scoped_constraints`, serialize via `emit_ir.rs`, extend the CallBodyShape triviality predicate and the stub-fill `StubFillError` to cover scoped constraints. Reuses the span-emission machinery from this spec."
   - *Source spans on IrCall:* "Thread a `SourceSpan` (or byte-offset pair) through `IrCall` from parser → lower → IR so `G::expand::llm-required-for-call` can carry a real source span instead of the synthetic zero-width file-level span the introductory spec uses."

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
| `local_refs` non-empty, no modifier | Bare `substitute_local_refs_in` substitution where applicable | Span emitted (with raw-slot `resolved_body` on tier-1 sites; other tiers carry whatever anchor that site uses, unchanged); stub hard-fails |
| `with "…"` + `local_refs` non-empty | Both silently degraded | Span emitted with both payloads; stub hard-fails listing both reasons |
| Empty body + `with "…"` (tier 1) | Empty step text (modifier dropped) | Span emitted with `resolved_body: Some("")` and modifier; stub hard-fails |
| Scoped-constraint Call | Constraints silently dropped | **Still silent today** — explicit follow-up (§7). Not regressed by this spec; tracked separately. |

## 5. Affected files

```
crates/glyph-core/src/emit/scaffold.rs
  - Add ProjectionMode enum per §3.5.
  - Extend SpanPayload (target_name, projection_mode, local_refs: Vec<crate::ir::LocalRef>) per §3.5.
  - Add call_needs_llm_fill helper per §3.3.
  - Replace tier 1/2/3/stdlib literal emission (~L1037–L1091) with span-when-needed.
  - Tier-1 non-trivial path uses RAW c.resolved_body (with {name} slots) per §3.4.

crates/glyph-core/src/emit/branch.rs
  - Replace tier 1/2/3 in-arm literal emission (L300–L336) with span-when-needed.
  - Tier-1 in-arm non-trivial path uses RAW c.resolved_body per §3.4.

crates/glyph-core/src/emit/stub_fill.rs
  - Change fill() signature to Result<HashMap, Vec<StubFillError>>.
  - Define StubFillError per §3.6.

crates/glyph-core/src/emit/merger.rs
  - No signature change required — merger still receives the OK fill map.
  - New behaviour: when a CallBodyShape span carries
    payload.post_merge_return_sentence == Some(sent), run
    templates::append_return_sentence(merged_body, sent) before emitting the
    span's contribution to the final string. Naming-sentence post-span Literal
    chunks (already in the chunk stream) merge in their existing position.
  - Update internal call sites and test fixtures that assumed an infallible fill.

crates/glyph-core/src/emit/mod.rs
  - Change emit() signature to Result<String, Vec<StubFillError>> per §3.6.

crates/glyph-core/src/lib.rs
  - In every CompileOutcome-producing function that calls emit::emit
    (compile_source_with_effects ~L142, compile_source_with_resolved_imports ~L2672/2741):
      convert Err(errors) into CompileOutcome::Diagnostics(bag) with one
      G::expand::llm-required-for-call diagnostic per failing span, using the
      synthetic zero-width file-level SourceSpan pattern at L1738–1743.
  - compile_directory_with_layout (L1462): no change — existing Diagnostics
    branch at L1693 already routes to FileOutcome::Failed and skips atomic_write.

docs/reference/diagnostics.md
  - Register the diagnostic ID with trigger text (§3.8 item 1).

docs/architecture/expand.md
  - §3.5 stub-behavior cell update; new "Step 2 fill-time diagnostics" subsection (§3.8 item 2).

llm_expand_pass.md
  - Preamble note on refusal semantics (§3.8 item 3).

todo/expand-todos.md
  - Add scoped-constraints follow-up entry (§3.8 item 4a).
  - Add IrCall source-span follow-up entry (§3.8 item 4b).
```

## 6. Test plan

Covers seven emit sites × responsibility combinations, plus regression coverage for the deterministic paths that are intentionally unchanged.

### 6.1 Regression: trivial Calls unchanged

Per-site regression tests (one per emit path), each asserting the exact rendered Markdown is byte-identical to today's output:

- T1 top-level inline Call with no modifier and no local_refs → still emits resolved body inline.
- T2 top-level same_file_procedure Call → still emits `"N. Follow the {kebab} procedure below."`.
- T3 top-level external_file Call → still emits `templates::external_file_step(path)`.
- Stdlib/bound top-level Call with `bound_name.is_some()` and no modifier → still emits `"N. Call \`target\`."`.
- T1 in-arm inline Call → still emits substituted resolved body (i.e. `substitute_local_refs_in` runs).
- T2 in-arm same_file_procedure → still emits `"   X. Follow the {kebab} procedure."`.
- T3 in-arm external_file → still emits external_file_step.

These do not rely on snapshot-passes-through; each is an explicit assert against the rendered string. Branching inside formatting-sensitive emit code warrants targeted coverage.

### 6.2 New: hard-fail on `with` modifier (per site)

One test per emit site, each:
- Builds a skill exercising the site with a non-empty `site_modifier`.
- Asserts compile produces `CompileOutcome::Diagnostics` (not `Compiled`) carrying exactly one `G::expand::llm-required-for-call` diagnostic naming the correct IR node id.
- Asserts no `.md` file is written (via `compile_directory_with_layout`'s `FileOutcome::Failed` path).
- Asserts the diagnostic message includes the target name, IR node id, the `"a with modifier"` reason phrase, and `"the with modifier"` remediation.

Seven sites → seven tests. Existing `with`-modifier corpus fixtures (`flow_assign` and any multi-file fixtures) are updated to expect hard-failure (or moved to a dedicated `expected-failure` corpus directory if one exists).

### 6.3 New: hard-fail on `local_refs`

Per §3.3, `IrCall.local_refs` is **not** gated by tier today — `populate_local_refs_in_steps` can populate it on any Call whose `resolved_body` contains a `{name}` slot. The hard-fail path is uniform across tiers. Three tests pin the behaviour:

- Tier-1 top-level inline Call with non-empty `local_refs` and **no** modifier.
- Tier-1 in-arm inline Call with non-empty `local_refs` and **no** modifier.
- Tier-2 top-level Call with non-empty `local_refs` and **no** modifier (uniformity case).

Each asserts:
- Hard-fail with `"local-ref cross-references"` reason phrase and `"the local reference"` remediation.
- For tier-1: the `CallBodyShape` span's `payload.resolved_body` contains the **raw** `{name}` slot (no substitution) — inspected via deterministic-emit-only scaffold inspection.

(No negative assertion that other tiers "cannot host local_refs" — that invariant is not enforced by the IR today, see §3.3.)

### 6.4 New: combined modifier + local_refs (tier-1 only)

One test on a tier-1 Call carrying both. Asserts:
- One diagnostic per failing Call (not two).
- The reason phrase is exactly `"a with modifier and local-ref cross-references"` — deterministic order, with-first.
- The remediation is exactly `"the with modifier / rewrite the local reference"`.

### 6.5 New: multiple failing spans — deterministic ordering

Two complementary tests:

- **End-to-end.** A skill containing two distinct Calls each requiring LLM fill. Asserts two `G::expand::llm-required-for-call` diagnostics emitted, each naming the correct IR node id, in ascending node-id order. This is the realistic-shape integration assertion.
- **Unit test on the conversion helper.** Construct a deliberately reversed `Vec<StubFillError>` (e.g. `[StubFillError { ir_node: NodeId(7), .. }, StubFillError { ir_node: NodeId(3), .. }]`), pass it through the `Err`-to-`DiagBag` conversion described in §3.6, and assert the resulting bag presents diagnostics in node-id-ascending order. This directly pins the explicit `errors.sort_by_key(|e| e.ir_node.0)` without depending on contrived scaffold traversal (which, since node IDs typically follow source order and scaffold visit follows that same order, is fragile to produce naturally).

### 6.6 New: no output file on failure

Existing CI helpers assert exit non-zero and stderr carries the diagnostic. Explicitly assert `compiled.md` is absent on disk after the failing compile (matches `expand.md` §5.6). The mechanism: `compile_directory_with_layout`'s `Ok(CompileOutcome::Diagnostics(_))` branch at `lib.rs:1693` never reaches `atomic_write`.

### 6.7 New: span boundaries, return-fold carrier, and naming sentence

Deterministic-emit-only inspection of the scaffold chunks (no fill, no merge). The §8.4 return-fold is **not** a Literal chunk — it is carried on `payload.post_merge_return_sentence` and applied by the merger (see §3.4 "Return-fold mechanism"). Tests assert the carrier and the post-merge result separately.

- **Naming sentence case.** Tier-1 top-level Call with a `with` modifier and `bound_name = Some("foo")` → scaffold contains `[Literal("N. "), Span(CallBodyShape), Literal(" Refer to this result as foo."), Literal("\n")]`. The naming sentence is a separate post-span Literal chunk.
- **Return-fold carrier case.** Tier-1 top-level Call with a `with` modifier as the final Step with Identifier-form return `return id` → scaffold contains `[Literal("N. "), Span(CallBodyShape, payload), Literal("\n")]` and `payload.post_merge_return_sentence == Some(templates::compute_return_sentence(...))`, whose value matches today's template output (e.g. `"Produce \`id\`."`). No `Literal(return-fold)` chunk between Span and `Literal("\n")`.
- **Combined.** Tier-1 final Step + Identifier-form return + producer naming → scaffold contains `[Literal("N. "), Span(CallBodyShape, payload), Literal(" Refer to this result as <n>."), Literal("\n")]` and `payload.post_merge_return_sentence == Some(...)`. Merger order: append return sentence to the merged body first, then the naming-sentence Literal chunk runs in its existing position.
- **§9.3 flow-local prose negative assertion.** A skill whose `Your result is …` paragraph is emitted as a separate post-loop Step is unaffected — no `CallBodyShape` span carries the flow-local return prose; it remains its own numbered Step.
- **Raw-slot assertion.** For a tier-1 non-trivial Call with a `{name}` slot in `c.resolved_body`, the span's `payload.resolved_body` contains the literal `{name}` token (not substituted).

### 6.8 IR-node-id stability

Each new test asserts the diagnostic names the failing Call by its stable IR node id (`n0`, `n1`, …) consistent with `expand.md` §3.1.

### 6.9 Excluded (deferred)

- Scoped-constraint Calls: no test in this spec. The follow-up spec (§7) will add coverage when `IrCall.scoped_constraints` is introduced.
- Real source-span coordinates: tests assert the synthetic zero-width file-level span shape only. When IrCall gains a source span, a follow-up test row asserts real `line`/`col` values.

## 7. Follow-up work

- **Scoped constraints.** Lower callee constraints into a new `IrCall.scoped_constraints` field; serialize via `emit_ir.rs` (today hardcoded to `[]`); extend the §3.3 triviality predicate to `|| !c.scoped_constraints.is_empty()`; extend `SpanPayload` and `StubFillError` accordingly; reuse the span-emission machinery this spec introduces. Tracked in `todo/expand-todos.md` per §3.8 item 4a.
- **Real source spans on `IrCall`.** Thread a `SourceSpan` (or byte-offset pair) through `IrCall` from parser → lower → IR so `G::expand::llm-required-for-call` can carry a real source span instead of the synthetic zero-width file-level span this spec uses. Tracked in `todo/expand-todos.md` per §3.8 item 4b.
- **LLM filler wiring.** The actual LLM call that fills `CallBodyShape` spans is tracked separately. Once wired, this spec's diagnostic stops firing on well-formed inputs and Phase 6b's existing `modifier-leaked` / `unresolved-local-ref` checks take over enforcement.

## 8. Open questions

- **Architecture doc owner placement decision** for the new "Step 2 fill-time diagnostics" subsection (§3.8 item 2). I've left placement TBD between a new §3.x and a new §4.x.
- **Exact wording** of the deterministic reason and remediation phrases (§3.7) can be tightened during implementation; the prebuilt phrase tables are deterministic by construction, but the reviewer or end-user agent corpus may prefer alternate wording.
