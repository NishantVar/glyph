# Glyph for VS Code

VS Code extension that wraps the `glyph lsp` language server. Provides
diagnostics, go-to-definition (same-file + cross-file), and semantic-token
highlighting for `.glyph` files.

This is a thin client. All language behaviour ships from `glyph-lsp` so VS
Code, Neovim, and any other LSP-aware editor share the same logic.

## What ships today (M3)

- **Diagnostics** — published on `didOpen` and `didSave` (save-only, per
  design §10.C). Covers Phase 1 (Parse) + Phase 2 (Analyze). Includes
  cross-file diagnostics for imported files (M3 Phase A): if a `.glyph`
  file you save imports a dep that has its own diagnostics, they appear
  on the dep's tab/path.
- **Go-to-definition** — `gd` / F12 jumps to `block`, `text`,
  `export block` declarations and `{param}` slots in flow inline
  strings. Cross-file imports follow the
  `import "./<rel>.glyph" { name }` clause.
- **Semantic-token highlighting** — `textDocument/semanticTokens/full`,
  legend matched to the tree-sitter grammar so the highlighting stays
  consistent across editors. Token types: keyword, type, function,
  method, parameter, variable, property, string, namespace, number,
  comment.

## Prerequisites

The extension launches the `glyph` binary, so you need it on `PATH`:

```bash
cargo install --path crates/glyph-cli   # from the repo root
which glyph                              # confirm it resolves
```

If `glyph` lives elsewhere, set `glyph.serverPath` in your VS Code
settings to an absolute path.

## Develop and test (F5 flow)

1. Install dependencies and compile:

   ```bash
   cd editors/vscode
   npm install
   npm run compile   # writes ./out/extension.js
   ```

2. Open `editors/vscode/` as a workspace in VS Code, then press **F5**.
   This launches a second VS Code window ("Extension Development Host")
   with the extension loaded.

3. In the dev host, open any `.glyph` file (e.g.
   `crates/glyph-cli/tests/corpus/valid/imports/fix_bug.glyph`).
   Expect:
   - Syntax colors appear (driven by semantic tokens).
   - Save the file: any `G::parse::*` / `G::analyze::*` diagnostics
     appear in the Problems panel.
   - F12 / right-click → Go to Definition jumps to the declaration,
     including across imports.

4. Tail the LSP traffic if something looks off: set
   `"glyph.trace.server": "verbose"` in the dev-host settings, then
   open **Output → Glyph Language Server**.

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `glyph.serverPath` | `glyph` | Path to the glyph CLI binary; the extension launches `<serverPath> lsp`. |
| `glyph.enableEffects` | `false` | Enable the gated `effects:` subsystem (matches `glyph --enable-effects`). |
| `glyph.trace.server` | `off` | `off` / `messages` / `verbose` — JSON-RPC tracing for debugging. |

## Packaging

To produce a `.vsix` you can install with `code --install-extension`:

```bash
cd editors/vscode
npm install -g @vscode/vsce
vsce package
```

This is not part of CI yet — packaging instructions are here so you can
hand a build to a teammate without publishing.

## License

MIT OR Apache-2.0 (matches the workspace).
