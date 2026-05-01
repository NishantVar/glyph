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
  `languages/glyph/injections.scm` — **symlinked** to the
  grammar's top-level `queries/` directory so the extension
  layout exposes the files at the paths Zed reads from while
  preserving a single source of truth. Edit the top-level
  `queries/*.scm`; the symlinks pick up changes automatically.
  When packaging the extension archive, dereference the
  symlinks (`cp -L`) so the bundle carries real files:

```sh
cp -L queries/*.scm editors/zed/languages/glyph/
```

## Why this is only a scaffold

Zed extensions are distributed via the Zed extension index, which
requires a published Rust crate, a signed extension archive, and
CI verification through Zed's review pipeline. None of those
have been done in M3 — only the on-disk layout is in place.

## Next steps

1. Submit the extension to Zed's index per
   [Zed's extension docs](https://zed.dev/docs/extensions). The
   queries are already in place via symlinks; the packager just
   needs to dereference them when archiving.
2. Once published, users install via `zed: install extension`.
