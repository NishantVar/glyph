# Expand — Outstanding Work

Deferred Phase 6b and Expand-related items extracted from [[docs/architecture/expand]]. These are work-tracking notes, not design decisions.

## Phase 6b Validation — Deferred Checks

From [[docs/architecture/expand]] §4.3:

- **Full Markdown well-formedness via a real Markdown parser.** Today Phase 6b uses lightweight structural checks plus `G::expand::malformed-markdown`. A pull-parser-based pass would catch a broader class of malformation but is not required for MVP.
- **No-embedded-HTML scan.** A scan rejecting raw HTML in Step 2 output is deferred. False-positive risk on legitimate constraint prose mentioning HTML tags is real, and consuming LLMs treat the file as text. Worth revisiting if HTML leakage shows up in practice.
- **Predicate-framing verbatim check.** From [[docs/architecture/expand]] §3.3: the pure-predicate Branch framing sentences (`Decide whether <…> applies and, if so:` / `Decide which of the following applies and follow only that path:` / `Otherwise:`) are not checked for verbatim match today. If drift becomes a problem, add a structural check keyed on `resolved_predicates` shape.

## Step 2 — Open Questions

(None currently extracted beyond the deferred 6b checks above. Add new TODOs here as they come up.)

---

- **Scoped constraints on `IrCall`.** Lower callee constraints into a new `IrCall.scoped_constraints` field; serialize via `emit_ir.rs` (today hardcoded to `[]`); extend the §3.3 triviality predicate in `crates/glyph-core/src/emit/scaffold.rs::call_needs_llm_fill` with `|| !c.scoped_constraints.is_empty()`; extend `SpanPayload` and `StubFillError` accordingly; reuse the span-emission machinery from the 2026-05-18 CallBodyShape spec.
- **Real source spans on `IrCall`.** Thread a `SourceSpan` (or byte-offset pair) through `IrCall` from parser → lower → IR so `G::expand::llm-required-for-call` can carry a real source span instead of the synthetic zero-width file-level span the CallBodyShape spec ships with.
