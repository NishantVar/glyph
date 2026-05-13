# ADR 0008: Agent-Oriented Exit Codes

## Status

Accepted (v0).

## Context

Glyph is invoked by two distinct callers:

1. **Humans** at a terminal, expecting Unix-conventional behavior: `0` for success, non-zero for failure.
2. **Agents** orchestrating an LLM repair / reshape loop on top of the deterministic compiler. The agent needs to know not just "did it fail" but "should I (a) ask the LLM to repair the source, (b) surface a hard error to the author, or (c) stop because the invocation itself is broken."

A single failure code (`1`) cannot distinguish "the author wrote unresolvable nonsense" from "the source is close enough that an LLM can fix it" from "you passed a path that doesn't exist." Conflating these forces the agent to parse diagnostic output to decide control flow, which is brittle and couples agents to a textual contract.

## Decision

Use a 4-tier exit code scheme keyed to the agent's next action:

| Code | Meaning | Agent action |
|------|---------|--------------|
| `0` | Success. | Proceed to Expand Step 2 (LLM reshaping). |
| `1` | Hard errors. Cannot compile. | Surface diagnostics to author. Do not attempt repair. |
| `2` | Repairable diagnostics only. Pipeline stopped after Phase 2. | Run LLM repair on source, re-invoke. |
| `3` | Invocation error (bad flags, missing path, IO failure). | Surface to user. Stop. |

**`1` wins over `2`.** If both hard errors and repairable diagnostics exist, the exit code is `1` — there is no point repairing if a hard error still blocks compilation.

This is consistent across `compile`, `check`, and `validate-output`. `fmt` uses a simpler 0/1/3 scheme because it has no repair tier.

## Consequences

- Agent loops are control-flow-driven, not text-parsing-driven. The agent can dispatch on exit code alone.
- Humans still see the Unix-standard "0 good, non-zero bad" pattern.
- `--strict` exists as an escape valve so CI gates can collapse `2` into `1` and refuse to ship lint-dirty code.
- Repurposing any of `0`/`1`/`2`/`3` later would be a breaking change for every agent and CI integration. Future expansion uses unused codes (`4`+) rather than overloading existing ones.
- The diagnostic JSON format (NDJSON on stdout in `--format=json`) is a separate contract; exit codes are the coarse-grained control channel, JSON is the fine-grained one. Agents must read both — exit code first, then diagnostics for detail.
