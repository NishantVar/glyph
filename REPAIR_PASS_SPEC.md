# Glyph Repair Pass — What the LLM Has To Do

A complete description of every action the LLM performs across the repair passes. Compiler and agent-loop concerns (exit code handling, iteration counting, `glyph fmt` invocation, name-collision hard-fails) are intentionally omitted — see `design/agent-skill.md` and `design/repair.md` for the surrounding workflow.

This file is a working aid and will be deleted once the implementation stabilises.

## Phase 3b — Source Rewriting

The LLM receives one Glyph source file plus its full set of `repairable` diagnostics for that file in a single prompt. It produces one rewritten source file. One LLM call per file per iteration; up to 3 iterations per file.

### Triggers and Actions

| Diagnostic | LLM action |
|---|---|
| `G::parse::operator-in-expression` | Rewrite `x + y` as `combine(x, y)` or fold into a single inline instruction string. (Deterministic rewrite is a deferred candidate — keep LLM for now.) |
| `G::parse::param-slot-in-non-instruction-string` | Strip `{...}` braces, or move the slot into an instruction-bearing string. |
| `G::analyze::undefined-name` | Append `generated const <name> = "<one sentence>"` after all non-generated decls. |
| `G::analyze::undefined-call` | Append `generated block <name>(<inferred-params>)` with a single-string body, after all non-generated decls. |
| `G::analyze::ambiguous-role` | Add an explicit role marker (`require` / `avoid` / `must` / `context`), or convert to an instruction string / call. |
| `G::analyze::missing-return` | Append `return <expr>` as the final `flow:` statement. |
| `G::analyze::export-missing-return-type` | Infer a domain-type name from the return value and add ` -> DomainType` to the header. |
| `G::analyze::nested-branch` | Extract the inner branch into a `generated block`; replace it with a call passing captured outer-scope bindings. |
| `G::analyze::missing-description` | Generate a single-string `description:` for the skill, phrased as a trigger condition. |
| `G::analyze::applies-on-undescribed-block` (same file) | Add a trigger-shaped `description:` to the named block. |
| `G::analyze::applies-on-undescribed-block` (imported) | Surface as non-repairable; do not edit the imported file. |
| any unresolved name that already resolves via existing imports / stdlib / local decls | Skip — do not regenerate (idempotence). |
| any compound name like `avoid_unrelated_edits` (unresolved) | Generate one `generated const` under the full compound name, polarity baked into the body text. Do not split. |

### Output Validation

| Condition | LLM-side handling |
|---|---|
| Rewritten file does not parse (Phase 1 fails on LLM output) | No retry. The agent surfaces `G::repair::output-invalid` (with the LLM output captured for inspection) and aborts. The failed rewrite is not written back. |
| Network / 5xx during the LLM call | Retry up to 3× with exponential backoff. After exhaustion, surface `G::repair::llm-unavailable` and abort. |

## Phase 3c — Constraint Conflict Scan

Runs after `glyph compile` reaches exit 0. Triggered per declaration whose `constraints:` set has 2 or more entries. Independent of Phase 2 diagnostics. The LLM does not modify source — it only emits diagnostics.

### Input

The constraint set for one declaration: each entry as `{ id, resolved_text, strength, polarity }`. Identifiers are declaration-local indices `c0`, `c1`, … in source order.

### Output

Structured JSON: `{ conflicts: [{ pair: [id_A, id_B], type: "contradiction" | "tension" | "none", explanation: "..." }, ...] }`. Every pair is addressed; pairs classified `none` may be omitted.

### Triggers and Actions

| Constraint set | LLM action |
|---|---|
| Declaration with 0 or 1 entries in `constraints:` | Skip (no LLM call). |
| Declaration with ≥2 entries in `constraints:` | Run one LLM call classifying every pair as `contradiction` / `tension` / `none`. |

### Verdict Handling

| Output | Resulting diagnostic |
|---|---|
| All pairs `none` (or empty `conflicts` list) | No diagnostic; proceed. |
| ≥1 `tension` pair | Emit `G::repair::constraint-tension` (warning); both constraints survive. |
| ≥1 `contradiction` pair | Emit `G::repair::constraint-contradiction` (error); compilation fails. |

### Output Validation

| Condition | LLM-side handling |
|---|---|
| Output not valid JSON, doesn't address every pair, references an ID not in the input set, or returns a `type` outside the three-value enum | Retry up to 2× with the previous output, a structured violation report, and an edit directive. After two failed retries, emit `G::repair::constraint-scan-malformed` and abort. |
| Network / 5xx | Same as 3b: retry up to 3× with exponential backoff, then `G::repair::llm-unavailable`. |
