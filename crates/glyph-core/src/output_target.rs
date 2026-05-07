//! Pure parser for output-target forms (issue #85 identifier form, issue #86
//! descriptive form).
//!
//! Two accepted forms:
//!   - Identifier form `<IDENT>`: strictly `[a-zA-Z_][a-zA-Z0-9_]*` between
//!     angle brackets. Per `design/values-and-names.md` Allowed Characters
//!     (line 120).
//!   - Descriptive form `<"…">`: a double-quoted string between angle brackets.
//!     Escape handling mirrors `tokenize.rs` string literals (`\"`, `\\`,
//!     `\n`, `\t`; other `\X` passes through verbatim).
//!
//! Used by `parse.rs` (chunk 3) to validate `return <name>` targets.
//! Diagnostic-ID assignment lives in chunk 8; this module surfaces typed
//! `OutputTargetParseError` variants instead.

use crate::span::Span;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputTargetExpr {
    Identifier(Identifier),
    Description(Description),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Description {
    pub content: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identifier {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputTargetParseError {
    MissingOpenBracket,
    UnclosedBracket,
    TrailingChars { byte_offset: u32 },
    Empty,
    InvalidIdentStart { byte_offset: u32, ch: char },
    InvalidIdentChar { byte_offset: u32, ch: char },
    EmptyDescription,
    UnterminatedDescription { byte_offset: u32 },
}

/// Parse an output-target form. `source` must be the candidate substring
/// (e.g. `"<foo>"` or `r#"<"description">"#`). `span` describes its location
/// in the original file.
///
/// On success the returned variant's `span` equals the input `span`
/// (covering the whole form, including brackets — matches the `StringLit`
/// span convention in `tokenize.rs`).
pub fn parse_output_target(
    source: &str,
    span: Span,
) -> Result<OutputTargetExpr, OutputTargetParseError> {
    let bytes = source.as_bytes();
    if bytes.first().copied() != Some(b'<') {
        return Err(OutputTargetParseError::MissingOpenBracket);
    }
    // Dispatch: descriptive form starts with `<"`.
    if bytes.get(1).copied() == Some(b'"') {
        return parse_descriptive(source, span);
    }
    parse_identifier_form(source, span)
}

fn parse_descriptive(source: &str, span: Span) -> Result<OutputTargetExpr, OutputTargetParseError> {
    // Form is `<"…">`. After leading `<"`, walk content with escape handling
    // identical to `tokenize.rs` lines 244-256 (`\"` -> `"`, `\\` -> `\`,
    // `\n` -> '\n', `\t` -> '\t', other `\X` passes through verbatim). Stop
    // at the first unescaped `"`, then expect `>` immediately after, then EOF.
    //
    // Walk by char_indices (not bytes) so multi-byte UTF-8 characters are
    // decoded correctly and pushed as whole chars into `content`.
    debug_assert_eq!(source.as_bytes().first().copied(), Some(b'<'));
    debug_assert_eq!(source.as_bytes().get(1).copied(), Some(b'"'));

    // `inner` is the slice starting just after `<"` — includes content plus the
    // expected closing `">`.
    let inner = &source[2..];
    let mut content = String::new();
    let mut chars = inner.char_indices().peekable();

    // `close_quote_byte` is the byte offset of `"` within `inner` (so absolute
    // offset in `source` is `close_quote_byte + 2`).
    let close_quote_byte = loop {
        match chars.next() {
            None => {
                // Ran off end without finding closing `"`.
                return Err(OutputTargetParseError::UnterminatedDescription {
                    byte_offset: span.start + 1, // the opening `"`
                });
            }
            Some((byte_off, '"')) => {
                break byte_off;
            }
            Some((_, '\\')) => {
                match chars.next() {
                    Some((_, '"')) => content.push('"'),
                    Some((_, '\\')) => content.push('\\'),
                    Some((_, 'n')) => content.push('\n'),
                    Some((_, 't')) => content.push('\t'),
                    Some((_, other)) => {
                        content.push('\\');
                        content.push(other);
                    }
                    None => {
                        // Backslash at very end — unterminated.
                        return Err(OutputTargetParseError::UnterminatedDescription {
                            byte_offset: span.start + 1,
                        });
                    }
                }
            }
            Some((_, c)) => {
                content.push(c);
            }
        }
    };

    // Absolute byte offset of the closing `"` within `source`.
    let abs_close_quote = close_quote_byte + 2;

    // After closing `"`, must be exactly `>` then end-of-slice.
    let bytes = source.as_bytes();
    if bytes.get(abs_close_quote + 1).copied() != Some(b'>') {
        return Err(OutputTargetParseError::UnterminatedDescription {
            byte_offset: span.start + (abs_close_quote + 1) as u32,
        });
    }
    if abs_close_quote + 2 != bytes.len() {
        return Err(OutputTargetParseError::TrailingChars {
            byte_offset: span.start + (abs_close_quote + 2) as u32,
        });
    }
    if content.is_empty() {
        return Err(OutputTargetParseError::EmptyDescription);
    }
    Ok(OutputTargetExpr::Description(Description { content, span }))
}

fn parse_identifier_form(
    source: &str,
    span: Span,
) -> Result<OutputTargetExpr, OutputTargetParseError> {
    let bytes = source.as_bytes();
    let close_idx = match bytes.iter().skip(1).position(|&b| b == b'>') {
        Some(rel) => rel + 1,
        None => return Err(OutputTargetParseError::UnclosedBracket),
    };
    if close_idx + 1 != bytes.len() {
        return Err(OutputTargetParseError::TrailingChars {
            byte_offset: span.start + (close_idx + 1) as u32,
        });
    }
    let inner = &source[1..close_idx];
    let mut chars = inner.char_indices();
    let (first_idx, first_c) = match chars.next() {
        Some(pair) => pair,
        None => return Err(OutputTargetParseError::Empty),
    };
    if !is_ident_start(first_c) {
        return Err(OutputTargetParseError::InvalidIdentStart {
            byte_offset: span.start + 1 + first_idx as u32,
            ch: first_c,
        });
    }
    for (idx, c) in chars {
        if !is_ident_continue(c) {
            return Err(OutputTargetParseError::InvalidIdentChar {
                byte_offset: span.start + 1 + idx as u32,
                ch: c,
            });
        }
    }
    Ok(OutputTargetExpr::Identifier(Identifier {
        name: inner.to_string(),
        span,
    }))
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

fn is_ident_continue(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: u32, end: u32) -> Span {
        Span::new(0, start, end)
    }

    #[test]
    fn parses_simple_identifier() {
        let result = parse_output_target("<foo>", span(0, 5));
        assert_eq!(
            result,
            Ok(OutputTargetExpr::Identifier(Identifier {
                name: "foo".to_string(),
                span: span(0, 5),
            }))
        );
    }

    #[test]
    fn parses_mixed_case_with_digits_and_underscores() {
        let result = parse_output_target("<My_Var_2>", span(0, 10));
        assert_eq!(
            result,
            Ok(OutputTargetExpr::Identifier(Identifier {
                name: "My_Var_2".to_string(),
                span: span(0, 10),
            }))
        );
    }

    #[test]
    fn parses_leading_underscore() {
        let result = parse_output_target("<_x>", span(0, 4));
        assert_eq!(
            result,
            Ok(OutputTargetExpr::Identifier(Identifier {
                name: "_x".to_string(),
                span: span(0, 4),
            }))
        );
    }

    #[test]
    fn rejects_empty_brackets() {
        let result = parse_output_target("<>", span(0, 2));
        assert_eq!(result, Err(OutputTargetParseError::Empty));
    }

    #[test]
    fn rejects_digit_start() {
        let result = parse_output_target("<1foo>", span(0, 6));
        assert_eq!(
            result,
            Err(OutputTargetParseError::InvalidIdentStart {
                byte_offset: 1,
                ch: '1',
            })
        );
    }

    #[test]
    fn rejects_leading_whitespace() {
        let result = parse_output_target("< foo>", span(0, 6));
        assert_eq!(
            result,
            Err(OutputTargetParseError::InvalidIdentStart {
                byte_offset: 1,
                ch: ' ',
            })
        );
    }

    #[test]
    fn rejects_trailing_whitespace() {
        let result = parse_output_target("<foo >", span(0, 6));
        assert_eq!(
            result,
            Err(OutputTargetParseError::InvalidIdentChar {
                byte_offset: 4,
                ch: ' ',
            })
        );
    }

    #[test]
    fn rejects_dot_inside() {
        let result = parse_output_target("<a.b>", span(0, 5));
        assert_eq!(
            result,
            Err(OutputTargetParseError::InvalidIdentChar {
                byte_offset: 2,
                ch: '.',
            })
        );
    }

    #[test]
    fn rejects_parens_inside() {
        let result = parse_output_target("<foo()>", span(0, 7));
        assert_eq!(
            result,
            Err(OutputTargetParseError::InvalidIdentChar {
                byte_offset: 4,
                ch: '(',
            })
        );
    }

    #[test]
    fn descriptive_form_parses() {
        let src = r#"<"root cause analysis including affected files and severity">"#;
        let span = Span::new(0, 0, src.len() as u32);
        let got = parse_output_target(src, span).expect("ok");
        match got {
            OutputTargetExpr::Description(d) => {
                assert_eq!(
                    d.content,
                    "root cause analysis including affected files and severity"
                );
                assert_eq!(d.span, span);
            }
            OutputTargetExpr::Identifier(_) => panic!("expected Description variant"),
        }
    }

    #[test]
    fn descriptive_form_empty_is_malformed() {
        let src = r#"<"">"#;
        let span = Span::new(0, 0, src.len() as u32);
        let err = parse_output_target(src, span).expect_err("must reject empty description");
        assert!(matches!(err, OutputTargetParseError::EmptyDescription));
    }

    #[test]
    fn descriptive_form_unterminated_string_is_malformed() {
        // `<"abc>` — closing `"` missing; the closing `>` does not close the string
        let src = r#"<"abc>"#;
        let span = Span::new(0, 0, src.len() as u32);
        let err =
            parse_output_target(src, span).expect_err("must reject unterminated descriptive form");
        assert!(matches!(
            err,
            OutputTargetParseError::UnterminatedDescription { .. }
        ));
    }

    #[test]
    fn descriptive_form_preserves_inner_text_verbatim() {
        let src = r#"<"with spaces, punctuation; and {braces}">"#;
        let span = Span::new(0, 0, src.len() as u32);
        let got = parse_output_target(src, span).expect("ok");
        match got {
            OutputTargetExpr::Description(d) => {
                assert_eq!(d.content, "with spaces, punctuation; and {braces}");
            }
            _ => panic!("expected Description"),
        }
    }

    #[test]
    fn descriptive_form_processes_escapes_consistently_with_string_literals() {
        // AC11: "strings containing escapes". Mirror tokenize.rs lines 244-256:
        // `\"` -> `"`, `\\` -> `\`, `\n` -> 0x0A, `\t` -> 0x09. Other backslash
        // sequences pass through verbatim.
        let src = r#"<"escaped quote: \"x\" and newline\nhere">"#;
        let span = Span::new(0, 0, src.len() as u32);
        let got = parse_output_target(src, span).expect("ok");
        match got {
            OutputTargetExpr::Description(d) => {
                assert_eq!(d.content, "escaped quote: \"x\" and newline\nhere");
            }
            _ => panic!("expected Description"),
        }
    }

    #[test]
    fn identifier_form_still_parses() {
        // Regression — #85 path must not break.
        let src = "<current_branch>";
        let span = Span::new(0, 0, src.len() as u32);
        let got = parse_output_target(src, span).expect("ok");
        assert!(matches!(got, OutputTargetExpr::Identifier(ref id) if id.name == "current_branch"));
    }

    #[test]
    fn rejects_missing_open_bracket() {
        let result = parse_output_target("foo>", span(0, 4));
        assert_eq!(result, Err(OutputTargetParseError::MissingOpenBracket));
    }

    #[test]
    fn rejects_unclosed_bracket() {
        let result = parse_output_target("<foo", span(0, 4));
        assert_eq!(result, Err(OutputTargetParseError::UnclosedBracket));
    }

    #[test]
    fn rejects_trailing_chars_after_close() {
        let result = parse_output_target("<foo>x", span(0, 6));
        assert_eq!(
            result,
            Err(OutputTargetParseError::TrailingChars { byte_offset: 5 })
        );
    }

    #[test]
    fn rejects_literal_brief_form_with_surrounding_whitespace() {
        // The brief lists `< name >` as a must-reject form. Pinning it as a
        // single test (in addition to the leading- and trailing-whitespace
        // cases) so the AC's literal example fails deterministically.
        let result = parse_output_target("< name >", span(0, 8));
        assert_eq!(
            result,
            Err(OutputTargetParseError::InvalidIdentStart {
                byte_offset: 1,
                ch: ' ',
            })
        );
    }

    #[test]
    fn descriptive_form_preserves_unicode() {
        let src = r#"<"return José's name with — em dash">"#;
        let span = Span::new(0, 0, src.len() as u32);
        let got = parse_output_target(src, span).expect("ok");
        match got {
            OutputTargetExpr::Description(d) => {
                assert_eq!(d.content, "return José's name with — em dash");
            }
            _ => panic!("expected Description"),
        }
    }

    #[test]
    fn descriptive_form_preserves_emoji() {
        let src = r#"<"explain why ✅ vs ❌">"#;
        let span = Span::new(0, 0, src.len() as u32);
        let got = parse_output_target(src, span).expect("ok");
        match got {
            OutputTargetExpr::Description(d) => {
                assert_eq!(d.content, "explain why ✅ vs ❌");
            }
            _ => panic!("expected Description"),
        }
    }

    #[test]
    fn descriptive_form_allows_literal_gt_inside_string() {
        let src = r#"<"return value > 0 only">"#;
        let span = Span::new(0, 0, src.len() as u32);
        let got = parse_output_target(src, span).expect("ok");
        match got {
            OutputTargetExpr::Description(d) => {
                assert_eq!(d.content, "return value > 0 only");
            }
            _ => panic!("expected Description"),
        }
    }
}
