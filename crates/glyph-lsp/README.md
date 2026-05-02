# glyph-lsp

A `tower-lsp`-based language server that wraps `glyph-core`'s Phase 1 (Parse) +
Phase 2 (Analyze) phases and republishes the resulting `DiagBag` as LSP
`publishDiagnostics` notifications.

This is the **M1** milestone (diagnostics-only). Go-to-definition and other
features are M2/M3. See [`design/glyph-lsp.md`](../../design/glyph-lsp.md) for
the full design.

## What v1 does

- **Lifecycle:** `initialize`, `initialized`, `shutdown`, `exit`
- **Document sync:** `didOpen`, `didChange`, `didClose`, `didSave` (Full text sync)
- **Diagnostics:** republished on `didOpen` and `didSave` (save-only, per design §10.C)

`didChange` updates the in-memory buffer text but does **not** re-lint. The next
`didSave` runs the analyzer and publishes diagnostics. `didClose` clears any
previously published diagnostics for the buffer.

The LSP introduces no new compiler behaviour; it republishes the same
diagnostics `glyph check` would emit, in LSP shape.

## What v1 does not do

- `textDocument/definition` (M2)
- Cross-file import-aware analysis (M3)
- Hover, completion, formatting, code actions, rename, references, etc.

## Build and install

From the repo root:

```bash
cargo build --release -p glyph-cli
```

This produces `target/release/glyph` with the `lsp` subcommand.

To install the binary on your `PATH`:

```bash
cargo install --path crates/glyph-cli
```

The LSP is invoked as `glyph lsp`. There is also a standalone binary
`glyph-lsp` produced from this crate, but the recommended invocation is the
subcommand (matches the editor configuration in §9 of the design doc).

## Neovim setup (verified target)

This is the configuration that the M1 verification is performed against. Drop
the snippet into your `init.lua` (or a config file under `lua/plugins/`):

```lua
local lspconfig = require("lspconfig")
local configs = require("lspconfig.configs")

if not configs.glyph then
  configs.glyph = {
    default_config = {
      cmd = { "glyph", "lsp" },
      filetypes = { "glyph" },
      root_dir = lspconfig.util.root_pattern(
        ".git",
        "Cargo.toml",
        "*.glyph.md"
      ),
      single_file_support = true,
      init_options = {
        enableEffects = false, -- flip to true to enable the effects: subsystem
      },
      settings = {},
    },
  }
end

lspconfig.glyph.setup({})

vim.filetype.add({
  extension = {
    ["glyph.md"] = "glyph",
  },
  pattern = {
    [".*%.glyph%.md"] = "glyph",
  },
})
```

### Verification procedure

1. Build and install: `cargo install --path crates/glyph-cli`
2. Confirm `glyph lsp` is on `PATH`: `which glyph && glyph --help | grep lsp`
3. Drop the snippet above into `~/.config/nvim/lua/plugins/glyph.lua`
   (or your existing LSP config file).
4. Open a `.glyph.md` file in nvim (e.g. one of the corpus fixtures under
   `crates/glyph-cli/tests/corpus/invalid/`).
5. Run `:LspInfo`. Expect to see `glyph` listed as attached, with `cmd`
   resolving to your `glyph` binary.
6. Introduce a parse error (e.g. `if foo.applies` instead of
   `if foo.applies()`) and **save** the buffer. The squiggle should appear on
   save (not on edit — M1 is save-only).
7. `:lua vim.diagnostic.open_float()` shows the diagnostic, including its
   `G::parse::*` or `G::analyze::*` code.
8. Save the buffer with the error fixed; the squiggle clears on the next save.
9. `:bd` the buffer. Diagnostics for that URI are cleared.

## Smoke test (no editor required)

You can verify the JSON-RPC handshake without an editor. The script below pipes
`initialize` / `initialized` / `didOpen` / `shutdown` / `exit` through
`stdin`/`stdout` and prints the responses.

```bash
build_msg() {
  local body="$1"
  local len=${#body}
  printf 'Content-Length: %d\r\n\r\n%s' "$len" "$body"
}

INIT='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"rootUri":null,"capabilities":{}}}'
INITED='{"jsonrpc":"2.0","method":"initialized","params":{}}'
SHUTDOWN='{"jsonrpc":"2.0","id":2,"method":"shutdown"}'
EXIT='{"jsonrpc":"2.0","method":"exit"}'

{
  build_msg "$INIT";     sleep 0.3
  build_msg "$INITED";   sleep 0.1
  build_msg "$SHUTDOWN"; sleep 0.1
  build_msg "$EXIT";     sleep 0.1
} | glyph lsp
```

Expect:

- `initialize` → `result.capabilities.textDocumentSync == 1` (Full)
- `initialized` → `window/logMessage` notification
- `shutdown` → `result: null`
- `exit` → server terminates with exit code 0

> Note: the `sleep` between messages is important. tower-lsp cancels in-flight
> requests when stdin closes, so piping all four messages instantly causes
> `initialize` to fail with `-32800 Canceled`. This is a quirk of synthetic
> stdin testing — real editors keep stdin open for the lifetime of the
> session, so they don't hit it.
>
> Also note: `shutdown` and `exit` must not include `"params": null`.
> tower-lsp 0.20 strict-rejects that with `-32602 Unexpected params: null`.
> Real editors omit the `params` field entirely (LSP spec for parameterless
> requests/notifications), so this is also a synthetic-test-only concern.

## Architecture

All state lives in `Backend`, dispatched on by `tower-lsp`. The state shape —
`Arc<RwLock<HashMap<Url, Document>>>` for open buffers — comes from
`design/glyph-lsp.md` §5.

Diagnostic conversion is in `convert.rs`. It is the only place where the
inclusive-vs-exclusive end-column gotcha (design §10.B) is handled, and it
carries a unit test pinning that behaviour. glyph-core uses 1-indexed inclusive
end columns; LSP uses 0-indexed exclusive end columns. For end columns the +1
(inclusive→exclusive) and the −1 (1-indexed→0-indexed) cancel exactly, so the
column passes through unchanged. Lines do get the −1 adjustment.

## License

MIT OR Apache-2.0 (matches the workspace).
