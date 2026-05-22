//! Span types for source-location tracking.
//!
//! See `docs/adr/` §A3.

/// Byte-offset span into a source file.
///
/// Half-open range: `start <= end`, inclusive of `start`, exclusive of `end`.
/// For a single-character span, `end == start + 1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub file_id: u32,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const fn new(file_id: u32, start: u32, end: u32) -> Self {
        Self {
            file_id,
            start,
            end,
        }
    }
}

/// Wraps any AST or IR node with its source span.
#[derive(Clone, Debug)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Self { node, span }
    }
}

/// Maps byte offsets to 1-indexed (line, column) for diagnostic rendering.
///
/// Built once during tokenization, queried only at diagnostic emission time.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset of each line's first character. `line_starts[0]` is always 0.
    line_starts: Vec<u32>,
    /// Total source length in bytes. Used by [`Self::byte_offset`] to
    /// clamp out-of-range column requests to end-of-line instead of
    /// bleeding into the following line (or past EOF).
    source_len: u32,
}

impl LineIndex {
    /// Build a line index from raw source text.
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0u32];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        Self {
            line_starts,
            source_len: source.len() as u32,
        }
    }

    /// Convert a byte offset to a 1-indexed (line, column) pair.
    ///
    /// `column` counts bytes from the start of the line (Glyph rejects tabs,
    /// so byte == character == column for valid source).
    pub fn line_col(&self, byte_offset: u32) -> (u32, u32) {
        let idx = match self.line_starts.binary_search(&byte_offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let line = (idx as u32) + 1;
        let col = byte_offset - self.line_starts[idx] + 1;
        (line, col)
    }

    /// Inverse of [`line_col`]: convert a 1-indexed (line, column) pair back
    /// to a byte offset.
    ///
    /// Used by the LSP go-to-def handler (M2) to map an editor cursor — which
    /// arrives as 0-indexed `Position { line, character }` and is bumped to
    /// 1-indexed by the caller — onto a byte offset that can be looked up
    /// against AST `Span`s.
    ///
    /// Out-of-range inputs (line past EOF or column past the line length) are
    /// clamped to the nearest in-range offset rather than returning an error.
    /// This matches what the LSP wants: a cursor past the end of a line
    /// should resolve to "no identifier here" via the resolution lookup, not
    /// crash the server.
    pub fn byte_offset(&self, line: u32, col: u32) -> u32 {
        if line == 0 {
            return 0;
        }
        let line_idx = (line - 1) as usize;
        if line_idx >= self.line_starts.len() {
            // Past EOF — return the last known offset so callers degrade
            // gracefully rather than panicking.
            return *self.line_starts.last().unwrap_or(&0);
        }
        let line_start = self.line_starts[line_idx];
        // End of this line in bytes: just before the trailing `\n` for
        // non-last lines, end-of-source for the last line. Clamping the
        // requested column here keeps an editor cursor past end-of-line
        // (common in LSP requests) from spilling into the next line.
        let line_end = if line_idx + 1 < self.line_starts.len() {
            self.line_starts[line_idx + 1].saturating_sub(1)
        } else {
            self.source_len
        };
        let col_offset = col.saturating_sub(1);
        (line_start + col_offset).min(line_end)
    }
}

/// Convert a 0-indexed byte column on a single line of source text to the
/// matching 0-indexed UTF-16 code-unit column. LSP positions default to
/// UTF-16, so anywhere a byte column crosses the LSP boundary it has to
/// go through this helper.
///
/// `byte_col` past the byte length of `line_text` clamps to the UTF-16
/// length — the LSP spec recommends end-of-line as the clamp target for
/// out-of-range positions.
pub fn byte_col_to_utf16_col(line_text: &str, byte_col: u32) -> u32 {
    let bytes = line_text.as_bytes();
    let cap = bytes.len();
    let mut utf16 = 0u32;
    let target = (byte_col as usize).min(cap);
    let mut i = 0usize;
    while i < target {
        let b = bytes[i];
        let (step, units) = utf8_lead_to_step_and_utf16_units(b);
        i += step;
        if i > target {
            break;
        }
        utf16 += units;
    }
    utf16
}

/// Inverse of [`byte_col_to_utf16_col`]: convert a 0-indexed UTF-16
/// column on a single line to a 0-indexed byte column.
///
/// `utf16_col` past the UTF-16 length of `line_text` clamps to the byte
/// length, matching the LSP recommendation.
pub fn utf16_col_to_byte_col(line_text: &str, utf16_col: u32) -> u32 {
    let bytes = line_text.as_bytes();
    let cap = bytes.len();
    let mut utf16 = 0u32;
    let mut i = 0usize;
    while i < cap && utf16 < utf16_col {
        let b = bytes[i];
        let (step, units) = utf8_lead_to_step_and_utf16_units(b);
        if utf16 + units > utf16_col {
            break;
        }
        i += step;
        utf16 += units;
    }
    i as u32
}

/// Helper: given a UTF-8 lead byte, return (byte step, UTF-16 unit count
/// for the codepoint). Stray continuation bytes count zero UTF-16 units
/// but advance one byte so traversal terminates on malformed input.
fn utf8_lead_to_step_and_utf16_units(b: u8) -> (usize, u32) {
    if b < 0x80 {
        (1, 1)
    } else if b < 0xC0 {
        (1, 0)
    } else if b < 0xE0 {
        (2, 1)
    } else if b < 0xF0 {
        (3, 1)
    } else {
        (4, 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_index_basic() {
        let src = "abc\ndef\nghi";
        let li = LineIndex::new(src);
        assert_eq!(li.line_col(0), (1, 1));
        assert_eq!(li.line_col(2), (1, 3));
        assert_eq!(li.line_col(4), (2, 1));
        assert_eq!(li.line_col(8), (3, 1));
    }

    #[test]
    fn byte_offset_roundtrips_line_col() {
        let src = "abc\ndef\nghi";
        let li = LineIndex::new(src);
        // Pin the inverse of line_index_basic.
        assert_eq!(li.byte_offset(1, 1), 0);
        assert_eq!(li.byte_offset(1, 3), 2);
        assert_eq!(li.byte_offset(2, 1), 4);
        assert_eq!(li.byte_offset(3, 1), 8);
    }

    #[test]
    fn byte_offset_clamps_past_eof() {
        let src = "abc\ndef";
        let li = LineIndex::new(src);
        // Line beyond EOF degrades gracefully.
        assert_eq!(li.byte_offset(99, 1), 4);
        // Line 0 (caller error) degrades to start of file.
        assert_eq!(li.byte_offset(0, 1), 0);
    }

    /// `byte_offset` must clamp column-past-end-of-line to the line's
    /// end-of-line byte index, NOT bleed into the following line. The doc
    /// comment promises this; the implementation must back it up.
    #[test]
    fn byte_offset_clamps_past_line_end() {
        let src = "abc\ndef\nghi";
        let li = LineIndex::new(src);
        // Line 1 "abc" — end-of-line byte index is 3 (just before the \n).
        assert_eq!(li.byte_offset(1, 99), 3);
        // Line 2 "def" — end-of-line byte index is 7 (just before the \n).
        assert_eq!(li.byte_offset(2, 99), 7);
        // Last line "ghi" — end-of-source byte index is 11.
        assert_eq!(li.byte_offset(3, 99), 11);
    }

    /// LSP defaults to UTF-16 code units for `Position.character`, but Glyph
    /// spans are byte-based. `byte_col_to_utf16_col` performs the conversion
    /// for a single line of source given the 0-indexed byte column.
    #[test]
    fn byte_col_to_utf16_col_ascii_is_identity() {
        let line = "abc";
        assert_eq!(byte_col_to_utf16_col(line, 0), 0);
        assert_eq!(byte_col_to_utf16_col(line, 2), 2);
        // Past EOL clamps to UTF-16 length.
        assert_eq!(byte_col_to_utf16_col(line, 99), 3);
    }

    #[test]
    fn byte_col_to_utf16_col_handles_multibyte_bmp_char() {
        // `α` is 2 bytes in UTF-8 but 1 UTF-16 code unit.
        let line = "αbc";
        // byte 0 → utf16 0 (before α)
        assert_eq!(byte_col_to_utf16_col(line, 0), 0);
        // byte 2 → utf16 1 (after α)
        assert_eq!(byte_col_to_utf16_col(line, 2), 1);
        // byte 4 → utf16 3 (end of "αbc")
        assert_eq!(byte_col_to_utf16_col(line, 4), 3);
    }

    #[test]
    fn byte_col_to_utf16_col_handles_supplementary_char() {
        // `🦀` (U+1F980) is 4 bytes in UTF-8, 2 UTF-16 code units (surrogate pair).
        let line = "🦀x";
        assert_eq!(byte_col_to_utf16_col(line, 0), 0);
        // After the crab — past the 4 UTF-8 bytes, 2 UTF-16 units.
        assert_eq!(byte_col_to_utf16_col(line, 4), 2);
        // After "x".
        assert_eq!(byte_col_to_utf16_col(line, 5), 3);
    }

    /// Inverse direction: LSP gives us a UTF-16 column; we need the byte column.
    #[test]
    fn utf16_col_to_byte_col_handles_multibyte_chars() {
        let line = "αbc";
        assert_eq!(utf16_col_to_byte_col(line, 0), 0);
        assert_eq!(utf16_col_to_byte_col(line, 1), 2);
        assert_eq!(utf16_col_to_byte_col(line, 3), 4);
        // Past end clamps to byte length.
        assert_eq!(utf16_col_to_byte_col(line, 99), 4);
    }
}
