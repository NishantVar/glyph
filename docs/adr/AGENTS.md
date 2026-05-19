# Glyph — Architecture Decision Records

Short records of implementation decisions whose rationale would otherwise be lost. Each ADR captures **context**, **decision**, and **consequences** in a page or two. ADRs are written when the decision is hard to infer from code and likely to be questioned later.

## Numbering

ADRs are numbered sequentially (`NNNN-<slug>.md`). Numbers are stable once assigned. Add new ADRs with the next free number; do not renumber existing ones.

## Index

| #    | Title                                                                                       | Domain       |
| ---- | ------------------------------------------------------------------------------------------- | ------------ |
| 0001 | [[0001-hand-rolled-parser\|Hand-rolled parser, not a parser generator]]                     | Parser       |
| 0002 | [[0002-two-crate-workspace\|Two-crate workspace: `glyph-core` + `glyph-cli`]]               | Workspace    |
| 0003 | [[0003-sync-only-architecture\|Sync-only compiler, no async runtime]]                       | Architecture |
| 0004 | [[0004-plain-span-struct\|Plain `Span` struct over packed `u64`]]                           | Internals    |
| 0005 | [[0005-arena-allocated-ir\|Single hand-rolled arena per file for IR]]                       | Internals    |
| 0006 | [[0006-json-output-determinism\|JSON output is byte-stable across runs]]                    | Output       |
| 0007 | [[0007-two-error-channels\|Two error channels: `DiagBag` and `CompileError`]]               | Error model  |
| 0008 | [[0008-agent-oriented-exit-codes\|Agent-oriented exit codes]]                               | CLI          |
| 0009 | [[0009-safety-sandwich\|Safety Sandwich — bound every LLM pass with deterministic checks]]  | Pipeline     |
| 0010 | [[0010-seven-phase-pipeline\|Seven-phase pipeline (not three, not twelve)]]                 | Pipeline     |
| 0011 | [[0011-strictly-serial-multi-file-compilation\|Strictly serial multi-file compilation in MVP]] | Pipeline  |
| 0012 | [[0012-parameterless-compilation\|Parameterless compilation — parameters survive as named slots]] | Pipeline |
| 0013 | [[0013-compiler-driven-repair-via-companion-agent\|Compiler-driven repair via a companion agent]] | Repair |
| 0014 | [[0014-deterministic-compiler-with-bounded-llm-repair\|Deterministic compiler with bounded LLM repair]] | Repair |
| 0015 | [[0015-repair-idempotence-as-name-resolution\|Repair idempotence anchored on name resolution]] | Repair    |
| 0016 | [[0016-llm-reshape-no-deterministic-fallback\|LLM reshape has no deterministic fallback]]   | Expand       |
| 0017 | [[0017-step-2-prose-reshape-as-separate-pass\|Step 2 prose reshape as a separate pass]]     | Expand       |
| 0018 | [[0018-phase-6b-structural-only-gate\|Phase 6b is a structural-only gate]]                  | Expand       |
| 0019 | [[0019-four-form-constraint-model\|Four-form constraint model from three keywords]]         | Semantics    |
| 0020 | [[0020-fixed-effect-keyword-vocabulary\|Fixed effect keyword vocabulary]]                   | Semantics    |
| 0021 | [[0021-closed-five-role-ir\|Closed five-role IR]]                                           | Semantics    |
| 0022 | [[0022-tree-sitter-external-scanner\|Tree-sitter external scanner for indentation]]         | Tooling      |
| 0023 | [[0023-tower-lsp-over-lsp-server\|`tower-lsp` over `lsp-server` for the Glyph LSP]]         | Tooling      |
| 0024 | [[0024-lsp-shares-glyph-core\|LSP depends on `glyph-core`, not `glyph-cli`]]                | Tooling      |
| 0025 | [[0025-context-preamble-format\|Locked context preamble format for procedure-tier blocks]]  | Output       |
