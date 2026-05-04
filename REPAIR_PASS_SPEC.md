# Glyph Repair Pass — What It Does

A flat list of every trigger the LLM repair pass handles and the action it takes.

Deterministic auto-fixes (Phase 3a, `glyph fmt`) are not listed here — see issue [#106](https://github.com/NishantVar/glyph/issues/106) and `design/repair.md` §4.10. The agent runs `glyph fmt` before invoking the LLM repair pass; the diagnostics covered by 3a are never seen by 3b.

## Phase 3b — LLM Source Repairs (when `glyph compile` exits 2 after `glyph fmt`)

| If repair sees... | It does... |
|---|---|
| `G::parse::operator-in-expression` | Rewrite `x + y` as `combine(x, y)` or fold into a single inline instruction string. (Deterministic rewrite is a deferred candidate — keep LLM for now.) |
| `G::parse::param-slot-in-non-instruction-string` | Strip `{...}` braces, or move the slot into an instruction-bearing string. |
| `G::analyze::undefined-name` | Append `generated const <name> = "<one sentence>"` after all non-generated decls. |
| `G::analyze::undefined-call` | Append `generated block <name>(<inferred-params>)` with a single-string body, after all non-generated decls. |
| `G::analyze::ambiguous-role` | Add an explicit role marker (`require` / `avoid` / `must` / `context`), or convert to an instruction string / call. |
| `G::analyze::missing-return` | Append `return <expr>` as the final `flow:` statement. |
| `G::analyze::export-missing-return-type` | Infer a domain-type name from the return value and add ` -> DomainType` to the header. |
| `G::analyze::nested-branch` | Extract the inner branch into a `generated block`; replace it with a call passing captured outer-scope bindings; emit `G::repair::branch-extracted`. |
| `G::analyze::missing-description` | Generate a single-string `description:` for the skill, phrased as a trigger condition. |
| `G::analyze::applies-on-undescribed-block` (same file) | Add a trigger-shaped `description:` to the named block. |
| `G::analyze::applies-on-undescribed-block` (imported) | Surface as non-repairable; do not edit the imported file. |
| any unresolved name that already resolves via existing imports / stdlib / local decls | Skip — do not regenerate (idempotence). |
| any compound name like `avoid_unrelated_edits` (unresolved) | Generate one `generated const` under the full compound name, polarity baked into the body text. Do not split. |

## Phase 3c — Constraint Conflict Scan (after exit 0)

| If repair sees... | It does... |
|---|---|
| Declaration with 0 or 1 entries in `constraints:` | Skip (no LLM call). |
| Declaration with ≥2 entries in `constraints:` | Run one LLM call classifying every pair as `contradiction` / `tension` / `none`. |
| All pairs `none` | No diagnostic; proceed. |
| ≥1 `tension` pair | Emit `G::repair::constraint-tension` (warning); both constraints survive. |
| ≥1 `contradiction` pair | Emit `G::repair::constraint-contradiction` (error); compilation fails. |
| Phase 3c LLM output malformed (bad JSON, missing pair, bad enum, unknown id) | Retry with violation report (≤2 retries). |

## Loop Control

| If repair sees... | It does... |
|---|---|
| `glyph compile` exit 0 | Stop Phase 3b for this file; run Phase 3c; then hand off to Step 2. |
| `glyph compile` exit 1 (hard error) | Stop. Surface to user. Do not edit source. |
| `glyph compile` exit 2 with file already at iteration 3 | Hard fail with `G::repair::no-convergence`; surface residuals. |
| `glyph compile` exit 2 otherwise | Run `glyph fmt` (Phase 3a). If it changed the file, re-invoke compile. If still exit 2, apply all 3b fixes for the file in one rewrite; increment that file's counter; re-invoke compiler. |
| `glyph compile` exit 3 | Stop. Surface invocation error. |
| File emitted zero `repairable` diagnostics this iteration | Mark "done"; skip on next iteration. |
| LLM rewrite that does not parse | Hard fail with `G::repair::output-invalid`; do **not** write to disk; no retry. |
| Network / 5xx LLM failure | Retry up to 3× with exponential backoff; then hard fail with `G::repair::llm-unavailable`. |

## Collisions

| If repair sees... | It does... |
|---|---|
| Author decl + `generated` decl with same name | Compiler deletes the generated decl; warning. |
| Two author decls with same name | Hard fail (`G::analyze::name-collision`). |
| Two distinct unresolved use-sites that would generate the same name | Hard fail (`G::analyze::name-collision`). |
