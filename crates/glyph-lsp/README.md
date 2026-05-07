# glyph-lsp

A `tower-lsp`-based language server that wraps `glyph-core`'s Phase 1 (Parse) +
Phase 2 (Analyze) phases and republishes the resulting `DiagBag` as LSP
`publishDiagnostics` notifications. M2 added `textDocument/definition`
(same-file and cross-file) over the resolution table that
`glyph_core::analyze::analyze_with_resolutions` exposes. M3 extends
diagnostic publishing to cover imported deps and adds semantic-token
highlighting via `textDocument/semanticTokens/full`.

See [`design/glyph-lsp.md`](../../design/glyph-lsp.md) for the full design.

## What ships today

- **Lifecycle:** `initialize`, `initialized`, `shutdown`, `exit`
- **Document sync:** `didOpen`, `didChange`, `didClose`, `didSave` (Full text sync)
- **Diagnostics:** republished on `didOpen` and `didSave` (save-only, per design §10.C)
- **Cross-file diagnostics (M3 Phase A):** when an open buffer's analyze
  walk visits an imported dep, that dep's diagnostics are published on
  the dep's `file://` URI. If the import set changes between saves, the
  previously-published dep URIs get a clearing publish.
- **Go-to-definition (M2):** jumps to `block` / `text` / `export block`
  declarations and `{param}` slots in flow inline strings. Cross-file
  imports follow the `import "./<rel>.glyph" { name }` clause and jump
  to the declaration in the imported file. Stdlib targets (`subagent`,
  `send`) and unresolvable identifiers return `null`.
- **Semantic tokens (M3 Phase B):** `textDocument/semanticTokens/full`.
  Legend matches the tree-sitter `highlights.scm` grammar so VS Code,
  Neovim, and Helix colorize Glyph identically. Token types: keyword,
  type, function, method, parameter, variable, property, string,
  namespace, number, comment.

`didChange` updates the in-memory buffer text but does **not** re-lint. The next
`didSave` runs the analyzer and publishes diagnostics. `didClose` clears any
previously published diagnostics for the buffer (and any dep diagnostics
that were attributed to it).

The LSP introduces no new compiler behaviour; it republishes the same
diagnostics `glyph check` would emit, in LSP shape.

## What is not yet implemented

- FileGraph-style cross-buffer dep cache (M3 design §378 sharpening —
  deferred per the M3 brief; currently every save re-walks imports).
- `semanticTokens/range` and `semanticTokens/full/delta` (the `full`
  request alone is fast enough for our file sizes).
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
        "*.glyph"
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
    ["glyph"] = "glyph",
  },
})
```

### Verification procedure

1. Build and install: `cargo install --path crates/glyph-cli`
2. Confirm `glyph lsp` is on `PATH`: `which glyph && glyph --help | grep lsp`
3. Drop the snippet above into `~/.config/nvim/lua/plugins/glyph.lua`
   (or your existing LSP config file).
4. Open a `.glyph` file in nvim (e.g. one of the corpus fixtures under
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

### Go-to-definition (M2)

With the same setup:

1. Open a file containing a `block` declaration plus a call site, e.g.:

   ```
   skill main()
       description: "main."
       require accuracy
       flow:
           validate_plan()

   block validate_plan()
       "Check the plan."

   text accuracy = "Be accurate."
   ```

2. Place the cursor on `validate_plan` inside the `flow:` call and press
   `gd` (or run `:lua vim.lsp.buf.definition()`). The cursor should jump
   to the `block validate_plan` line.
3. Place the cursor on `accuracy` after `require` and press `gd` — jumps
   to the `text accuracy = ...` declaration.
4. With a parameter slot like `"Inspect {scope} for issues."` in `flow:`
   and `skill main(scope = ".")`, place the cursor inside `{scope}` and
   press `gd` — jumps to the `scope = "."` parameter in the header.
5. Open `crates/glyph-cli/tests/corpus/valid/imports/fix_bug.glyph` —
   place the cursor inside the `inspect_repo` call in the flow block and
   press `gd`. The cursor should jump to the `export block inspect_repo`
   declaration in the sibling file `repo_tools.glyph` (cross-file).
6. On a stdlib reference (`subagent` imported via `@glyph/std`) or any
   unresolvable name, `gd` reports "no definition found" — the LSP
   returns `null` (per design §10.D).

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
