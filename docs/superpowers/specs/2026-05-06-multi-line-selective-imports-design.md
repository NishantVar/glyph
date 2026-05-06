# Multi-line Selective Imports — Design

**Date:** 2026-05-06
**Tracking issues:** [#116](https://github.com/NishantVar/glyph/issues/116) (PRD), [#117](https://github.com/NishantVar/glyph/issues/117) (implementation slice)
**Status:** Ready for implementation planning

## Source of truth

The substantive design lives in **GitHub issue #116**. This document does not
restate it. Read #116 first; this file is a corrigendum capturing what was
verified, what changed since the PRD was written, and the corrections the
implementation must fold in.

## Verified still accurate

- The bug exists at parser level. The `TokenKind::Lbrace` arm of
  `Parser::parse_import` (`crates/glyph-core/src/parse.rs:917-951`) has no
  `LineStart` handling between items, so a newline inside the brace list
  fails to parse.
- `LineStart { indent }` is still emitted at every newline
  (`crates/glyph-core/src/tokenize.rs:139`); comment-only lines emit zero
  tokens, so comments inside the brace list will work for free.
- AST/IR/lowering/LSP scope claims hold: `ImportDecl` and `ImportName`
  shapes are unchanged; the change is local to one parser branch.
- Test-module convention (`#[cfg(test)] mod <topic>_tests` at the bottom of
  `parse.rs`) is still in use — the new `mod import_decl_tests` will follow
  it.
- `GLYPH_LANGUAGE_GUIDE.md` §5.5 is `import` (line 195) — correct target
  for the multi-line example block.

## Corrections to fold in

### 1. Test-module line numbers (off by ~700 — file has grown)

| #116 cites | Actual |
| --- | --- |
| `mod const_decl_tests` at `parse.rs:2601` | **`parse.rs:3344`** |
| `mod none_return_tests` at `parse.rs:2844` | **`parse.rs:3587`** |
| `mod output_target_return_tests` at `parse.rs:3335` | **`parse.rs:4078`** |
| `mod export_block_terminal_return_tests` at `parse.rs:3539` | **`parse.rs:4330`** |

Implementation/plan should reference the actual locations.

### 2. Post-parse leftover-token scan ranges

| #116 cites | Actual |
| --- | --- |
| Arrow scan at `parse.rs:235-258` | **`parse.rs:238-258`** (cosmetic shift) |
| `<`-scan at `parse.rs:263-277` | **`parse.rs:260-305`** |

### 3. PR #140 partially mitigated the cascade already

Commit `1ff3b0f` (PR #140, merged after #116 was authored) scoped the
post-parse `<`-scan to tokens at-or-before the parse-failure offset
(`parse.rs:274-295`). For a multi-line-import failure, the failure offset
sits at the `LineStart` inside `{ … }`, which precedes any later
`<output_target>` site, so the `output-target-outside-return` half of the
cascade is **already suppressed today**.

The Arrow scan still runs unconditionally (see comment at
`parse.rs:202-208`: "we want the post-parse scan to run whether
`parse_file` succeeded or failed"), so the `operator-in-expression` half
of the cascade still mis-fires today on a `return … -> target` later in
the same file.

**Implication for AC test #7 (`#117`):** the regression assertion
("zero diagnostics; specifically neither `output-target-outside-return`
nor `operator-in-expression`") is still meaningful — once `parse_import`
succeeds, both consumed-offset registries (`consumed_arrows` and
`consumed_output_targets`) are populated, so neither scan fires. Keep the
test wording; just be aware that one of the two named cascades is
*already* protected by an earlier mechanism, and this fix makes the other
one impossible.

### 4. Wrong design-doc target

#116 says "`design/expand.md` §5.5 Imports" should gain a normative
bullet. That section in `design/expand.md` is actually **"Retry Budget
Constants"** — `expand.md` is about Phase 6b validation, not language
surface.

The correct target is **`design/language-surface.md` §3.5 `import`**
(line 279). Add the normative bullet there. Optionally also touch
`design/imports.md` (which covers semantic rules — paths, cycles,
unused-removal — and currently has no §about syntactic shape; a one-line
cross-reference is sufficient).

### 5. `lib.rs` import test references

#116 cites `lib.rs:3411`, `:3437`, `:3481` as prior-art import integration
tests. `lib.rs` is now 5057 lines (vs. ~3500 when #116 was written), so
those line numbers are very likely shifted. They are mentioned only as
prior-art context, not as edit targets — implementation does not need
them, but if the plan or PR references them, refresh the line numbers.

## Scope confirmed unchanged from #116

- Helper: private `Parser::skip_line_starts` advancing `self.pos` past
  consecutive `LineStart` tokens.
- Three call sites in the selective-import branch: after `{`, after each
  `,`, before the closing `}` check.
- Items remain atomic: no `LineStart` skipping inside an item
  (`name`, `as`, `alias`).
- Final `expect(&TokenKind::Rbrace)` replaced with peek-and-match emitting
  `ParseError::Unexpected { message: "expected ',' or '}' after import name", … }`.
- No tokenizer change. No AST/IR change. No LSP, lowering, or downstream
  change.
- Seven new tests in `mod import_decl_tests` at the bottom of `parse.rs`.
- Doc updates: `design/language-surface.md` §3.5 (corrected target),
  `GLYPH_LANGUAGE_GUIDE.md` §5.5.

## Out of scope (unchanged from #116)

- Tokenizer changes.
- AST/IR shape changes for `ImportDecl` / `ImportName`.
- LSP behavior changes.
- Lowering, codegen, or compilation-pass changes.
- Multi-line behavior for any other brace-delimited construct.
- Allowing `name as alias` to span multiple lines.
- Indent-style enforcement inside the brace list.
- Generalizing the `"expected ',' or '}'"` diagnostic improvement to other
  comma-separated lists.
- Touching the existing `convert_md_to_glyph.glyph.md` authoring file.
