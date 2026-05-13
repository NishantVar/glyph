# ADR 0016 — LLM Reshape Has No Deterministic Fallback

Status: accepted
Date: 2026-05-13
Component: Expand (Phase 6 Step 2)

## Context

The Expand pass's Step 2 reshapes resolved IR content into natural-language Markdown. The reshape is necessary because Step 1's mechanical resolution produces correct-but-stilted output — short declarative sentences with no application of `with` modifiers, no scoped-constraint weaving, no folded `OutputContract` prose, and no calibrated constraint wording. The LLM is load-bearing in exactly those places.

A natural design question: when the LLM produces output that fails Phase 6b, should the compiler fall back to a deterministic emitter that produces a uglier-but-passing Markdown rather than hard-failing?

## Decision

There is **no deterministic fallback for span content that requires natural-language judgement.** When Step 2 cannot produce a Phase-6b-passing output within the retry budget ([[docs/architecture/expand]] §5.5), the compiler aborts with the specific 6b diagnostic on stderr and writes no `.md` file. The user re-runs.

Span kinds whose stub fill happens to read acceptably today (`BranchCondition` verbatim slotting, `DescriptionReturnFold` verbatim slotting) are explicit stub behaviors, not architectural fallbacks. They are listed alongside their LLM contracts in [[docs/architecture/expand]] §3.5.

## Consequences

- **Quality stays bounded.** The pipeline never silently emits a low-quality `.md` produced by a "fallback" emitter that the LLM normally outperforms. Trust in the abstraction is preserved.
- **Failures are loud.** A persistent reshape failure produces a non-zero exit and a specific 6b diagnostic. The agent workflow surfaces this through the `validate-output` subcommand ([[docs/reference/cli]]).
- **Retry budget matters more.** Because there is no fallback, the per-span retry budget ([[docs/architecture/expand]] §5.5) is the only buffer between transient LLM failure and a user-visible abort. Tune the budget rather than adding a fallback path.
- **Maintainers may not add a "always-ugly" emitter.** The temptation is real for partial failure modes; resist it. If a class of skill consistently trips Phase 6b, the right move is to push more work into Step 1 (so the LLM has less latitude) or to grow the Phase 6b retry budget — not to introduce a parallel deterministic path.
