# Glyph for Helix (scaffold)

Status: **scaffold**, not tested in M3. The on-disk layout mirrors
Helix's runtime conventions; pairing it with a `languages.toml`
entry in the user's config is enough to get tree-sitter
highlighting working once the parser is built locally.

## Files in this directory

- [`runtime/queries/glyph/`](runtime/queries/glyph/) — drop-in
  queries directory. The three query files
  (`highlights.scm`, `locals.scm`, `injections.scm`) are
  **symlinked** to the grammar's top-level `queries/` directory,
  so a single source of truth is preserved while Helix sees
  files at the path it expects. Treat the top-level
  `queries/*.scm` as the canonical files to edit; the symlinks
  pick up changes automatically.
- [`languages.toml`](languages.toml) — user-config snippet to
  append to `~/.config/helix/languages.toml`. Declares the
  `glyph` language, file extension, comment marker, and
  grammar source.

## Install (once packaged)

1. Copy the queries (symlinks resolved to real files):
   ```sh
   mkdir -p ~/.config/helix/runtime/queries/glyph
   cp -L editors/helix/runtime/queries/glyph/*.scm \
        ~/.config/helix/runtime/queries/glyph/
   ```
2. Append `editors/helix/languages.toml` to your
   `~/.config/helix/languages.toml`.
3. Build the parser:
   ```sh
   hx --grammar fetch
   hx --grammar build
   ```
4. Open a `.glyph` file in Helix.

## Why this is only a scaffold

Helix grammar fetch expects a published git URL with a tagged
commit. The `[language.grammar]` block in `languages.toml` uses
a `PLACEHOLDER` URL — replace it with the real grammar
repository before running `hx --grammar fetch`. No upstream PR
or marketplace step is required for Helix; users install
language support directly through the editor.
