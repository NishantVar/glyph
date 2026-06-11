# BUG-027: Stray/unpaired backtick in resolved prose permanently drops the rest of the word count

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/expand.rs:31-44`
**Found by:** expand-cond | **Audit date:** unknown-date

## Description

`count_words` tracks an `in_backtick` flag that is set true whenever a whitespace-separated token starts with a backtick but is not itself a complete `` `foo` `` span (lines 31-34), and only reset when a later token ends with a backtick (lines 36-39). A single, unpaired backtick token in the prose (e.g. resolved body text `"use the \` character then do x y z"`, or any odd number of backtick-delimited spans) flips `in_backtick` to true and never clears it, so every subsequent word is treated as "inside a backtick span" and skipped.

Empirically: `count_words("use the \` char then x y z")` returns 3 instead of 7; the stray backtick swallows the rest of the string. Because `count_words` feeds the `wc >= 150` Tier-1 → Tier-2 promotion gate (expand.rs ~line 159), a block whose resolved prose contains a stray backtick can be undercounted below 150 and stay Tier 1 (inlined) when it should be promoted to a Tier 2 procedure section — silently producing different compiled output. Backticks have no escaping/special meaning inside Glyph string literals, so an author can easily place a lone backtick in prose.

## Trigger / Reproduction

Any skill block whose resolved prose string contains a standalone backtick character (not part of a balanced `` `...` `` span). The `count_words` function is called at lines 62 and 125 of `expand.rs`. A block that would otherwise cross the 150-word Tier-2 threshold may incorrectly stay Tier 1 and be inlined rather than promoted to a procedure section.

## Evidence

```rust
if !in_backtick && token.starts_with('`') {
    in_backtick = true;
    count += 1; // Opening backtick span counts as 1 word.
    continue;
}
if in_backtick && token.ends_with('`') {
    in_backtick = false; ...
```

## Recommended Resolution

The proposed fix of "reset `in_backtick` at end of input" is incorrect — the words inside the unterminated span were already skipped during iteration. A correct minimal fix is to change the condition at line 31 from `token.starts_with('`')` to `token.starts_with('`') && token.len() > 1`, so a standalone single-backtick token is treated as an ordinary word rather than opening a span. Alternatively, add an `else if token == "\`"` arm that does `count += 1` before the general `starts_with` arm. A more thorough fix would buffer words inside a potential span and only suppress them if the span actually closes. The existing test `count_words_backtick_span` only exercises balanced spans and should be extended to cover the unbalanced case.

## Verification Notes

A lone backtick token (single character `` ` ``) passes the `starts_with('`')` check at line 31 but fails the combined `starts_with && ends_with && len >= 2` check at line 26 (because `len == 1`). This sets `in_backtick = true` and counts 1 word, then every subsequent whitespace-separated token is skipped. Python simulation of the exact algorithm confirms `count_words("use the \` char then x y z")` returns 3 instead of 7. The `body_text` field reaches `count_words` without any sanitization, and the tokenizer treats backticks as ordinary characters within string literals (no rejection). The `wc >= 150` gate is only reached when `is_tier2` is already false, meaning a simple block with a lone backtick in its prose could be incorrectly kept at Tier 1.

## Independent Agent Finding

**Verdict:** Reproduced.

**Reproduction/Refutation:** I used Graphify first to locate `count_words()` in `crates/glyph-core/src/expand.rs` and confirm it feeds `expand_step1_with_imported_descriptions()`. I then ran a scratch Cargo binary under `tmp/bug027_repro` that path-depended on the real `glyph-core` crate and called `glyph_core::expand::count_words` directly; no production source files were edited.

**Evidence:** Command run:

```sh
cargo run --quiet --manifest-path tmp/bug027_repro/Cargo.toml
```

Summarized output:

```text
sample="use the ` char then x y z"
sample_split_count=8
sample_count_words=3
threshold_plain_split_count=150
threshold_count_words=3
threshold_promotes_by_wc=false
```

This reproduces the permanent undercount: once the standalone backtick token is seen, later tokens are skipped as if they were inside a code span. The threshold case also demonstrates the Tier-2 promotion risk: a body with 150 whitespace-separated tokens is counted as 3, so the `wc >= 150` promotion condition would not fire. One nuance: the sample sentence has 8 whitespace-separated tokens if the standalone backtick token is counted as an ordinary word; if policy ignores that punctuation token, the expected count would be 7. Either interpretation still reproduces the reported bug because the current result is 3.

**Resolution Input:** Preserve the existing suggested resolution. Do not use an end-of-input reset as the fix, because the skipped words have already been lost. A targeted regression should exercise an unbalanced standalone backtick token and the 150-word promotion boundary.
