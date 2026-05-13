# 0007. Two error channels: `DiagBag` and `CompileError`

## Status

Accepted.

## Context

The compiler emits two structurally different kinds of failure:

- **User diagnostics** — "your source has an undefined name on line 5."
  These are expected output and flow through the pipeline, accumulating
  across phases. They are rendered as pretty stderr text or NDJSON
  stdout.
- **Compiler errors** — "file not found," "arena index out of bounds,"
  "a previous phase produced input that violates an invariant." These
  mean the compiler itself is broken or its environment is wrong. They
  are not user-facing diagnostics.

The instinct is to wrap both in a single `Error` enum and rely on
`thiserror` or `anyhow`. The instinct is wrong: doing so blurs the
contract between "your code is wrong" and "the compiler is wrong" at
exactly the boundary where agents and humans need them separated.

## Decision

Two separate channels, no error-handling framework:

- **`DiagBag { diagnostics: Vec<Diagnostic> }`** accumulated across all
  pipeline phases. Each `Diagnostic` follows the diagnostic-doc shape
  (`id`, `classification`, `message`, `span`, optional `related`,
  optional `hints`).
- **`CompileError`** enum (~3–4 variants: `Io { path, source }`,
  `Internal(String)`) with a hand-rolled `Display` impl. Surfaces only
  invocation- and bug-class failures.

No `thiserror`: the enum is small enough that a 15-line `Display` impl
is cheaper than the proc-macro dependency chain. No `anyhow`: the
error surface is small and well-defined; dynamic error chaining has
no payoff here.

## Consequences

- The shape of "user error" and "compiler error" is visibly different
  in the code, matching the user/agent-facing exit-code contract.
- Adding `thiserror`/`anyhow` later is easy if the enum grows; doing
  so today buys nothing.
- The diagnostic struct, its IDs, and its ordering rule (see ADR
  0006) form a single coherent contract independent of internal
  compile errors.
