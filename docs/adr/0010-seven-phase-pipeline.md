# ADR 0010: Seven-Phase Pipeline (Not Three, Not Twelve)

## Status

Accepted. Supersedes the 5-pass diagram in `README.md` and the 9-step list in [[language-surface]] §5.

## Context

Earlier descriptions of the Glyph compiler used different decompositions:

- `README.md` showed a 5-pass picture (Parse → Analyze → Transform → Validate → Output).
- [[language-surface]] §5 listed 9 steps (Parse, Diagnose, Repair, Re-parse, Resolve, Infer, Normalize, Type, Validate).
- [[foundations]] #18 described it in prose.

These three descriptions did not line up. In particular: where did the LLM passes belong? Was Repair its own phase or a sub-step of Analyze? Where did Expand fit?

## Decision

The canonical pipeline has **seven phases**:

1. Parse — text to structural AST.
2. Analyze — structural AST to annotated AST + structured diagnostics.
3. Repair — bounded loop that edits source until diagnostics clear or hard-fail.
4. Lower — repaired AST to typed IR.
5. Validate — IR correctness gate.
6. Expand — IR to expanded IR with agent-facing prose.
7. Emit — expanded IR to compiled Markdown on disk.

The choice of seven (not three, not twelve) is driven by:

- **Each LLM pass needs its own phase boundary.** Repair and Expand are independently bounded by the Safety Sandwich (ADR 0009), so each gets its own slot. Folding them into Analyze or Lower would muddy the contract of those passes.
- **The validated-IR boundary is load-bearing.** Visualization branches off after Validate without running Expand or Emit. Caching keys split on the same boundary. So Validate is a phase, not a sub-step of Lower.
- **Lower is one phase, not four.** The 9-step list's Resolve/Infer/Normalize/Type are sub-operations of Lower. Splitting them into separate phases buys no architectural property — they share input, output, and ordering.
- **Emit is separate from Expand.** Emit is a deterministic formatter; Expand contains an LLM step. Conflating them would lose the property that Emit is byte-stable given an expanded IR.

## Consequences

- The README and [[language-surface]] reconciliation tables are mechanical: [[compiler-pipeline]] is the canonical reference and other docs defer to it.
- Adding a phase later (e.g., a separate Optimize pass) requires inserting it without breaking the Safety Sandwich. The phase count can grow but the deterministic-LLM-deterministic alternation must be preserved.
- A reviewer can ask "which phase owns this invariant?" and get a single canonical answer. This eliminates the ambiguity that the prior 5-pass / 9-step / prose triad introduced.
