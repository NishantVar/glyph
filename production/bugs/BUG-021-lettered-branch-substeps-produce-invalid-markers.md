# BUG-021: Lettered branch substeps produce invalid markers (and overflow-panic) for arms with 27+ steps

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/emit/branch.rs:304-310`
**Found by:** emit-core | **Audit date:** unknown-date

## Description

`emit_lettered_substeps` iterates `(b'a'..).zip(body)` over a `u8` `RangeFrom` and casts
each byte to `char` as the list marker. For a branch arm body with more than 26
step-projecting nodes the letter increments past `'z'` (122) into `'{'` (123), `'|'` (124),
`'}'` (125), `'~'` (126), DEL (127), and beyond, emitting markers like `   {. ...`,
`   |. ...` instead of valid lettered sub-items.

This produces malformed markdown that also breaks `validate_output`'s
`check_branch_substeps` lettered-item accounting. Worse, for an arm with 160+ substeps the
range reaches u8 255 and the next `RangeFrom<u8>::next()` computes 255+1, which panics with
`attempt to add with overflow` in debug builds (wraps to 0, producing duplicate `'a'` markers
in release).

Trigger: a single branch arm (`then`/`elif`/`else` body) containing 27+ inline/call/branch
step nodes.

## Trigger / Reproduction

Create a `.glyph` file with a branch arm containing 28 inline steps. Run `cargo run -p
glyph-cli -- compile`. Steps 27 and 28 will be marked `{.` and `|.` (non-letter ASCII)
rather than valid substep markers. With 160+ steps in a debug build, the compiler panics
with `thread 'main' panicked: attempt to add with overflow`.

## Evidence

```rust
// emit/branch.rs lines 304-310:
for (letter, node_id) in (b'a'..).zip(body) {
    // ...
    s.push_literal(format!("   {}. {}\n", letter as char, text));
}
// After 'z' (122), letter becomes '{' (123), '|' (124), etc. — invalid markdown markers.
// At letter == 255, RangeFrom<u8>::next() panics: attempt to add with overflow.
```

## Recommended Resolution

Generate alphabetic labels safely using a multi-letter scheme (e.g. spreadsheet column
labels: a-z, then aa, ab, ...) rather than a raw `u8` cast, so arms with >26 steps render
valid markers and large arms cannot overflow-panic.

The fix is incomplete without also updating the validator side. In addition to changing
`emit_lettered_substeps`, the following functions in
`crates/glyph-core/src/validate_output.rs` must be updated to recognize and strip
variable-length letter prefixes:

- `is_lettered_subitem` — currently hardcodes `bytes[0].is_ascii_lowercase() && bytes[1] == b'.' && bytes[2] == b' '`
- `strip_letter_prefix` — currently hardcodes slicing past 3 bytes

Without updating these two validator functions, sub-items with 2+ character labels like
`aa.` will not be recognized as lettered sub-items, causing spurious
`substep-count-mismatch` violations.

## Verification Notes

Both failure modes were directly reproduced. Running `cargo run -p glyph-cli -- compile`
on a file with 28 steps in a branch arm produced output with `{.` and `|.` as list markers
for steps 27 and 28. Running with 160 steps panicked with
`thread 'main' panicked: attempt to add with overflow` at `range.rs:434` in the debug build.
There are no upstream constraints capping branch arm body sizes to 26 nodes, and the test
corpus only exercises small arms.

## Independent Agent Finding

**Verdict:** Reproduced. The report is valid for both the invalid-marker case and the
debug-build overflow panic.

**Reproduction/Refutation:** I created two scratch `.glyph` files under `tmp/bug-021/`:
one branch arm with 28 inline steps, and one branch arm with 160 inline steps. Compiling
the 28-step file completed with exit code 0 but emitted non-letter substep markers after
`z.`. Compiling the 160-step file in the default dev/debug build exited 101 with the
reported `RangeFrom<u8>` overflow panic.

**Evidence:**

- `cargo run -p glyph-cli -- compile --format json --output tmp/bug-021/repro-28.md tmp/bug-021/repro-28.glyph`
  exited 0. The generated markdown contained:
  - `y. step 025.`
  - `z. step 026.`
  - `{. step 027.`
  - `|. step 028.`
- `cargo run -p glyph-cli -- compile --format json --output tmp/bug-021/repro-160.md tmp/bug-021/repro-160.glyph`
  exited 101 and printed:
  `panicked at .../library/core/src/iter/range.rs:434:1: attempt to add with overflow`.
- Narrow source inspection matched the root cause: `emit_lettered_substeps` iterates
  `(b'a'..).zip(body)` and formats `letter as char`, while the validator helpers
  `is_lettered_subitem` and `strip_letter_prefix` only accept/strip a single
  lowercase-letter prefix.

**Resolution Input:** Keep the existing recommended resolution. The emitter should use a
bounded, alphabetic multi-letter label generator rather than raw `u8` iteration, and the
validator-side letter-prefix parsing must be updated at the same time so labels like
`aa.` are accepted and stripped consistently. Also consider adding regression coverage
for both 28-step and 160-step branch arms; the 28-step case currently emits malformed
markdown without a non-zero CLI exit.
