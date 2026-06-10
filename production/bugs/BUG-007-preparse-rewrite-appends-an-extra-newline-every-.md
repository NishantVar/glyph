# BUG-007: preparse_rewrite appends an extra newline every run when source ends with a blank line (\n\n), breaking idempotency

**Severity:** high | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/fmt.rs:109-126`
**Found by:** fmt-1 | **Audit date:** unknown-date

## Description

`preparse_rewrite` iterates `source.split('\n')` and pushes one `'\n'` per segment (lines 103–107). For a source with N newlines this emits N+1 newlines, so one extra trailing newline must be removed afterward. The cleanup at lines 112–121 only pops the extra when `source.ends_with('\n') && !source.ends_with("\n\n")`. For a source ending with a blank line (i.e. ending with `\n\n`), the `!source.ends_with("\n\n")` guard is false, so the pop is skipped and the function returns one more newline than the input.

This is non-idempotent and content-changing: `"a\n\n"` → `"a\n\n\n"` → `"a\n\n\n\n"`, growing by one newline on each run. Trailing blank lines at EOF are extremely common in real files.

The `fmt_source` parse-failure path (fmt.rs lines 68–76) returns `after_preparse` directly without going through `ast_rewrite_inner`, so any file with a trailing blank line that also fails to parse (unterminated string, bad indent, etc.) will be silently mutated on every `glyph fmt` invocation and never reaches a fixed point.

## Trigger / Reproduction

Any `.glyph` file with a trailing blank line (ending in `\n\n`) that also has a parse error. Running `glyph fmt` on such a file repeatedly grows the file by one newline per invocation. With a valid file the extra newline is introduced in `after_preparse` and persists into `ast_rewrite_inner`, which then preserves it.

Empirical reproduction: a file with an unterminated string literal and a trailing blank line grew from 36 bytes to 37, then 38, then 39 bytes across three consecutive `glyph fmt` invocations.

## Evidence

```rust
// Cleanup at the end of preparse_rewrite:
if source.ends_with('\n') && !source.ends_with("\n\n") {
    out.pop(); // remove trailing \n
}
// When source ends with "\n\n", the guard is false — the extra \n from split('\n') is never removed.
// "a\n\n" → split produces ["a", "", ""] → pushes "a\n", "\n", "\n" → out = "a\n\n\n" (one too many)
```

## Recommended Resolution

Track the trailing-newline count of the source explicitly and reproduce it exactly, rather than using the single/double heuristic:

```rust
// Count trailing newlines in source
let trailing_newlines = source.len() - source.trim_end_matches('\n').len();
// After building out, truncate trailing newlines to exactly trailing_newlines
while out.ends_with("\n\n") && out.len() - out.trim_end_matches('\n').len() > trailing_newlines {
    out.pop();
}
```

Or equivalently: after building `out` via `split('\n')`, pop all trailing newlines from `out` and then re-append exactly `trailing_newlines` newlines. This is idempotent regardless of how many trailing newlines the source has.

## Verification Notes

Code at fmt.rs lines 109–126 confirmed exactly as described. Empirically reproduced: a file with a parse-triggering unterminated string and a trailing blank line grew by one byte per `glyph fmt` run across three consecutive invocations. The parse-failure fast path at lines 68–76 returns `after_preparse` directly, confirming the growth path. The proposed fix — counting trailing newlines and reproducing that exact count — is correct and complete.
