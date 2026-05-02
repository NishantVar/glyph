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
    /// Unsigned numeric literal — source-text slice (e.g. `"3"`, `"0.0"`, `"3.14"`).
    /// Grammar: `[0-9]+(\.[0-9]+)?`. No leading `.`, no trailing `.`, no exponent,
    /// no sign. Disambiguation between Int and Float is performed by
    /// `kind_infer::infer_primitive` based on `'.'` presence.
    NumericLit(String),
    Lparen,
    Rparen,
    Colon,
    Comma,
    Equals,
    /// `.` — dot separator (e.g., `block_name.applies()`).
    Dot,
    /// `==` — branch condition equality (not a value-level operator).
    DoubleEquals,
    /// `->` — return-type arrow on `skill` / `block` / `export block` headers
    /// per `design/language-surface.md` §3.1/§3.2/§3.3. The parser optionally
    /// consumes `Arrow Ident` after the header `(...)`. Per `design/types.md`
    /// §none Value lines 81–96, `-> None` is rejected at the parser layer
    /// with `G::parse::none-as-return-type`.
    Arrow,
    /// `{` — opening brace (selective imports).
    Lbrace,
    /// `}` — closing brace (selective imports).
    Rbrace,
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
    /// Multi-digit integer (or float integer part) starting with `0` —
    /// rejected per `design/values-and-names.md` §Integers ("Leading zeros
    /// are not allowed."). `0` alone and `0.X` floats remain valid.
    LeadingZeroNumeric { byte_offset: u32 },
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
            } else if b == b'-' && p + 1 < content_end && bytes[p + 1] == b'>' {
                // `->` — return-type arrow. Consumed as a single 2-byte token;
                // bare `-` continues to fall through to `UnexpectedChar` so the
                // existing `G::parse::operator-in-expression` repairable
                // diagnostic is preserved for `5 - 2`-style stray operators.
                tokens.push(Token {
                    kind: TokenKind::Arrow,
                    span: Span::new(file_id, p as u32, (p + 2) as u32),
                });
                p += 2;
            } else if b == b'{' {
                tokens.push(Token {
                    kind: TokenKind::Lbrace,
                    span: Span::new(file_id, p as u32, (p + 1) as u32),
                });
                p += 1;
            } else if b == b'}' {
                tokens.push(Token {
                    kind: TokenKind::Rbrace,
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
            } else if b.is_ascii_digit() {
                // Unsigned numeric literal: [0-9]+(\.[0-9]+)?.
                // Carved out for #81 const keyword: bare numeric RHS at parse time.
                let start = p;
                while p < content_end && bytes[p].is_ascii_digit() {
                    p += 1;
                }
                // Reject leading zeros on the integer part per
                // `design/values-and-names.md` §Integers ("Leading zeros are
                // not allowed."). `0` alone and `0.X` floats remain valid;
                // only multi-digit integer parts starting with `0` are
                // rejected. Applies to both pure integers (`03`) and float
                // integer parts (`01.5`); the fractional part of a float
                // (`1.05`) is unaffected.
                if p - start > 1 && bytes[start] == b'0' {
                    return Err(TokenizeError::LeadingZeroNumeric { byte_offset: start as u32 });
                }
                if p < content_end
                    && bytes[p] == b'.'
                    && p + 1 < content_end
                    && bytes[p + 1].is_ascii_digit()
                {
                    p += 1; // consume '.'
                    while p < content_end && bytes[p].is_ascii_digit() {
                        p += 1;
                    }
                }
                let text = std::str::from_utf8(&bytes[start..p])
                    .expect("ASCII numeric literal")
                    .to_string();
                tokens.push(Token {
                    kind: TokenKind::NumericLit(text),
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
    fn tokenize_int_numeric_lit() {
        let src = "const x = 42\n";
        let (toks, _) = tokenize(src, 0).unwrap();
        // [LineStart, Ident("const"), Ident("x"), Equals, NumericLit("42"), Eof]
        assert!(matches!(&toks[4].kind, TokenKind::NumericLit(s) if s == "42"));
    }

    #[test]
    fn tokenize_float_numeric_lit() {
        let src = "const pi = 3.14\n";
        let (toks, _) = tokenize(src, 0).unwrap();
        assert!(matches!(&toks[4].kind, TokenKind::NumericLit(s) if s == "3.14"));
    }

    #[test]
    fn tokenize_rejects_leading_zero_integer() {
        // Per `design/values-and-names.md` §Integers, multi-digit integers
        // starting with `0` are forbidden. This is a regression-proof for
        // chunk 2's NumericLit carve-out (the prior `text` decl path didn't
        // accept numbers, so this is a NEW class of malformed source #81
        // introduces — must reject at tokenize time).
        let src = "const x = 03\n";
        match tokenize(src, 0) {
            Err(TokenizeError::LeadingZeroNumeric { byte_offset }) => {
                assert_eq!(byte_offset, 10, "byte offset should point at the `0`");
            }
            Err(other) => panic!("expected LeadingZeroNumeric, got {:?}", other),
            Ok(_) => panic!("`03` should fail to tokenize"),
        }
    }

    #[test]
    fn tokenize_rejects_leading_zero_float_integer_part() {
        // Same rule applies to the integer part of a float: `01.5` rejects.
        let src = "const x = 01.5\n";
        match tokenize(src, 0) {
            Err(TokenizeError::LeadingZeroNumeric { .. }) => {}
            Err(other) => panic!("expected LeadingZeroNumeric, got {:?}", other),
            Ok(_) => panic!("`01.5` should fail to tokenize"),
        }
    }

    #[test]
    fn tokenize_accepts_zero_alone() {
        // `0` is the canonical integer-zero literal — must remain valid.
        let src = "const x = 0\n";
        let (toks, _) = tokenize(src, 0).expect("`0` should tokenize");
        assert!(matches!(&toks[4].kind, TokenKind::NumericLit(s) if s == "0"));
    }

    #[test]
    fn tokenize_accepts_zero_dot_float() {
        // `0.001` is a leading-zero float (allowed — single `0` followed by
        // `.` and digits). Mirrors the existing `const_float.glyph.md`
        // fixture which uses `0.001`.
        let src = "const x = 0.001\n";
        let (toks, _) = tokenize(src, 0).expect("`0.001` should tokenize");
        assert!(matches!(&toks[4].kind, TokenKind::NumericLit(s) if s == "0.001"));
    }

    #[test]
    fn tokenize_accepts_leading_zero_in_fractional_part() {
        // The leading-zero rule only applies to the INTEGER part. `1.05` is
        // a valid float — the `0` is in the fractional digits, not the
        // integer part.
        let src = "const x = 1.05\n";
        let (toks, _) = tokenize(src, 0).expect("`1.05` should tokenize");
        assert!(matches!(&toks[4].kind, TokenKind::NumericLit(s) if s == "1.05"));
    }

    #[test]
    fn tokenize_arrow_is_single_token() {
        // `->` lexes as a single Arrow token, not as `-` + `>`. This is the
        // tokenizer half of issue #82's return-type-annotation support.
        let src = "skill foo() -> SomeType\n";
        let (toks, _) = tokenize(src, 0).expect("`->` should tokenize cleanly");
        // [LineStart, "skill", "foo", Lparen, Rparen, Arrow, "SomeType", Eof]
        assert_eq!(toks[5].kind, TokenKind::Arrow);
        assert!(matches!(&toks[6].kind, TokenKind::Ident(s) if s == "SomeType"));
        // Span is 2 bytes covering `->`.
        assert_eq!(toks[5].span.end - toks[5].span.start, 2);
    }

    #[test]
    fn tokenize_stray_minus_still_unexpected() {
        // A stray `-` not followed by `>` continues to fail tokenization so
        // `G::parse::operator-in-expression` keeps firing for value-level
        // operator misuse like `5 - 2`.
        let src = "const x = 5 - 2\n";
        match tokenize(src, 0) {
            Err(TokenizeError::UnexpectedChar { ch: '-', .. }) => {}
            Err(other) => panic!("expected UnexpectedChar('-'), got error {:?}", other),
            Ok(_) => panic!("expected UnexpectedChar('-'), but tokenize succeeded"),
        }
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
