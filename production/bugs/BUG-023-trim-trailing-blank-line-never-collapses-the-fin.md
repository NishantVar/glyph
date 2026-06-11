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

## Independent Agent Finding

**Verdict:** Reproduced.

**Reproduction/Refutation:** I compiled one checked-in fixture with the existing local CLI
binary and wrote the result only to scratch space:

```text
./target/debug/glyph compile crates/glyph-cli/tests/fixtures/predicate_inline_literal.glyph \
  --output tmp/bug-023/predicate_inline_literal.md
# exit 0, no stdout/stderr

tail -c 32 tmp/bug-023/predicate_inline_literal.md | xxd -g 1
00000000: 20 61 6e 64 20 63 6f 6e 74 69 6e 75 65 20 77 69   and continue wi
00000010: 74 68 6f 75 74 20 63 68 61 6e 67 65 73 2e 0a 0a  thout changes...
```

`cmp -s tmp/bug-023/predicate_inline_literal.md crates/glyph-cli/tests/fixtures/predicate_inline_literal.expected.md`
also exited 0, so the checked-in expected fixture encodes the same trailing `0a 0a` bytes.

**Evidence:** Graphify located the relevant scaffold emitter path at
`trim_trailing_blank_line()`, `emit_flow_section()`, `push_literal()`, and `Chunk` in
`crates/glyph-core/src/emit/scaffold.rs`. Bounded source inspection confirmed the causal
mechanism:

- `Scaffold::push_literal` at `scaffold.rs:536` appends a fresh `Chunk::Literal` without
  coalescing adjacent literal text.
- Procedure and flow step renderers emit step content ending in `\n`, e.g. procedure
  `format!("{}. {}\n", step_num, body)` and flow `format!("{}. {}\n", idx + 1, body)`.
- Both procedure and flow emitters then append a separate `s.push_literal("\n")` separator.
- `trim_trailing_blank_line` at `scaffold.rs:1484-1498` only mutates the last literal's own
  text while it ends with `"\n\n"`, so a final standalone `"\n"` chunk is not removed even
  when the previous chunk already ends with `\n`.

I also rechecked the fixture corpus claim:

```text
find crates/glyph-cli/tests/fixtures -maxdepth 1 -name '*.expected.md' -print | wc -l
12

find crates/glyph-cli/tests/fixtures -maxdepth 1 -name '*.expected.md' \
  -exec sh -c 'test "$(tail -c 2 "$1" | xxd -p)" = 0a0a' sh {} \; -print | wc -l
12
```

The referenced formatting contract is present at `design/compiled-output.md:500`: "No double
blank lines, no trailing whitespace."

**Resolution Input:** Preserve the existing Recommended Resolution. The observed failure is
specifically a cross-chunk trailing-newline normalization bug, so the fix should treat adjacent
literal chunks at the tail as continuous and remove a final lone `"\n"` chunk when the preceding
literal already ends in `\n`. After the emitter fix, update the fixture snapshots that currently
end in `0a 0a`.
