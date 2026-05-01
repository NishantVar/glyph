# Glyph for Zed (scaffold)

Status: **scaffold**, not tested in M3. This directory mirrors
the layout Zed expects for community-contributed language
extensions.

Zed loads tree-sitter grammars from a published extension
manifest. The mapping looks like:

- `extension.toml` — extension metadata, declares the language.
- `languages/glyph/config.toml` — language config (comment style,
  brackets, file extensions, grammar reference).
- `languages/glyph/highlights.scm`,
  `languages/glyph/locals.scm`,
  `languages/glyph/injections.scm` — copied (or symlinked) from
  the grammar's `queries/` directory.

## Files in this directory

- [`extension.toml`](extension.toml) — manifest stub.
- [`languages/glyph/config.toml`](languages/glyph/config.toml) —
  language config.
- `languages/glyph/highlights.scm`,
  `languages/glyph/locals.scm`,
  `languages/glyph/injections.scm` — copied from the grammar's
  `queries/` directory at extension-build time. Until the
  extension is built, these are intentionally absent here; copy
  them in (or symlink) when packaging:

```sh
cp queries/*.scm editors/zed/languages/glyph/
```

## Why this is only a scaffold

Zed extensions are distributed via the Zed extension index, which
requires a published Rust crate, a signed extension archive, and
CI verification through Zed's review pipeline. None of those
have been done in M3 — only the on-disk layout is in place.

## Next steps

1. Copy the queries (`cp queries/*.scm editors/zed/languages/glyph/`).
2. Submit the extension to Zed's index per
   [Zed's extension docs](https://zed.dev/docs/extensions).
3. Once published, users install via `zed: install extension`.
