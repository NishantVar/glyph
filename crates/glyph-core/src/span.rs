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
    /// Total byte length of the source — the clamp target for the last line
    /// (the last line has no following `line_starts` entry to bound it).
    len: u32,
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
            len: source.len() as u32,
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
        // End of this line's content: the byte before the next line's `\n`,
        // or the source length for the last line. Clamp the column so an
        // out-of-line column resolves to this line's end rather than
        // bleeding into the following line.
        let line_end = match self.line_starts.get(line_idx + 1) {
            Some(&next) => next.saturating_sub(1),
            None => self.len,
        };
        let target = line_start + col.saturating_sub(1);
        target.min(line_end)
    }
}

/// Count the UTF-16 code units in `line` that precede `byte_col`.
///
/// LSP measures `Position.character` in UTF-16 code units while Glyph
/// spans are byte offsets; this converts a byte column within a single
/// line to the corresponding LSP column. A `byte_col` at or past the end
/// of `line` saturates at the line's total UTF-16 length.
pub fn utf16_len(line: &str, byte_col: u32) -> u32 {
    let byte_col = byte_col as usize;
    line.char_indices()
        .take_while(|(idx, _)| *idx < byte_col)
        .map(|(_, ch)| ch.len_utf16() as u32)
        .sum()
}

/// Convert a UTF-16 `character` column within `line` to a byte offset.
///
/// Inverse of [`utf16_len`]: walks characters while accumulating UTF-16
/// code units until `utf16_col` is reached, then returns that character's
/// byte offset within the line. A column at or past the line's end
/// saturates at the line's byte length.
pub fn utf16_to_byte(line: &str, utf16_col: u32) -> u32 {
    let mut units = 0u32;
    for (idx, ch) in line.char_indices() {
        if units >= utf16_col {
            return idx as u32;
        }
        units += ch.len_utf16() as u32;
    }
    line.len() as u32
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

    #[test]
    fn byte_offset_clamps_column_to_line_end() {
        // `LineIndex::byte_offset`'s doc promises an out-of-line column is
        // clamped to the line's end. A column past a non-last line must
        // resolve to that line's end, never bleed into the following line.
        let src = "abc\ndef";
        let li = LineIndex::new(src);
        // Line 1 content ends at byte 3 (the `\n`); line 2 starts at byte 4.
        assert_eq!(li.byte_offset(1, 99), 3);
        assert!(li.byte_offset(1, 99) < li.byte_offset(2, 1));
        // Last line: an out-of-line column clamps to the source length.
        assert_eq!(li.byte_offset(2, 99), src.len() as u32);
    }

    #[test]
    fn utf16_len_counts_code_units_not_bytes() {
        // `café`: `é` is 2 UTF-8 bytes but a single UTF-16 code unit.
        let line = "café x";
        assert_eq!(utf16_len(line, 0), 0);
        assert_eq!(utf16_len(line, 3), 3); // before `é` — c, a, f
        assert_eq!(utf16_len(line, 5), 4); // after `é` (starts at byte 3)
        assert_eq!(utf16_len(line, 6), 5); // after the space
                                           // A byte column past the line end saturates at its UTF-16 length.
        assert_eq!(utf16_len(line, 99), 6);

        // Astral-plane `😀`: 4 UTF-8 bytes, 2 UTF-16 code units.
        let emoji = "😀x";
        assert_eq!(utf16_len(emoji, 4), 2); // after the emoji
        assert_eq!(utf16_len(emoji, 5), 3); // after `x`
    }

    #[test]
    fn utf16_to_byte_inverts_utf16_len() {
        // `café`: UTF-16 column 3 is the `é`, which starts at byte 3.
        let line = "café x";
        assert_eq!(utf16_to_byte(line, 0), 0);
        assert_eq!(utf16_to_byte(line, 3), 3);
        assert_eq!(utf16_to_byte(line, 4), 5); // the space, after the 2-byte `é`
                                               // A column past the line end saturates at its byte length.
        assert_eq!(utf16_to_byte(line, 99), line.len() as u32);

        // Astral-plane `😀` spans 2 UTF-16 units; `x` is at byte 4.
        let emoji = "😀x";
        assert_eq!(utf16_to_byte(emoji, 2), 4);
    }
}
