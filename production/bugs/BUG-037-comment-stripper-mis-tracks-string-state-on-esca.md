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

## Independent Agent Finding

**Verdict:** Reproduced. The bug is real in the tokenizer. The report's underlying state-tracking analysis and suggested resolution are correct.

**Reproduction/Refutation:** I used Graphify first; it pointed to `crates/glyph-core/src/tokenize.rs:538` and the formatter counterpart at `crates/glyph-core/src/fmt.rs:1194`. I then reproduced the failure with a valid flow inline-string fixture:

```glyph
skill s()
    flow:
        "she said \"hi // bye"
```

Command summary: `cargo run -q -p glyph-cli -- check tmp/bug-037-valid-flow-repro.glyph --format json` exited 1 and emitted `G::parse::unterminated-string` at line 3, column 9. Two controls isolate the combined trigger: `"she said \"hi / bye"` and `"she said hi // bye"` both reached analyze and emitted only `G::analyze::missing-description` repairable diagnostics, not tokenizer/parser string errors.

I also ran the report's `msg: "she said \"hi // bye"` shape; it reproduced `G::parse::unterminated-string`, but the current language guide describes flow assignments as `x = ...` and inline strings as standalone flow statements. The bare inline-string fixture above is the cleaner valid-source reproduction.

**Evidence:** `tokenize.rs` still strips comments before token scanning with `strip_trailing_comment(bytes, k, line_end)`. The tokenizer helper at `crates/glyph-core/src/tokenize.rs:538-551` toggles `in_string` on every `"` byte and has no escape handling before the `//` check. The inline string scanner at `crates/glyph-core/src/tokenize.rs:311-324` does treat `\"` as a valid escape, so the comment stripper's earlier truncation is what prevents the valid scanner path from seeing the real closing quote. The formatter helper at `crates/glyph-core/src/fmt.rs:1194-1211` already avoids toggling on an escaped quote via `prev != '\\'`, matching the report's comparison.

**Resolution Input:** Preserve the existing suggested resolution: make the tokenizer's `strip_trailing_comment` skip escaped characters while inside strings before considering quote toggles or `//` comment starts. Add a regression test using the valid flow inline-string form above, plus controls for escaped quotes without `//` and in-string `//` without escaped quotes.
