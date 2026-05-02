//! Pure parser for the output-target identifier form `<IDENT>` (issue #85).
//!
//! Strict: rejects whitespace, dots, parens, quotes — anything other than
//! `[a-zA-Z_][a-zA-Z0-9_]*` between the angle brackets. Per
//! `design/values-and-names.md` Allowed Characters (line 120).
//!
//! Used by `parse.rs` (chunk 3) to validate `return <name>` targets.
//! Diagnostic-ID assignment lives in chunk 8; this module surfaces typed
//! `OutputTargetParseError` variants instead.

use crate::span::Span;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputTargetExpr {
    Identifier(Identifier),
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
}

/// Parse an output-target form. `source` must be the candidate substring
/// (e.g. `"<foo>"`). `span` describes its location in the original file.
///
/// On success the returned `Identifier.span` equals the input `span`
/// (covering the whole `<IDENT>` form, including brackets — matches
/// the `StringLit` span convention in `tokenize.rs`).
pub fn parse_output_target(
    source: &str,
    span: Span,
) -> Result<OutputTargetExpr, OutputTargetParseError> {
    let bytes = source.as_bytes();
    if bytes.first().copied() != Some(b'<') {
        return Err(OutputTargetParseError::MissingOpenBracket);
    }
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
    fn rejects_descriptive_form() {
        // `<"...">` is OUT OF SCOPE for #85 (planner decision D2). The deep
        // module rejects it as a malformed identifier-form output target —
        // chunk 8 surfaces the diagnostic.
        let result = parse_output_target("<\"description\">", span(0, 15));
        assert_eq!(
            result,
            Err(OutputTargetParseError::InvalidIdentStart {
                byte_offset: 1,
                ch: '"',
            })
        );
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
}
