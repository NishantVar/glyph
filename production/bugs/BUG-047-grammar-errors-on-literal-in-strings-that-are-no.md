# BUG-047: Grammar errors on literal '{' in strings that are not valid {identifier} interpolations

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `tree-sitter-glyph/grammar.js:586-646`
**Found by:** x-grammar-parity | **Audit date:** unknown-date

## Description

In the grammar, every `{` inside a `string_literal` or `block_string` must begin an `interpolation` = `seq(token.immediate("{"), $.identifier, "}")`, because `string_content`/`block_string_content` exclude `{`. The compiler tokenizer treats the entire `"..."` as one opaque `StringLit` and does not parse or validate interpolation slots at tokenize time (`tokenize.rs` lines 298-339).

So any string containing a `{` not immediately followed by a valid `identifier}` is an ERROR in tree-sitter but valid for the compiler. Trigger: `"value is {0}"` parses cleanly in the compiler (exit 0) but tree-sitter emits `(string_literal (string_content) (ERROR (integer_literal)))` because `0` is not an identifier (verified). Same for JSON-like content `"JSON: {\"key\": 1}"`.

Literal braces, format placeholders like `{0}`, and embedded JSON are common in instruction prose, so editors will show frequent false parse errors on valid `.glyph` files.

Additionally, the `extras` rule includes whitespace, so `{ name }` with spaces is silently treated as a valid interpolation by tree-sitter, while the compiler treats it as literal text — a secondary semantics mismatch.

## Trigger / Reproduction

Parse a `.glyph` file containing a string with a non-identifier brace expression:

```
skill test()
    description: "value is {0}"
    flow:
        "value is {0}"
```

Tree-sitter emits `(ERROR [line, col] - [line, col])` for the `{0}` portion. The compiler compiles the file cleanly with exit 0.

## Evidence

```javascript
// tree-sitter-glyph/grammar.js line 593
string_content: (_) => token.immediate(prec(1, /[^"\\{]+/)),
// '{' is excluded — every '{' must start a valid interpolation

// tree-sitter-glyph/grammar.js lines 641-646
interpolation: ($) =>
  seq(
    token.immediate("{"),
    $.identifier,    // identifier: /[a-zA-Z_][a-zA-Z0-9_]*/
    "}",
  ),
// '{0}', '{}', '{"key": 1}' → ERROR: '0' / '' / '"key"' not an identifier
```

Compiler tokenizer (`tokenize.rs` lines 298-339): hand-written byte scanner pushes every character including `{` into the string buffer — no interpolation parsing at tokenize time. `slot.rs` explicitly skips malformed braces (digit-start, empty, no closing brace) per design intent.

## Recommended Resolution

Allow a literal `{` that does not form a valid `identifier}` interpolation. Add a fallback `_literal_brace` token with negative precedence — mirroring the `_embedded_quotes` trick already used for `block_string` — so non-interpolation braces fall back to content instead of ERROR:

```javascript
_literal_brace: (_) => token.immediate(prec(-1, "{")),
```

And add it to the `string_literal` repeat's `choice`. Update corpus tests to cover `{0}`, `{}`, and `{ name }` cases.

## Verification Notes

Live reproduction confirmed: `tree-sitter parse` on a file containing `"value is {0}"` produces `(ERROR [2, 18] - [2, 21])` with exit 1. The compiler compiles the same file cleanly. None of the workspace crates depend on tree-sitter; the LSP uses `glyph_core::check_source_with_resolutions`. Impact is limited to editor syntax-highlighting plugins (Neovim/Helix/VS Code/Zed) using tree-sitter-glyph. The `extras` whitespace issue creates a secondary mismatch where `{ name }` is a valid interpolation in tree-sitter but literal text in the compiler.
