# BUG-037: Comment stripper mis-tracks string state on escaped quotes, rejecting valid strings

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/tokenize.rs:538-551`
**Found by:** tokenize | **Audit date:** unknown-date

## Description

`strip_trailing_comment` toggles `in_string` on every `"` byte but ignores backslash escapes, so it treats an escaped quote `\"` (a literal quote inside the string) as a string terminator. When an inline string contains an odd number of `\"` escapes followed by a `//` sequence that is itself inside the string, the stripper flips `in_string` to false at the escaped quote and then truncates `content_end` at the in-string `//`. The inline string scanner (lines 300-334) then runs with this truncated `content_end`, never reaches the real closing `"`, and returns `TokenizeError::UnterminatedString`.

Concrete trigger: the valid line `    msg: "she said \"hi // bye"` (string value: `she said "hi // bye`) is rejected as an unterminated string. The language spec explicitly specifies `\"` as a valid escape sequence in inline strings, making this bug reachable on valid Glyph source.

The function's own doc comment acknowledges the limitation: "the walking skeleton has no `//` characters inside string literals, so the simple lexical scan is safe here" — making this an acknowledged simplification that is now violated by the documented language surface. The `fmt.rs` counterpart correctly uses `prev != '\\'` to handle this case.

## Trigger / Reproduction

A `.glyph` file containing an inline string with an escaped quote followed by `//` inside the string value:

```
skill s()
    flow:
        msg: "she said \"hi // bye"
```

`glyph check` returns `TokenizeError::UnterminatedString` on the `msg:` line despite the string being syntactically valid.

## Evidence

```rust
if b == b'"' {
    in_string = !in_string;
} else if !in_string && b == b'/' && bytes[p + 1] == b'/' {
    return p;
}
```

No backslash-escape check precedes the quote-toggle. A `\"` inside a string flips `in_string` to false, so the subsequent `//` passes the `!in_string` guard and the function returns early, truncating `content_end` before the real closing quote.

## Recommended Resolution

Add a backslash-escape skip before the quote/comment checks inside `strip_trailing_comment`:

```rust
if in_string && b == b'\\' {
    p += 2;
    continue;
}
```

This mirrors how `fmt.rs` handles this case (`prev != '\\'`) and ensures escaped quotes do not flip `in_string`, so an in-string `//` is never mistaken for a comment.

## Verification Notes

The `strip_trailing_comment` function toggles `in_string` on every `"` byte with no backslash-escape handling. Concrete simulation confirms: for the source line `msg: "she said \"hi // bye"`, the stripper sees the opening `"` (in_string=true), the `\"` at byte offset 20 (in_string flips to false), then the `//` at offset 24 with in_string==false returns offset 24 as content_end. The inline scanner then runs up to content_end=24 and never finds the real closing `"` at offset 30, setting closed=false and returning `TokenizeError::UnterminatedString`. The `fmt.rs` counterpart at line 1194 correctly uses `prev != '\\'` to handle this case. The proposed fix is correct and sufficient.
