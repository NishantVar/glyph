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
