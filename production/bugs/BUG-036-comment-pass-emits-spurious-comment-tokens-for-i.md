# BUG-036: Comment pass emits spurious Comment tokens for `//` inside multi-line block strings

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/semantic_tokens.rs:399-434`
**Found by:** semtok-typepos | **Audit date:** unknown-date

## Description

`classify_comments` tracks an `in_string` flag to avoid treating `//` inside a string as a comment, but it resets `in_string = false` on every `\n` (line 406). That works for single-line strings, but a triple-quoted block string (`"""..."""`) spans multiple lines: the opening `"""` is on one line and content lines have no preceding quote on the same line. On a content line, `in_string` has been reset to false, so a `//` there is mistaken for a comment opener and a `Comment` token is emitted.

The tokenizer does NOT strip `//` inside block strings (it consumes the whole block via `scan_triple_string`), and the lex pass correctly emits a `GlyphFlowString` segment for that content line, so the result is two overlapping tokens on the same line with the `// ...` portion wrongly colored as a comment.

## Trigger / Reproduction

```
skill s()
    flow:
        """
        see // note
        """
```

The `// note` on the content line is emitted as `SemTokenType::Comment` even though it is inside the block string. The existing test `comment_inside_string_is_not_classified` only covers single-line strings and does not catch this case.

## Evidence

```rust
while p < bytes.len() {
    let b = bytes[p];
    if b == b'\n' {
        in_string = false;   // resets state mid-block-string
        p += 1;
        continue;
    }
    if b == b'"' { in_string = !in_string; ... }
    if !in_string && b == b'/' && ... bytes[p + 1] == b'/' { /* emit Comment */ }
```

The unconditional `in_string = false` at every `\n` means that for a triple-quoted block string, `in_string` is always `false` on content lines (after the newline following the opening `"""`), so any `//` sequence in the content is misidentified as a comment opener.

## Recommended Resolution

Make the comment scan aware of triple-quoted block strings: detect `"""` openers and keep `in_string` set across newlines until the matching closing `"""`, rather than unconditionally resetting `in_string` at every newline. Track an `in_triple` boolean separately — when `in_triple` is true, skip the newline reset entirely. Alternatively, reuse the lexer's actual token stream / string spans to mask out comment scanning inside any `StringLit` span, avoiding the need to re-scan raw bytes.

## Verification Notes

The `classify_comments` function resets `in_string = false` on every `\n`. For a triple-quoted block string, the three `"` bytes on the opening line toggle `in_string` as `false→true→false→true`, leaving it `true`, but the subsequent `\n` immediately resets it to `false`. On the next content line (e.g. `see // note`), `in_string` is `false`, so the `//` check fires and emits a spurious `Comment` token. A test insertion confirmed: `spurious Comment tokens inside block string: [RawSemToken { line: 3, start: 12, length: 7, token_type: 10 }]`. The existing tests `comment_inside_string_is_not_classified` and `triple_quoted_string_splits_into_per_line_tokens` do not cover this combination.

## Independent Agent Finding

**Verdict:** Reproduced.

**Reproduction/Refutation:** I used the existing public collector API directly against the reported fixture:

```glyph
skill s()
    flow:
        """
        see // note
        """
```

The temporary harness called `glyph_core::semantic_tokens::collect_semantic_tokens(src, 0)` and filtered emitted tokens for `SemTokenType::Comment`, `SemTokenType::GlyphFlowString`, and line 3. The harness was created under `tmp/bug036-repro/` and removed after the run.

**Evidence:** Graphify first located semantic-token/token context, and a bounded read confirmed `crates/glyph-core/src/semantic_tokens.rs` still has `classify_comments` reset `in_string = false` on every newline before scanning for `//`.

Command run:

```sh
cargo run --manifest-path tmp/bug036-repro/Cargo.toml --quiet
```

Summarized output:

```text
line3="        see // note"
comment_tokens=[RawSemToken { line: 3, start: 12, length: 7, token_type: 10, modifiers: 0 }]
flow_string_tokens=[..., RawSemToken { line: 3, start: 0, length: 19, token_type: 11, modifiers: 0 }, ...]
line3_tokens=[
  RawSemToken { line: 3, start: 0, length: 19, token_type: 11, modifiers: 0 },
  RawSemToken { line: 3, start: 12, length: 7, token_type: 10, modifiers: 0 }
]
```

This reproduces the reported overlap: the whole block-string content line is emitted as `GlyphFlowString` (`token_type: 11`) while the `// note` suffix is also emitted as `Comment` (`token_type: 10`).

**Resolution Input:** Keep the existing recommended resolution. The narrowest fix remains making the comment pass triple-quote-aware, or better, masking comment scanning with lexer string spans so `//` inside any `StringLit` cannot be classified as a comment.
