//! Hand-rolled tokenizer (Phase 1, sub-step A/B).
//!
//! Walking-skeleton scope: just enough to tokenize `update_docs.glyph.md`.
//! Two-phase approach per `design/build-foundation.md` §A2:
//!   - Phase A: line-oriented pre-processing (compute indent levels, strip comments).
//!   - Phase B: token-level scanning within each line.
//!
//! The output is a flat stream of `Token`s preserving line and indent metadata
//! so the parser can drive structure off significant indentation.

use crate::span::{LineIndex, Span};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenKind {
    /// Marks a logical line's start. Carries the indent level (in 4-space units).
    LineStart { indent: u32 },
    Ident(String),
    /// Quoted string literal — contents (without surrounding quotes), value already unescaped.
    StringLit(String),
    Lparen,
    Rparen,
    Colon,
    Comma,
    Equals,
    /// `.` — dot separator (e.g., `block_name.applies()`).
    Dot,
    /// `==` — branch condition equality (not a value-level operator).
    DoubleEquals,
    /// End of file.
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenizeError {
    TabIndent { byte_offset: u32 },
    MixedIndent { byte_offset: u32 },
    BadIndent { byte_offset: u32 },
    UnterminatedString { byte_offset: u32 },
    UnexpectedChar { byte_offset: u32, ch: char },
}

pub fn tokenize(source: &str, file_id: u32) -> Result<(Vec<Token>, LineIndex), TokenizeError> {
    let line_index = LineIndex::new(source);
    let mut tokens: Vec<Token> = Vec::new();

    let bytes = source.as_bytes();
    let mut i: usize = 0;

    // Phase A: walk line by line.
    while i < bytes.len() {
        // Find end of line.
        let line_start = i;
        let mut j = i;
        while j < bytes.len() && bytes[j] != b'\n' {
            j += 1;
        }
        let line_end = j;
        // Position past the newline (if any) for the next iteration.
        let next_line_pos = if j < bytes.len() { j + 1 } else { j };

        // Compute indent — count leading spaces. Reject tabs / mixed.
        let mut k = line_start;
        let mut space_count: u32 = 0;
        let mut saw_tab = false;
        let mut saw_space_then_tab = false;
        while k < line_end {
            match bytes[k] {
                b' ' => {
                    space_count += 1;
                    k += 1;
                }
                b'\t' => {
                    saw_tab = true;
                    if space_count > 0 {
                        saw_space_then_tab = true;
                    }
                    k += 1;
                }
                _ => break,
            }
        }
        if saw_space_then_tab {
            return Err(TokenizeError::MixedIndent { byte_offset: line_start as u32 });
        }
        if saw_tab {
            return Err(TokenizeError::TabIndent { byte_offset: line_start as u32 });
        }
        // The content of the line starts at byte index `k`.
        // Strip a trailing line comment for this line (`//` outside strings).
        let content_end = strip_trailing_comment(bytes, k, line_end);

        // Skip blank / comment-only lines entirely (no LineStart emitted).
        let is_blank = (k..content_end).all(|p| matches!(bytes[p], b' '));
        if is_blank {
            i = next_line_pos;
            continue;
        }

        // Indent must be a multiple of 4.
        if space_count % 4 != 0 {
            return Err(TokenizeError::BadIndent { byte_offset: line_start as u32 });
        }
        let indent = space_count / 4;
        tokens.push(Token {
            kind: TokenKind::LineStart { indent },
            span: Span::new(file_id, line_start as u32, k as u32),
        });

        // Phase B: scan tokens within `[k, content_end)`.
        let mut p = k;
        while p < content_end {
            let b = bytes[p];
            if b == b' ' {
                p += 1;
                continue;
            }
            if b == b'(' {
                tokens.push(Token {
                    kind: TokenKind::Lparen,
                    span: Span::new(file_id, p as u32, (p + 1) as u32),
                });
                p += 1;
            } else if b == b')' {
                tokens.push(Token {
                    kind: TokenKind::Rparen,
                    span: Span::new(file_id, p as u32, (p + 1) as u32),
                });
                p += 1;
            } else if b == b':' {
                tokens.push(Token {
                    kind: TokenKind::Colon,
                    span: Span::new(file_id, p as u32, (p + 1) as u32),
                });
                p += 1;
            } else if b == b',' {
                tokens.push(Token {
                    kind: TokenKind::Comma,
                    span: Span::new(file_id, p as u32, (p + 1) as u32),
                });
                p += 1;
            } else if b == b'=' && p + 1 < content_end && bytes[p + 1] == b'=' {
                tokens.push(Token {
                    kind: TokenKind::DoubleEquals,
                    span: Span::new(file_id, p as u32, (p + 2) as u32),
                });
                p += 2;
            } else if b == b'=' {
                tokens.push(Token {
                    kind: TokenKind::Equals,
                    span: Span::new(file_id, p as u32, (p + 1) as u32),
                });
                p += 1;
            } else if b == b'.' {
                tokens.push(Token {
                    kind: TokenKind::Dot,
                    span: Span::new(file_id, p as u32, (p + 1) as u32),
                });
                p += 1;
            } else if b == b'"' {
                // Walking skeleton: only single-line `"..."` strings.
                let start = p;
                p += 1;
                let mut value = String::new();
                let mut closed = false;
                while p < content_end {
                    let c = bytes[p];
                    if c == b'"' {
                        closed = true;
                        p += 1;
                        break;
                    }
                    if c == b'\\' && p + 1 < content_end {
                        // Minimal escape handling: \" \\ \n \t.
                        let esc = bytes[p + 1];
                        match esc {
                            b'"' => value.push('"'),
                            b'\\' => value.push('\\'),
                            b'n' => value.push('\n'),
                            b't' => value.push('\t'),
                            other => {
                                value.push('\\');
                                value.push(other as char);
                            }
                        }
                        p += 2;
                    } else {
                        value.push(c as char);
                        p += 1;
                    }
                }
                if !closed {
                    return Err(TokenizeError::UnterminatedString { byte_offset: start as u32 });
                }
                tokens.push(Token {
                    kind: TokenKind::StringLit(value),
                    span: Span::new(file_id, start as u32, p as u32),
                });
            } else if is_ident_start(b) {
                let start = p;
                while p < content_end && is_ident_continue(bytes[p]) {
                    p += 1;
                }
                let text = std::str::from_utf8(&bytes[start..p])
                    .expect("ASCII identifier")
                    .to_string();
                tokens.push(Token {
                    kind: TokenKind::Ident(text),
                    span: Span::new(file_id, start as u32, p as u32),
                });
            } else {
                return Err(TokenizeError::UnexpectedChar {
                    byte_offset: p as u32,
                    ch: b as char,
                });
            }
        }

        i = next_line_pos;
    }

    let eof_span = Span::new(file_id, bytes.len() as u32, bytes.len() as u32);
    tokens.push(Token { kind: TokenKind::Eof, span: eof_span });

    Ok((tokens, line_index))
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Find the effective end of the line's tokenizable content, stopping before any `//` comment.
/// Comments inside strings are not stripped — but the walking skeleton has no `//` characters
/// inside string literals, so the simple lexical scan is safe here.
fn strip_trailing_comment(bytes: &[u8], start: usize, end: usize) -> usize {
    let mut p = start;
    let mut in_string = false;
    while p + 1 < end {
        let b = bytes[p];
        if b == b'"' {
            in_string = !in_string;
        } else if !in_string && b == b'/' && bytes[p + 1] == b'/' {
            return p;
        }
        p += 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_skill_header() {
        let src = "skill update_docs()\n";
        let (toks, _) = tokenize(src, 0).unwrap();
        assert!(matches!(toks[0].kind, TokenKind::LineStart { indent: 0 }));
        assert!(matches!(&toks[1].kind, TokenKind::Ident(s) if s == "skill"));
        assert!(matches!(&toks[2].kind, TokenKind::Ident(s) if s == "update_docs"));
        assert_eq!(toks[3].kind, TokenKind::Lparen);
        assert_eq!(toks[4].kind, TokenKind::Rparen);
    }

    #[test]
    fn tokenize_string_lit() {
        let src = "    description: \"hello\"\n";
        let (toks, _) = tokenize(src, 0).unwrap();
        assert!(matches!(toks[0].kind, TokenKind::LineStart { indent: 1 }));
        assert!(matches!(&toks[1].kind, TokenKind::Ident(s) if s == "description"));
        assert_eq!(toks[2].kind, TokenKind::Colon);
        assert!(matches!(&toks[3].kind, TokenKind::StringLit(s) if s == "hello"));
    }

    #[test]
    fn tokenize_double_equals() {
        let src = "        if mode == \"fast\"\n";
        let (toks, _) = tokenize(src, 0).unwrap();
        assert!(matches!(toks[0].kind, TokenKind::LineStart { indent: 2 }));
        assert!(matches!(&toks[1].kind, TokenKind::Ident(s) if s == "if"));
        assert!(matches!(&toks[2].kind, TokenKind::Ident(s) if s == "mode"));
        assert_eq!(toks[3].kind, TokenKind::DoubleEquals);
        assert!(matches!(&toks[4].kind, TokenKind::StringLit(s) if s == "fast"));
    }

    #[test]
    fn tokenize_dot() {
        let src = "        if my_block.applies()\n";
        let (toks, _) = tokenize(src, 0).unwrap();
        assert!(matches!(&toks[2].kind, TokenKind::Ident(s) if s == "my_block"));
        assert_eq!(toks[3].kind, TokenKind::Dot);
        assert!(matches!(&toks[4].kind, TokenKind::Ident(s) if s == "applies"));
    }

    #[test]
    fn tokenize_skips_blank_lines() {
        let src = "skill x()\n\n    flow:\n";
        let (toks, _) = tokenize(src, 0).unwrap();
        let line_starts: Vec<_> = toks
            .iter()
            .filter(|t| matches!(t.kind, TokenKind::LineStart { .. }))
            .collect();
        assert_eq!(line_starts.len(), 2);
    }
}
