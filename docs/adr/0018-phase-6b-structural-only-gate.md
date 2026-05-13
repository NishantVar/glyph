# ADR 0018 — Phase 6b Is a Structural-Only Gate

Status: accepted
Date: 2026-05-13
Component: Expand (Phase 6b validation gate)

## Context

After Step 2 produces the compiled Markdown body, Phase 6b runs as a deterministic pass/fail gate before Phase 7 (Emit). A natural question: how much should 6b verify? Two extremes are possible:

- Pure structure (counts, ordering, parameter references, section presence).
- Structure + semantic fidelity (does the LLM's prose actually reflect the IR node's meaning?).

The same question applies to external tools: the user-facing `glyph validate-output` subcommand ([[docs/reference/cli]]) is the external face of this gate.

## Decision

Phase 6b verifies **structural** invariants only:

- Section shape (H2 catalogue, ordering, conditional sections).
- Role preservation 1-to-1 (top-level Step count, per-Branch sub-step count, Constraint count, ordering under `## Steps`).
- Parameter reference validity (no invented `{...}`, no dropped declared parameter references).
- Procedure and external-file projection consistency.
- Output-target leak detection (no `<name>` or `<"…">` token residue).
- `with` modifier non-leakage.
- Markdown parses cleanly; no spurious frontmatter.

The full catalog of `G::expand::*` diagnostic IDs is enumerated in [[docs/architecture/expand]] §4.2.

Phase 6b does **not** verify:

- Semantic faithfulness of wording.
- Effect correctness (owned by Phase 4/5).
- Style (tone, formality, clarity).

## Consequences

- **The Safety Sandwich is structural, not semantic.** This matches [[foundations]] #18 — deterministic passes own correctness, and the LLM is bounded by the IR shapes on both sides. Semantic drift is mitigated by separate mechanisms: the single-string rule for generated bodies ([[docs/architecture/repair]] §5), the resolved-body text flowing through Step 2 unchanged, and the 1-to-1 role mapping that 6b enforces.
- **`glyph validate-output` is reusable for agents.** Because the gate is structural and IR-driven, the same checks run inside the compiler (as Phase 6b) and outside the compiler (as the `validate-output` subcommand for agents that rewrite the compiled `.md`). Exit codes are defined in [[docs/reference/cli]].
- **Failure is sharp.** A 6b failure produces one diagnostic per violation with a stable ID; the diagnostic catalog ([[docs/architecture/expand]] §4.2) is the contract that retry prompts (§5.3) and the `validate-output` consumer use.
- **Authors get a 1-to-1 contract.** Because 6b enforces structural role preservation, [[design/expand]] can advertise the 1-to-1 mapping as something an author may rely on — even though the prose itself is not idempotent.
