# Glyph — Reference

Stable contracts for users, tools, agents, and downstream integrations. State **what an external consumer can rely on**. No implementation rationale here — that goes in [`../architecture/`](../architecture/) or [`../adr/`](../adr/).

## Documents

- [[docs/reference/cli]] — `glyph` CLI surface: subcommands (`compile`, `check`, `fmt`, `validate-output`), flags (`--emit-ir`, `--out-dir`, `--format`, `--strict`, `--enable-effects`, `-v`/`-vv`, `--color`), the four-tier exit code matrix, stdout/stderr channel discipline, NDJSON diagnostic shape, multi-file behavior, partial-failure policy, library-file IR no-op, deferred features
- [[docs/reference/compiled-output]] — the compiled Markdown shape downstream agents consume: frontmatter fields, body sections (`## Goal`, `## Parameters`, `## Context`, `## Steps`, `## Constraints`, freeform), three-tier block projection, return-fold templates, parameter slots, stability statement
- [[docs/reference/diagnostics]] — the diagnostic contract: `G::<phase>::<name>` ID scheme, three-tier classification (`error`/`repairable`/`warning`), structured `Diagnostic` shape, `SourceSpan` shape, full catalog of stable IDs across Parse/Analyze/Validate/Build/Validate-output, behavioral notes for repair and compiled-output interactions
- [[ir-json]] — the IR JSON contract that `glyph compile --emit-ir` produces and `glyph validate-output` consumes: top-level envelope (`ir_version`/`compiler`/`source_file`/`skill`), per-node-kind JSON shapes, Expression/Value unions, enum serialization (all snake_case), versioning policy, worked example
- [[mvp-acceptance]] — what a Glyph author may rely on at MVP: feature list, byte-stable output guarantee, multi-file build behavior, `--strict` semantics, diagnostic-ID and exit-code commitments

## Rule

Reference docs are a **contract**: a change here is potentially breaking for someone outside this repo. If you find yourself adding "we chose X because Y" reasoning, move that to `../architecture/` or write an ADR.
