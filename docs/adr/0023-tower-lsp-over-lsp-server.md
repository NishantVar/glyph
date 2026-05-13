# ADR 0023: `tower-lsp` Over `lsp-server` for the Glyph LSP

## Status

Accepted (v1).

## Context

The Glyph LSP needs a Rust framework to handle JSON-RPC framing,
method dispatch, and the LSP request/response shape. Two candidates
are mature and in active use:

- `tower-lsp` — high-level trait-driven dispatch on top of `tokio`.
  One `async fn` per LSP method.
- `lsp-server` (rust-analyzer's) — low-level framed JSON-RPC plus a
  dispatcher you own. Synchronous; build your own threadpool / event
  loop.

The choice shapes the dependency footprint, the boilerplate budget,
and the future cost of adding incremental computation.

| Axis | `tower-lsp` | `lsp-server` |
|------|-------------|--------------|
| Layer | High: `LanguageServer` trait per method | Low: framed JSON-RPC + your dispatcher |
| Async | `tokio` + `async fn` per request | Sync; you build the runtime |
| Boilerplate to first message | ~50 LOC | ~300 LOC |
| Cancellation | Built-in via `tokio` task drop | Manual (rust-analyzer wires it through salsa) |
| Incremental work | Naive (re-run from scratch per request) | Idiomatic with salsa/query graph |
| Fit for "rerun the compiler" model | Excellent | Overkill |

## Decision

Use **`tower-lsp` (v0.20.x)**.

The reasoning:

1. **Glyph scope is "rerun the compiler per request."** Source files
   are kilobytes. A full Parse + Analyze on save is sub-millisecond in
   release mode. We do not need an incremental query graph, salsa, or
   query cancellation. `tower-lsp`'s "request handler runs
   `glyph_core::check_source(...)` and replies" model fits the shape.
2. **Async fits the work.** Even without compute parallelism, async
   makes it easy to await reads of unsaved-but-imported files, push
   debounced diagnostics with `tokio::time::sleep` if added later,
   and compose with future capabilities (hover, completion) without
   a reactor rewrite.
3. **rust-analyzer chose `lsp-server` because they need salsa-style
   incremental computation across 10M-LOC projects.** That requirement
   does not apply to Glyph and likely never will — skill files are
   intentionally small.
4. **Trait-driven dispatch is honest documentation.** A
   `tower-lsp::LanguageServer` impl reads as a list of the LSP methods
   we support.

The single mild downside is the `tokio` runtime dependency. The
workspace already pulls `serde`/`serde_json`/`clap`; adding `tokio`
and `tower-lsp` is a one-line bump.

## Consequences

- The LSP code stays small and readable. Every LSP method is one
  `async fn`.
- We accept a `tokio` build-time cost (~5 s on a clean build).
- We are not set up for true incremental cross-file reanalysis. If
  Glyph ever needs that, the swap to `lsp-server` is mostly
  mechanical because the `DocumentStore` and `glyph-core` interactions
  live below the framework layer.
- Cancellation, debouncing, and future async-friendly work are
  basically free.
