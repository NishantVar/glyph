# ADR 0014: Deterministic Compiler With Bounded LLM Repair

## Status

Accepted (MVP).

## Context

Glyph source can be authored from a kernel surface that intentionally permits undefined names, missing markers, and shorthand calls. Some authoring style — especially for novices — produces source that does not parse or does not type-check. We need a way to turn under-specified source into compilable source without giving up the auditability and reproducibility of a deterministic compiler.

Two extremes were considered:

- **Fully deterministic compiler.** Every fix must be a mechanical rewrite (e.g. add a parenthesis, normalize indentation). Novices cannot use shorthand names without first hand-writing declarations — the kernel becomes unusable.
- **LLM compiler.** The compiler itself is an LLM end-to-end. Every compile is non-deterministic and uncacheable; CI builds can drift across runs and across model versions; the compiled `.md` is unstable.

Neither extreme is acceptable. The MVP needs the kernel surface (novice authors writing undefined names) and reproducible downstream builds (CI agents compiling committed source).

## Decision

The compiler is **deterministic everywhere except in Phase 3 (Repair) and Phase 6 Step 2 (Expand prose reshape)**. Repair is the only phase that may invoke an LLM during normal compilation. Its job is bounded: turn invalid or under-specified source into source the deterministic compiler accepts, then exit. It does not flatten source into compiled instructions and does not bypass any later validation.

Specifically:

- Repair runs **only** when Phase 2 (Analyze) emits `repairable` diagnostics, or when Phase 3c's constraint-conflict scan triggers on constraint count.
- After Repair writes back the rewrite, Phases 1, 2, 4, and 5 re-run. Repair is accepted only if the deterministic compiler accepts the rewritten source.
- The compiler in CI mode does not run Phase 3 at all. If `repairable` diagnostics survive Phase 2, the compiler exits with code 2 and it is the agent's responsibility to invoke LLM repair and re-run the compiler. CI configured to run the compiler directly (no agent) treats exit 2 as build failure, enforcing the "commit post-repair source" workflow.
- The cache key is the **post-repair** source hash. Once an author commits post-repair source, downstream compiles read identical source and produce identical IR.

## Consequences

**Positive.**

- Novice authors can use the kernel surface; Repair materializes generated definitions on first compile.
- Committed `.glyph` files are reproducible across machines: CI runs the deterministic compiler directly and never invokes the LLM.
- The non-determinism of Repair is a one-time authoring-time cost, not a build-time cost.
- The deterministic compiler remains the authority on correctness; Repair is never proof of correctness.

**Negative.**

- The "commit post-repair source" workflow is a convention the author must follow. If un-repaired source is committed and CI runs the agent skill (instead of the compiler directly), different machines may produce different compiled `.md`. The mitigation is the recommended CI configuration (compiler-only, no agent).
- Two LLM-driven phases (Repair and Expand Step 2) each have their own retry / failure policy. They are not unified — Step 2 has no deterministic fallback ([[0016-llm-reshape-no-deterministic-fallback]]), while Repair retries transient failures and aborts on unparseable output. The compiler maintains two distinct failure-handling code paths.

## Alternatives Considered

- **"Repair is just another phase of the deterministic compiler."** Rejected: the kernel surface fundamentally requires content generation (undefined names becoming `generated const`/`generated block` bodies), which cannot be done mechanically without losing the novice ergonomic.
- **"Run Repair eagerly on every compile."** Rejected: every CI run would be non-deterministic and uncacheable. The "commit post-repair source" model is cheaper and stricter.
- **"Single end-to-end LLM compile."** Rejected: see Context above. Loses determinism, caching, and validation guarantees.

## See Also

- [[design/repair]] — author-facing contract.
- [[docs/architecture/repair]] — implementation architecture.
- [[0013-compiler-driven-repair-via-companion-agent]] — companion-agent invocation model.
- [[0016-llm-reshape-no-deterministic-fallback]] — distinct policy for Expand Step 2.
