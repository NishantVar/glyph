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
    /// `<` — open angle bracket. Only appears in the output-target form
    /// `<IDENT>` (issue #85); MVP has no value-level `<` operator per
    /// `design/values-and-names.md` §No Value-Level Operators (47–55), so
    /// emission is context-free at the lex layer. Position is enforced by
    /// the parser / mid-flow validator.
    LAngle,
    /// `>` — close angle bracket. Mirror of `LAngle`. Standalone only;
    /// `->` is captured earlier as `Arrow`.
    RAngle,
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
        let mut line_end = j;
        // Position past the newline (if any) for the next iteration.
        let mut next_line_pos = if j < bytes.len() { j + 1 } else { j };

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
        let mut content_end = strip_trailing_comment(bytes, k, line_end);

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
            } else if b == b'<' {
                tokens.push(Token {
                    kind: TokenKind::LAngle,
                    span: Span::new(file_id, p as u32, (p + 1) as u32),
                });
                p += 1;
            } else if b == b'>' {
                // Standalone `>`. The `->` form is captured earlier by the
                // `b == b'-'` branch (which consumes both bytes), so reaching
                // here means a `>` not preceded by `-`.
                tokens.push(Token {
                    kind: TokenKind::RAngle,
                    span: Span::new(file_id, p as u32, (p + 1) as u32),
                });
                p += 1;
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
                // Triple-quoted (block) string: `"""..."""`. May span multiple
                // lines. Per `design/values-and-names.md` §Block Strings,
                // common leading indentation is stripped (Python `textwrap.dedent`
                // semantics) and a single `\n` immediately after the opening
                // `"""` or immediately before the closing `"""` is stripped
                // from the value.
                if p + 2 < bytes.len() && bytes[p + 1] == b'"' && bytes[p + 2] == b'"' {
                    let start = p;
                    let (value, end) = scan_triple_string(bytes, start)?;
                    tokens.push(Token {
                        kind: TokenKind::StringLit(value),
                        span: Span::new(file_id, start as u32, end as u32),
                    });
                    p = end;
                    // If the multi-line scan crossed past the current line,
                    // re-anchor the per-line state so the rest of the line
                    // containing the closing `"""` is tokenized normally.
                    if p > line_end {
                        let mut new_j = p;
                        while new_j < bytes.len() && bytes[new_j] != b'\n' {
                            new_j += 1;
                        }
                        line_end = new_j;
                        next_line_pos = if new_j < bytes.len() { new_j + 1 } else { new_j };
                        content_end = strip_trailing_comment(bytes, p, line_end);
                    }
                    continue;
                }
                // Inline (single-line) `"..."` string. Accumulate raw bytes so
                // multi-byte UTF-8 sequences in the source survive verbatim.
                let start = p;
                p += 1;
                let mut buf: Vec<u8> = Vec::new();
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
                            b'"' => buf.push(b'"'),
                            b'\\' => buf.push(b'\\'),
                            b'n' => buf.push(b'\n'),
                            b't' => buf.push(b'\t'),
                            _ => {
                                buf.push(b'\\');
                                buf.push(esc);
                            }
                        }
                        p += 2;
                    } else {
                        buf.push(c);
                        p += 1;
                    }
                }
                if !closed {
                    return Err(TokenizeError::UnterminatedString { byte_offset: start as u32 });
                }
                let value = String::from_utf8(buf).expect("source is valid UTF-8");
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
                // Decode the actual UTF-8 char at `p` so the diagnostic
                // reports the real character (not the lead byte cast as char).
                let ch = source[p..]
                    .chars()
                    .next()
                    .expect("source has a char at p since p < content_end <= bytes.len()");
                return Err(TokenizeError::UnexpectedChar {
                    byte_offset: p as u32,
                    ch,
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

/// Scan a `"""..."""` block string starting at `start` (which points at the
/// first `"` of the opening triple). Returns the post-processed value and the
/// byte offset one past the closing `"""`.
///
/// Post-processing per `design/values-and-names.md` §Block Strings:
///   1. Strip a single `\n` immediately following the opening `"""`.
///   2. Strip a single `\n` immediately preceding the closing `"""`.
///   3. Strip the common leading whitespace prefix from non-empty content
///      lines (Python `textwrap.dedent` semantics).
fn scan_triple_string(bytes: &[u8], start: usize) -> Result<(String, usize), TokenizeError> {
    debug_assert!(
        start + 2 < bytes.len()
            && bytes[start] == b'"'
            && bytes[start + 1] == b'"'
            && bytes[start + 2] == b'"'
    );
    let mut p = start + 3;
    // Accumulate bytes (not chars) so multi-byte UTF-8 sequences in the source
    // are preserved verbatim. Escapes (`\"`, `\\`, `\n`, `\t`) decode to ASCII
    // bytes, and unknown escapes preserve the literal `\X` source bytes.
    let mut raw: Vec<u8> = Vec::new();
    loop {
        if p + 3 <= bytes.len() && &bytes[p..p + 3] == b"\"\"\"" {
            p += 3;
            let raw_str = String::from_utf8(raw).expect("source is valid UTF-8");
            let value = dedent_block_string(&strip_block_newlines(&raw_str));
            return Ok((value, p));
        }
        if p >= bytes.len() {
            return Err(TokenizeError::UnterminatedString { byte_offset: start as u32 });
        }
        let c = bytes[p];
        if c == b'\\' && p + 1 < bytes.len() {
            let esc = bytes[p + 1];
            match esc {
                b'"' => raw.push(b'"'),
                b'\\' => raw.push(b'\\'),
                b'n' => raw.push(b'\n'),
                b't' => raw.push(b'\t'),
                _ => {
                    raw.push(b'\\');
                    raw.push(esc);
                }
            }
            p += 2;
        } else {
            raw.push(c);
            p += 1;
        }
    }
}

/// Strip a single `\n` from the start of a block string body, and the final
/// `\n` plus any pure-whitespace suffix that follows it. This handles both
/// `"""\n…\n"""` (closing `"""` at column 0) and `"""\n…\n    """` (closing
/// `"""` indented to match content).
fn strip_block_newlines(s: &str) -> String {
    let mut out: &str = s;
    if out.starts_with('\n') {
        out = &out[1..];
    }
    if let Some(idx) = out.rfind('\n') {
        if out[idx + 1..].chars().all(|c| c == ' ' || c == '\t') {
            out = &out[..idx];
        }
    }
    out.to_string()
}

/// Strip the common leading whitespace prefix from non-empty content lines.
/// Whitespace-only lines are normalized to empty (matches Python
/// `textwrap.dedent`).
fn dedent_block_string(s: &str) -> String {
    let lines: Vec<&str> = s.split('\n').collect();
    let mut common: Option<&str> = None;
    for line in &lines {
        let stripped = line.trim_start_matches(|c: char| c == ' ' || c == '\t');
        if stripped.is_empty() {
            continue;
        }
        let indent = &line[..line.len() - stripped.len()];
        common = Some(match common {
            None => indent,
            Some(prev) => {
                let n = prev.bytes().zip(indent.bytes()).take_while(|(a, b)| a == b).count();
                &prev[..n]
            }
        });
    }
    let prefix = common.unwrap_or("");
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let stripped = line.trim_start_matches(|c: char| c == ' ' || c == '\t');
        if stripped.is_empty() {
            // whitespace-only line → empty
        } else if line.starts_with(prefix) {
            out.push_str(&line[prefix.len()..]);
        } else {
            out.push_str(line);
        }
    }
    out
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

    #[test]
    fn tokenize_langle() {
        // Issue #85: `<` is a standalone token (output-target form).
        let src = "<\n";
        let (toks, _) = tokenize(src, 0).expect("`<` should tokenize");
        assert_eq!(toks[1].kind, TokenKind::LAngle);
        assert_eq!(toks[1].span.end - toks[1].span.start, 1);
    }

    #[test]
    fn tokenize_rangle() {
        let src = ">\n";
        let (toks, _) = tokenize(src, 0).expect("`>` should tokenize");
        assert_eq!(toks[1].kind, TokenKind::RAngle);
        assert_eq!(toks[1].span.end - toks[1].span.start, 1);
    }

    #[test]
    fn tokenize_triple_quoted_classic_form() {
        // Classic block-string form per `design/values-and-names.md`
        // §Block Strings: opening `"""` on its own line, indented body,
        // closing `"""` at base indent. Common indent stripped, leading
        // and trailing newlines stripped.
        let src = "const x = \"\"\"\n    hello\n    world\n\"\"\"\n";
        let (toks, _) = tokenize(src, 0).expect("triple-quoted should tokenize");
        // [LineStart, "const", "x", "=", StringLit("hello\nworld"), Eof]
        assert!(matches!(&toks[4].kind, TokenKind::StringLit(s) if s == "hello\nworld"));
        assert_eq!(toks[5].kind, TokenKind::Eof);
    }

    #[test]
    fn tokenize_triple_quoted_single_line() {
        // `"""hello"""` is legal and equivalent to `"hello"`.
        let src = "const x = \"\"\"hello\"\"\"\n";
        let (toks, _) = tokenize(src, 0).expect("single-line triple-quoted should tokenize");
        assert!(matches!(&toks[4].kind, TokenKind::StringLit(s) if s == "hello"));
    }

    #[test]
    fn tokenize_triple_quoted_content_on_opening_line() {
        // Content can begin on the opening line. Min-indent calc sees `hello`
        // (col 0) and `    world` (col 4) → min = 0, no dedent applied.
        let src = "const x = \"\"\"hello\n    world\"\"\"\n";
        let (toks, _) = tokenize(src, 0).expect("opening-line content should tokenize");
        assert!(matches!(&toks[4].kind, TokenKind::StringLit(s) if s == "hello\n    world"));
    }

    #[test]
    fn tokenize_triple_quoted_empty() {
        // `""""""` (six quotes) is a legal empty block string.
        let src = "const x = \"\"\"\"\"\"\n";
        let (toks, _) = tokenize(src, 0).expect("empty triple should tokenize");
        assert!(matches!(&toks[4].kind, TokenKind::StringLit(s) if s.is_empty()));
    }

    #[test]
    fn tokenize_triple_quoted_unterminated() {
        // EOF before closing `"""` is `UnterminatedString`.
        let src = "const x = \"\"\"hello\nworld";
        match tokenize(src, 0) {
            Err(TokenizeError::UnterminatedString { .. }) => {}
            other => panic!("expected UnterminatedString, got {:?}", other),
        }
    }

    #[test]
    fn tokenize_triple_quoted_preserves_inner_double_quotes() {
        // `"` and `""` inside a `"""` body are literal content; only `"""` closes.
        let src = "const x = \"\"\"a\"b\"\"c\"\"\"\n";
        let (toks, _) = tokenize(src, 0).expect("inner quotes should tokenize");
        assert!(matches!(&toks[4].kind, TokenKind::StringLit(s) if s == "a\"b\"\"c"));
    }

    #[test]
    fn tokenize_triple_quoted_in_description_field() {
        // Block strings work anywhere inline strings work, including after `:`.
        let src = "    description: \"\"\"\n        multi\n        line\n        \"\"\"\n";
        let (toks, _) = tokenize(src, 0).expect("description block should tokenize");
        // [LineStart, "description", Colon, StringLit("multi\nline"), Eof]
        assert!(matches!(&toks[1].kind, TokenKind::Ident(s) if s == "description"));
        assert_eq!(toks[2].kind, TokenKind::Colon);
        assert!(matches!(&toks[3].kind, TokenKind::StringLit(s) if s == "multi\nline"));
    }

    #[test]
    fn tokenize_triple_quoted_continues_outer_loop_after_close() {
        // After the closing `"""`, the lexer must resume tokenizing the rest
        // of the file correctly (re-anchored line state).
        let src = "const a = \"\"\"\n    foo\n\"\"\"\nconst b = \"bar\"\n";
        let (toks, _) = tokenize(src, 0).expect("post-close tokenization should work");
        // First decl: [LineStart, "const", "a", "=", StringLit("foo")]
        assert!(matches!(&toks[4].kind, TokenKind::StringLit(s) if s == "foo"));
        // Second decl on next line: LineStart at index 5, then "const", "b", "=", StringLit("bar")
        assert!(matches!(toks[5].kind, TokenKind::LineStart { indent: 0 }));
        assert!(matches!(&toks[6].kind, TokenKind::Ident(s) if s == "const"));
        assert!(matches!(&toks[7].kind, TokenKind::Ident(s) if s == "b"));
        assert_eq!(toks[8].kind, TokenKind::Equals);
        assert!(matches!(&toks[9].kind, TokenKind::StringLit(s) if s == "bar"));
    }

    #[test]
    fn tokenize_triple_quoted_blank_content_lines_normalized() {
        // Whitespace-only content lines normalize to empty (textwrap.dedent).
        let src = "const x = \"\"\"\n    hello\n        \n    world\n\"\"\"\n";
        let (toks, _) = tokenize(src, 0).expect("blank lines should normalize");
        assert!(matches!(&toks[4].kind, TokenKind::StringLit(s) if s == "hello\n\nworld"));
    }

    #[test]
    fn tokenize_triple_quoted_escapes() {
        // Escapes (\", \\, \n, \t) work inside `"""` the same as in `"`.
        let src = "const x = \"\"\"a\\nb\\tc\"\"\"\n";
        let (toks, _) = tokenize(src, 0).expect("escapes should work");
        assert!(matches!(&toks[4].kind, TokenKind::StringLit(s) if s == "a\nb\tc"));
    }

    #[test]
    fn tokenize_inline_string_preserves_utf8() {
        // Multi-byte UTF-8 in a regular `"..."` string must survive verbatim.
        let src = "    description: \"café — 🌟\"\n";
        let (toks, _) = tokenize(src, 0).expect("UTF-8 inline should tokenize");
        assert!(matches!(&toks[3].kind, TokenKind::StringLit(s) if s == "café — 🌟"));
    }

    #[test]
    fn tokenize_unexpected_char_reports_full_utf8_char() {
        // A stray non-ASCII char outside a string position reports the actual
        // character (not the lead byte cast as char).
        let src = "skill 🌟()\n";
        match tokenize(src, 0) {
            Err(TokenizeError::UnexpectedChar { ch, .. }) => {
                assert_eq!(ch, '🌟');
            }
            other => panic!("expected UnexpectedChar('🌟'), got {:?}", other),
        }
    }

    #[test]
    fn tokenize_triple_quoted_preserves_utf8() {
        // Multi-byte UTF-8 (é = C3 A9, em-dash = E2 80 94, emoji 🌟 = F0 9F 8C 9F)
        // must round-trip through the block-string scanner unchanged.
        let src = "const x = \"\"\"café — 🌟\"\"\"\n";
        let (toks, _) = tokenize(src, 0).expect("UTF-8 should tokenize");
        assert!(matches!(&toks[4].kind, TokenKind::StringLit(s) if s == "café — 🌟"));
    }

    #[test]
    fn tokenize_triple_quoted_comment_after_close_stripped() {
        // A `// comment` after the closing `"""` is stripped from the line
        // (the multi-line scan re-runs strip_trailing_comment for the closing
        // line's residual content).
        let src = "const x = \"\"\"\n    hi\n\"\"\" // a trailing comment\n";
        let (toks, _) = tokenize(src, 0).expect("trailing comment should not break tokenizing");
        assert!(matches!(&toks[4].kind, TokenKind::StringLit(s) if s == "hi"));
        assert_eq!(toks[5].kind, TokenKind::Eof);
    }

    #[test]
    fn tokenize_output_target_form_yields_three_tokens() {
        // `<foo>` lexes as three primitive tokens — the parser (chunk 3)
        // assembles them and hands the source slice to `parse_output_target`.
        let src = "        return <foo>\n";
        let (toks, _) = tokenize(src, 0).expect("`<foo>` should tokenize");
        // [LineStart, "return", LAngle, "foo", RAngle, Eof]
        assert!(matches!(&toks[1].kind, TokenKind::Ident(s) if s == "return"));
        assert_eq!(toks[2].kind, TokenKind::LAngle);
        assert!(matches!(&toks[3].kind, TokenKind::Ident(s) if s == "foo"));
        assert_eq!(toks[4].kind, TokenKind::RAngle);
    }
}
