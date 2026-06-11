# BUG-068: param-slot/description slot diagnostic computes wrong source span for triple-quoted (block) strings and strings with escapes

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/parse.rs:2934-2935`
**Found by:** parse-1 | **Audit date:** unknown-date

## Description

The slot-in-non-instruction-string diagnostics map the in-content byte offset back to a source span via `let span_start = lit_span.start + 1 + slot.start_in_content as u32` (description at line 2934; parameter default at line 2701). The `+ 1` assumes the literal token starts at a single `"` so content begins exactly one byte later. But `scan_triple_string` starts content 3 bytes after the token's `start` (`p = start + 3`), and additionally applies `dedent_block_string`/`strip_block_newlines` plus escape decoding, so `slot.start_in_content` (an offset into the decoded/dedented content) does not map linearly to a source byte. For a block-string description like `description: """text {foo}"""`, the caret is at least 2 bytes too early; with multi-line dedent or escapes before the slot it can be off by many bytes and land outside the literal. The diagnostic ID/classification are still correct and nothing panics — the span is only used for caret display — so user impact is a mis-positioned underline. The code comment at line 2699-2700 acknowledges "only meaningful for ASCII content".

## Trigger / Reproduction

Write a description field using a triple-quoted block string containing a `{slot}` reference:

```
description: """
  some text {param}
"""
```

The resulting `G::parse::slot-in-description` diagnostic will have its caret positioned at least 2 bytes too early (inside the `"""` prefix), worsened further by any indentation stripped by dedent or escape sequences preceding the slot.

## Evidence

```rust
for slot in scan_slots(&s) {
    // +1 wrong for """...""" (content at +3) and for pre-slot escapes/dedent
    let span_start = lit_span.start + 1 + slot.start_in_content as u32;
    let span = Span::new(self.file_id, span_start, span_start + 1);
```

## Recommended Resolution

Compute the slot span from the source text rather than the decoded content. Options:

1. Have the tokenizer record the source byte offset of the content start (1 for inline `"`, 3 for block `"""`) plus any escape/dedent adjustment, then use that recorded offset instead of a hardcoded `+ 1`.
2. At minimum, branch on inline vs. block string: detect by checking if the source at `lit_span.start` is `"""` and use `+ 3` for block strings instead of `+ 1`.
3. For full correctness, scan the raw source slice for the actual `{name}` bytes, bypassing the decoded-content offset entirely.

## Verification Notes

Both call sites (parse.rs:2701 and 2934) hardcode `+1`. The tokenizer produces the same `TokenKind::StringLit(String)` for both inline and triple-quoted strings with no distinguishing flag. Triple-quoted strings are confirmed valid in `description:` fields (tokenizer test `tokenize_triple_quoted_in_description_field`). `start_in_content` is an offset into the already-decoded-and-dedented string, not raw source bytes. For `"""text {foo}"""` the caret lands 2 bytes too early. Impact is cosmetic (mis-positioned underline only); diagnostic ID, message, and hints are all correct; nothing panics. Final severity is low per verifier consensus, though the bug is filed at medium due to the span being materially wrong for block strings.

## Independent Agent Finding

**Verdict:** Reproduced.

**Reproduction/Refutation:** I reproduced the wrong span in the current tree with scratch `.glyph` fixtures and `glyph check --format json`. The diagnostic ID, classification, message, and recovery behavior are correct; the reported span is the wrong byte/column for block strings and for decoded escapes before the slot.

**Evidence:**

- Source inspection with `rg`/bounded reads found both parser call sites still computing `span_start` from decoded content via `lit_span.start + 1`: `crates/glyph-core/src/parse.rs:2701` for parameter defaults and `crates/glyph-core/src/parse.rs:2934` for `description:`.
- `crates/glyph-core/src/tokenize.rs:431-447` confirms triple strings begin content scanning at `start + 3`, decode escapes, then apply `strip_block_newlines` and `dedent_block_string`. `crates/glyph-core/src/slot.rs:40-76` confirms `scan_slots` returns offsets in that decoded string content.
- `cargo run -p glyph-cli -- check --format json tmp/BUG-068-repro.glyph` on `description: """text {param}"""` reported span line 2 col 24; `awk` found the actual `{param}` starts at line 2 col 26.
- `cargo run -p glyph-cli -- check --format json tmp/BUG-068-repro-multiline.glyph` on a dedented multi-line block description reported span line 3 col 8; the actual `{param}` starts at line 3 col 17.
- `cargo run -p glyph-cli -- check --format json tmp/BUG-068-repro-param-default.glyph` on `skill foo(x = """text {param}""")` reported span line 1 col 21; the actual `{param}` starts at line 1 col 23.
- `cargo run -p glyph-cli -- check --format json tmp/BUG-068-repro-escape.glyph` on `description: "a\n {param}"` reported span line 2 col 22; the actual `{param}` starts at line 2 col 23.

**Resolution Input:** Preserve the existing recommended resolution. Option 2 (`+ 3` for block strings) would fix only the simple single-line triple-quote offset; it would not fix dedent/newline stripping or escape decoding. Option 3, scanning the raw source slice for the offending `{name}` bytes, is the most complete resolution.
