# BUG-073: did_change silently drops incremental edits and the 'didSave will resync' claim is false (stale buffer text)

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-lsp/src/lib.rs:312-337`
**Found by:** lsp-lib | **Audit date:** unknown-date

## Description

Under advertised FULL sync, `did_change` ignores any change event whose `range` is `Some` (incremental). The comment at 316-317 asserts "The next didSave will resync from the editor's authoritative copy." This is not true: `did_save` (321-337) prefers the cached `doc.text` and only falls back to `params.text` if the cache is missing; and `params.text` is `None` anyway because the server never requested `include_text` (no save options advertised). So if a non-conformant client ever sends an incremental edit (the very case this branch is written to defend against), the dropped edit is never recovered — `did_save` re-lints the stale cached text, producing diagnostics that do not correspond to the file's real content. The fallback path the code intends as a safety net does not actually resync.

## Trigger / Reproduction

A non-conformant LSP client sends an incremental change event (`range` is `Some`) despite the server advertising `TextDocumentSyncCapability::Kind(FULL)`. `did_change` silently drops the edit (keeping stale cached text). On the next save, `did_save` re-lints the cached stale text rather than the editor's real content because `params.text` is `None` (save options with `include_text: true` were never advertised).

## Evidence

```rust
// did_change (lines 312-318): incremental edits silently dropped
for change in params.content_changes {
    if change.range.is_none() {
        doc.text = change.text;
    }
    // Else: incremental edit under FULL sync — ignored.
    // The next didSave will resync from the editor's authoritative copy.
}

// did_save (lines 327-334): prefers cached doc.text; params.text is None
// (include_text not requested — no SaveOptions advertised at initialize)
let text = match (doc_text_cache, params.text) {
    (Some(t), _) => t,         // always takes cached (potentially stale) text
    (None, Some(t)) => t,      // unreachable: params.text is always None
    (None, None) => return,
};
```

## Recommended Resolution

Either request `include_text` on save and have `did_save` prefer `params.text` when present, or apply incremental changes properly in `did_change` instead of dropping them, or correct the comment to reflect that incremental edits under FULL sync are unrecoverable. Advertising save options with `include_text: true` (see related finding) also fixes the resync claim.

## Verification Notes

The initialize handler advertises only `TextDocumentSyncCapability::Kind(FULL)` — not a `TextDocumentSyncOptions` struct — so no `SaveOptions` and no `include_text: true` are ever set. The `did_save` handler explicitly prefers `doc.text` via `(Some(t), _) => t`, meaning the promised resync path is unreachable for conformant and non-conformant clients alike. Only affects non-conformant clients that send incremental changes despite FULL sync being advertised — any LSP-conformant client (Neovim, VS Code) sends full document text, so this dead-code path is never reached in practice, justifying low severity.
