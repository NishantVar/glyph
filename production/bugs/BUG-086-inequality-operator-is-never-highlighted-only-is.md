# BUG-086: Inequality operator != is never highlighted (only == is captured)

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `tree-sitter-glyph/queries/highlights.scm:110`
**Found by:** treesitter | **Audit date:** unknown-date

## Description

The `comparison` rule in `grammar.js` (line 449) is `prec(4, seq($._condition_atom, choice("==", "!="), $._condition_atom))`, so both `==` and `!=` are valid anonymous tokens. `highlights.scm` captures `"==" @punctuation.special` (line 110) but has no entry for `"!="`.

Concrete trigger: a condition such as `if mode != "plan"` leaves the `!=` operator with no highlight capture, so it renders as default/unstyled text while `==` in the same file is styled. This is a cosmetic correctness gap (inconsistent highlighting of a valid operator), not a parse failure.

## Trigger / Reproduction

Write a `.glyph` file containing a condition using `!=`, e.g. `if mode != "plan"`. Open in any editor that uses the tree-sitter grammar for highlighting (nvim-treesitter, Helix, `tree-sitter highlight` CLI). The `!=` token renders as unstyled default text while `==` in the same file receives `@punctuation.special` styling.

## Evidence

```scheme
; highlights.scm lines 107-110
"=" @punctuation.special
"->" @punctuation.special
"." @punctuation.special
"==" @punctuation.special
; no entry for "!=" — the other arm of grammar.js comparison rule choice("==", "!=")
```

## Recommended Resolution

Add `"!=" @punctuation.special` alongside the existing `"=="` capture in `highlights.scm`:

```scheme
"==" @punctuation.special
"!=" @punctuation.special
```

## Verification Notes

Grammar.js line 449 confirms both `==` and `!=` are valid anonymous tokens in the `comparison` rule. `highlights.scm` line 110 captures `"==" @punctuation.special` but there is no entry for `"!="` anywhere in the file. This means `!=` in a condition like `if mode != "plan"` is parsed correctly but receives no syntax highlight capture, rendering as unstyled text while `==` is styled. The bug is purely cosmetic (a highlighting gap, not a parse or compile failure), consistent with the claimed low severity.

## Independent Agent Finding

**Verdict:** Reproduced. The report is valid.

**Reproduction/Refutation:** I used a temporary `.glyph` file containing adjacent comparison conditions with `==` and `!=`, then ran `tree-sitter highlight -H --css-classes -p tree-sitter-glyph --scope source.glyph tmp/bug-086-highlight.glyph`. The command exited 0 and reproduced the mismatch: the equality operator was emitted as `<span class='punctuation special'>==</span>`, while the inequality line rendered `risk != <span class='string'>&quot;low&quot;</span>` with `!=` outside any highlight span.

**Evidence:** Graphify first located the generated `comparison` grammar node under `tree-sitter-glyph/src/grammar.json`. Bounded exact checks then showed `tree-sitter-glyph/grammar.js:449` accepts `choice("==", "!=")`, while `tree-sitter-glyph/queries/highlights.scm:110` only contains `"==" @punctuation.special`. `rg -n '"=="|"!="|comparison' tree-sitter-glyph/grammar.js tree-sitter-glyph/queries/highlights.scm` returned no `"!="` capture in `highlights.scm`. The existing corpus also includes an inequality comparison case at `tree-sitter-glyph/test/corpus/control_flow.txt:131-138`, so the operator is valid grammar input rather than an invalid parse edge.

**Resolution Input:** Preserve the existing recommended resolution: add `"!=" @punctuation.special` next to the existing `"==" @punctuation.special` capture. No source code change was made because this investigation's write scope is limited to this bug report.
