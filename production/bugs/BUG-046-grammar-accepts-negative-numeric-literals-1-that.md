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

## Independent Agent Finding

### Verdict

Reproduced. The report is still valid in the current workspace: tree-sitter accepts negative integer and float literals as clean literal nodes, while the compiler-side tokenizer/parser rejects the leading `-` with `G::parse::operator-in-expression`.

### Reproduction/Refutation

Scratch input used:

```glyph
const x = -1
const y = -1.5
```

Targeted compiler check:

```text
$ cargo run -p glyph-cli -- check tmp/bug046-negative.glyph
warning[G::parse::operator-in-expression]: operator `-` is not supported in expressions; MVP Glyph has no value-level operators
  --> tmp/bug046-negative.glyph:1:11
```

The two-line compiler check stops after the first repairable diagnostic, so the float case was checked in isolation:

```text
$ cargo run -p glyph-cli -- check tmp/bug046-negative-float.glyph
warning[G::parse::operator-in-expression]: operator `-` is not supported in expressions; MVP Glyph has no value-level operators
  --> tmp/bug046-negative-float.glyph:1:11
```

Tree-sitter parse of the two-line file from `tree-sitter-glyph/`:

```text
$ tree-sitter parse ../tmp/bug046-negative.glyph
(source_file
  (const_declaration
    name: (identifier)
    value: (integer_literal))
  (const_declaration
    name: (identifier)
    value: (float_literal)))
```

No `ERROR` node was emitted by tree-sitter.

### Evidence

Current grammar still allows a leading sign:

```javascript
integer_literal: (_) => /-?[0-9]+/,
float_literal: (_) => /-?[0-9]+\.[0-9]+/,
```

Current compiler tokenizer still only enters numeric tokenization when the current byte is an ASCII digit, and the `->` branch comment explicitly says bare `-` falls through to `UnexpectedChar` to preserve `G::parse::operator-in-expression`. The existing `tokenize_stray_minus_still_unexpected` test also expects `TokenizeError::UnexpectedChar { ch: '-', .. }`.

The documentation split also remains:
- `design/values-and-names.md` says signed numeric literals are deferred beyond MVP.
- `design/language-surface.md` says negative numeric literals are not yet supported.
- `GLYPH_LANGUAGE_GUIDE.md` says negative integer literals are allowed.

### Resolution Input

Preserve the existing recommended resolution. My input is that the compiler tokenizer and design docs agree on "negative literals deferred", while the tree-sitter grammar and user guide are the outliers. Unless there is a deliberate product decision to support signed numeric literals now, the lower-risk alignment is the second branch above: remove `-?` from the tree-sitter integer/float regexes, update the language guide, and add a negative-literal tree-sitter corpus case that expects an error.
