# BUG-070: line_col reports byte column, not character column, for lines with multi-byte UTF-8

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/span.rs:67-78`
**Found by:** tokenize | **Audit date:** unknown-date

## Description

`line_col` computes `col = byte_offset - line_starts[idx] + 1`, a 1-indexed byte column, and its doc comment claims "byte == character == column for valid source". That equivalence only holds for ASCII; Glyph explicitly allows multi-byte UTF-8 in string literals (confirmed by tokenize tests for `"café — 🌟"`). The doc comment assertion is false for any line containing multi-byte characters in a string literal.

However, the user-facing impact is narrower than initially described. The CLI pretty-print path correctly inverts the byte column back to byte offsets (via `LineIndex::byte_offset`), so `codespan-reporting` renders the caret at the right source position. The LSP path explicitly converts byte columns to UTF-16 via `utf16_character`/`utf16_len` in `convert.rs`. Only the JSON output path (`--format json`) exposes the raw byte column as `col`, violating the public contract at `docs/reference/diagnostics.md:32` which states "column = number of characters from start of line". The defect is a false doc comment and a JSON contract violation for lines with multi-byte UTF-8 string literals.

## Trigger / Reproduction

Write a `.glyph` file with a string literal containing multi-byte UTF-8 characters followed by a syntax error on the same line, e.g.:

```
x = "café" 🌟
```

Run `glyph check --format json`. The reported `col` value for the diagnostic on that line will be a byte column rather than a character column, disagreeing with the contract at `docs/reference/diagnostics.md:32`. Live reproduction: `cargo run -q -p glyph-cli -- check --format json` on such a file reports `col: 17` (byte) where the character column should be `16`.

## Evidence

```rust
/// `column` counts bytes from the start of the line (Glyph rejects tabs,
/// so byte == character == column for valid source).
pub fn line_col(&self, byte_offset: u32) -> (u32, u32) {
    // ...
    let col = byte_offset - self.line_starts[idx] + 1;
```

## Recommended Resolution

The fix should be limited to correcting the false doc comment at span.rs:68-69. The assertion "byte == character == column for valid source" should be replaced with:

```rust
/// `column` is a 1-indexed byte offset from the line start.
/// It equals the character column only for ASCII-only lines;
/// string literals may contain multi-byte UTF-8, so callers
/// that need a character column must convert using the char helpers.
```

No code change is needed in `line_col` itself, because all consumers (pretty-print round-trip, LSP UTF-16 conversion) already correctly handle byte columns. Changing `line_col` to emit character columns would break every caller that treats `col` as a byte column. The corresponding claim at `diagnostic.rs:85` should also be clarified to note that string literals may contain multi-byte UTF-8, making byte != character on such lines. The `docs/reference/diagnostics.md:32` contract text ("column = number of characters") should be corrected to say "byte offset" or the JSON serializer should convert to character column before emitting.

## Verification Notes

The doc comment at span.rs:68-69 asserting byte == character is demonstrably false — multi-byte UTF-8 is allowed in string literals. Live reproduction on `--format json` confirmed: `col: 17` (byte) vs expected character column `16` for a line with `é`. The CLI pretty-print and LSP paths are not broken — both correctly handle byte columns downstream. Only `--format json` exposes the raw byte column, violating the public diagnostic contract. Final severity is low per verifier consensus given the limited affected path; filed at medium due to the documented public contract violation.

## Independent Agent Finding

**Verdict:** Reproduced the core defect. `glyph check --format json` emits byte-based columns for diagnostics on UTF-8 lines, while the public diagnostics contract describes character columns. I refute only the exact numeric example in the existing prose for `x = "café" 🌟`: current output is `col: 13`, and the character column is `12`, not `17` vs `16`.

**Reproduction/Refutation:** I created `tmp/bug070.glyph` with:

```glyph
x = "café" 🌟
```

Then ran:

```sh
cargo run -q -p glyph-cli -- check --format json tmp/bug070.glyph
```

The command exited `1` and emitted:

```json
{"id":"G::parse::unexpected-char","classification":"error","message":"unexpected character `🌟`; not part of any Glyph token","span":{"file":"tmp/bug070.glyph","start":{"line":1,"col":13},"end":{"line":1,"col":13}}}
```

For the same source line, `python3 -c 's="x = \"café\" 🌟"; i=s.index("🌟"); print(f"star byte_col={len(s[:i].encode())+1} char_col={len(s[:i])+1}")'` printed:

```text
star byte_col=13 char_col=12
```

This confirms the JSON span column is the byte column, not the character column. The existing report's `17`/`16` numbers do not match this exact snippet, but the reported failure mode is valid.

**Evidence:** Graphify first located the relevant implementation path at `LineIndex::line_col` / `LineIndex::byte_offset` in `crates/glyph-core/src/span.rs`. Bounded source reads confirmed `LineIndex::line_col` computes `col = byte_offset - self.line_starts[idx] + 1`; `SourceSpan::from_byte_span` uses that result directly; and `render_ndjson` serializes `Diagnostic` without converting columns. A bounded read of `docs/reference/diagnostics.md` confirmed the public contract says column equals the number of characters from the start of the line.

**Resolution Input:** Preserve the existing recommended resolution. The important decision remains whether JSON diagnostics should continue exposing byte columns and the public contract should be corrected, or whether the JSON serialization path should convert to character columns before emission. If this report is later edited, update the reproduction numbers for the exact sample to `13` byte column vs `12` character column.
