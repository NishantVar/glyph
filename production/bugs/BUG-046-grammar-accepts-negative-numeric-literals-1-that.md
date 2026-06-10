# BUG-046: Grammar accepts negative numeric literals (-1) that the compiler tokenizer rejects

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `tree-sitter-glyph/grammar.js:649-650`
**Found by:** x-grammar-parity | **Audit date:** unknown-date

## Description

`integer_literal: /-?[0-9]+/` and `float_literal: /-?[0-9]+\.[0-9]+/` both allow an optional leading `-`. The compiler tokenizer never lexes a sign as part of a numeric literal: the only `-` handling consumes `->` (in `tokenize.rs`); a bare `-` falls through to the `UnexpectedChar` arm (confirmed by the `tokenize_stray_minus_still_unexpected` test).

Thus `const x = -1` parses in tree-sitter as a clean `(integer_literal)`, but the compiler reports `G::parse::operator-in-expression` for the `-` (verified with live compiler invocation).

The grammar and compiler disagree on whether negative literals exist. Additionally, `GLYPH_LANGUAGE_GUIDE.md` §9.2 (line 934) says "Negative literals allowed: `-1`", meaning the compiler also contradicts its own user-facing docs. The canonical design documents (`design/values-and-names.md`, `design/language-surface.md`) explicitly state signed numeric literals are "deferred beyond MVP."

## Trigger / Reproduction

```
# In a .glyph file:
const x = -1

# Compiler output:
warning[G::parse::operator-in-expression]: operator '-' is not supported in expressions

# Tree-sitter parse: clean (integer_literal) node with no error
```

## Evidence

```javascript
// tree-sitter-glyph/grammar.js lines 649-650
integer_literal: (_) => /-?[0-9]+/,
float_literal: (_) => /-?[0-9]+\.[0-9]+/,
```

Compiler tokenizer never lexes a leading `-` as part of a numeric literal. The `tokenize_stray_minus_still_unexpected` test in `tokenize.rs` (line 669) confirms `-` is always treated as an unexpected character when not part of `->`.

Design docs:
- `design/values-and-names.md` L77: signed numeric literals "deferred beyond MVP"
- `design/language-surface.md` L296: "the tokenizer rejects a leading `-` on a numeric literal today"
- `GLYPH_LANGUAGE_GUIDE.md` L934: "Negative literals allowed: `-1`" (contradicts the above)

## Recommended Resolution

Decide the canonical behavior and align all three:
- **If negatives are intended (per the user guide):** implement leading-`-` lexing in `tokenize.rs` before a digit sequence, and add corpus tests.
- **If negatives are deferred (per the design docs):** drop the `-?` from both regexes in `grammar.js`, update `GLYPH_LANGUAGE_GUIDE.md` §9.2 to state negatives are not yet supported, and add a corpus test for negative literals that expects ERROR.

## Verification Notes

Live reproduction confirmed: `cargo run -p glyph-cli -- compile` on a file with `const x = -1` emits `warning[G::parse::operator-in-expression]` for both `-1` and `-1.5`. The grammar at lines 649-650 uses `/-?[0-9]+/`, accepting these as single tokens without error. No tree-sitter corpus test covers negative literals, so the grammar's permissiveness is untested. This is a real three-way divergence (grammar vs. compiler vs. user guide) that silently degrades valid-per-guide input.
