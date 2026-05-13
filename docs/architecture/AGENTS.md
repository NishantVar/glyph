# Glyph — Architecture

Durable maintainer-facing architecture for the Glyph compiler and adjacent tools. Explain **why** the system is shaped this way and **what invariants** must be preserved. Do not duplicate every line of code — code, tests, and the IR schema own the mechanical details.

## Documents

- [[compiler-pipeline]] — the 7-phase pipeline (Parse, Analyze, Repair, Lower, Validate, Expand, Emit), the Safety Sandwich invariant, multi-file ordering, partial-failure policy, cacheability
- [[docs/architecture/repair]] — repair pass internals: Phase 3a/3b/3c structure, validation loop, retry policy, multi-file scope rationale, diagnostic plumbing
- [[docs/architecture/expand]] — Expand pass Step 2 (LLM reshape) mechanics, Phase 6b structural validation algorithm, retry / deterministic-fallback / hard-fail policy, span model, worked example
- [[ir-schema]] — canonical IR node schema for `glyph-core`: every node type, enum values, resolved-IR shape, node-identifier spec, internal invariants
- [[ir-semantics]] — IR role-computation evidence ordering, Lower hoisting mechanics, effect-inference algorithm, Tier-3 validation, freeform-section IR plumbing
- [[docs/architecture/diagnostics]] — why the 3-tier classification, why the `G::<phase>::<name>` scheme, the repair-vs-error boundary, by-construction satisfaction as defense-in-depth
- [[lsp]] — LSP process model, cache layers (`DocumentStore` / `FileGraph` / `DiagSnapshot`), `glyph-core` API surface, diagnostic mapping, go-to-def algorithm
- [[tree-sitter]] — tree-sitter grammar architecture: external scanner for indentation, divergence/parity rules with the hand-rolled parser, editor integration
- [[agent-skill]] — companion-agent architecture: workflow state machine, repair guidance, constraint conflict scan, Step 2 orchestration, `validate-output` invocation
- [[mvp-walking-skeleton]] — the `update_docs.glyph` walking-skeleton example and per-phase walkthrough that defines the MVP starvation set

## Rule

Architecture docs explain rationale and invariants. Reference contracts go to [`../reference/`](../reference/). Short, durable decision records go to [`../adr/`](../adr/). Implementation TODOs go to [`../../todo/`](../../todo/).
