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
