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

## Independent Agent Finding

**Verdict:** Reproduced. The report is correct: a ranged `textDocument/didChange` payload is ignored under the advertised FULL sync path, and a subsequent `textDocument/didSave` without `text` re-lints the stale cached buffer rather than the editor's real content.

**Reproduction/Refutation:** I reproduced this against the real `target/debug/glyph-lsp` process over JSON-RPC, without editing source. The script opened a buffer with invalid text (`skill main(\n`), then sent a non-conformant ranged change whose replacement text was valid Glyph, then sent `didSave` with no `text` field. Diagnostics stayed stale. Sending the same valid text as a full-change event (`range == None`) and saving immediately cleared diagnostics, isolating the stale behavior to the ranged-change branch.

**Evidence:**

- Graphify first-pass context query for LSP document sync pointed at the LSP document state in `crates/glyph-lsp/src/lib.rs`.
- Bounded source read of `crates/glyph-lsp/src/lib.rs:216-337` confirmed:
  - `initialize` advertises `text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL))`, which serializes as bare `1`.
  - `did_change` only assigns `doc.text = change.text` when `change.range.is_none()`.
  - `did_save` matches `(Some(t), _) => t`, so cached text wins over any save payload, and there is no advertised `include_text` save option.
- `rg -n "SaveOptions|include_text|text_document_sync|did_change|did_save" crates/glyph-lsp/src/lib.rs` found no save-options capability and only the cited handlers/capability path.
- `cargo build -p glyph-lsp` passed.
- `cargo test -p glyph-lsp --lib` passed: `32 passed; 0 failed`.
- Live LSP repro output:

```json
{
  "advertised_textDocumentSync": 1,
  "after_open_bad_text": {
    "count": 1,
    "messages": ["expected identifier"]
  },
  "after_ranged_change_then_save_no_text": {
    "count": 1,
    "messages": ["expected identifier"]
  },
  "after_full_change_then_save_no_text": {
    "count": 0,
    "messages": []
  }
}
```

The temporary repro directory under `tmp/bug073-lsp-repro` was removed after the run.

**Resolution Input:** Preserve the existing suggested resolution. Either request `include_text` on save and have `did_save` prefer `params.text` when present, apply incremental changes properly in `did_change` instead of dropping them, or correct the comment to state that ranged/incremental edits under FULL sync are not recoverable through the current save path. Advertising save options with `include_text: true` also supports the intended resync behavior, but only if `did_save` gives the save payload priority over stale cached text.
