# BUG-040: lint_and_publish republishes diagnostics to a file that was concurrently closed (stale squiggles after close)

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-lsp/src/lib.rs:196-210`
**Found by:** lsp-lib | **Audit date:** unknown-date

## Description

`lint_and_publish` is a long async chain with several `.await` points (publish_diagnostics on deps at 173-175, the documents read/write locks, and blocking fs reads). The write-back that stamps the document is guarded with `if let Some(doc) = docs.get_mut(&uri)` (199-203), but the final `publish_diagnostics(uri, buffer_diagnostics, None)` at 208-210 — and the per-dep publishes at 173-175 — are UNCONDITIONAL.

tower-lsp 0.20 does not serialize handlers; a `did_close` notification can run during an in-flight `lint_and_publish` triggered by a preceding save. Concrete trigger: user hits Save then immediately closes the buffer. `did_close` (339-364) removes the Document and publishes an empty diagnostic array to clear squiggles; then the still-running lint finishes and re-publishes the computed buffer diagnostics (and any cross-file dep diagnostics) to the now-closed URI. The editor ends up showing diagnostics for a file that is no longer open, with no Document entry left to ever clear them.

The guarded write-back also means `last_published`/`last_dep_uris` are silently dropped while the publish still happens, so the dep diagnostics published at 173-175 are leaked (never tracked for future clearing): `stale_dep_clearing` at lines 181-191 reads via `unwrap_or_default()` from the already-removed document, returning empty, so cross-file dep diagnostics published at line 174 are never cleared.

## Trigger / Reproduction

1. Open a `.glyph` file in an LSP-connected editor.
2. Save the file (triggers `did_save` → `lint_and_publish`).
3. Immediately close the buffer before the lint completes.
4. Observe: diagnostic squiggles persist on the closed file; the editor has no mechanism to clear them since there is no longer a Document entry.

This is a routine "save then close" workflow pattern.

## Evidence

```rust
{
    let mut docs = self.documents.write().await;
    if let Some(doc) = docs.get_mut(&uri) { /* guarded */ }
}
self.client.publish_diagnostics(uri, buffer_diagnostics, None).await; // unconditional
```

The final `publish_diagnostics` call at line 209 executes regardless of whether the document still exists in `docs`. The per-dep publishes at lines 173-175 (inside the `for (path, bag) in bags.iter()` loop) are similarly unconditional.

## Recommended Resolution

Gate all publish calls on document existence. A complete fix must address both the buffer URI publish and the dep URI publishes:

(a) Check document existence (inside the same lock scope) before publishing any dep diagnostics (at line 173) and again before the final buffer publish (at line 209); or

(b) Use a per-URI generation counter (epoch) incremented on `did_close` and checked before each publish call inside `lint_and_publish` — if the epoch has advanced, abort the publish.

The partial fix of only guarding line 209 is insufficient: it leaves the unconditional dep URI publishes at lines 173-175 able to re-attach squiggles to dep files that `did_close` already cleared.

## Verification Notes

`lint_and_publish` publishes dep diagnostics unconditionally at line ~174, then stamps `last_dep_uris` only under a guarded write-lock at line ~201-205, then publishes buffer diagnostics unconditionally at line ~209. `did_close` (line 339) removes the Document from `docs` and publishes empties using the `last_dep_uris` that existed at close time. tower-lsp 0.20 does not serialize handlers; a concurrent save+close results in `did_close` clearing squiggles, then the still-running `lint_and_publish` re-publishing diagnostics to the now-closed URI. The dep diagnostic leak is also confirmed: dep URIs published at line ~174 after the document was removed are never added to `last_dep_uris` (guarded block returns None), so they are never cleared by any future operation.

## Independent Agent Finding

**Verdict:** Reproduced. The production bug report is valid.

**Reproduction/Refutation:** I drove the real `target/debug/glyph-lsp` over stdio with JSON-RPC, using a scratch fixture under `tmp/` containing `main.glyph` plus 80 imported dependency files. I first opened the file and waited for an initial non-empty `textDocument/publishDiagnostics` for `main.glyph`, then sent `textDocument/didSave` immediately followed by `textDocument/didClose`. The post-save/close notification stream contained an empty diagnostics publish for the closed `main.glyph`, followed later by a non-empty diagnostics publish for the same closed URI.

**Evidence:**

- Graphify first pass: `query_graph("lint publish diagnostics behavior CLI...")` only surfaced `crates/glyph-cli/src/main.rs` and did not contain enough LSP detail, so I used bounded source reads for exact code.
- Source check: `crates/glyph-lsp/src/lib.rs` still has unconditional dep publishes at lines 173-175, a guarded document stamp at lines 201-205, and an unconditional final buffer publish at lines 208-210. `did_close` removes the document at lines 341-347 and publishes empty diagnostics for the buffer/deps at lines 353-362.
- Concurrency check: tower-lsp 0.20 routes server tasks through `buffer_unordered(self.max_concurrency)` with default max concurrency 4, so notifications are not handler-serialized.
- Build command: `cargo build -q -p glyph-lsp` completed with exit 0.
- Live LSP repro command: inline Python JSON-RPC harness spawning `target/debug/glyph-lsp`; scratch directory `tmp/bug040-*` was removed afterward.
- Summarized repro output:

```text
NUM_DEPS 80
POST_SAVE_CLOSE_MAIN_COUNTS [0, 81]
POST_SAVE_CLOSE_DEP_NONEMPTY 80
POST_SAVE_CLOSE_DEP_EMPTY 80
EMPTY_THEN_NONEMPTY_MAIN True
POST_SAVE_CLOSE_SEQUENCE excerpt:
005 dt_ms=21.6 uri=main.glyph count=0 first_id=None
120 dt_ms=26.8 uri=main.glyph count=81 first_id=G::analyze::unused-import
```

The `main.glyph count=0` notification is the close-time clear. The later `main.glyph count=81` notification is the in-flight save lint publishing diagnostics after the document had been closed.

**Resolution Input:** Preserve the existing recommended resolution. A complete fix must gate both dep publishes and the final buffer publish, or use a per-URI epoch/generation check that aborts stale in-flight publishes after `did_close`. Guarding only the final buffer publish would still allow dep diagnostics to be reattached and lost from `last_dep_uris` tracking.
