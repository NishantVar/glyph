# Glyph for Neovim

End-to-end syntax highlighting for `.glyph` files in Neovim, via
[nvim-treesitter](https://github.com/nvim-treesitter/nvim-treesitter).

## Install

### 1. Register the parser

Add the snippet below to your Neovim config (e.g. `init.lua` or a
plugin setup file). It tells nvim-treesitter where to find this
parser and the `glyph` filetype it handles. Replace
`<path-to-tree-sitter-glyph>` with the absolute path to the
checkout (or a `git` URL if you publish the grammar later).

```lua
local parser_config = require("nvim-treesitter.parsers").get_parser_configs()

parser_config.glyph = {
  install_info = {
    url = "<path-to-tree-sitter-glyph>", -- local path or git URL
    files = { "src/parser.c", "src/scanner.c" },
    branch = "main",
    generate_requires_npm = false,
    requires_generate_from_grammar = false,
  },
  filetype = "glyph",
}
```

### 2. Detect `.glyph` as the `glyph` filetype

Drop `ftdetect/glyph.lua` (provided in this directory) into your
runtimepath, or copy its contents:

```lua
vim.filetype.add({
  extension = {
    glyph = "glyph",
  },
})
```

### 3. Install the parser

Restart Neovim and run:

```vim
:TSInstall glyph
```

`nvim-treesitter` clones the configured `url`, runs the C compiler
on `src/parser.c` + `src/scanner.c`, and drops the resulting `.so`
into your runtime parser directory. This step needs a working C
toolchain (`cc`/`clang`/`gcc`) on `$PATH`.

### 4. Enable highlighting and the locals module

Either rely on nvim-treesitter's defaults, or set them explicitly:

```lua
require("nvim-treesitter.configs").setup({
  ensure_installed = { "glyph" },
  highlight = { enable = true },
  -- Scope-aware features (rename, dim unused locals, etc.)
  -- read queries/locals.scm.
  refactor = {
    highlight_definitions = { enable = true },
  },
})
```

The grammar's `queries/` directory ships:

- `highlights.scm` — colors for keywords, identifiers, strings,
  constraint markers, etc.
- `locals.scm` — parameter and text-binding scope tracking.
- `injections.scm` — empty (Glyph files contain no other languages).

## Verify

Open any file under `crates/glyph-cli/tests/corpus/valid/`. You
should see colored declaration keywords (`skill`, `block`,
`text`), distinct coloring for constraint markers (`require`,
`avoid`, `must`, `must avoid`) versus the `context` marker, and
highlighted interpolation slots inside instruction strings.

`:InspectTree` (Neovim 0.9+) renders the parsed tree-sitter AST
for the current buffer — useful for confirming the parser is
loaded.

## Troubleshooting

- **`No parser for filetype glyph`**: the `ftdetect` step did not
  run, or the parser failed to install. Check `:checkhealth
  nvim-treesitter`.
- **`scanner.c: command not found`**: nvim-treesitter could not
  invoke a C compiler. Install Xcode CLI tools or `gcc`.
- **Highlighting works but locals do not**: the `refactor` module
  is opt-in; enable it as shown above.
