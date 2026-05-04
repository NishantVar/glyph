# Phase 3a deterministic auto-fixes — design

**Parent PRD:** [#106](https://github.com/NishantVar/glyph/issues/106)
**Date:** 2026-05-04
**Branch:** `repair_deterministic`
**Ship as:** single PR covering all remaining sub-issues.

## Background

PRD #106 reclassifies seven `repairable` diagnostics from the LLM repair pass (Phase 3b) into deterministic auto-fixes in `glyph fmt` (Phase 3a). Sub-issue #109 (recoverable duplicate sub-section + merge) shipped via PR #120. This spec covers the remaining work:

| Issue | Auto-fix |
|---|---|
| #107 | Duplicate import collapse |
| #108 | Unused import removal |
| #110 | Stdlib auto-import (`subagent`, `send`, `load`) |
| #111 | Const-in-flow parens-add |
| #112 | Effects auto-insert |
| #113 | Placeholder return rewrite — *already implemented in `fmt.rs`*; this PR closes the issue and documents the existing behavior |
| #114 | Agent skill: run `glyph fmt` before LLM repair |

PRD #106 already settles most decisions. This spec records the remaining design calls and how the work fits into existing `fmt.rs` architecture.

## Scope

In scope:

- Five new auto-fixes (#107, #108, #110, #111, #112) inside `glyph-core/src/fmt.rs`.
- Recording #113 as already-done (small notes in spec, no code change).
- Updating `design/agent-skill.md` and `REPAIR_PASS_SPEC.md` per PRD §Further Notes (#114).
- Per-fix unit tests + one cross-cutting integration test in `glyph-core`.

Out of scope (per PRD):

- Phase 3b LLM repairs (semantic content generation).
- Phase 3c constraint conflict scan.
- Compile-pipeline auto-fix (`glyph compile` stays read-only).
- Cross-file repair.
- Deterministic operator-in-expression rewrite.

## Architecture

`fmt.rs` keeps its two-stratum shape (`preparse_rewrite` → parse → `ast_rewrite`). A new *Analyze stratum* sits between parse and AST rewrite to feed the fixes that need resolver / effect-inference info.

```
preparse_rewrite (text)
  → parse → AST + diagnostics
  → analyze (NEW: produces resolver + effect-inference info; only when parse succeeded)
  → ast_rewrite (existing fixes + 5 new fixes, fed by analyze info when needed)
  → serialize
```

Fallback behavior:

- **Parse fails:** skip analyze + ast_rewrite. Preparse text fixes (tab→space, mixed-indent, legacy `-> None` strip, etc.) still apply. Matches PRD US 16.
- **Analyze fails or produces partial info:** fixes that need it become no-ops on the un-analyzable parts; fixes that don't (duplicate-import, stdlib auto-import for the empty-import case) still fire.

The public `fmt_source(source: &str, enable_effects: bool) -> FmtResult` signature does not change. `FmtResult.changed: bool` remains the agent's signal that fmt rewrote something (PRD US 11).

## Per-fix specification

### #107 Duplicate import collapse

**Stratum:** AST file-level.
**Needs Analyze:** no.

Walk top-level imports. Group by canonical path. For each group with >1 entry:

- If any entry is whole-module, keep the first whole-module entry; drop the rest.
- Otherwise (all selective), keep one line; selector list is the union of all groups' selectors, ordered by first occurrence (first line's selectors first; new selectors from later lines appended in source order).

Comments on dropped lines: leave the dropped line's trailing comment attached to the surviving sibling if simplest; never delete a comment-bearing line silently.

After fmt, `G::analyze::duplicate-import` does not fire on the rewritten source.

### #108 Unused import removal

**Stratum:** AST file-level.
**Needs Analyze:** yes (`referenced_names`).

For each top-level import line:

- Whole-module import whose namespace is never referenced → drop the line.
- Selective import: trim selector names that are never referenced. If all selectors trimmed, drop the line. If at least one selector remains, keep the line with the trimmed list.

Order preservation: surviving names keep their source order.

Comment preservation: same rule as #107.

After fmt, `G::analyze::unused-import` does not fire on the rewritten source.

### #110 Stdlib auto-import

**Stratum:** AST file-level.
**Needs Analyze:** yes (`unresolved_names`).

Source of truth: the existing stdlib registry in `glyph-core` (single canonical list of `subagent`, `send`, `load` per `design/stdlib.md`). No fuzzy matching — exact name match only.

For each unresolved name that exactly matches a stdlib export:

- If the file has a selective `import "@glyph/std" { … }` line, append the missing name to its selector list (in source order, deduped).
- Otherwise, insert a new selective `import "@glyph/std" { … }` line at the top of the imports block (or at the top of the file if no imports block yet).

Does not fire when:

- Name resolves to a local declaration (e.g., user wrote their own `const subagent = …` — leave alone).
- Name doesn't match any stdlib export.

After fmt, `G::analyze::undefined-name` does not fire for stdlib names that the registry could have resolved.

### #111 Const-in-flow parens-add

**Stratum:** per-decl, inside `rewrite_decl_body` (or sibling).
**Needs Analyze:** yes (`referenced_names` + binding info).

For each `flow:` body, find bare names (no surrounding parens, no keyword prefix) that:

1. Don't resolve as a local binding (`const`, parameter, etc.) in scope.
2. Don't resolve as an imported name.

Rewrite `name` → `name()`. The downstream `undefined-call` diagnostic from `name()` body generation stays in the LLM repair pass — that's semantic work and out of scope here (PRD §Solution).

No-op when the bare name resolves locally (PRD US 20).

After fmt, the original `bare-name-in-flow` repairable goes away; whatever further repair the now-`name()` call needs (if any) is the LLM's problem.

### #112 Effects auto-insert

**Stratum:** per-decl, inside `rewrite_decl_body`.
**Needs Analyze:** yes (inferred effect set per decl).
**Gated on `enable_effects = true`.** When `enable_effects = false`, the parser rejects `effects:` sub-sections, so insertion is meaningless.

For each declaration that:

1. Has no `effects:` sub-section in source.
2. Has a non-empty inferred effect set from analyze.

Insert the `effects:` sub-section with the inferred set, placed in the canonical sub-section position (the existing reorder pass handles ordering).

No-op when:

- User wrote `effects:` already (PRD US 21) — leave their declared set alone, even if it disagrees with inferred.
- Inferred set is empty.
- `enable_effects = false`.

After fmt, `G::analyze::missing-effects` does not fire on the rewritten source.

### #113 Placeholder return rewrite — already implemented

`fmt.rs` already contains:

- `placeholder_string_return_target` (line ~469)
- `flow_placeholder_target`, `return_expr_placeholder_target`
- `placeholder_identifier`, `placeholder_description`
- `is_domain_return_type`, `rewrite_placeholder_return_line`
- Tests `fmt_source_rewrites_placeholder_string_return_to_output_target`, `fmt_source_rewrites_descriptive_placeholder_string_return_to_output_target`, `fmt_source_leaves_placeholder_string_return_with_inner_quotes_unrewritten`, etc.

Behavior matches issue #113's acceptance criteria with one nuance:

- The descriptive-form path **refuses to rewrite** placeholders whose inner content contains `"`, `\`, `\n`, `\t`, or `\r` (rather than escaping them per the issue's "correctly escapes" criterion). This is conservative — the diagnostic remains, but no malformed output is produced. Defensible behavior; we record it here rather than chase the escape semantics.

This PR closes #113 with no code change.

## Analyze reuse

Per PRD §Implementation Decisions, option (B): run Analyze inside `fmt_source` rather than re-implementing resolver / effect inference.

Concretely, expose two structured outputs from analyze for fmt's consumption:

- `referenced_names: HashSet<String>` — set of names referenced anywhere in the file (top-level + per-scope as needed). Drives unused-import (#108), stdlib auto-import (#110), const-in-flow (#111).
- `inferred_effects: HashMap<DeclId, EffectSet>` — analyzer's inferred effect set per declaration. Drives effects auto-insert (#112).

If analyze can't produce these for some declarations (e.g., post-parse-recovery damage), the dependent fixes become no-ops on those decls; other fixes still run.

Trade-off acknowledged in PRD: `glyph fmt` becomes dependent on analyze succeeding far enough to produce these signals. Acceptable because both signals come from passes that already run after parse-recovery and don't fatal-error on the inputs we care about.

## Agent skill update (#114)

Two design-doc edits, no runtime change:

1. **`design/agent-skill.md`** — Phase 3a list and Phase 3b table:
   - Phase 3a section: list the seven now-deterministic fixes (six landing in this PR + #109 already shipped) so it's clear what fmt covers.
   - Phase 3b §"Repair Guidance" table: remove the seven now-deterministic items so the table lists only what the LLM still has to do.
   - Existing wording at line 24 already says "fmt runs exactly once at top, before first compile, doesn't re-run between LLM iterations" — keep that placement.

2. **`REPAIR_PASS_SPEC.md`** — drop the seven now-deterministic items per PRD §Further Notes, so the file lists only LLM-pass repairs.

PRD #106's casual phrasing ("on compile exit 2, run fmt, re-run compile") was reconciled during design: the existing fmt-up-front placement wins because:

- fmt is cheap (parse + AST walk + serialize) and idempotent on clean source.
- Up-front fmt gives every later step a canonical baseline (indentation, section order).
- Exit-2 placement adds work in the dirty case (compile → fmt → compile = 2 compiles), only saving time on clean source — wrong direction.
- Agent state machine stays simple.

## Testing

### Per-fix unit tests in `fmt.rs`

Pattern follows existing `strip_none_return_*` and `fmt_source_rewrites_placeholder_string_return_*` tests: build a source string, call `fmt_source`, assert on `output` and `changed`.

Each new auto-fix gets:

- One golden-path test (canonical case fires, output matches expected).
- One no-op test (target diagnostic absent → output equals input, `changed == false`).
- One idempotence test (fmt twice → identical output).

Plus per-fix edge cases:

- **#107 duplicate import:** disjoint vs overlapping selector lists; whole-module + selective same path; comment on duplicated line.
- **#108 unused import:** selective import with mixed used/unused names; whole-module unreferenced; whole-module referenced via at-least-one symbol.
- **#110 stdlib auto-import:** existing `@glyph/std` line vs no stdlib import; user-shadowed name (no-op); name not in registry (no-op).
- **#111 const-in-flow:** unresolved bare name (rewrite); locally-bound name (no-op); imported name (no-op).
- **#112 effects auto-insert:** missing sub-section + non-empty inferred (rewrite); user-declared `effects:` even disagreeing (no-op); empty inferred (no-op); `enable_effects = false` (no-op).
- **#113 placeholder return:** already covered by existing tests; no new tests.

### Integration test

One test at `glyph-core` lib level: source containing several auto-fixable diagnostics → `fmt_source` → re-run `compile` (in-process via lib API) → exit 0. Proves the agent loop's first iteration converges on multi-fix files.

### CLI tests

`crates/glyph-cli/tests/fmt.rs` already exercises the `glyph fmt` subcommand. Add one corpus fixture exercising the multi-fix integration case end-to-end.

## Comment & order preservation

Inherited from `fmt.rs` contract:

- Comments are not deleted, moved, or rewritten (per `design/repair.md` §4.1).
- Source order is preserved; canonical sub-section reorder is the only structural reorder (PRD US 13).
- Idempotent: fmt twice = fmt once (PRD US 14).
- Safe on clean source: no rewrites when no target diagnostics present (PRD US 15).

## Open follow-ups (deferred, not blocking)

- Operator-in-expression deterministic rewrite (`x + y` → `combine(x, y)`) — captured in PRD §Out of Scope, stays in 3b for now.
- If the stdlib registry grows post-MVP (more than `subagent`, `send`, `load`), the auto-import auto-fix grows automatically because it reads the registry as source of truth.

## Acceptance criteria (rolled up from sub-issues)

- All sub-issue acceptance criteria for #107, #108, #110, #111, #112 are met by their golden-path / no-op / idempotence / edge-case tests in `fmt.rs`.
- Integration test passes: multi-fix source → fmt → compile exit 0.
- `design/agent-skill.md` and `REPAIR_PASS_SPEC.md` reflect the new Phase 3a / 3b split.
- `glyph fmt` is byte-deterministic across runs (PRD US 8).
- Committing post-fmt source means CI sees no `repairable` diagnostics for any of the seven covered categories (PRD §Further Notes).
