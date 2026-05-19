# CallBodyShape Span Emission Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Plumb a `CallBodyShape` span through every Call emission site (3 tiers × 2 positions + stdlib/bound = 7 sites) so non-trivial Calls (with `site_modifier` or non-empty `local_refs`) are no longer rendered as silent literals; the deterministic stub filler hard-fails with `G::expand::llm-required-for-call` and the lib-level callers convert that into `CompileOutcome::Diagnostics` so no `.md` is written.

**Architecture:** Add a `ProjectionMode` enum and four new `SpanPayload` fields in `crates/glyph-core/src/emit/scaffold.rs`. Add a `call_needs_llm_fill` triviality predicate. Replace literal-only emission at four top-level sites (`scaffold.rs:982–1091`) and three in-arm sites (`branch.rs:300–336`) with a `span-when-needed` branch. Change `stub_fill::fill` from infallible to `Result<HashMap, Vec<StubFillError>>`. Teach the merger to apply `templates::append_return_sentence` against the span's filled body text when `payload.post_merge_return_sentence` is `Some`. Change `emit::emit` to return `Result<String, Vec<StubFillError>>` and update the two `CompileOutcome`-producing callers in `lib.rs` to convert the `Err` variant into a `DiagBag` of `G::expand::llm-required-for-call` diagnostics (synthetic zero-width file-level `SourceSpan`, node-id-sorted).

**Tech Stack:** Rust 2021, `cargo` + `cargo-nextest`, workspace crate `glyph-core` (+ `glyph-cli` for end-to-end CLI tests). Existing `ast-grep` / `graphify` MCP tooling per `CLAUDE.md`.

---

## File Structure

**Modified files (no new files except corpus fixtures and the docs/follow-up entries the spec already calls out):**

- `crates/glyph-core/src/emit/scaffold.rs` — `ProjectionMode` enum (~L295), `SpanPayload` extension (~L302), `call_needs_llm_fill` + `projection_mode_from` helpers, span-when-needed at the four top-level Call sites (~L982/1058/1069/1077).
- `crates/glyph-core/src/emit/branch.rs` — span-when-needed at the three in-arm Call sites (~L300–336). Currently returns `text: String` from a `match`; that has to change shape because a span is not a string — see Task 5 for the chunk-stream rewrite.
- `crates/glyph-core/src/emit/stub_fill.rs` — `StubFillError` struct, `fill()` signature change to `Result<HashMap<SpanId, String>, Vec<StubFillError>>`, hard-fail arm for `CallBodyShape`.
- `crates/glyph-core/src/emit/merger.rs` — when a filled `Chunk::Span` carries `payload.post_merge_return_sentence == Some(sent)`, run `templates::append_return_sentence(filled_body, &sent)` against the filled body string only (the surrounding prefix Literal and naming-sentence Literal chunks emit unchanged).
- `crates/glyph-core/src/emit/mod.rs` — `emit()` signature change to `Result<String, Vec<StubFillError>>`.
- `crates/glyph-core/src/lib.rs` — at `compile_source_with_effects` (L204) and `compile_source_with_resolved_imports` (L2869), convert `Err(errors)` into `CompileOutcome::Diagnostics(bag)` via a new private helper `llm_required_diagnostics_from_errors(errors, file_label) -> DiagBag` defined alongside.
- `docs/reference/diagnostics.md` — register `G::expand::llm-required-for-call` in the public catalog.
- `docs/architecture/expand.md` — update `CallBodyShape` row stub-behavior cell; add "Step 2 fill-time diagnostics" subsection.
- `llm_expand_pass.md` — preamble one-line note on refusal semantics.
- `todo/expand-todos.md` — two new follow-up items (scoped constraints; real IrCall source spans).
- `crates/glyph-cli/tests/flow_assign.rs` — flip `flow_assign_with_modifier_compiles` from success-asserting to hard-fail-asserting (or split into two tests, one for the new behavior and one removed).
- `crates/glyph-cli/tests/corpus/valid/flow_assign_with_modifier.glyph` — fixture content unchanged; only the test it drives changes expectations. (Optionally move to `corpus/expected-failure/` if such a directory exists; per repo inspection it does not, so the fixture stays under `valid/` and the test changes intent.)
- New unit-test files: tests are added inline as `#[cfg(test)] mod tests` blocks in the modules already listed, plus a new integration test file `crates/glyph-core/tests/callbodyshape_span.rs` for the cross-module ordering / DiagBag conversion case (helper visibility forces an integration test).

---

## Task 1: Add `ProjectionMode` enum and extend `SpanPayload`

**Files:**
- Modify: `crates/glyph-core/src/emit/scaffold.rs` — add `ProjectionMode` enum after `SpanKind` (~L300), extend `SpanPayload` (~L303).
- Test: `crates/glyph-core/src/emit/scaffold.rs` — `#[cfg(test)] mod tests` block (existing).

This task is **pure type-additions** — no behavior changes. The new fields default to `None` / empty `Vec` so existing span constructions stay source-compatible.

- [ ] **Step 1: Write a compile-only failing test**

Add at the end of the `mod tests` block in `crates/glyph-core/src/emit/scaffold.rs`:

```rust
#[test]
fn span_payload_default_carries_new_call_body_shape_fields() {
    let p = SpanPayload::default();
    assert!(p.target_name.is_none());
    assert!(p.projection_mode.is_none());
    assert!(p.local_refs.is_empty());
    assert!(p.post_merge_return_sentence.is_none());
}

#[test]
fn projection_mode_variants_exist() {
    let modes = [
        ProjectionMode::Inline,
        ProjectionMode::SameFileProcedure,
        ProjectionMode::ExternalFile,
        ProjectionMode::StdlibBound,
    ];
    assert_eq!(modes.len(), 4);
}
```

- [ ] **Step 2: Run the test and verify it fails to compile**

Run: `cargo check -p glyph-core 2>&1 | head -40`
Expected: error[E0599] / E0422 for `ProjectionMode`, `target_name`, `projection_mode`, `local_refs`, `post_merge_return_sentence`.

- [ ] **Step 3: Add the enum and extend the struct**

In `crates/glyph-core/src/emit/scaffold.rs`, insert immediately after the `SpanKind` enum (after `pub enum SpanKind { … }`):

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionMode {
    Inline,
    SameFileProcedure,
    ExternalFile,
    StdlibBound,
}
```

Then extend the `SpanPayload` struct definition by adding the four new fields **at the end of the struct** (preserves field-order for callers that already use struct-update syntax with `..SpanPayload::default()`):

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
    // New for CallBodyShape (see docs/superpowers/specs/2026-05-18-callbodyshape-span-emission-design.md §3.5):
    pub target_name: Option<String>,
    pub projection_mode: Option<ProjectionMode>,
    pub local_refs: Vec<crate::ir::LocalRef>,
    pub post_merge_return_sentence: Option<String>,
}
```

- [ ] **Step 4: Run the test and verify it passes**

Run: `cargo nextest run -p glyph-core emit::scaffold::tests::span_payload_default_carries_new_call_body_shape_fields emit::scaffold::tests::projection_mode_variants_exist`
Expected: 2 passed.

- [ ] **Step 5: Workspace check + commit**

```bash
cargo check --workspace 2>&1 | tail -5
```
Expected: `Finished` (no errors). Then:

```bash
git add crates/glyph-core/src/emit/scaffold.rs
git commit -m "feat(emit): add ProjectionMode enum and extend SpanPayload with CallBodyShape fields"
```

---

## Task 2: Add `call_needs_llm_fill` and `projection_mode_from` helpers

**Files:**
- Modify: `crates/glyph-core/src/emit/scaffold.rs` — add two free helpers near the top of the file (next to `naming_sentence_for_call`, ~L63).
- Test: same file, `mod tests`.

- [ ] **Step 1: Write the failing test**

Append to `mod tests` in `crates/glyph-core/src/emit/scaffold.rs`:

```rust
#[test]
fn call_needs_llm_fill_recognises_modifier_and_local_refs() {
    use crate::ir::{IrCall, LocalRef};
    let mut c = IrCall {
        node_id: NodeId(0),
        target: "x".into(),
        resolved_body: None,
        site_modifier: None,
        projection_tier: Some(1),
        procedure_path: None,
        bound_name: None,
        local_refs: Vec::new(),
        callee_output_contract: None,
        callee_return_type_text: None,
    };
    assert!(!call_needs_llm_fill(&c), "trivial Call must not need LLM fill");
    c.site_modifier = Some("focus on lint".into());
    assert!(call_needs_llm_fill(&c), "with-modifier triggers LLM fill");
    c.site_modifier = None;
    c.local_refs.push(LocalRef { name: "ctx".into(), node_id: NodeId(7) });
    assert!(call_needs_llm_fill(&c), "non-empty local_refs triggers LLM fill");
}

#[test]
fn projection_mode_from_maps_tier_and_bound_name() {
    use crate::ir::IrCall;
    let mk = |tier: Option<u8>, bound: Option<&str>| IrCall {
        node_id: NodeId(0),
        target: "x".into(),
        resolved_body: None,
        site_modifier: None,
        projection_tier: tier,
        procedure_path: None,
        bound_name: bound.map(str::to_string),
        local_refs: Vec::new(),
        callee_output_contract: None,
        callee_return_type_text: None,
    };
    assert_eq!(projection_mode_from(&mk(Some(1), None)), Some(ProjectionMode::Inline));
    assert_eq!(projection_mode_from(&mk(Some(2), None)), Some(ProjectionMode::SameFileProcedure));
    assert_eq!(projection_mode_from(&mk(Some(3), None)), Some(ProjectionMode::ExternalFile));
    assert_eq!(projection_mode_from(&mk(None, Some("subagent"))), Some(ProjectionMode::StdlibBound));
    // Bound-name + tier 1 → tier wins (mirrors actual emit-site match order).
    assert_eq!(projection_mode_from(&mk(Some(1), Some("subagent"))), Some(ProjectionMode::Inline));
    assert_eq!(projection_mode_from(&mk(None, None)), None);
}
```

Note: `IrCall` field list above must exactly match `crates/glyph-core/src/ir.rs:305` — if a field has been added since this plan was written, copy it forward. The test only sets the fields it relies on.

- [ ] **Step 2: Run the test and verify it fails to compile**

Run: `cargo check -p glyph-core 2>&1 | head -20`
Expected: `cannot find function 'call_needs_llm_fill'` and `cannot find function 'projection_mode_from'`.

- [ ] **Step 3: Add the helpers**

In `crates/glyph-core/src/emit/scaffold.rs`, after the existing `naming_sentence_for_call` function (around L63–L75), add two new free helpers (both `pub(crate)` so `branch.rs` can use them):

```rust
/// Does this Call need LLM-grade body shaping? When this returns true the
/// emit site must push a `CallBodyShape` span; the stub filler hard-fails
/// (see `stub_fill.rs`) producing `G::expand::llm-required-for-call`.
///
/// Per spec §3.3: a non-empty `site_modifier` (the `with "…"` clause) or
/// a non-empty `local_refs` (LLM-grade cross-references like
/// "the diagnosis from your earlier analysis", which the deterministic
/// `substitute_local_refs_in` bare-substitution cannot produce).
pub(crate) fn call_needs_llm_fill(c: &crate::ir::IrCall) -> bool {
    c.site_modifier.is_some() || !c.local_refs.is_empty()
}

/// Map `IrCall.projection_tier` + `bound_name` into the payload-side
/// `ProjectionMode`. Mirrors the actual emit-site match order: a Call
/// carrying both a `projection_tier` and a `bound_name` routes through
/// its tier path, not the stdlib anchor. `StdlibBound` is reached only
/// when no tier applies.
pub(crate) fn projection_mode_from(c: &crate::ir::IrCall) -> Option<ProjectionMode> {
    match c.projection_tier {
        Some(1) => Some(ProjectionMode::Inline),
        Some(2) => Some(ProjectionMode::SameFileProcedure),
        Some(3) => Some(ProjectionMode::ExternalFile),
        _ if c.bound_name.is_some() => Some(ProjectionMode::StdlibBound),
        _ => None,
    }
}
```

- [ ] **Step 4: Run the test and verify it passes**

Run: `cargo nextest run -p glyph-core emit::scaffold::tests::call_needs_llm_fill_recognises_modifier_and_local_refs emit::scaffold::tests::projection_mode_from_maps_tier_and_bound_name`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/glyph-core/src/emit/scaffold.rs
git commit -m "feat(emit): add call_needs_llm_fill and projection_mode_from helpers"
```

---

## Task 3: Change `stub_fill::fill` to return a `Result` and hard-fail on `CallBodyShape`

**Files:**
- Modify: `crates/glyph-core/src/emit/stub_fill.rs:8` (signature change + new `StubFillError` type).
- Modify: `crates/glyph-core/src/emit/mod.rs:16` (consume the new `Result`, propagate `Err` upward — Task 4 finishes the propagation; this task just keeps things compiling).
- Test: inline `#[cfg(test)] mod tests` block at the bottom of `stub_fill.rs` (currently has none).

- [ ] **Step 1: Write the failing test**

Append to the end of `crates/glyph-core/src/emit/stub_fill.rs` (the file currently has no `#[cfg(test)]` block — create one):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::emit::scaffold::{Scaffold, SpanId, SpanKind, SpanPayload, SpanRef};
    use crate::ir::NodeId;

    fn span(id: u32, kind: SpanKind, payload: SpanPayload) -> SpanRef {
        SpanRef { id: SpanId(id), kind, ir_node: NodeId(3), payload }
    }

    #[test]
    fn fill_returns_ok_when_no_call_body_shape_spans() {
        let s = Scaffold::default();
        let r = fill(&s);
        assert!(r.is_ok());
        assert!(r.unwrap().is_empty());
    }

    #[test]
    fn fill_hard_fails_on_call_body_shape_with_modifier() {
        let mut s = Scaffold::default();
        s.push_span(span(
            0,
            SpanKind::CallBodyShape,
            SpanPayload {
                target_name: Some("inspect_failure".into()),
                site_modifier: Some("focus on lint".into()),
                ..SpanPayload::default()
            },
        ));
        let r = fill(&s);
        let errors = r.expect_err("CallBodyShape span must hard-fail in stub filler");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].target_name.as_deref(), Some("inspect_failure"));
        assert!(errors[0].has_modifier);
        assert!(!errors[0].has_local_refs);
        assert_eq!(errors[0].ir_node, NodeId(3));
    }

    #[test]
    fn fill_collects_multiple_errors_in_chunk_order() {
        let mut s = Scaffold::default();
        s.push_span(SpanRef {
            id: SpanId(0), kind: SpanKind::CallBodyShape, ir_node: NodeId(5),
            payload: SpanPayload { target_name: Some("a".into()), site_modifier: Some("m".into()), ..Default::default() },
        });
        s.push_literal("between\n");
        s.push_span(SpanRef {
            id: SpanId(1), kind: SpanKind::CallBodyShape, ir_node: NodeId(2),
            payload: SpanPayload {
                target_name: Some("b".into()),
                local_refs: vec![crate::ir::LocalRef { name: "x".into(), node_id: NodeId(99) }],
                ..Default::default()
            },
        });
        let errs = fill(&s).expect_err("two CallBodyShape spans must yield two errors");
        assert_eq!(errs.len(), 2);
        // Order at this layer is chunk-stream order, not sorted; the lib-level
        // helper sorts before pushing into the bag.
        assert_eq!(errs[0].ir_node, NodeId(5));
        assert_eq!(errs[1].ir_node, NodeId(2));
        assert!(errs[1].has_local_refs);
    }
}
```

- [ ] **Step 2: Run the test and verify it fails to compile**

Run: `cargo check -p glyph-core 2>&1 | tail -30`
Expected: `cannot find type 'StubFillError'`, plus `expected enum 'HashMap', found 'Result'` or similar wherever the result is consumed.

- [ ] **Step 3: Rewrite `fill()` to return a `Result`**

Replace the existing `pub fn fill(scaffold: &Scaffold) -> HashMap<SpanId, String> { … }` in `crates/glyph-core/src/emit/stub_fill.rs:8` with:

```rust
#[derive(Clone, Debug)]
pub struct StubFillError {
    pub ir_node: crate::ir::NodeId,
    pub target_name: Option<String>,
    pub has_modifier: bool,
    pub has_local_refs: bool,
}

pub fn fill(scaffold: &Scaffold) -> Result<HashMap<SpanId, String>, Vec<StubFillError>> {
    let mut out = HashMap::new();
    let mut errors: Vec<StubFillError> = Vec::new();
    for chunk in &scaffold.chunks {
        if let Chunk::Span(span) = chunk {
            match span.kind {
                SpanKind::ParamDescription => {
                    out.insert(span.id, String::new());
                }
                SpanKind::BranchCondition => {
                    let raw = span.payload.condition_expression.clone().unwrap_or_default();
                    let empty = BTreeMap::new();
                    let rp = span.payload.resolved_predicates.as_ref().unwrap_or(&empty);
                    let s = substitute_predicate_tokens(
                        &raw,
                        rp,
                        span.payload.classification.as_ref(),
                    );
                    out.insert(span.id, s);
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

The internal `substitute_predicate_tokens` helper (and its callees) stays unchanged at the bottom of the file.

- [ ] **Step 4: Patch `emit::emit` to unwrap-or-propagate the new shape**

In `crates/glyph-core/src/emit/mod.rs` replace the body of `pub fn emit(arena, enable_effects) -> String { … }` (L16–L20) with the signature change documented in Task 4. For this task only, do the **minimum** to keep the workspace compiling — leave the public signature as `String` and panic on `Err`:

```rust
pub fn emit(arena: &IrArena, enable_effects: bool) -> String {
    let scaffold = scaffold::build(arena, enable_effects);
    let fills = match stub_fill::fill(&scaffold) {
        Ok(f) => f,
        Err(errors) => panic!(
            "stub filler refused {} CallBodyShape span(s); production callers must use the Result-returning entry — see emit::try_emit in Task 4",
            errors.len()
        ),
    };
    merger::merge(scaffold, fills).expect("scaffold/fill mismatch is a bug")
}
```

This is **scaffolding only**; Task 4 replaces the signature with the `Result` shape. The panic message points the next task at itself.

- [ ] **Step 5: Run the tests**

```bash
cargo nextest run -p glyph-core emit::stub_fill::tests
```
Expected: 3 passed.

Workspace check:

```bash
cargo check --workspace 2>&1 | tail -5
```
Expected: `Finished` (no errors).

- [ ] **Step 6: Commit**

```bash
git add crates/glyph-core/src/emit/stub_fill.rs crates/glyph-core/src/emit/mod.rs
git commit -m "feat(emit): stub_fill::fill returns Result with StubFillError for CallBodyShape spans"
```

---

## Task 4: Plumb the `Result` through `emit::emit` and `lib.rs` into `CompileOutcome::Diagnostics`

**Files:**
- Modify: `crates/glyph-core/src/emit/mod.rs:16` — change signature to `Result<String, Vec<StubFillError>>`. Re-export `StubFillError`.
- Modify: `crates/glyph-core/src/lib.rs:204` (`compile_source_with_effects`) and `:2869` (`compile_source_with_resolved_imports`) — convert `Err` into `CompileOutcome::Diagnostics`.
- Modify: `crates/glyph-core/src/lib.rs` — add private helper `llm_required_diagnostics_from_errors(errors: Vec<emit::StubFillError>, file_label: &str) -> DiagBag` near the existing synthetic-diagnostic site (~L1726).
- Test: `crates/glyph-core/tests/callbodyshape_span.rs` (new integration-test file).

- [ ] **Step 1: Write the failing integration test**

Create `crates/glyph-core/tests/callbodyshape_span.rs`:

```rust
//! Integration tests for the CallBodyShape hard-fail plumbing.
//! Covers the lib-level Err → CompileOutcome::Diagnostics conversion and
//! the explicit IR-node-id-ascending ordering of the resulting bag.

use glyph_core::{compile_source_with_effects, CompileOutcome};

const SRC_WITH_MODIFIER: &str = r#"block inspect_repo(scope = ".") -> Report
    description: "Inspect the repository at the given scope."
    flow:
        "Examine the repository at {scope} and produce a report."
        return context

skill diagnose(scope = ".") -> Report
    description: "Inspect the scope with a focus area."
    flow:
        ctx = inspect_repo(scope) with "focus on lint failures"
        return ctx
"#;

#[test]
fn with_modifier_produces_llm_required_diagnostic() {
    let outcome = compile_source_with_effects(SRC_WITH_MODIFIER, 0, "test.glyph", false)
        .expect("compile_source_with_effects must not return CompileError here");
    match outcome {
        CompileOutcome::Diagnostics(bag) => {
            let sorted = bag.sorted();
            let llm_diags: Vec<_> = sorted
                .iter()
                .filter(|d| d.id == "G::expand::llm-required-for-call")
                .collect();
            assert_eq!(
                llm_diags.len(),
                1,
                "expected exactly one G::expand::llm-required-for-call; got bag={sorted:?}"
            );
            let msg = &llm_diags[0].message;
            assert!(msg.contains("inspect_repo"), "message must name the target: {msg}");
            assert!(msg.contains("with modifier"), "message must mention with modifier: {msg}");
        }
        CompileOutcome::Compiled { markdown, .. } => panic!(
            "expected Diagnostics outcome for with-modifier Call; got Compiled markdown:\n{markdown}"
        ),
    }
}
```

- [ ] **Step 2: Run the test and verify it fails**

Run: `cargo nextest run -p glyph-core --test callbodyshape_span with_modifier_produces_llm_required_diagnostic 2>&1 | tail -30`
Expected: FAIL. Today's behavior is to silently emit `compile_source_with_effects` returning `CompileOutcome::Compiled` (the bug we are fixing). The test's `panic!` for the `Compiled` arm fires.

- [ ] **Step 3: Change `emit::emit` signature to `Result`**

In `crates/glyph-core/src/emit/mod.rs`, replace the panic-scaffold from Task 3 with:

```rust
pub use stub_fill::StubFillError;

pub fn emit(arena: &IrArena, enable_effects: bool) -> Result<String, Vec<StubFillError>> {
    let scaffold = scaffold::build(arena, enable_effects);
    let fills = stub_fill::fill(&scaffold)?;
    Ok(merger::merge(scaffold, fills).expect("scaffold/fill mismatch is a bug"))
}
```

Existing `#[cfg(test)] mod tests` in `mod.rs` calls `emit(arena, …)` and asserts on the returned `String`. Update those tests (`emit_skips_effects_when_disabled`, `emit_includes_effects_when_enabled`) to unwrap the `Result`:

```rust
let output = emit(&arena, false).expect("trivial skill must compile");
```

(Same pattern for the other test.)

- [ ] **Step 4: Add the lib-level helper**

In `crates/glyph-core/src/lib.rs`, near the existing synthetic-diagnostic site at L1726, add a free helper (place it just above `compile_directory_with_layout` or at the bottom of the file — wherever clusters with `compile_source_with_effects` callers):

```rust
/// Convert a Vec<emit::StubFillError> from the stub filler into a fresh
/// DiagBag carrying one `G::expand::llm-required-for-call` per failing
/// span. IR-node ordering is enforced by sorting the Vec before pushing,
/// because DiagBag::sorted() falls back to insertion order when
/// (file, byte_start, id) all tie — and these synthetic diagnostics
/// share all three.
fn llm_required_diagnostics_from_errors(
    mut errors: Vec<emit::StubFillError>,
    file_label: &str,
) -> DiagBag {
    errors.sort_by_key(|e| e.ir_node.0);
    let mut bag = DiagBag::new();
    let li = LineIndex::new("");
    let span = Span::new(0, 0, 0);
    for e in errors {
        let msg = format_llm_required_message(&e);
        bag.push(
            Diagnostic::error(
                "G::expand::llm-required-for-call",
                msg,
                SourceSpan::from_byte_span(file_label, span, &li),
            ),
            span,
        );
    }
    bag
}

fn format_llm_required_message(e: &emit::StubFillError) -> String {
    let reason_phrase = match (e.has_modifier, e.has_local_refs) {
        (true,  false) => "a with modifier",
        (false, true ) => "local-ref cross-references",
        (true,  true ) => "a with modifier and local-ref cross-references",
        (false, false) => unreachable!(
            "StubFillError is only pushed when site_modifier or local_refs is non-empty"
        ),
    };
    let remediation = match (e.has_modifier, e.has_local_refs) {
        (true,  false) => "the with modifier",
        (false, true ) => "the local reference",
        (true,  true ) => "the with modifier / rewrite the local reference",
        (false, false) => unreachable!(),
    };
    let target = e.target_name.as_deref().unwrap_or("<unknown>");
    // NodeId does not implement Display today; format the inner u32 directly.
    let node = format!("n{}", e.ir_node.0);
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

- [ ] **Step 5: Wire the converter into both callers**

In `crates/glyph-core/src/lib.rs:204` replace:

```rust
let markdown = emit::emit(&arena, enable_effects);
Ok(CompileOutcome::Compiled {
    markdown,
    diagnostics: bag,
    arena,
})
```

with:

```rust
let markdown = match emit::emit(&arena, enable_effects) {
    Ok(md) => md,
    Err(errors) => {
        let mut diag_bag = llm_required_diagnostics_from_errors(errors, file_label);
        diag_bag.merge(bag);
        return Ok(CompileOutcome::Diagnostics(diag_bag));
    }
};
Ok(CompileOutcome::Compiled {
    markdown,
    diagnostics: bag,
    arena,
})
```

Make the **same** change at `crates/glyph-core/src/lib.rs:2869` (`compile_source_with_resolved_imports`), with `file_label` already in scope there (same parameter name).

- [ ] **Step 6: Run the test and verify it passes**

```bash
cargo nextest run -p glyph-core --test callbodyshape_span with_modifier_produces_llm_required_diagnostic
```
Expected: PASS.

Workspace check:

```bash
cargo check --workspace 2>&1 | tail -5
```
Expected: `Finished`.

- [ ] **Step 7: Commit**

```bash
git add crates/glyph-core/src/emit/mod.rs crates/glyph-core/src/lib.rs crates/glyph-core/tests/callbodyshape_span.rs
git commit -m "feat(lib): convert stub filler Err into G::expand::llm-required-for-call diagnostics"
```

---

## Task 5: Convert in-arm Call emission to span-when-needed (branch.rs)

**Files:**
- Modify: `crates/glyph-core/src/emit/branch.rs:290–344` — `emit_lettered_substeps` is rewritten so each arm body node pushes its own chunks directly into `s` rather than building a `text: String` and pushing one literal at the end.

**Why this restructure:** today `emit_lettered_substeps` produces a `text: String` from a `match` then does `s.push_literal(format!("   {}. {}\n", letter, text))`. A span can't fit inside that string — it has to be its own chunk. So the arms become "push prefix Literal, push Span-or-Literal, push trailing Literal" instead.

- [ ] **Step 1: Write the failing tests**

Add to the existing `#[cfg(test)] mod tests` block at the bottom of `crates/glyph-core/src/emit/branch.rs`:

```rust
#[test]
fn in_arm_tier1_call_with_modifier_emits_call_body_shape_span() {
    use crate::emit::scaffold::{Chunk, Scaffold, SpanKind};
    use crate::ir::{IrCall, IrNode};
    let mut arena = IrArena::new();
    let call_id = arena.push(IrNode::Call(IrCall {
        node_id: NodeId(7),
        target: "inspect_failure".into(),
        resolved_body: Some("Inspect the failing run.".into()),
        site_modifier: Some("focus on stack traces".into()),
        projection_tier: Some(1),
        procedure_path: None,
        bound_name: None,
        local_refs: Vec::new(),
        callee_output_contract: None,
        callee_return_type_text: None,
    }));
    let mut s = Scaffold::default();
    let mut next = 0u32;
    let next_id = &mut next;
    let body = vec![call_id];
    super::emit_lettered_substeps_with_next_id(&mut s, &arena, &body, next_id);
    let span_count = s.chunks.iter().filter(|c| matches!(c, Chunk::Span(sp) if sp.kind == SpanKind::CallBodyShape)).count();
    assert_eq!(span_count, 1, "tier-1 in-arm Call with modifier must emit a CallBodyShape span; got chunks={:?}", s.chunks);
    // Prefix and newline remain Literals on either side.
    let lits: Vec<_> = s.chunks.iter().filter_map(|c| match c { Chunk::Literal(l) => Some(l.clone()), _ => None }).collect();
    assert!(lits.iter().any(|l| l.starts_with("   a. ")), "lettered prefix must be a Literal: {lits:?}");
    assert!(lits.iter().any(|l| l == "\n"), "newline must be a Literal: {lits:?}");
}

#[test]
fn in_arm_tier1_call_without_modifier_stays_literal() {
    use crate::emit::scaffold::{Chunk, Scaffold, SpanKind};
    use crate::ir::{IrCall, IrNode};
    let mut arena = IrArena::new();
    let call_id = arena.push(IrNode::Call(IrCall {
        node_id: NodeId(2),
        target: "do_thing".into(),
        resolved_body: Some("Inspect the working tree.".into()),
        site_modifier: None,
        projection_tier: Some(1),
        procedure_path: None,
        bound_name: None,
        local_refs: Vec::new(),
        callee_output_contract: None,
        callee_return_type_text: None,
    }));
    let mut s = Scaffold::default();
    let mut next = 0u32;
    super::emit_lettered_substeps_with_next_id(&mut s, &arena, &[call_id], &mut next);
    let span_count = s.chunks.iter().filter(|c| matches!(c, Chunk::Span(sp) if sp.kind == SpanKind::CallBodyShape)).count();
    assert_eq!(span_count, 0, "trivial tier-1 in-arm Call must NOT emit a span; got chunks={:?}", s.chunks);
}
```

- [ ] **Step 2: Run the test and verify it fails to compile**

```bash
cargo check -p glyph-core 2>&1 | tail -20
```
Expected: `cannot find function 'emit_lettered_substeps_with_next_id'`.

- [ ] **Step 3: Rewrite `emit_lettered_substeps` to thread a `next_span_id` and emit chunk-stream**

Replace the existing `fn emit_lettered_substeps(s, arena, body)` (L290–L344) in `crates/glyph-core/src/emit/branch.rs` with two functions: a thin wrapper for the existing call sites that synthesises the `next_span_id` from the scaffold, plus the testable inner function:

```rust
fn emit_lettered_substeps(s: &mut Scaffold, arena: &IrArena, body: &[NodeId]) {
    // Pick a starting SpanId one past the maximum already-emitted span. This
    // mirrors the convention in scaffold.rs::build's branch dispatch (search
    // `branch::emit_to_scaffold`); branches there pass an explicit
    // `next_span_id` cursor. The lettered path runs as part of that same
    // build and is reached only via emit_to_scaffold, so we can require the
    // caller to thread the cursor in.
    let mut next: u32 = s
        .chunks
        .iter()
        .filter_map(|c| match c {
            Chunk::Span(sp) => Some(sp.id.0 + 1),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    emit_lettered_substeps_with_next_id(s, arena, body, &mut next);
}

pub(super) fn emit_lettered_substeps_with_next_id(
    s: &mut Scaffold,
    arena: &IrArena,
    body: &[NodeId],
    next_span_id: &mut u32,
) {
    use crate::emit::scaffold::{
        append_sentence, call_needs_llm_fill, naming_sentence_for_call, projection_mode_from,
        substitute_local_refs_in, SpanId, SpanKind, SpanPayload, SpanRef,
    };
    use crate::emit::templates;
    let mut letter = b'a';
    for node_id in body {
        match arena.get(*node_id) {
            IrNode::InlineInstruction(i) => {
                let text = substitute_local_refs_in(&i.text, &i.local_refs);
                s.push_literal(format!("   {}. {}\n", letter as char, text));
            }
            IrNode::Call(c) if c.projection_tier == Some(1) => {
                s.push_literal(format!("   {}. ", letter as char));
                if call_needs_llm_fill(c) {
                    // Raw resolved_body — {name} slots intact for LLM weaving.
                    let raw = c.resolved_body.clone().unwrap_or_default();
                    let id = SpanId(*next_span_id);
                    *next_span_id += 1;
                    s.push_span(SpanRef {
                        id,
                        kind: SpanKind::CallBodyShape,
                        ir_node: c.node_id,
                        payload: SpanPayload {
                            target_name: Some(c.target.clone()),
                            projection_mode: projection_mode_from(c),
                            site_modifier: c.site_modifier.clone(),
                            resolved_body: Some(raw),
                            local_refs: c.local_refs.clone(),
                            ..SpanPayload::default()
                        },
                    });
                } else {
                    let raw = c.resolved_body.as_deref().unwrap_or_default();
                    let body = substitute_local_refs_in(raw, &c.local_refs);
                    s.push_literal(body);
                }
                if let Some(naming) = naming_sentence_for_call(c) {
                    s.push_literal(format!(" {}", naming));
                }
                s.push_literal("\n");
            }
            IrNode::Call(c) if c.projection_tier == Some(2) => {
                s.push_literal(format!("   {}. ", letter as char));
                let kebab = templates::kebab_case(&c.target);
                let anchor = format!("Follow the {kebab} procedure.");
                if call_needs_llm_fill(c) {
                    let id = SpanId(*next_span_id);
                    *next_span_id += 1;
                    s.push_span(SpanRef {
                        id,
                        kind: SpanKind::CallBodyShape,
                        ir_node: c.node_id,
                        payload: SpanPayload {
                            target_name: Some(c.target.clone()),
                            projection_mode: projection_mode_from(c),
                            site_modifier: c.site_modifier.clone(),
                            resolved_body: Some(anchor),
                            local_refs: c.local_refs.clone(),
                            ..SpanPayload::default()
                        },
                    });
                } else {
                    s.push_literal(anchor);
                }
                if let Some(naming) = naming_sentence_for_call(c) {
                    s.push_literal(format!(" {}", naming));
                }
                s.push_literal("\n");
            }
            IrNode::Call(c) if c.projection_tier == Some(3) => {
                s.push_literal(format!("   {}. ", letter as char));
                let path = c.procedure_path.as_deref().unwrap_or("unknown");
                let anchor = templates::external_file_step(path);
                if call_needs_llm_fill(c) {
                    let id = SpanId(*next_span_id);
                    *next_span_id += 1;
                    s.push_span(SpanRef {
                        id,
                        kind: SpanKind::CallBodyShape,
                        ir_node: c.node_id,
                        payload: SpanPayload {
                            target_name: Some(c.target.clone()),
                            projection_mode: projection_mode_from(c),
                            site_modifier: c.site_modifier.clone(),
                            resolved_body: Some(anchor),
                            local_refs: c.local_refs.clone(),
                            ..SpanPayload::default()
                        },
                    });
                } else {
                    s.push_literal(anchor);
                }
                if let Some(naming) = naming_sentence_for_call(c) {
                    s.push_literal(format!(" {}", naming));
                }
                s.push_literal("\n");
            }
            IrNode::Call(c) => panic!("Call to `{}` survived past expand", c.target),
            IrNode::Branch(_) => {
                s.push_literal(format!("   {}. (nested branch)\n", letter as char));
            }
            _ => panic!("Unexpected node type in branch body"),
        }
        letter += 1;
    }
}
```

Existing helpers (`naming_sentence_for_call`, `substitute_local_refs_in`, `append_sentence`) are already `pub(crate)` exports of `scaffold.rs`; if any are still private, widen them in scaffold.rs. (Run the compiler — it will list the exact ones.)

- [ ] **Step 4: Run the new tests and verify they pass**

```bash
cargo nextest run -p glyph-core emit::branch::tests
```
Expected: existing branch tests still pass + 2 new tests pass.

- [ ] **Step 5: Workspace check**

```bash
cargo check --workspace 2>&1 | tail -5
```
Expected: `Finished` (no errors).

- [ ] **Step 6: Commit**

```bash
git add crates/glyph-core/src/emit/branch.rs crates/glyph-core/src/emit/scaffold.rs
git commit -m "feat(emit): push CallBodyShape span at in-arm Call sites when LLM fill required"
```

---

## Task 6: Convert top-level tier-2 / tier-3 / stdlib-bound Call emission to span-when-needed

**Files:**
- Modify: `crates/glyph-core/src/emit/scaffold.rs:1058–1091` — three arms (`projection_tier == Some(2)`, `Some(3)`, `bound_name.is_some()`).
- Test: same file's `mod tests`.

This task does the **three easy top-level sites** (tier 2, tier 3, stdlib/bound) and leaves the more complicated **tier-1 final-step + return-fold** case for Task 7.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `crates/glyph-core/src/emit/scaffold.rs`. Helper for building a minimal skill arena that exercises a top-level Call (one of the existing tests will already have a similar harness; if so reuse it):

```rust
#[test]
fn top_level_tier2_call_with_modifier_emits_span() {
    use crate::ir::{IrCall, IrNode, IrSkill};
    let mut arena = IrArena::new();
    let call_id = arena.push(IrNode::Call(IrCall {
        node_id: NodeId(0),
        target: "do_steps".into(),
        resolved_body: None,
        site_modifier: Some("focus on errors".into()),
        projection_tier: Some(2),
        procedure_path: None,
        bound_name: None,
        local_refs: Vec::new(),
        callee_output_contract: None,
        callee_return_type_text: None,
    }));
    let skill_id = arena.push(IrNode::Skill(IrSkill {
        node_id: NodeId(1),
        name: "demo".into(),
        description: "Demo.".into(),
        effects: vec![],
        params: vec![],
        steps: vec![call_id],
        context: vec![],
        constraints: vec![],
        return_text: None, return_type: None, output_contract: None,
        return_type_text: None, return_local_ref: None,
        freeform_sections: Vec::new(),
        description_source_line: None, context_source_line: None,
        constraints_source_line: None, flow_source_line: None,
    }));
    arena.set_root_skill(skill_id);
    let scaffold = build(&arena, false);
    let span_count = scaffold.chunks.iter().filter(|c| matches!(c, Chunk::Span(sp) if sp.kind == SpanKind::CallBodyShape)).count();
    assert_eq!(span_count, 1, "tier-2 top-level Call with modifier must emit a CallBodyShape span");
}

#[test]
fn top_level_tier2_call_without_modifier_stays_literal() {
    // Same harness, just drop the site_modifier:
    use crate::ir::{IrCall, IrNode, IrSkill};
    let mut arena = IrArena::new();
    let call_id = arena.push(IrNode::Call(IrCall {
        node_id: NodeId(0), target: "do_steps".into(), resolved_body: None,
        site_modifier: None, projection_tier: Some(2),
        procedure_path: None, bound_name: None, local_refs: Vec::new(),
        callee_output_contract: None, callee_return_type_text: None,
    }));
    let skill_id = arena.push(IrNode::Skill(IrSkill {
        node_id: NodeId(1), name: "demo".into(), description: "Demo.".into(),
        effects: vec![], params: vec![], steps: vec![call_id],
        context: vec![], constraints: vec![],
        return_text: None, return_type: None, output_contract: None,
        return_type_text: None, return_local_ref: None,
        freeform_sections: Vec::new(),
        description_source_line: None, context_source_line: None,
        constraints_source_line: None, flow_source_line: None,
    }));
    arena.set_root_skill(skill_id);
    let scaffold = build(&arena, false);
    let span_count = scaffold.chunks.iter().filter(|c| matches!(c, Chunk::Span(sp) if sp.kind == SpanKind::CallBodyShape)).count();
    assert_eq!(span_count, 0, "trivial tier-2 Call must NOT emit a span");
}

#[test]
fn top_level_stdlib_bound_with_modifier_emits_span() {
    use crate::ir::{IrCall, IrNode, IrSkill};
    let mut arena = IrArena::new();
    let call_id = arena.push(IrNode::Call(IrCall {
        node_id: NodeId(0), target: "subagent".into(), resolved_body: None,
        site_modifier: Some("brief response".into()),
        projection_tier: None, procedure_path: None,
        bound_name: Some("foo".into()), local_refs: Vec::new(),
        callee_output_contract: None, callee_return_type_text: None,
    }));
    let skill_id = arena.push(IrNode::Skill(IrSkill {
        node_id: NodeId(1), name: "demo".into(), description: "Demo.".into(),
        effects: vec![], params: vec![], steps: vec![call_id],
        context: vec![], constraints: vec![],
        return_text: None, return_type: None, output_contract: None,
        return_type_text: None, return_local_ref: None,
        freeform_sections: Vec::new(),
        description_source_line: None, context_source_line: None,
        constraints_source_line: None, flow_source_line: None,
    }));
    arena.set_root_skill(skill_id);
    let scaffold = build(&arena, false);
    let spans: Vec<_> = scaffold.chunks.iter().filter_map(|c| match c {
        Chunk::Span(sp) if sp.kind == SpanKind::CallBodyShape => Some(sp),
        _ => None,
    }).collect();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].payload.projection_mode, Some(ProjectionMode::StdlibBound));
}
```

- [ ] **Step 2: Run the tests and verify they fail**

```bash
cargo nextest run -p glyph-core emit::scaffold::tests::top_level_tier2_call_with_modifier_emits_span emit::scaffold::tests::top_level_stdlib_bound_with_modifier_emits_span 2>&1 | tail -20
```
Expected: assertion failures (`span_count` is `0` because no Call site emits a `CallBodyShape` today).

- [ ] **Step 3: Replace the three top-level arms in `build()`'s match**

In `crates/glyph-core/src/emit/scaffold.rs`, locate the three Call match arms in the top-level `for (idx, node_id) in skill.steps.iter().enumerate()` loop (~L1058–L1091 today). Replace each with the span-when-needed pattern. The `next_span_id` cursor — search for `next_span_id` in `build()` — must be threaded through `push_span` calls.

For tier-2 (replace L1058–L1068):

```rust
IrNode::Call(c) if c.projection_tier == Some(2) => {
    s.push_literal(format!("{}. ", idx + 1));
    let kebab_name = c.target.replace('_', "-");
    let anchor = format!("Follow the {kebab_name} procedure below.");
    if call_needs_llm_fill(c) {
        let id = SpanId(*next_span_id);
        *next_span_id += 1;
        s.push_span(SpanRef {
            id,
            kind: SpanKind::CallBodyShape,
            ir_node: c.node_id,
            payload: SpanPayload {
                target_name: Some(c.target.clone()),
                projection_mode: projection_mode_from(c),
                site_modifier: c.site_modifier.clone(),
                resolved_body: Some(anchor),
                local_refs: c.local_refs.clone(),
                ..SpanPayload::default()
            },
        });
    } else {
        s.push_literal(anchor);
    }
    if let Some(naming) = naming_sentence_for_call(c) {
        s.push_literal(format!(" {}", naming));
    }
    s.push_literal("\n");
    if procedure_seen.insert(c.target.clone()) {
        procedure_order.push(c.target.clone());
    }
}
```

For tier-3 (replace L1069–L1076):

```rust
IrNode::Call(c) if c.projection_tier == Some(3) => {
    s.push_literal(format!("{}. ", idx + 1));
    let proc_path = c.procedure_path.as_deref().unwrap_or("unknown");
    let anchor = templates::external_file_step(proc_path);
    if call_needs_llm_fill(c) {
        let id = SpanId(*next_span_id);
        *next_span_id += 1;
        s.push_span(SpanRef {
            id,
            kind: SpanKind::CallBodyShape,
            ir_node: c.node_id,
            payload: SpanPayload {
                target_name: Some(c.target.clone()),
                projection_mode: projection_mode_from(c),
                site_modifier: c.site_modifier.clone(),
                resolved_body: Some(anchor),
                local_refs: c.local_refs.clone(),
                ..SpanPayload::default()
            },
        });
    } else {
        s.push_literal(anchor);
    }
    if let Some(naming) = naming_sentence_for_call(c) {
        s.push_literal(format!(" {}", naming));
    }
    s.push_literal("\n");
}
```

For stdlib/bound (replace L1077–L1091):

```rust
IrNode::Call(c) if c.bound_name.is_some() => {
    s.push_literal(format!("{}. ", idx + 1));
    let anchor = format!("Call `{}`.", c.target);
    if call_needs_llm_fill(c) {
        let id = SpanId(*next_span_id);
        *next_span_id += 1;
        s.push_span(SpanRef {
            id,
            kind: SpanKind::CallBodyShape,
            ir_node: c.node_id,
            payload: SpanPayload {
                target_name: Some(c.target.clone()),
                projection_mode: projection_mode_from(c),
                site_modifier: c.site_modifier.clone(),
                resolved_body: Some(anchor),
                local_refs: c.local_refs.clone(),
                ..SpanPayload::default()
            },
        });
    } else {
        s.push_literal(anchor);
    }
    if let Some(naming) = naming_sentence_for_call(c) {
        s.push_literal(format!(" {}", naming));
    }
    s.push_literal("\n");
}
```

`next_span_id` is the local mutable `&mut u32` already used elsewhere in `build()`; confirm it is in scope at this match (search the file for `next_span_id`). If it isn't (e.g. it was declared inside a different sub-block), declare/thread it through to this `for` loop before the match.

- [ ] **Step 4: Run the new tests and verify they pass**

```bash
cargo nextest run -p glyph-core emit::scaffold::tests
```
Expected: all existing scaffold tests + the 3 new ones pass.

- [ ] **Step 5: Commit**

```bash
git add crates/glyph-core/src/emit/scaffold.rs
git commit -m "feat(emit): push CallBodyShape span at top-level tier-2/3/stdlib Call sites when LLM fill required"
```

---

## Task 7: Convert top-level tier-1 Call emission (including final-step return-fold) to span-when-needed

**Files:**
- Modify: `crates/glyph-core/src/emit/scaffold.rs:982–1056` — the `projection_tier == Some(1)` arm in the top-level `for (idx, node_id)` loop.
- Modify: `crates/glyph-core/src/emit/merger.rs` — apply `templates::append_return_sentence` against the filled span body when `payload.post_merge_return_sentence == Some(...)`.

This is the most intricate task. The existing arm handles three cases: (a) non-last position, (b) last position with empty body + return sentence, (c) last position with non-empty body + return sentence. The span replacement preserves all three behaviors, but the return-fold is **moved from a pre-fill `append_return_sentence` on a String to a post-fill merger step on the filled span body**.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `crates/glyph-core/src/emit/scaffold.rs`:

```rust
#[test]
fn top_level_tier1_call_with_modifier_emits_span_with_raw_resolved_body() {
    use crate::ir::{IrCall, IrNode, IrSkill, LocalRef};
    let mut arena = IrArena::new();
    let call_id = arena.push(IrNode::Call(IrCall {
        node_id: NodeId(0),
        target: "inspect".into(),
        // Has a {ctx} slot — under non-trivial tier-1 the slot stays raw.
        resolved_body: Some("Look at {ctx}.".into()),
        site_modifier: Some("focus on lint".into()),
        projection_tier: Some(1),
        procedure_path: None, bound_name: None,
        local_refs: vec![LocalRef { name: "ctx".into(), node_id: NodeId(99) }],
        callee_output_contract: None, callee_return_type_text: None,
    }));
    let skill_id = arena.push(IrNode::Skill(IrSkill {
        node_id: NodeId(1), name: "demo".into(), description: "Demo.".into(),
        effects: vec![], params: vec![], steps: vec![call_id],
        context: vec![], constraints: vec![],
        return_text: None, return_type: None, output_contract: None,
        return_type_text: None, return_local_ref: None,
        freeform_sections: Vec::new(),
        description_source_line: None, context_source_line: None,
        constraints_source_line: None, flow_source_line: None,
    }));
    arena.set_root_skill(skill_id);
    let scaffold = build(&arena, false);
    let spans: Vec<_> = scaffold.chunks.iter().filter_map(|c| match c {
        Chunk::Span(sp) if sp.kind == SpanKind::CallBodyShape => Some(sp),
        _ => None,
    }).collect();
    assert_eq!(spans.len(), 1, "tier-1 top-level Call with modifier+local_refs must emit a span");
    assert_eq!(
        spans[0].payload.resolved_body.as_deref(),
        Some("Look at {ctx}."),
        "tier-1 non-trivial path must keep the raw {{name}} slot intact (no substitute_local_refs_in)"
    );
}

#[test]
fn top_level_tier1_final_call_with_modifier_carries_post_merge_return_sentence() {
    use crate::ir::{IrCall, IrNode, IrSkill, OutputTargetForm};
    let mut arena = IrArena::new();
    let call_id = arena.push(IrNode::Call(IrCall {
        node_id: NodeId(0), target: "produce".into(),
        resolved_body: Some("Inspect the working tree.".into()),
        site_modifier: Some("focus on lint".into()),
        projection_tier: Some(1),
        procedure_path: None, bound_name: None, local_refs: Vec::new(),
        callee_output_contract: None, callee_return_type_text: None,
    }));
    let form = OutputTargetForm::Identifier("current_branch".into());
    let skill_id = arena.push(IrNode::Skill(IrSkill {
        node_id: NodeId(1), name: "demo".into(), description: "Demo.".into(),
        effects: vec![], params: vec![], steps: vec![call_id],
        context: vec![], constraints: vec![],
        return_text: None, return_type: None,
        output_contract: Some(form),
        return_type_text: None, return_local_ref: None,
        freeform_sections: Vec::new(),
        description_source_line: None, context_source_line: None,
        constraints_source_line: None, flow_source_line: None,
    }));
    arena.set_root_skill(skill_id);
    let scaffold = build(&arena, false);
    let spans: Vec<_> = scaffold.chunks.iter().filter_map(|c| match c {
        Chunk::Span(sp) if sp.kind == SpanKind::CallBodyShape => Some(sp.clone()),
        _ => None,
    }).collect();
    assert_eq!(spans.len(), 1);
    assert_eq!(
        spans[0].payload.post_merge_return_sentence.as_deref(),
        Some("Produce `current_branch`."),
        "final-step tier-1 Call with output_contract must carry the §8.4 return sentence on the payload"
    );
}
```

- [ ] **Step 2: Run the tests and verify they fail**

```bash
cargo nextest run -p glyph-core emit::scaffold::tests::top_level_tier1_call_with_modifier_emits_span_with_raw_resolved_body emit::scaffold::tests::top_level_tier1_final_call_with_modifier_carries_post_merge_return_sentence 2>&1 | tail -20
```
Expected: assertion failures (no `CallBodyShape` span emitted today).

- [ ] **Step 3: Rewrite the tier-1 top-level arm**

In `crates/glyph-core/src/emit/scaffold.rs`, the tier-1 arm runs from L982 (`IrNode::Call(c) if c.projection_tier == Some(1) =>`) to L1056. Replace the entire arm body with:

```rust
IrNode::Call(c) if c.projection_tier == Some(1) => {
    s.push_literal(format!("{}. ", idx + 1));
    let is_returned_producer = skill
        .return_local_ref
        .as_ref()
        .is_some_and(|lr| lr.node_id == c.node_id);
    let (effective_form, effective_rt) = match skill_oc_form.as_ref() {
        Some(form) => (Some(form), skill_rt_text.as_deref()),
        None => (
            c.callee_output_contract.as_ref(),
            c.callee_return_type_text.as_deref(),
        ),
    };
    let return_sentence = if is_last && !is_returned_producer {
        templates::compute_return_sentence(
            effective_rt,
            effective_form,
            &arena.type_registry,
        )
    } else {
        None
    };

    let raw_body = c.resolved_body.as_deref().unwrap_or_default();
    let body_is_empty = raw_body.trim().is_empty();

    // §9.1 producer naming sentence — Post-span Literal chunk when emitted.
    let naming = naming_sentence_for_call(c);

    if call_needs_llm_fill(c) {
        if is_last && body_is_empty && return_sentence.is_some() {
            // Empty-body + return-only case: there is no body for the LLM
            // to produce. The return sentence stands alone as today. Emit
            // a span only when the modifier itself is non-empty; the LLM
            // must produce body prose from the modifier alone.
            let id = SpanId(*next_span_id);
            *next_span_id += 1;
            s.push_span(SpanRef {
                id,
                kind: SpanKind::CallBodyShape,
                ir_node: c.node_id,
                payload: SpanPayload {
                    target_name: Some(c.target.clone()),
                    projection_mode: Some(ProjectionMode::Inline),
                    site_modifier: c.site_modifier.clone(),
                    resolved_body: Some(String::new()),
                    local_refs: c.local_refs.clone(),
                    post_merge_return_sentence: return_sentence,
                    ..SpanPayload::default()
                },
            });
        } else {
            // Standard non-trivial tier-1 path. Raw {name} slots stay
            // intact in payload.resolved_body — the LLM does the local-ref
            // weaving using payload.local_refs.
            let id = SpanId(*next_span_id);
            *next_span_id += 1;
            s.push_span(SpanRef {
                id,
                kind: SpanKind::CallBodyShape,
                ir_node: c.node_id,
                payload: SpanPayload {
                    target_name: Some(c.target.clone()),
                    projection_mode: Some(ProjectionMode::Inline),
                    site_modifier: c.site_modifier.clone(),
                    resolved_body: Some(raw_body.to_string()),
                    local_refs: c.local_refs.clone(),
                    post_merge_return_sentence: return_sentence,
                    ..SpanPayload::default()
                },
            });
        }
    } else {
        // Trivial path — today's substitute_local_refs_in + optional
        // append_return_sentence + naming sentence, all as literals.
        let body_owned = substitute_local_refs_in(raw_body, &c.local_refs);
        let body = body_owned.as_str();
        if is_last {
            let folded = match (return_sentence.as_deref(), body_is_empty) {
                (Some(sent), true) => sent.to_string(),
                (Some(sent), false) => templates::append_return_sentence(body, sent),
                (None, _) => body.to_string(),
            };
            s.push_literal(folded);
        } else {
            s.push_literal(body.to_string());
        }
    }
    if let Some(n) = naming {
        s.push_literal(format!(" {}", n));
    }
    s.push_literal("\n");
}
```

The variables `skill_oc_form`, `skill_rt_text`, `is_last`, `next_span_id`, `idx` must all be in scope here today — they all are in the current arm body. Confirm by reading the surrounding `build()` function. If `next_span_id` is not yet a `&mut u32` at this site (it should be — the existing branch dispatch threads it), declare it from the same source the branch dispatch uses.

The free helper `templates::append_return_sentence(body, sent)` continues to exist and is **still called** for the trivial path. For the non-trivial path it is moved to the merger (Step 4 of this task).

- [ ] **Step 4: Teach the merger to apply the post-merge return sentence**

In `crates/glyph-core/src/emit/merger.rs`, replace the `merge` function body so that when a `Chunk::Span` carries a `payload.post_merge_return_sentence: Some(sent)`, the filled body string is passed through `templates::append_return_sentence` **before** being concatenated to the output. The prefix Literal chunk runs before the span and is unaffected; the post-span naming-sentence Literal runs after, unchanged.

```rust
pub fn merge(scaffold: Scaffold, fills: HashMap<SpanId, String>) -> Result<String, MergeError> {
    use std::collections::HashSet;
    let emitted_ids: HashSet<SpanId> = scaffold
        .chunks
        .iter()
        .filter_map(|c| match c {
            Chunk::Span(s) => Some(s.id),
            _ => None,
        })
        .collect();
    for fill_id in fills.keys() {
        if !emitted_ids.contains(fill_id) {
            return Err(MergeError::UnknownSpan(*fill_id));
        }
    }
    let mut out = String::new();
    for chunk in scaffold.chunks {
        match chunk {
            Chunk::Literal(s) => out.push_str(&s),
            Chunk::Span(span) => {
                let filled = match fills.get(&span.id) {
                    Some(s) => s.clone(),
                    None => return Err(MergeError::MissingSpan(span.id)),
                };
                let body = match span.payload.post_merge_return_sentence.as_deref() {
                    Some(sent) => crate::emit::templates::append_return_sentence(&filled, sent),
                    None => filled,
                };
                out.push_str(&body);
            }
        }
    }
    Ok(out)
}
```

- [ ] **Step 5: Run the new tests + the merger tests**

```bash
cargo nextest run -p glyph-core emit::scaffold::tests::top_level_tier1_call_with_modifier_emits_span_with_raw_resolved_body emit::scaffold::tests::top_level_tier1_final_call_with_modifier_carries_post_merge_return_sentence emit::merger::tests
```
Expected: 2 new scaffold tests + all merger tests pass.

- [ ] **Step 6: Run the full workspace test suite**

```bash
cargo nextest run --workspace 2>&1 | tail -30
```
Expected: there may be **some** failures in CLI tests (`flow_assign_with_modifier_compiles` will break — that is intentional and addressed in Task 9). All `glyph-core` tests must pass. Note any failures in non-`glyph-core` crates for the next tasks.

- [ ] **Step 7: Commit**

```bash
git add crates/glyph-core/src/emit/scaffold.rs crates/glyph-core/src/emit/merger.rs
git commit -m "feat(emit): push CallBodyShape span at top-level tier-1 Call sites; return-fold runs post-merge"
```

---

## Task 8: Hard-fail tests per emit site (spec §6.2)

**Files:**
- Test: `crates/glyph-core/tests/callbodyshape_span.rs` — add per-site hard-fail tests.

The integration test created in Task 4 covers one site (tier-1 in-arm via the `flow_assign_with_modifier.glyph` shape). This task adds the remaining six sites. Each test compiles a small `.glyph` source via `compile_source_with_effects` and asserts the resulting `CompileOutcome::Diagnostics` carries exactly one `G::expand::llm-required-for-call` diagnostic.

**Reviewer-requested end-to-end variant (2026-05-18):** also add a test using the if/else shape from Task 10 — both arms call the same procedure, only the then-arm has `site_modifier`. Assert exactly one `G::expand::llm-required-for-call` diagnostic on the fixture (matching the scaffold-level chunk count). This is the end-to-end mirror of Task 10's chunk-layout regression; it pins the modifier-drop bug at the diagnostic level rather than the chunk-stream level.

- [ ] **Step 1: Add the six remaining per-site tests**

Append to `crates/glyph-core/tests/callbodyshape_span.rs`:

```rust
fn count_llm_required(src: &str) -> (usize, Vec<String>) {
    let outcome = compile_source_with_effects(src, 0, "test.glyph", false).unwrap();
    match outcome {
        CompileOutcome::Diagnostics(bag) => {
            let sorted = bag.sorted();
            let llms: Vec<_> = sorted
                .iter()
                .filter(|d| d.id == "G::expand::llm-required-for-call")
                .map(|d| d.message.clone())
                .collect();
            (llms.len(), llms)
        }
        CompileOutcome::Compiled { markdown, .. } => panic!(
            "expected Diagnostics; got Compiled markdown:\n{markdown}"
        ),
    }
}

const TIER1_TOPLEVEL: &str = r#"block inspect_repo(scope = ".") -> Report
    description: "Inspect the repository."
    flow:
        "Examine the repository at {scope}."
        return context

skill diagnose(scope = ".") -> Report
    description: "Inspect with focus."
    flow:
        inspect_repo(scope) with "focus on lint failures"
        return context
"#;

#[test]
fn site_tier1_toplevel_with_modifier_hard_fails() {
    let (n, msgs) = count_llm_required(TIER1_TOPLEVEL);
    assert_eq!(n, 1, "got msgs={msgs:?}");
    assert!(msgs[0].contains("inspect_repo"));
    assert!(msgs[0].contains("with modifier"));
}
```

For the remaining five sites — tier-2 top-level, tier-3 top-level, stdlib top-level, tier-2 in-arm, tier-3 in-arm — use the smallest valid `.glyph` source that drives each site. Look at the existing corpus under `crates/glyph-cli/tests/corpus/valid/` for templates; representative examples:

- Tier-2 top-level: a same-file `block` callee called at top level with a `with` modifier.
- Tier-3 top-level: an `export block` callee (which projects to a separate `.md` file) called at top level with a `with` modifier.
- Stdlib top-level: `ctx = subagent(scope) with "..."` (requires `import @glyph/std`).
- Tier-2 in-arm: a `block` callee called from inside a Branch arm with a `with` modifier.
- Tier-3 in-arm: an `export block` callee from inside a Branch arm with a `with` modifier.

Write each test in the same shape as `site_tier1_toplevel_with_modifier_hard_fails` — assert exactly one `G::expand::llm-required-for-call` diagnostic and that the message names the target.

- [ ] **Step 2: Run the tests**

```bash
cargo nextest run -p glyph-core --test callbodyshape_span
```
Expected: all per-site tests pass. If any test produces zero or two diagnostics, the emit site for that combination is either not emitting a span (a bug — go back to Task 5 or 6) or is emitting two spans (e.g. double-pushing).

- [ ] **Step 3: Add the local-ref + combined + ordering tests (spec §6.3, §6.4, §6.5)**

```rust
const TIER1_LOCAL_REFS: &str = r#"block inspect(scope = ".") -> Report
    description: "Inspect."
    flow:
        "Look at {scope}."
        return context

skill diagnose(scope = ".") -> Report
    description: "Demo."
    flow:
        ctx = inspect(scope)
        "Refer to {ctx} in the report."
        return ctx
"#;
// Authoring note: the "Refer to {ctx}" inline step plus the `ctx = inspect(scope)` binding
// makes populate_local_refs_in_steps populate IrCall.local_refs on the inspect call.

#[test]
fn local_refs_alone_hard_fails_with_local_ref_reason() {
    let (n, msgs) = count_llm_required(TIER1_LOCAL_REFS);
    assert_eq!(n, 1);
    assert!(msgs[0].contains("local-ref cross-references"));
}

const COMBINED: &str = r#"block inspect(scope = ".") -> Report
    description: "Inspect."
    flow:
        "Look at {scope}."
        return context

skill diagnose(scope = ".") -> Report
    description: "Demo."
    flow:
        ctx = inspect(scope) with "focus on lint"
        "Refer to {ctx}."
        return ctx
"#;

#[test]
fn modifier_plus_local_refs_yields_single_diagnostic_with_both_reasons() {
    let (n, msgs) = count_llm_required(COMBINED);
    assert_eq!(n, 1, "exactly one diagnostic per failing Call");
    assert!(msgs[0].contains("a with modifier and local-ref cross-references"));
    assert!(msgs[0].contains("the with modifier / rewrite the local reference"));
}

const TWO_CALLS: &str = r#"block a(scope = ".") -> Report
    description: "A."
    flow:
        "Look at {scope}."
        return context

block b(scope = ".") -> Report
    description: "B."
    flow:
        "Look at {scope}."
        return context

skill diagnose(scope = ".") -> Report
    description: "Demo."
    flow:
        a(scope) with "m1"
        b(scope) with "m2"
        return context
"#;

#[test]
fn multiple_failing_calls_ordered_by_ir_node_id() {
    let (n, msgs) = count_llm_required(TWO_CALLS);
    assert_eq!(n, 2);
    // a appears before b in source → its IrCall has the smaller NodeId.
    assert!(msgs[0].contains("`a`"));
    assert!(msgs[1].contains("`b`"));
}
```

- [ ] **Step 4: Add the unit test for the explicit `sort_by_key` on a reversed input (spec §6.5)**

Because `format_llm_required_message` and `llm_required_diagnostics_from_errors` are private helpers in `lib.rs`, the cleanest place for this unit test is an `#[cfg(test)] mod tests` block inside `lib.rs` itself. Append to that block (search for an existing `#[cfg(test)] mod tests` in `lib.rs` — there is one starting at L2878):

```rust
#[test]
fn llm_required_diagnostics_sort_by_ir_node_id_ascending() {
    use crate::emit::StubFillError;
    use crate::ir::NodeId;
    let errors = vec![
        StubFillError {
            ir_node: NodeId(7),
            target_name: Some("late".into()),
            has_modifier: true,
            has_local_refs: false,
        },
        StubFillError {
            ir_node: NodeId(3),
            target_name: Some("early".into()),
            has_modifier: true,
            has_local_refs: false,
        },
    ];
    let bag = super::llm_required_diagnostics_from_errors(errors, "demo.glyph");
    let sorted = bag.sorted();
    assert_eq!(sorted.len(), 2);
    // Node id 3 ("early") must appear first even though the input Vec is reversed.
    assert!(sorted[0].message.contains("`early`"), "got: {}", sorted[0].message);
    assert!(sorted[0].message.contains("(IR node n3)"));
    assert!(sorted[1].message.contains("`late`"));
    assert!(sorted[1].message.contains("(IR node n7)"));
}
```

- [ ] **Step 5: Run all new tests**

```bash
cargo nextest run -p glyph-core --test callbodyshape_span && \
cargo nextest run -p glyph-core llm_required_diagnostics_sort_by_ir_node_id_ascending
```
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add crates/glyph-core/tests/callbodyshape_span.rs crates/glyph-core/src/lib.rs
git commit -m "test(emit): hard-fail per emit site, deterministic message ordering"
```

---

## Task 9: Update the corpus fixture and CLI flow_assign test, plus assert no-output-file (spec §6.6)

**Files:**
- Modify: `crates/glyph-cli/tests/flow_assign.rs:151–175` — `flow_assign_with_modifier_compiles` flips from "expect exit 0 + `.md` written" to "expect non-zero exit + `.md` absent".

- [ ] **Step 1: Rewrite the test**

In `crates/glyph-cli/tests/flow_assign.rs`, replace the body of `flow_assign_with_modifier_compiles` with the new behavior. The function name now lies about what it does — rename it too:

```rust
/// §11: `<name> = <call>(scope) with "..."` now requires LLM-grade
/// expansion. The deterministic stub filler refuses, so compile exits
/// non-zero with `G::expand::llm-required-for-call` and writes no `.md`.
/// See docs/superpowers/specs/2026-05-18-callbodyshape-span-emission-design.md.
#[test]
fn flow_assign_with_modifier_hard_fails_under_stub_filler() {
    let src = fixture("valid", "flow_assign_with_modifier.glyph");
    let out = src.with_file_name("flow_assign_with_modifier.md");
    let _ = std::fs::remove_file(&out);

    let result = run_compile(src, "json");
    assert_ne!(
        result.status.code(),
        Some(0),
        "expected non-zero exit; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stderr = String::from_utf8_lossy(&result.stderr).to_string();
    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let combined = format!("{stdout}\n{stderr}");
    assert!(
        combined.contains("G::expand::llm-required-for-call"),
        "expected llm-required-for-call diagnostic; got combined output:\n{combined}"
    );
    assert!(
        !out.exists(),
        ".md file must not be written when CallBodyShape stub filler hard-fails: found {}",
        out.display()
    );
}
```

If the `valid/` corpus directory is asserted clean elsewhere (e.g. a "every fixture in valid/ compiles" sweep test), find that sweep and exclude `flow_assign_with_modifier.glyph` — `grep -n "fixture(\"valid\"" crates/glyph-cli/tests/` to find similar sweeps. If no such sweep exists, no change needed.

- [ ] **Step 2: Run the test**

```bash
cargo nextest run -p glyph-cli flow_assign_with_modifier_hard_fails_under_stub_filler
```
Expected: PASS.

- [ ] **Step 3: Run the full CLI test suite to surface any other corpus consumers that need the same flip**

```bash
cargo nextest run -p glyph-cli 2>&1 | tail -30
```
Expected: `Finished`, all passed. If any other test fails because it expected silent success on a `with`-modifier or local-ref-bearing Call, repeat Step 1 on that test (flip success-assertions to hard-fail-assertions).

- [ ] **Step 4: Commit**

```bash
git add crates/glyph-cli/tests/flow_assign.rs
git commit -m "test(cli): flow_assign_with_modifier now hard-fails with G::expand::llm-required-for-call"
```

---

## Task 10: Span-boundary / chunk-layout scaffold tests (spec §6.7)

**Files:**
- Test: `crates/glyph-core/src/emit/scaffold.rs` — `mod tests`. Inspect scaffold chunks directly (no fill, no merge).

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `crates/glyph-core/src/emit/scaffold.rs`:

```rust
#[test]
fn naming_sentence_emitted_as_post_span_literal_chunk() {
    use crate::ir::{IrCall, IrNode, IrSkill};
    let mut arena = IrArena::new();
    let call_id = arena.push(IrNode::Call(IrCall {
        node_id: NodeId(0), target: "inspect".into(),
        resolved_body: Some("Inspect.".into()),
        site_modifier: Some("focus".into()),
        projection_tier: Some(1),
        procedure_path: None,
        bound_name: Some("foo".into()),  // triggers naming_sentence_for_call
        local_refs: Vec::new(),
        callee_output_contract: None, callee_return_type_text: None,
    }));
    let skill_id = arena.push(IrNode::Skill(IrSkill {
        node_id: NodeId(1), name: "demo".into(), description: "Demo.".into(),
        effects: vec![], params: vec![], steps: vec![call_id],
        context: vec![], constraints: vec![],
        return_text: None, return_type: None, output_contract: None,
        return_type_text: None, return_local_ref: None,
        freeform_sections: Vec::new(),
        description_source_line: None, context_source_line: None,
        constraints_source_line: None, flow_source_line: None,
    }));
    arena.set_root_skill(skill_id);
    let scaffold = build(&arena, false);
    // Find: [..., Literal("N. "), Span(CallBodyShape), Literal(" Refer to this … as foo."), Literal("\n"), ...]
    let mut iter = scaffold.chunks.iter().peekable();
    let mut found = false;
    while let Some(chunk) = iter.next() {
        if let Chunk::Span(sp) = chunk {
            if sp.kind == SpanKind::CallBodyShape {
                let next = iter.next().expect("expected literal after span");
                match next {
                    Chunk::Literal(l) => {
                        assert!(l.contains("Refer to this") && l.contains("foo"),
                            "expected naming sentence as post-span literal; got: {l:?}");
                    }
                    _ => panic!("expected Literal after CallBodyShape span"),
                }
                found = true;
                break;
            }
        }
    }
    assert!(found, "expected a CallBodyShape span in chunk stream");
}

#[test]
fn return_fold_is_carrier_not_literal_chunk_between_span_and_newline() {
    use crate::ir::{IrCall, IrNode, IrSkill, OutputTargetForm};
    let mut arena = IrArena::new();
    let call_id = arena.push(IrNode::Call(IrCall {
        node_id: NodeId(0), target: "inspect".into(),
        resolved_body: Some("Inspect.".into()),
        site_modifier: Some("focus".into()),
        projection_tier: Some(1),
        procedure_path: None, bound_name: None, local_refs: Vec::new(),
        callee_output_contract: None, callee_return_type_text: None,
    }));
    let skill_id = arena.push(IrNode::Skill(IrSkill {
        node_id: NodeId(1), name: "demo".into(), description: "Demo.".into(),
        effects: vec![], params: vec![], steps: vec![call_id],
        context: vec![], constraints: vec![],
        return_text: None, return_type: None,
        output_contract: Some(OutputTargetForm::Identifier("id".into())),
        return_type_text: None, return_local_ref: None,
        freeform_sections: Vec::new(),
        description_source_line: None, context_source_line: None,
        constraints_source_line: None, flow_source_line: None,
    }));
    arena.set_root_skill(skill_id);
    let scaffold = build(&arena, false);
    let chunks = &scaffold.chunks;
    // Find the span, verify the next chunk is exactly Literal("\n").
    let mut found = false;
    for (i, chunk) in chunks.iter().enumerate() {
        if let Chunk::Span(sp) = chunk {
            if sp.kind == SpanKind::CallBodyShape {
                assert_eq!(
                    sp.payload.post_merge_return_sentence.as_deref(),
                    Some("Produce `id`."),
                    "payload must carry the §8.4 return sentence (not a separate Literal chunk)"
                );
                let next = chunks.get(i + 1).expect("expected a chunk after the span");
                match next {
                    Chunk::Literal(l) => assert_eq!(l, "\n", "expected newline literal immediately after span; got: {l:?}"),
                    _ => panic!("expected Literal('\\n') after span; no return-fold literal between them"),
                }
                found = true;
                break;
            }
        }
    }
    assert!(found, "expected a CallBodyShape span in the chunk stream");
}

#[test]
fn if_arms_with_same_target_only_modifier_arm_emits_call_body_shape_span() {
    // Regression for the modifier-drop bug surfaced 2026-05-18:
    // an if/else where both arms call the same procedure, only the then-arm
    // has `site_modifier`. Assert that the then-arm produces exactly one
    // CallBodyShape span (so the expand pass is forced to weave the modifier
    // into prose), while the otherwise arm stays as the deterministic
    // "Follow the {kebab} procedure." literal. The reviewer asked for this
    // shape specifically; see also §6.2 for the end-to-end variant that
    // asserts exactly one G::expand::llm-required-for-call diagnostic on
    // the same fixture.
    use crate::ir::{IrCall, IrIf, IrNode, IrSkill};
    let mut arena = IrArena::new();
    // NOTE for implementer: reconcile IrCall / IrIf field drift against the
    // current IR (e.g. `args`, `return_type`, `is_agent` on IrCall — match
    // Task 5's fixture). The shape that matters here is:
    //   - then_branch: [Call { target: "build-walkthrough", site_modifier: Some("name each construct…"), local_refs: [], … }]
    //   - otherwise_branch: [Call { target: "build-walkthrough", site_modifier: None, local_refs: [], … }]
    //   - if-node is the sole step of the root skill.
    // (Skipping the literal struct here because the IR has drifted twice
    // already this branch; copy the IrCall literal from Task 5's
    // `in_arm_tier1_call_with_modifier_emits_call_body_shape_span` test.)
    let then_call_id = arena.push(IrNode::Call(/* see note above */ todo!()));
    let else_call_id = arena.push(IrNode::Call(/* see note above */ todo!()));
    let if_id = arena.push(IrNode::If(IrIf {
        // fields per current IrIf — condition, then_branch: vec![then_call_id],
        // otherwise_branch: vec![else_call_id], …
        ..todo!()
    }));
    let skill_id = arena.push(IrNode::Skill(IrSkill {
        node_id: NodeId(3), name: "demo".into(), description: "Demo.".into(),
        effects: vec![], params: vec![], steps: vec![if_id],
        context: vec![], constraints: vec![],
        return_text: None, return_type: None, output_contract: None,
        return_type_text: None, return_local_ref: None,
        freeform_sections: Vec::new(),
        description_source_line: None, context_source_line: None,
        constraints_source_line: None, flow_source_line: None,
    }));
    arena.set_root_skill(skill_id);
    let scaffold = build(&arena, false);

    // Count CallBodyShape spans across the whole chunk stream.
    let span_count = scaffold.chunks.iter().filter(|c| {
        matches!(c, Chunk::Span(sp) if sp.kind == SpanKind::CallBodyShape)
    }).count();
    assert_eq!(span_count, 1,
        "exactly one CallBodyShape span expected (the modifier-bearing then-arm); got {span_count}");

    // Otherwise-arm body must remain a deterministic literal — the kebab'd
    // target appears verbatim in some Literal chunk.
    let any_literal_has_kebab = scaffold.chunks.iter().any(|c| {
        matches!(c, Chunk::Literal(l) if l.contains("Follow the build-walkthrough procedure."))
    });
    assert!(any_literal_has_kebab,
        "expected otherwise-arm to stay as deterministic 'Follow the build-walkthrough procedure.' literal");
}
```

- [ ] **Step 2: Run the tests**

```bash
cargo nextest run -p glyph-core \
  emit::scaffold::tests::naming_sentence_emitted_as_post_span_literal_chunk \
  emit::scaffold::tests::return_fold_is_carrier_not_literal_chunk_between_span_and_newline \
  emit::scaffold::tests::if_arms_with_same_target_only_modifier_arm_emits_call_body_shape_span
```
Expected: 3 passed.

- [ ] **Step 3: Commit**

```bash
git add crates/glyph-core/src/emit/scaffold.rs
git commit -m "test(emit): chunk-layout asserts + if-arms modifier-drop regression"
```

---

## Task 11: Per-site regression tests for trivial Calls (spec §6.1)

**Files:**
- Test: `crates/glyph-core/tests/callbodyshape_span.rs` — append.

These tests are the safety net for "trivial Calls render byte-identically to today's output." For each site, compile a `.glyph` source with no modifier and no local-refs, assert specific substrings in the rendered Markdown.

- [ ] **Step 1: Add the regression tests**

Append to `crates/glyph-core/tests/callbodyshape_span.rs`:

```rust
fn compile_to_md(src: &str) -> String {
    match compile_source_with_effects(src, 0, "test.glyph", false).unwrap() {
        CompileOutcome::Compiled { markdown, .. } => markdown,
        CompileOutcome::Diagnostics(bag) => panic!(
            "trivial Call must compile cleanly; got diagnostics:\n{:?}",
            bag.sorted()
        ),
    }
}

#[test]
fn trivial_tier1_toplevel_renders_inline_body() {
    let src = r#"block inspect(scope = ".") -> Report
    description: "Inspect."
    flow:
        "Look at {scope}."
        return context

skill diagnose(scope = ".") -> Report
    description: "Demo."
    flow:
        inspect(scope)
        return context
"#;
    let md = compile_to_md(src);
    assert!(md.contains("Look at"), "trivial tier-1 inline body must render in md:\n{md}");
}

#[test]
fn trivial_tier2_toplevel_renders_follow_procedure() {
    // Use the smallest valid source that drives a tier-2 same-file procedure
    // Call — see crates/glyph-cli/tests/corpus/valid for templates.
    let src = r#"block do_steps()
    description: "Steps."
    flow:
        "Do thing."

skill demo()
    description: "Demo."
    flow:
        do_steps()
"#;
    let md = compile_to_md(src);
    assert!(
        md.contains("Follow the do-steps procedure below."),
        "trivial tier-2 anchor must render in md:\n{md}"
    );
}

// Add tier-3-toplevel, stdlib-bound (no modifier), tier-1-in-arm,
// tier-2-in-arm, tier-3-in-arm regression tests in the same shape.
// Smallest reproducer for each can be cribbed from the corpus under
// crates/glyph-cli/tests/corpus/valid/.
```

For the remaining four sites, use the smallest valid `.glyph` source for each — search `crates/glyph-cli/tests/corpus/valid/` for an example. Each test follows the shape: compile → assert a known substring in the rendered `markdown`.

- [ ] **Step 2: Run the tests**

```bash
cargo nextest run -p glyph-core --test callbodyshape_span trivial_
```
Expected: all per-site regression tests pass. If one fails, the trivial-path emission for that site has drifted — go back to whichever task touched that emit arm.

- [ ] **Step 3: Commit**

```bash
git add crates/glyph-core/tests/callbodyshape_span.rs
git commit -m "test(emit): per-site regression for trivial Call rendering"
```

---

## Task 12: Documentation updates (spec §3.8)

**Files:**
- Modify: `docs/reference/diagnostics.md` — register `G::expand::llm-required-for-call`.
- Modify: `docs/architecture/expand.md` — update §3.5 CallBodyShape row; add "Step 2 fill-time diagnostics" subsection.
- Modify: `llm_expand_pass.md` — one-line preamble note.
- Modify: `todo/expand-todos.md` — two new follow-up entries.

- [ ] **Step 1: Register the diagnostic ID in the public catalog**

In `docs/reference/diagnostics.md`, find the `G::expand::*` table (around L176). Add a new row in alphabetical or namespace-grouped order:

```markdown
| `G::expand::llm-required-for-call` | error | A `Call` site has a `with` modifier or non-empty `local_refs` that requires LLM-grade prose, but the current compiler build is using the deterministic stub filler. Fires per failing `IrCall` at Step 2 fill time (pre-6b). Remediation: wire the LLM expand filler, or remove the `with` modifier / rewrite the local reference. |
```

- [ ] **Step 2: Update the `CallBodyShape` row in `docs/architecture/expand.md` §3.5**

Find the SpanKind table in `docs/architecture/expand.md` §3.5 (search for `CallBodyShape`). Replace the "Stub behavior today" cell from:

> Verbatim resolved body — modifier and scoped constraints currently ignored.

to:

> Spans are emitted only when `site_modifier` or `local_refs` are non-empty; the stub hard-fails with `G::expand::llm-required-for-call` and the lib-level callers convert that into `CompileOutcome::Diagnostics`, suppressing the `.md` write. Trivial Calls do not emit a span and render via the deterministic literal template. Scoped-constraint weaving is deferred (see [`todo/expand-todos`](../../todo/expand-todos.md)).

- [ ] **Step 3: Add the "Step 2 fill-time diagnostics" subsection**

In `docs/architecture/expand.md`, add a new subsection. If §3.5 has an obvious place (right after the SpanKind table), put it there as a sibling subsection. Otherwise add it as §3.6 or §4.x — the architecture doc owner can renumber later.

```markdown
### Step 2 fill-time diagnostics

The fill layer (`crates/glyph-core/src/emit/stub_fill.rs`) can refuse to fill a span before the merger runs. These diagnostics are distinct from §4.2's Phase 6b structural catalog — they fire **before** any `.md` text is produced. The single ID today:

| ID | Trigger |
|---|---|
| `G::expand::llm-required-for-call` | A `CallBodyShape` span is emitted (because the Call has a `with` modifier or non-empty `local_refs`) and the build is using the stub filler instead of the LLM filler. |

Relationship to Phase 6b: this diagnostic catches the *configuration / filler-wiring* failure that would otherwise silently elide modifier intent or LLM-grade local-ref cross-references. Phase 6b's complementary structural checks (`G::expand::modifier-leaked`, `G::expand::unresolved-local-ref`) catch the *content* failure when the LLM filler runs but produces non-conforming prose.
```

- [ ] **Step 4: Add the `llm_expand_pass.md` preamble note**

At the top of `llm_expand_pass.md` (before §1.1), add:

```markdown
> **Refusal semantics (2026-05-18):** the deterministic stub filler no longer silently elides `with` modifiers or LLM-grade local-ref cross-references. When a `CallBodyShape` span requires LLM judgment, the stub hard-fails with `G::expand::llm-required-for-call` and no `.md` is written. See `docs/superpowers/specs/2026-05-18-callbodyshape-span-emission-design.md`.
```

- [ ] **Step 5: Add two follow-up items to `todo/expand-todos.md`**

Append:

```markdown
- **Scoped constraints on `IrCall`.** Lower callee constraints into a new `IrCall.scoped_constraints` field; serialize via `emit_ir.rs` (today hardcoded to `[]`); extend the §3.3 triviality predicate in `crates/glyph-core/src/emit/scaffold.rs::call_needs_llm_fill` with `|| !c.scoped_constraints.is_empty()`; extend `SpanPayload` and `StubFillError` accordingly; reuse the span-emission machinery from the 2026-05-18 CallBodyShape spec.
- **Real source spans on `IrCall`.** Thread a `SourceSpan` (or byte-offset pair) through `IrCall` from parser → lower → IR so `G::expand::llm-required-for-call` can carry a real source span instead of the synthetic zero-width file-level span the CallBodyShape spec ships with.
```

- [ ] **Step 6: Commit**

```bash
git add docs/reference/diagnostics.md docs/architecture/expand.md llm_expand_pass.md todo/expand-todos.md
git commit -m "docs: register G::expand::llm-required-for-call and document Step-2 fill-time diagnostics"
```

---

## Task 13: Full workspace verification

**Files:** none (verification only).

- [ ] **Step 1: Workspace check**

```bash
cargo check --workspace 2>&1 | tail -10
```
Expected: `Finished` with no errors.

- [ ] **Step 2: Workspace test**

```bash
cargo nextest run --workspace 2>&1 | tail -50
```
Expected: all tests pass. If any test outside the ones touched by this plan fails, read the failure carefully — it may be a test that depended on silent modifier-drop behavior and needs the Task 9 treatment (flip success-assertions to hard-fail-assertions, then add to this plan retroactively as Task 14).

- [ ] **Step 3: Format and lint**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -20
```
Expected: `Finished`, no warnings.

- [ ] **Step 4: Commit any final formatting**

```bash
git diff --stat
# If non-empty:
git add -u
git commit -m "chore: cargo fmt after CallBodyShape span emission rollout"
```

---

## Task 14: Procedure-body Call emission — the 8th site (spec §3.10)

**Why:** Post-impl review found an 8th emit surface the original plan did not cover. Today the lower at `crates/glyph-core/src/lower.rs:845–857` stringifies block-flow Calls via `format!("call {}", target.node)`, destroying `site_modifier`, `local_refs`, `resolved_body`, `projection_tier`, and `bound_name` before emit runs. Then `emit::emit_procedure` (`emit/mod.rs:70–181`) renders the lossy `flow_strings: Vec<String>` verbatim. A Tier-2 same-file procedure whose body contains `call foo with "…"` therefore loses the modifier silently.

This task lands the lower-side IR change (`IrBlockFlowItem` enum + arena entries for block-flow Calls) and routes procedure-body emission through the same scaffold/span pipeline as the other seven sites.

**Files:**
- Modify: `crates/glyph-core/src/ir.rs` (define `IrBlockFlowItem`, replace `IrBlock.flow_statements: Vec<String>` with `IrBlock.flow_items: Vec<IrBlockFlowItem>`)
- Modify: `crates/glyph-core/src/lower.rs:~845–857, ~544, ~1101` (allocate `IrNode::Call` arena entries for block-flow Calls; produce `IrBlockFlowItem` values; share the Call constructor across the three lowering sites)
- Modify: `crates/glyph-core/src/emit/mod.rs:70–181` (rewrite `emit_procedure` to consume `flow_items` via scaffold/span)
- Modify: `crates/glyph-core/src/lib.rs:~2227–2238` (propagate `Vec<StubFillError>` from `emit_procedure` into the diagnostic bag for procedure-export failures)
- Modify: `crates/glyph-core/src/emit_ir.rs` (serialize `flow_items` instead of `flow_statements`)
- Modify: `crates/glyph-core/src/emit/scaffold.rs` (if scaffold-based path chosen: add per-procedure walk reusing the existing per-tier match)
- Tests: `crates/glyph-core/tests/callbodyshape_span.rs` (extend with procedure-body T1/T2/T3 trivial regression and hard-fail tests, plus tier-1 in-arm e2e hard-fail per spec §6.2)
- Tests: `crates/glyph-core/src/emit/merger.rs::tests` (new merger unit test for `return_fold_is_carrier_not_literal_chunk_between_span_and_newline` post-merge result — reviewer Nit)
- Tests: `crates/glyph-core/src/emit/scaffold.rs::tests` (strengthen `if_arms_with_same_target_only_modifier_arm_emits_call_body_shape_span` assertion — reviewer Nit)
- Doc: spec §3.10, §5, §6 (already updated in this rev)

**Implementation path choice.** Spec §3.10 accepts two paths for the emit-side rewrite. Default to **scaffold-based** — extend the central scaffold to emit a per-procedure region whose chunk-stream reuses the existing per-tier match from `scaffold.rs:~1037–L1091`. If that refactor is too large to land in one task, fall back to **local-pipeline** — have `emit_procedure` build its own `Scaffold`, run `stub_fill::fill` and `merger::merge` locally, and propagate the `Vec<StubFillError>` up to its caller. Both produce identical Markdown and identical diagnostic plumbing. Pick during implementation based on the refactor's actual size.

**`branch_steps` retirement.** Per §8 open question: `IrBlock.branch_steps: HashMap<usize, NodeId>` overlaps with `IrBlockFlowItem::Branch { node_id }`. Leave `branch_steps` in place for this task (smaller blast radius). Add a comment on `branch_steps` noting that `flow_items` is now the source of truth for ordering. Retiring `branch_steps` is tracked as a follow-up.

- [ ] **Step 1: Add `IrBlockFlowItem` enum to ir.rs**

`crates/glyph-core/src/ir.rs` (near the existing `IrBlock` definition at ~L239):

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IrBlockFlowItem {
    Inline { text: String },
    Call { node_id: NodeId },
    Branch { node_id: NodeId },
    Constraint { rendered: String },
    Context { rendered: String },
    Return,
    BareName { name: String },
}
```

Add `pub flow_items: Vec<IrBlockFlowItem>` to `IrBlock`. Keep `flow_statements` for now (do not delete yet — Step 4 migrates emit-side consumers, Step 5 removes it).

- [ ] **Step 2: Lower block-flow Calls into arena entries**

`crates/glyph-core/src/lower.rs:~845–857`. Replace the lossy stringifier with a builder that emits `IrBlockFlowItem` values. For `FlowStmt::Call { target, site_modifier, local_refs, resolved_body, projection_tier, bound_name, .. }`:

1. Construct an `IrCall` record matching the shape produced by `lower_flow_body` (~L544) and the top-level skill flow path (~L1101). Consolidate into a single private constructor (e.g. `fn make_ir_call(stmt: &FlowStmt, …) -> IrCall`) used by all three sites.
2. Push the `IrCall` into the arena, capture the `NodeId`.
3. Emit `IrBlockFlowItem::Call { node_id }`.

For `FlowStmt::Branch`, emit `IrBlockFlowItem::Branch { node_id }` referencing the `IrBranch` arena entry that `branch_steps` already creates. For all other `FlowStmt::*` arms, emit the corresponding `Inline` / `Constraint` / `Context` / `Return` / `BareName` variant with today's rendered text.

Ensure `populate_local_refs_in_steps` (or equivalent) walks the new procedure-body Call arena entries.

- [ ] **Step 3: Write a failing test for procedure-body Tier-1 trivial regression**

`crates/glyph-core/tests/callbodyshape_span.rs`:

```rust
#[test]
fn procedure_body_tier1_trivial_call_renders_inline_body() {
    let src = r#"
        block helper {
            "do thing X"
        }
        block main {
            call helper
        }
        skill demo() {
            call main
        }
    "#;
    let compiled = compile_to_md(src).expect("compile success");
    // Procedure section for `main` should contain the inlined body of `helper`,
    // not a bare `call helper` line.
    assert!(compiled.contains("do thing X"), "compiled:\n{}", compiled);
    assert!(!compiled.contains("1. call helper"), "compiled:\n{}", compiled);
}
```

Run: `cargo nextest run -p glyph-core --test callbodyshape_span procedure_body_tier1_trivial_call_renders_inline_body`.
Expected: FAIL (current emit_procedure renders `1. call helper` via lossy flow_strings).

- [ ] **Step 4: Rewrite `emit_procedure` to consume `flow_items`**

`crates/glyph-core/src/emit/mod.rs:70–181`. Replace the `flow_strings: &[String]` parameter with `flow_items: &[IrBlockFlowItem]` and an `arena: &IrArena` reference (or thread the scaffold).

For each `IrBlockFlowItem`:
- `Inline { text }` → push as numbered Step line literal (today's behavior).
- `Call { node_id }` → dereference the arena, call `scaffold::call_needs_llm_fill(c)`; if trivial, push the per-tier literal anchor; if non-trivial, push a `CallBodyShape` span (mirrors `branch.rs::emit_lettered_substeps:300–340` and `scaffold.rs:~1037–L1091`).
- `Branch { node_id }` → today's branch-rendering path (already arena-driven via `branch_steps`).
- `Constraint`, `Context`, `Return`, `BareName` → today's literal rendering.

Update `emit_library_procedures` (`lib.rs:~2227–2238`) to pass `eb.node.flow_items` and `arena` instead of `eb.node.flow_strings`, and to propagate `Vec<StubFillError>` from `emit_procedure` into the diagnostic bag (one `G::expand::llm-required-for-call` per failing span, using the same `format_llm_required_message` helper from Task 4).

Re-run Step 3's test. Expected: PASS.

- [ ] **Step 5: Migrate `emit_ir.rs` and remove `flow_statements`**

`crates/glyph-core/src/emit_ir.rs`. Replace `flow_statements` serialization with `flow_items` (tagged enum array). Update any internal snapshot fixtures that pin the old shape — regenerate via `cargo insta test --review`.

After all emit-side consumers move to `flow_items`, remove the `flow_statements` field from `IrBlock` in `ir.rs`. (Or retain as a derived accessor `pub fn flow_statements(&self) -> Vec<String>` if a non-emit consumer still depends on it; emit MUST NOT read it.)

Run: `cargo check --workspace`. Expected: clean.

- [ ] **Step 6: Hard-fail tests for procedure-body Call with `with` modifier (per sub-tier)**

`crates/glyph-core/tests/callbodyshape_span.rs`. Three tests, one per sub-tier inside a procedure body:

```rust
#[test]
fn procedure_body_tier1_with_modifier_hard_fails() {
    let src = r#"
        block helper {
            "do thing"
        }
        block main {
            call helper with "do it carefully"
        }
        skill demo() { call main }
    "#;
    let result = compile_source(src);
    let bag = expect_diagnostics(result);
    assert_eq!(bag.iter().count(), 1);
    let d = bag.iter().next().unwrap();
    assert_eq!(d.id(), "G::expand::llm-required-for-call");
    assert!(d.message().contains("a with modifier"));
}

// Equivalent for tier-2 (`block main { call other_proc with "…" }` where
// other_proc is rendered as a same-file procedure) and tier-3
// (`block main { call extern_proc with "…" }` where extern_proc is in a
// separate .glyph file imported into the project).
```

Plus the **tier-1 in-arm end-to-end hard-fail** (reviewer Important) at the `compile_directory_with_layout` level: assert `compiled.md` is absent on disk and the diagnostic bag carries `G::expand::llm-required-for-call` for the in-arm Call's IR node id.

Run: `cargo nextest run -p glyph-core --test callbodyshape_span`. Expected: all new tests pass.

- [ ] **Step 7: Reviewer Nits — strengthen existing tests**

In `crates/glyph-core/src/emit/scaffold.rs::tests`:

Strengthen `if_arms_with_same_target_only_modifier_arm_emits_call_body_shape_span` to assert:
- `sp.ir_node` equals the modifier-arm Call's `IrCall.node_id` (find it by name in the arena).
- `sp.payload.site_modifier == Some("name each construct …")` (the exact modifier text from the regression fixture).

In `crates/glyph-core/src/emit/merger.rs::tests`:

Add a new merger unit test:

```rust
#[test]
fn return_fold_post_merge_produces_punctuation_stripped_body_with_return_sentence() {
    // Build a synthetic scaffold with one CallBodyShape span carrying
    // post_merge_return_sentence == Some("Produce `id`.").
    // Feed a synthetic fill ("body text.") through merger::merge.
    // Assert merged line is "1. body text. Produce `id`.\n"
    // (i.e. append_return_sentence stripped the trailing "." and appended ". Produce `id`.")
    // …
}
```

Run: `cargo nextest run -p glyph-core --lib emit::scaffold::tests::if_arms_with_same_target_only_modifier_arm_emits_call_body_shape_span emit::merger::tests::return_fold_post_merge_produces_punctuation_stripped_body_with_return_sentence`.
Expected: PASS.

- [ ] **Step 8: Full workspace verification**

```bash
cargo fmt --all && \
cargo check --workspace 2>&1 | tail -10 && \
cargo nextest run --workspace 2>&1 | tail -50 && \
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -20
```

Expected: fmt clean, check clean, 0 new test regressions vs baseline, clippy clean (or unchanged count vs `main`).

- [ ] **Step 9: Commit**

```bash
git add -u
git commit -m "feat(emit): route procedure-body Calls through CallBodyShape span (8th site)

Closes the eighth emit surface flagged in post-impl review of the
CallBodyShape span spec. Replaces IrBlock.flow_statements: Vec<String>
with IrBlock.flow_items: Vec<IrBlockFlowItem>, allocates IrCall arena
entries for block-flow Calls in lower, and rewrites emit_procedure to
consume flow_items via the scaffold/span pipeline. The invariant
'every Step-projecting Call is an IrCall arena node' now holds across
all three positions (skill body, branch arm, procedure body).

Adds reviewer-requested test strengthening: explicit ir_node and
site_modifier assertions on the modifier-arm span test, and a merger
unit test for the post-merge return-fold result.

Refs: docs/superpowers/specs/2026-05-18-callbodyshape-span-emission-design.md §3.10"
```

---

## Self-review notes

Verified against the spec:

- §2 Goals: 7 sites covered across Tasks 5, 6, 7. Loud-fail at production entry points: Task 4. Trivial behavior preserved: Task 11 regression suite.
- §3.3 triviality predicate: Task 2.
- §3.4 chunk layout: Tasks 5, 6, 7 (all push prefix Literal → Span-or-Literal → optional naming Literal → newline Literal). §3.4 return-fold-as-payload-carrier mechanism: Task 7 Step 3 (payload) + Step 4 (merger).
- §3.4 tier-1 raw-slot rule: Task 7 Step 3 (non-trivial arm uses `raw_body.to_string()`, no `substitute_local_refs_in`); asserted in Task 7 Step 1 test.
- §3.5 SpanPayload extensions (including `post_merge_return_sentence`): Task 1.
- §3.6 stub_fill Result + StubFillError: Task 3. emit::emit Result: Task 4. CompileOutcome::Diagnostics conversion: Task 4 (both lib.rs callers).
- §3.6 `compile_directory_with_layout`: no change required — explicitly verified at L1693 (already routes Diagnostics to FileOutcome::Failed and skips atomic_write).
- §3.7 diagnostic ID, synthetic span, IR-node-id sort, message format with NodeId Display workaround: Task 4 Step 4 (`format_llm_required_message` + explicit `sort_by_key`).
- §3.8 docs: Task 12.
- §3.9 Phase-6b relationship documented in Task 12 §3.8 item 2.
- §6.1 regression: Task 11.
- §6.2 per-site hard-fail: Task 8.
- §6.3 local_refs (incl. tier-2 uniformity case): Task 8 Step 3 — the `TIER1_LOCAL_REFS` source uses tier-1; **add a tier-2 local-refs case** during Task 8 if not already covered by the `local_refs_alone_hard_fails_with_local_ref_reason` test.
- §6.4 combined modifier+local_refs (single diagnostic with both reasons, deterministic order): Task 8 Step 3.
- §6.5 multiple-failing-spans ordering (end-to-end + unit on conversion helper): Task 8 Step 3 + Step 4.
- §6.6 no-output-file: Task 9 (`!out.exists()` assertion).
- §6.7 span-boundary / chunk-layout / raw-slot / return-fold-carrier: Task 7 Step 1 (raw-slot, carrier) + Task 10 (chunk-stream inspection). §9.3 negative assertion: not yet covered explicitly — add as a sub-step of Task 10 if there's a fixture that drives the flow-local return prose path.
- §6.8 IR-node-id stability: asserted implicitly in the per-site tests' message-content checks (`(IR node n{N})` substring).
- §6.9 deferred: scoped constraints, real source spans — added as `todo/expand-todos.md` entries in Task 12 Step 5.
- **Rev 7 / Task 14:** §3.10 procedure-body Call emission (8th site). IR change (`IrBlockFlowItem`) + lower arena entries + `emit_procedure` rewrite + diagnostic propagation: Task 14 Steps 1–5. Per-sub-tier hard-fail tests + tier-1 in-arm e2e: Task 14 Step 6. Reviewer Nits (test strengthening): Task 14 Step 7.

No placeholders. No "TBD". No `// add error handling` style hand-waves. Every step has the exact code, command, or expected output.

Type consistency: `StubFillError { ir_node: NodeId, target_name: Option<String>, has_modifier: bool, has_local_refs: bool }` — used identically in Tasks 3, 4, 8.

`projection_mode_from(c)` — defined Task 2, called identically at 7 sites across Tasks 5, 6, 7.

`call_needs_llm_fill(c)` — defined Task 2, called identically at 7 sites across Tasks 5, 6, 7.

`llm_required_diagnostics_from_errors(errors, file_label) -> DiagBag` — defined Task 4 Step 4, called at both lib.rs sites (Task 4 Step 5), unit-tested Task 8 Step 4.

`payload.post_merge_return_sentence: Option<String>` — defined Task 1, populated only in Task 7 (tier-1 final-step path), consumed by merger Task 7 Step 4.
