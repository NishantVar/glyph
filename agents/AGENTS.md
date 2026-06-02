# agents/ — Flux Org Scaffold

This `agents/` tree is the **Flux org scaffold** for the Glyph repo: the durable
home for the seated roster, ownership truth, handoffs, reviews, and roster
decisions.

It is **not** the same as `.agents/**`. `.agents/**` is Glyph **product /
dogfood** artifact surface (skills, commands, the round-trip harness). This
`agents/**` tree is Flux team scaffold. Keep them separate — never collapse or
symlink one onto the other.

## Layout

- [org-transition-plan.md](org-transition-plan.md) — durable approved org truth:
  seated roster, ownership matrix, gate coverage, day-one artifact contract, and
  bridge status. Source of record after human approval.
- `handoffs/` — approved role-to-role and Maya/Ari handoff artifacts.
- `reviews/` — review and verification records, and linked gate evidence.
- `decisions/` — durable roster/org decision history.
- `quality/roundtrip/` — semantic round-trip drift findings, coverage maps, and
  chronic-drift policy. **Owned by the QA Engineer (`evaluation-engineer-agent`)**,
  distinct from the rest of this scaffold.

## Ownership

The Integration Engineer (`runtime-integration-engineer-agent`) owns the
`agents/**` scaffold mechanics **except** `agents/quality/roundtrip/**`, which the
QA Engineer owns. Role behavioral contracts under `.claude/agents/**` and
`.codex/agents/**` are owned by the corresponding role, not by the Integration
Engineer; the Integration Engineer owns the runtime-port mechanics only.
