# Expand — Outstanding Work

Deferred Phase 6b and Expand-related items extracted from [[docs/architecture/expand]]. These are work-tracking notes, not design decisions.

## Phase 6b Validation — Deferred Checks

From [[docs/architecture/expand]] §4.3:

- **Full Markdown well-formedness via a real Markdown parser.** Today Phase 6b uses lightweight structural checks plus `G::expand::malformed-markdown`. A pull-parser-based pass would catch a broader class of malformation but is not required for MVP.
- **No-embedded-HTML scan.** A scan rejecting raw HTML in Step 2 output is deferred. False-positive risk on legitimate constraint prose mentioning HTML tags is real, and consuming LLMs treat the file as text. Worth revisiting if HTML leakage shows up in practice.
- **Predicate-framing verbatim check.** From [[docs/architecture/expand]] §3.3: the pure-predicate Branch framing sentences (`Decide whether <…> applies and, if so:` / `Decide which of the following applies and follow only that path:` / `Otherwise:`) are not checked for verbatim match today. If drift becomes a problem, add a structural check keyed on `resolved_predicates` shape.

## Phase 6b — `step-order-mismatch` ↔ `unresolved-local-ref` Mutual Exclusion

Two Phase 6b validators are mutually unsatisfiable when an instruction string's first 3 whitespace-separated tokens contain a `{param}` local ref.

- `G::expand::step-order-mismatch` builds its required substring from the IR's first 3 whitespace-split tokens of `resolved_body_text` and requires that substring to appear in the corresponding compiled `## Steps` step. If a `{param}` sits in those first 3 tokens, the substring includes the literal `{param}` token.
- `G::expand::unresolved-local-ref` forbids any unresolved `{param}` from surviving into the compiled output — local refs must be rewritten as natural-language cross-references.

When both apply, the expand pass cannot satisfy them together: keeping the literal `{param}` to pass step-order-mismatch trips unresolved-local-ref, and resolving the ref trips step-order-mismatch.

**Reproducer.** `block squash_commits_in_worktree(worktree)` with body `"In {worktree}, squash the issue-by-issue commits into a small meaningful set, …"` — first 3 tokens are `In`, `{worktree},`, `squash`.

**Author-side workaround.** Rewrite the instruction so no `{param}` lands in the first 3 tokens (e.g., `"Squash the issue-by-issue commits inside {worktree} into …"`). Brittle — authors don't think in IR-token windows.

**Fix direction.** Either (a) build the step-order-mismatch required substring after local-ref resolution (skip `{param}` tokens when picking the substring), or (b) record the resolved natural-language equivalent of each local ref in the IR so the step-order check can match against either form.

## Step 2 — Open Questions

(None currently extracted beyond the deferred 6b checks above. Add new TODOs here as they come up.)

---

- **Scoped constraints on `IrCall`.** Lower callee constraints into a new `IrCall.scoped_constraints` field; serialize via `emit_ir.rs` (today hardcoded to `[]`); extend the §3.3 triviality predicate in `crates/glyph-core/src/emit/scaffold.rs::call_needs_llm_fill` with `|| !c.scoped_constraints.is_empty()`; extend `SpanPayload` and `StubFillError` accordingly; reuse the span-emission machinery from the 2026-05-18 CallBodyShape spec.
- **Real source spans on `IrCall`.** Thread a `SourceSpan` (or byte-offset pair) through `IrCall` from parser → lower → IR so `G::expand::llm-required-for-call` can carry a real source span instead of the synthetic zero-width file-level span the CallBodyShape spec ships with.
