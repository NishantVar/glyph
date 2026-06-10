# BUG-064: strip_trailing_comment / trailing_comment_after_keyword use a naive `prev != '\\'` escape check that misreads a string ending in `\\"`

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/fmt.rs:1194-1234`
**Found by:** fmt-1 | **Audit date:** unknown-date

## Description

Both string-literal-aware comment scanners (`strip_trailing_comment` and `trailing_comment_after_keyword`) detect the closing quote with `if ch == '"' && prev != '\\'`. This single-char lookback is fooled by an escaped backslash immediately before the closing quote: in `"path\\"` (a valid literal whose value is `path\`), the bytes are `"`, ..., `\`, `\`, `"`. When the final `"` is reached, `prev` is the second `\`, so the close quote is treated as escaped and `in_string` wrongly stays `true`.

Any following `// comment` is then never detected. This matters only on the duplicate-section merge path (single sections are emitted verbatim). For an `effects` or `description` merge whose anchor line is e.g. `description: "path\\"  // note`, `strip_trailing_comment` returns the whole tail (including `// note`) as the payload; `unwrap_string_literal` then calls `strip_suffix('"')` on a string that doesn't end in `"`, returns `None`, and the description body is silently dropped from the merge — a content-loss defect.

## Trigger / Reproduction

Write a `.glyph` file with two `description:` sections (triggering the merge path) where the description value ends in an escaped backslash, followed by an inline comment:

```
description: "path\\"  // a note about paths
description: "second desc"
```

The merged description will silently drop the first `description:` value.

## Evidence

```rust
// In strip_trailing_comment and trailing_comment_after_keyword:
} else if ch == '"' {
    in_string = true;
}
// Close-quote detection (both functions):
// `ch == '"' && prev != '\\'`
// For "path\\" the final '"' has prev == '\\', so in_string stays true
// and the following '// comment' is never returned as a comment boundary.
```

## Recommended Resolution

Replace the one-character `prev != '\\'` lookback with a proper escape-state toggle that consumes the character after a backslash, so `\\"` correctly closes the string:

```rust
let mut in_string = false;
let mut escape_next = false;
for ch in s.chars() {
    if escape_next {
        escape_next = false;
        continue;
    }
    if in_string {
        if ch == '\\' { escape_next = true; }
        else if ch == '"' { in_string = false; }
    } else {
        if ch == '"' { in_string = true; }
        else if ch == '/' && /* peek next */ ... { return &s[..pos]; }
    }
}
```

Apply the same fix to both `strip_trailing_comment` and `trailing_comment_after_keyword`.

## Verification Notes

The `prev != '\\'` single-char lookback is confirmed in both functions at `fmt.rs` lines 1194-1234. For `"path\\"  // note`, when the scanner reaches the closing `"` at the end of the literal, `prev` is the second `\`, so the condition `prev != '\\'` is false and `in_string` stays `true`. The `// note` comment is never detected. Downstream, `unwrap_string_literal` receives `"path\\"  // note`, calls `strip_suffix('"')` which fails (last char is `e`), returns `None`, and `bodies.push(...)` is skipped — the description value is silently dropped from the merge. This only fires on the duplicate-section merge path (`matching.len() > 1`); single sections use the verbatim early-return and are unaffected. No existing tests cover this triple combination (escaped-backslash-terminated string + trailing comment + duplicate section merge).
