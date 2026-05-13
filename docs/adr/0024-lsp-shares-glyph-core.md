# ADR 0024: LSP Depends on `glyph-core`, Not `glyph-cli`

## Status

Accepted.

## Context

The Glyph LSP needs to call the same Parse + Analyze pipeline the CLI
uses. Two structural choices:

1. **Depend on `glyph-core`** (the library crate that owns parse,
   analyze, lower, etc.) and add LSP plumbing in a new
   `crates/glyph-lsp` crate.
2. **Depend on `glyph-cli`** and shell out to or library-link against
   the CLI's entry points (`glyph check`, `glyph compile`).

Option 2 is tempting at first glance because the CLI already wires up
the pipeline behind a single function call. But it would make the LSP
a peer of the CLI and entangle it with concerns it does not need
(argument parsing, exit codes, terminal pretty-printing, agent-
oriented output channels).

## Decision

The LSP depends on `glyph-core` only. It does not depend on
`glyph-cli` and does not shell out. The new crate is
`crates/glyph-lsp`, a sibling of `glyph-cli`.

A `glyph lsp` subcommand on the existing `glyph-cli` binary calls
into `glyph-lsp::run(stdin, stdout)`. The binary is shared (one
`cargo install`); the libraries are separate.

## Consequences

- The LSP is built on the same primitives as the CLI but does not
  inherit CLI-specific concerns: exit codes, pretty-printing,
  argument parsing, JSON-vs-human output mode selection, agent
  workflow scripting.
- Both the CLI and the LSP see the same diagnostics from
  `glyph-core` automatically — adding a check to `glyph-core` lights
  it up in both.
- Changes to the CLI's surface (flags, exit codes, output format) do
  not ripple into the LSP. Conversely, changes to LSP internals do
  not ripple into the CLI.
- A separate `glyph-lsp` binary was considered and rejected: more
  distribution surface, and most editors call `glyph lsp` once and
  forget it. The subcommand path keeps everything one `cargo install`
  away.
- If the LSP later needs CLI-only helpers (e.g., agent-oriented
  output formatting), they would be lifted into `glyph-core` rather
  than imported from `glyph-cli`.
