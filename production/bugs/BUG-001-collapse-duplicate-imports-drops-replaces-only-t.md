# BUG-001: collapse_duplicate_imports drops/replaces only the first line of a multi-line selective import, leaving orphaned continuation lines

**Severity:** critical | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/fmt.rs:199-252`
**Found by:** fmt-1 | **Audit date:** unknown-date

## Description

`collapse_duplicate_imports` records each import occurrence by the line index of its keyword token (`line_col(imp.span.start)` → `line_idx`). When collapsing duplicates it either drops that single line index (`to_drop.insert(idx)`) or replaces it with a single merged line (`replacements.insert(g.first_line_idx, merged)`). The reconstruction loop skips or substitutes only those single indexed lines.

However, Glyph selective imports may span multiple physical lines — the `import "..." {` keyword line followed by name lines and a closing `}` line. For a duplicate multi-line import, only the `import "..." {` keyword line is dropped or replaced. The continuation lines (`name,`, `name2`, `}`) are never added to `to_drop` and are emitted verbatim, producing corrupted output such as a stray `subagent,` / `}` with no `import` keyword. This silently corrupts the user's file on `glyph fmt`.

## Trigger / Reproduction

A file containing a multi-line selective import followed by a duplicate (single- or multi-line) of the same path. Running `glyph fmt` produces a merged replacement line at position 0, while the continuation lines from both occurrences are emitted as orphaned text in the output.

Example input:
```
import "./h" {
    subagent,
    loop
}
import "./h" { subagent }
```

Formatted output (corrupted):
```
import "./h" { subagent, loop }
    subagent,
    loop
}
```

## Evidence

```rust
// Each occurrence contributes only its keyword line to line_indices:
// line_idx = line_col(imp.span.start) - 1  (records only the `import` keyword line)

for &idx in g.line_indices.iter().skip(1) {
    to_drop.insert(idx);   // idx is only the keyword line of each non-first occurrence
}
// ...
replacements.insert(g.first_line_idx, merged);  // single-line replacement; continuation lines remain

// Reconstruction loop (lines 236-247) skips/substitutes only those single indices;
// continuation lines of multi-line imports are never in to_drop and are emitted verbatim.
```

## Recommended Resolution

Compute each import's full line span (from `line_col(span.start)` to `line_col(span.end)`) and drop or replace the entire range of lines, not just the first line. `imp.span.end` is already available and unused — the fix is to track full line ranges in `to_drop` and in the reconstruction loop. Alternatively, normalize multi-line imports to single-line form before the collapse pass so each occurrence is always exactly one line.

## Verification Notes

Bug confirmed by both code analysis and live CLI reproduction. Running `glyph fmt` on a file with a multi-line selective import followed by a duplicate produced corrupted output with orphaned continuation lines from the first occurrence. `imp.span.end` is set in `parse.rs` from `kw_span.start` to `end_span.end`, making the full range available to fix this. All existing collapse tests use only single-line imports, so this code path was never exercised.
