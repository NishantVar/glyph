# Glyph for VS Code (scaffold)

Status: **scaffold**, not tested in M3. Ship-ready VS Code support
needs a published `.vsix` bundle with the parser compiled to WASM
(via `tree-sitter build --wasm`) and a host extension that registers
the language with `vscode-tree-sitter` (or the upcoming first-party
tree-sitter API — Microsoft is in the process of adding one).

This directory contains the manifest and language-configuration
stubs an extension would need. The next steps to make it work are
listed at the bottom.

## Files

- [`package.json`](package.json) — VS Code extension manifest.
  Declares the `glyph` language, the `.glyph` file association,
  and pulls in `language-configuration.json`.
- [`language-configuration.json`](language-configuration.json) —
  comment markers, bracket pairs, auto-closing pairs. This is the
  TextMate-fallback story; even without tree-sitter, VS Code uses
  this file for indent and bracket behavior.
- [`syntaxes/glyph.tmLanguage.json`](syntaxes/glyph.tmLanguage.json) —
  intentionally absent. Without a full TextMate grammar, VS Code
  shows files unhighlighted. Adding one is the largest piece of
  follow-up work; see "Next steps" below.

## Why this is only a scaffold

VS Code consumes tree-sitter grammars via WebAssembly, not native
`.so` libraries. That means a working extension needs:

1. `tree-sitter build --wasm` to produce `glyph.wasm`.
2. The extension host to load `glyph.wasm` via the
   `vscode-tree-sitter` library or the first-party API.
3. Highlight token translation from tree-sitter capture names
   (e.g. `@keyword.directive`) to VS Code semantic-token kinds
   (e.g. `decorator`).
4. Marketplace publishing (`vsce publish`).

Each of those is straightforward but adds a meaningful layer of
ceremony — bundling, signing, semantic-token mapping, version
pinning to the VS Code API. The M3 scope was "scaffold one
editor end-to-end and stub the rest"; nvim got the end-to-end
slot. VS Code is staged for follow-up.

## Next steps

1. `tree-sitter build --wasm` from the grammar root. Drop the
   resulting `glyph.wasm` into this directory.
2. Add a `src/extension.ts` that:
   - imports `vscode-tree-sitter` (or the first-party tree-sitter
     API);
   - registers `glyph.wasm` with the host;
   - publishes a semantic-tokens provider that runs the
     `queries/highlights.scm` query and maps captures to VS Code
     token kinds.
3. Run `vsce package` to produce a `.vsix` and side-load via
   `Extensions: Install from VSIX...`.
4. Once stable, publish to the Marketplace.

## Quick start (local side-load, partial functionality)

Even without tree-sitter wired up, the bracket-matching and
comment-toggle behavior works after side-loading:

```sh
cd editors/vscode
npm install
npx vsce package
code --install-extension glyph-vscode-0.1.0.vsix
```

Open any `.glyph` file. Comments and brackets behave; coloring is
plain text until a TextMate grammar or the tree-sitter wiring above
is added.
