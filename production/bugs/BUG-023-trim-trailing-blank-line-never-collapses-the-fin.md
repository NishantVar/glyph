# BUG-023: trim_trailing_blank_line never collapses the final cross-chunk blank line; every compiled file ends with \n\n

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/emit/scaffold.rs:1484-1498`
**Found by:** emit-scaffold | **Audit date:** unknown-date

## Description

`trim_trailing_blank_line` only inspects the single last `Literal` chunk and pops `\n` only
while that one chunk's own text ends with `"\n\n"`. But in real output the trailing blank line
is split across the last two chunks:

- Section/step renderers emit final content ending in `\n` (e.g. `emit_flow_section`
  line 1273 `format!("{}. {}\n", ...)`, or the procedure step at line 1064).
- A separate lone `"\n"` chunk is then pushed as a section separator
  (`emit_flow_section` line 1400; the procedure loop line 1085).

The last chunk is therefore `"\n"`, which does NOT end with `"\n\n"` and is not empty, so the
outer `while let` breaks immediately without ever popping it or looking at the previous chunk.
The concatenated output thus ends with `...content\n` + `\n` = `\n\n`.

This violates the function's own docstring ("pop ... until output doesn't end with `\"\n\n\"`")
and design/compiled-output.md §Formatting Rule 4 ("No double blank lines, no trailing
whitespace"). All 12 `crates/glyph-cli/tests/fixtures/*.expected.md` end with exactly two
trailing newlines (e.g. `predicate_inline_literal.expected.md` ends
`...continue without changes.\n\n`), confirming the compiler genuinely emits the trailing
blank line and the snapshot tests encode the buggy behavior as "expected."

## Trigger / Reproduction

Compile any `.glyph` skill or procedure. Inspect the emitted `.md` file with a hex viewer
(`xxd`). The file will end with bytes `0a 0a` (`\n\n`) instead of `0a` (`\n`).

## Evidence

```rust
// scaffold.rs lines 1484-1498:
while let Some(Chunk::Literal(text)) = s.chunks.last_mut() {
    while text.ends_with("\n\n") { text.pop(); }
    if text.is_empty() { s.chunks.pop(); continue; }
    break;
}
// The last chunk is the lone "\n" pushed at line 1400 (emit_flow_section) or
// line 1085 (procedure loop). It does not end with "\n\n" and is not empty,
// so the outer while let breaks immediately without popping it.
// The preceding chunk already ends in "\n", so the concatenation yields "\n\n".
```

## Recommended Resolution

Operate on the joined tail rather than per-chunk. Algorithm:

1. Repeatedly: if the last chunk is an empty or whitespace-only literal, pop it.
2. If the last non-empty literal ends with more than one `\n`, truncate to a single `\n`.
3. Treat the boundary between the last two literal chunks as continuous — pop a trailing
   lone `"\n"` chunk when the preceding chunk already ends in `\n`.

After fixing the emitter, update the 12 `*.expected.md` fixtures and the `branching.rs`
snapshot to the corrected single-`\n` ending.

## Verification Notes

The bug is confirmed via three independent sources. First, `push_literal` at scaffold.rs:536-538
always creates a new `Chunk::Literal` without coalescing, so section separators always land
as their own chunk. Second, `trim_trailing_blank_line` only checks `ends_with("\n\n")` per
chunk — the lone `"\n"` separator chunk never satisfies this condition and is never popped.
Third, all 12 `*.expected.md` fixture files were verified via `xxd` to end with bytes
`0a 0a`, confirming the compiler genuinely emits the trailing blank line and snapshot tests
encode this buggy behavior as "expected." The design spec explicitly prohibits double blank
lines.
