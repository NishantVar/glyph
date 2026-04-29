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
}
