//! Span types for source-location tracking.
//!
//! See `design/build-foundation.md` §A3.

/// Byte-offset span into a source file.
///
/// Half-open range: `start <= end`, inclusive of `start`, exclusive of `end`.
/// For a single-character span, `end == start + 1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub file_id: u32,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const fn new(file_id: u32, start: u32, end: u32) -> Self {
        Self { file_id, start, end }
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
        Self { line_starts }
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
        let col_offset = col.saturating_sub(1);
        line_start + col_offset
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
}
