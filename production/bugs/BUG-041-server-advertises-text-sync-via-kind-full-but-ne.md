# BUG-041: Server advertises text sync via Kind(FULL) but never advertises `save`, so spec-conformant clients never send didSave and diagnostics go stale after open

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-lsp/src/lib.rs:226-235`
**Found by:** lsp-lib | **Audit date:** unknown-date

## Description

The design (`design/glyph-lsp.md` §Diagnostic Behaviour: "On didOpen and on didSave") and the `did_save` handler (lines 321-337) make save the trigger for re-linting. But `initialize` sets:

```
text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL))
```

The `Kind` form carries only the sync kind; it does NOT register save support (no `TextDocumentSyncOptions { save: ... }`), and there is no dynamic `register_capability` call anywhere. Per the LSP spec a client should only send `textDocument/didSave` when the server advertises save support. So a strictly-conformant client calls `did_open` once (publishing initial diagnostics) and then never calls `did_save` — meaning diagnostics never update for the lifetime of the buffer, even after the user fixes the error and saves. VS Code and Neovim happen to send `didSave` unconditionally so the verified path works, but the capabilities-vs-behavior contract is violated and the core feature silently breaks on conformant clients.

## Trigger / Reproduction

Open a `.glyph` file in a spec-conformant LSP client (one that only sends `textDocument/didSave` when the server advertises save support). Fix a diagnostic error in the file and save. The diagnostic does not refresh — it remains stale for the entire session.

## Evidence

```rust
// crates/glyph-lsp/src/lib.rs ~line 231
text_document_sync: Some(TextDocumentSyncCapability::Kind(
    TextDocumentSyncKind::FULL,
)),
// No TextDocumentSyncOptions { save: ... } anywhere.
// No dynamic register_capability for save.
// TextDocumentSyncOptions and TextDocumentSyncSaveOptions are never
// imported or used in the file.

// did_change handler (~line 298-319): only updates cached buffer text,
// does NOT re-lint.

// did_save handler (~line 321-337): sole re-lint trigger per design doc.
// Will never be called by a spec-conformant client given the above caps.
```

## Recommended Resolution

Advertise sync via the options form so save is registered:

```rust
TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
    change: Some(TextDocumentSyncKind::FULL),
    save: Some(TextDocumentSyncSaveOptions::Supported(true)),
    ..Default::default()
})
```

Add `TextDocumentSyncOptions` and `TextDocumentSyncSaveOptions` to the `lsp_types` imports.

## Verification Notes

The `Kind` form serializes to a bare integer on the wire with no `save` field, giving conformant clients no signal that `textDocument/didSave` is expected. The `did_change` handler explicitly does not re-lint (only caches buffer text), so `did_save` is the only re-lint path. VS Code and Neovim paper over this by sending `didSave` unconditionally; the defect is masked in practice on the two most common editors but violates the LSP 3.17 capability contract.

## Independent Agent Finding

**Verdict:** Reproduced.

**Reproduction/Refutation:** I reproduced the capability mismatch by source inspection and by serializing the exact `lsp_types` capability variant used by `initialize`. The server advertises `TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)`, which serializes as the bare JSON value `1`. The recommended options form serializes as `{"change":1,"save":true}`. Because `did_change` only updates the cached buffer text and `did_save` is the re-lint/publish path, a client that waits for `textDocumentSync.save` before sending `textDocument/didSave` will not trigger post-open diagnostic refresh.

**Evidence:**
- Graphify was queried first for `glyph-lsp` initialize/text sync/save/change diagnostics context; it did not return useful code-level nodes, so I used bounded `rg`/`sed` reads for exact implementation details.
- `rg -n "TextDocumentSyncOptions|TextDocumentSyncSaveOptions|register_capability|text_document_sync|did_save|did_change" crates/glyph-lsp/src/lib.rs` returned only:
  - `231: text_document_sync: Some(TextDocumentSyncCapability::Kind(`
  - `298: async fn did_change(...)`
  - `321: async fn did_save(...)`
  This confirms there is no source import/use of `TextDocumentSyncOptions`, `TextDocumentSyncSaveOptions`, or dynamic `register_capability` in `crates/glyph-lsp/src/lib.rs`.
- Bounded source reads confirmed `initialize` sets `TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)`, `did_change` says it does not re-lint, and `did_save` calls `self.lint_and_publish(uri, &text).await`.
- `rg -n "didOpen|didSave|Diagnostic Behaviour|diagnostics" design/glyph-lsp.md docs/architecture/lsp.md` confirmed the intended diagnostic behavior is on `didOpen` and `didSave`; `docs/architecture/lsp.md` also says v1 does not auto-republish on change.
- Scratch serialization check (`cargo run --manifest-path tmp/lsp-sync-serialization/Cargo.toml --quiet`) output:

  ```text
  kind=1
  options={"change":1,"save":true}
  ```

- The LSP 3.17 specification documents `textDocumentSync.save` as the server capability indicating interest in `textDocument/didSave` notifications: https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocument_didSave

**Resolution Input:** Preserve the existing suggested resolution. Advertise text sync with `TextDocumentSyncCapability::Options(TextDocumentSyncOptions { change: Some(TextDocumentSyncKind::FULL), save: Some(TextDocumentSyncSaveOptions::Supported(true)), ..Default::default() })` and import `TextDocumentSyncOptions` plus `TextDocumentSyncSaveOptions`. No source change was made in this reproduction pass.
