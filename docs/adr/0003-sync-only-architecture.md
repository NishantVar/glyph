# 0003. Sync-only compiler, no async runtime

## Status

Accepted.

## Context

The compiler is a short-lived process: read source files, run deterministic
phases, write output files. There is no LLM call inside the binary (the
LLM phases are owned by the external agent skill), no network I/O, and no
inherent concurrency.

The original recommendation was "sync core, `tokio` at the LLM boundary."
With no LLM boundary in the binary, there is no async boundary at all.

## Decision

The compiler is fully synchronous. No `tokio`, no `async-std`, no async
runtime of any kind. Standard library `std::fs` for file I/O, `std::io` for
stdout/stderr.

Multi-file builds compile each `.glyph` file one at a time, in topological
order over the import DAG. Independent files in the DAG are not
parallelised. The topological ordering contract lives in the architecture
docs; this ADR only commits to serial execution.

## Consequences

- Zero async runtime weight in the binary or library.
- Every phase is a pure function from its input to output plus diagnostics,
  trivially testable.
- Cross-file parallelism is a post-MVP optimisation.
- If a future embedded LLM mode or LSP server requires async, introduce it
  in that crate only — `glyph-core` and `glyph-cli` need not change.
