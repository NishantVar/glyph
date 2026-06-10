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
