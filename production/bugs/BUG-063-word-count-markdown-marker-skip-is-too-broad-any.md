# BUG-063: Word-count markdown-marker skip is too broad: any token starting with '#' (and bare '-') is dropped

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/expand.rs:23-25`
**Found by:** expand-cond | **Audit date:** unknown-date

## Description

The marker-skip guard in `count_words` treats `token.starts_with('#')` as a heading marker and drops the whole token, and also drops a bare `-`. But the spec (`design/compiled-output.md §Word counting rule`) only excludes genuine Markdown formatting markers (`**`, list bullets, headings). A heading marker is `#`/`##`/... as a line-leading token — not any prose word that begins with `#`.

Concrete impact: `count_words("issue #42 is open")` returns 3 instead of 4; `count_words("a #hash b")` returns 2 instead of 3. Any resolved prose containing a `#`-prefixed token (issue refs like `#42`, anchors, channel names `#general`) or a standalone dash is undercounted, perturbing the 150-word tier-promotion boundary and potentially keeping a block at Tier 1 (inline) when it should be promoted to Tier 2 (procedure).

## Trigger / Reproduction

Author a skill block whose resolved prose contains `#`-prefixed tokens (e.g. `"See issue #42 for context."`) and whose word count sits near the 150-word boundary. The block may be kept at Tier 1 when Tier 2 promotion was intended.

## Evidence

```rust
if token == "**" || token == "-" || token.starts_with('#') {
    continue;
}
```

## Recommended Resolution

Only skip a token as a heading marker when it consists solely of `#` characters (matches `/^#+$/`), not when a prose word merely begins with `#`:

```rust
if token == "**" || token == "-" || token.chars().all(|c| c == '#') {
    continue;
}
```

Alternatively, perform heading/bullet detection based on line position rather than per-token prefix.

## Verification Notes

The guard at `expand.rs:23` is confirmed: `token.starts_with('#')` discards any whitespace-delimited token beginning with `#`, including `#42`, `#hash`, `#channel`. The word count feeds the `wc >= 150` tier-promotion gate; the four other conditions (`stmt_count >= 4`, `freq >= 2`, branches, freeform sections) are checked before the word-count gate, so only blocks passing all four would be mis-tiered — a narrow but real edge case. No existing corpus fixtures exercise this case and no test asserts the incorrect count. The proposed fix (match only all-`#` tokens) is correct and sufficient.

## Independent Agent Finding

**Verdict:** Reproduced.

**Reproduction/Refutation:** A throwaway crate under `tmp/bug063-repro` depended on the local `glyph-core` crate and called `glyph_core::expand::count_words` directly. The harness expected prose tokens such as `#42`, `#hash`, and a mid-sentence `-` to count as words, while real Markdown heading and list-bullet marker cases remained excluded. The harness exited nonzero because the broad marker guard undercounted the prose cases.

**Evidence:** Graphify located `count_words()` in `crates/glyph-core/src/expand.rs` and connected it to `expand_step1_with_imported_descriptions()`, where word counts feed projection-tier assignment. A bounded source read confirmed the guard:

```rust
if token == "**" || token == "-" || token.starts_with('#') {
    continue;
}
```

The referenced design contract says a word is a whitespace-separated token in resolved Step prose, with only Markdown formatting markers excluded. Running `cargo run --quiet --manifest-path tmp/bug063-repro/Cargo.toml` produced:

```text
"issue #42 is open" => actual=3, expected=4
"a #hash b" => actual=2, expected=3
"See issue #42 for context." => actual=4, expected=5
"a - b" => actual=2, expected=3
"## heading text" => actual=2, expected=2
"- item one" => actual=2, expected=2
```

The scratch crate was removed after reproduction.

**Resolution Input:** Keep the existing suggested resolution for `#`-prefixed prose tokens: skip heading markers only when the token is all `#` characters, or preferably detect headings by line position. The independent reproduction also confirms the unconditional `token == "-"` branch is too broad for a mid-sentence dash; the existing alternative resolution, line-position-aware marker detection, covers both the heading and bullet cases.
