# Glyph CLI — Product Surface

This document describes the Glyph command-line interface at a behavioral, product level: what an author or agent author needs to understand about how the compiler is invoked and what to expect from each subcommand. For the stable contract (exact flags, exit codes, output formats, channel discipline) that tools and agents depend on, see [[docs/reference/cli]].

## Shape

Glyph ships as a single binary, `glyph`, with subcommand dispatch:

- **`glyph compile <path>`** — turn `.glyph` source into compiled `.md` (and optional `.ir.json`).
- **`glyph check <path>`** — fast lint: parse and analyze, report diagnostics, no output written.
- **`glyph fmt <path>`** — deterministic source rewrites in place (analogous to `rustfmt` / `gofmt`).
- **`glyph validate-output <ir-json> <md>`** — confirm that agent-rewritten Markdown still structurally matches the IR.

`compile` and `check` accept a file or a directory; directory mode recurses and compiles every `.glyph` file it finds. Library files (zero `skill` declarations) compile silently and produce no compiled `.md` of their own.

## Deterministic Core, Agent-Driven LLM

The compiler binary is **fully deterministic**. The two LLM-assisted phases — Repair (Phase 3) and Expand Step 2 (LLM prose reshaping) — live in an external agent skill that orchestrates the CLI rather than being embedded in it. This shape is why the CLI exposes:

- An exit code that distinguishes "repairable" from "hard error" (so an agent loop knows whether to ask an LLM to fix the source and re-invoke).
- An `--emit-ir` flag for the resolved IR JSON (so the agent has structured context for prose reshaping).
- A `validate-output` subcommand (so the agent can re-check its own rewrite against the compiler's invariants).

A human typing `glyph compile` at a terminal does not need an LLM and never sees one; the agent loop is opt-in tooling on top of the same binary.

## Agent-Oriented Exit Codes

Exit codes are the coarse control channel. The four-tier scheme — success / hard error / repairable / invocation error — exists so agents can dispatch on exit code alone without parsing diagnostic text. See [[0008-agent-oriented-exit-codes]] for the rationale and [[docs/reference/cli]] for the exact codes.

## Diagnostic Surfaces

Two diagnostic formats are supported:

- **Pretty** (default): human-readable, colorized, goes to stderr. What you want at a terminal.
- **JSON** (`--format=json`): NDJSON, goes to stdout for actionable diagnostics. What an agent wants.

The split between stdout and stderr in JSON mode is intentional: actionable diagnostics are data the agent pipes; warnings and fatal compiler errors are noise the agent should not parse. See [[docs/reference/cli]] §Diagnostic Output for the exact channel discipline.

## Out of Scope for v0

The CLI v0 intentionally omits `glyph init`, `glyph watch`, `glyph lsp`, an embedded LLM mode, a project config file, and incremental compilation. The pipeline is architected to support incremental builds later, but v0 re-runs all phases on every invocation. See [[docs/reference/cli]] §Deferred for the full list.

## Cross-References

- [[docs/reference/cli]] — exact subcommands, flags, exit codes, diagnostic format, multi-file behavior.
- [[0008-agent-oriented-exit-codes]] — why exit codes are agent-oriented.
- [[agent-skill]] — the companion agent that drives the deterministic CLI through Repair and Expand Step 2.
- [[compiler-pipeline]] — what the compiler actually does between invocations.
