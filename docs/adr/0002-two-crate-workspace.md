# 0002. Two-crate workspace: `glyph-core` + `glyph-cli`

## Status

Accepted.

## Context

The Glyph compiler implements the deterministic phases of the Safety Sandwich
pipeline (Parse, Analyze, Lower, Validate, Expand Step 1, Emit). The
non-deterministic LLM phases (Repair and Expand Step 2) live in an external
agent skill that invokes the CLI between LLM calls.

The compiler therefore needs no API keys, no HTTP client, and no async
runtime — but it does need a library surface that integration tests and a
future LSP can call directly.

## Decision

Two-crate Cargo workspace:

- **`glyph-core`** — library. All deterministic compiler phases, IR/AST/
  diagnostic types, arena, span. Each phase is a public function. IR types
  derive `serde::Serialize` for `--emit-ir` JSON output.
- **`glyph-cli`** — binary. CLI argument parsing (`clap`), pipeline
  orchestration, pretty diagnostic rendering on stderr
  (`codespan-reporting`), writes `.md` and `.ir.json` to disk.

No `glyph-llm` crate. The compiler does not embed an LLM client.

## Consequences

- Integration tests call `glyph-core` directly without spawning a process.
- The binary stays thin; orchestration logic that has no business in the
  library lives in the binary.
- A future LSP, plugin host, or embedded build mode can depend on
  `glyph-core` without dragging in CLI dependencies.
- Future extractions are deferred until justified: `glyph-llm` if an
  embedded LLM mode is added, `glyph-diagnostics` if a language server
  needs shared diagnostic types, `glyph-emit` if Emit grows multi-format
  output.
