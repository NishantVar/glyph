# ADR 0009: Safety Sandwich — Bound Every LLM Pass With Deterministic Checks

## Status

Accepted.

## Context

Glyph's compiler uses LLMs for two passes (Repair and Expand) that would be impractical to make fully deterministic:

- Repair generates source-level definitions for novice authors who write incomplete `.glyph` files.
- Expand reshapes IR-level prose into agent-facing instructions (parameter descriptions, branch-condition prose, Call-body prose, return-fold suffixes).

LLM passes are stochastic, occasionally regress role assignments, and cannot be trusted as the final correctness gate of a compiler.

## Decision

Every LLM-assisted phase is bounded on both sides by deterministic phases that check its work:

```
Deterministic [Parse + Analyze]
  → LLM [Repair]
  → Deterministic [re-run Parse + Analyze, then Lower + Validate]
  → LLM [Expand]
  → Deterministic [Emit]
```

The invariant: deterministic compiler passes own correctness. An LLM pass may attempt anything inside its boundary; whatever it produces is re-checked by the next deterministic phase before any later work depends on it.

Concretely:

- Source is never compiled unless it has passed deterministic Validate.
- Compiled output is never written unless it has passed deterministic Emit.
- After Repair, the pipeline re-runs Parse + Analyze from scratch on the post-repair source. Repair's output is judged solely by whether the next deterministic pass accepts it.
- After Expand's LLM span fill, a deterministic role-preservation gate (Phase 6b) confirms that no role has been dropped or reshuffled.

## Consequences

- The compiler can hard-fail safely when the LLM cannot converge — the loop is bounded, the diagnostic surface is structured, and the deterministic gate decides acceptance.
- Caching is straightforward: the cache key is the post-repair source hash. Repair is the only place an LLM touches source; once it has run, all later phases are deterministic functions of source content.
- The price is two LLM invocations per compile in the worst case (Repair + Expand) instead of one combined pass. This is paid willingly because a single combined LLM pass cannot be checked by the compiler — there is no later deterministic phase that could detect drift between source intent and final prose.
- The Safety Sandwich pattern is referenced by every phase contract; relaxing it would require redesigning every phase's invariants.
