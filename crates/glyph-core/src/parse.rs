//! Phase 1 parser — hand-rolled recursive descent over the tokenizer's output.
//!
//! Walking-skeleton scope: parses exactly the constructs needed for
//! `update_docs.glyph.md` per `design/mvp-acceptance.md` §1.

use crate::ast::{
    ConstraintMarker, ConstraintMarkerKind, Decl, FlowStmt, Skill, SourceFile, TextDecl,
};
use crate::diagnostic::{Classification, DiagBag, Diagnostic, SourceSpan};
use crate::span::{LineIndex, Span, Spanned};
use crate::tokenize::{tokenize, Token, TokenKind, TokenizeError};

#[derive(Clone, Debug)]
pub enum ParseError {
    Tokenize(TokenizeError),
    Unexpected { span: Span, message: String },
    Eof { message: String },
}

impl From<TokenizeError> for ParseError {
    fn from(e: TokenizeError) -> Self {
        ParseError::Tokenize(e)
    }
}

pub fn parse(source: &str, file_id: u32) -> Result<(SourceFile, LineIndex), ParseError> {
    let (tokens, line_index) = tokenize(source, file_id)?;
    let mut p = Parser { tokens: &tokens, pos: 0 };
    let file = p.parse_file()?;
    Ok((file, line_index))
}

/// Diagnostic-aware Phase 1 entry point.
///
/// Runs the parser; if the resulting AST violates a structural rule that maps to
/// a structured diagnostic ID (`G::parse::empty-file`, `G::parse::empty-flow`),
/// pushes the corresponding `Diagnostic` onto `bag` and returns `None`. On a
/// successful parse with no structural issues, returns `Some(SourceFile)`.
///
/// `ParseError` (e.g., `Tokenize`, `Unexpected`, `Eof` from the recursive descent
/// itself) is converted into a generic placeholder error diagnostic that uses
/// the `G::parse::empty-file` ID only when the parse failure is the trivial
/// "no top-level declaration found in an otherwise-empty file" case. Other parse
/// errors continue to bubble up via the legacy `parse(...)` entry point until
/// later slices grow per-error structured diagnostics.
pub fn parse_with_diagnostics(
    source: &str,
    file_id: u32,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
) -> Option<SourceFile> {
    // Tokenize. Indent-shape failures (`TabIndent`, `MixedIndent`) are wired to
    // **repairable** structured diagnostics here per `pipeline.md` Phase 1
    // ("Flags tabs and mixed indentation as repairable diagnostics") so that
    // `glyph check`/`glyph compile` can surface an actionable, repair-eligible
    // diagnostic instead of an opaque tokenize error. Other tokenize errors
    // (`BadIndent`, `UnterminatedString`, `UnexpectedChar`) will pick up
    // structured IDs in later slices; for now they bubble through as `None`.
    let tokens = match tokenize(source, file_id) {
        Ok((toks, _)) => toks,
        Err(TokenizeError::TabIndent { byte_offset }) => {
            let span = Span::new(file_id, byte_offset, byte_offset + 1);
            bag.push(
                Diagnostic {
                    id: "G::parse::tab-indent".into(),
                    classification: Classification::Repairable,
                    message: "tab character used for indentation; Glyph requires 4-space indents".into(),
                    span: SourceSpan::from_byte_span(file_label, span, line_index),
                    related: Vec::new(),
                    hints: vec![
                        "`glyph fmt` (Phase 3a) converts tabs to 4-space indentation".into(),
                    ],
                },
                span,
            );
            return None;
        }
        Err(TokenizeError::MixedIndent { byte_offset }) => {
            let span = Span::new(file_id, byte_offset, byte_offset + 1);
            bag.push(
                Diagnostic {
                    id: "G::parse::mixed-indent".into(),
                    classification: Classification::Repairable,
                    message: "mixed space-then-tab indentation; Glyph requires consistent 4-space indents".into(),
                    span: SourceSpan::from_byte_span(file_label, span, line_index),
                    related: Vec::new(),
                    hints: vec![
                        "`glyph fmt` (Phase 3a) normalises mixed indentation to 4-space".into(),
                    ],
                },
                span,
            );
            return None;
        }
        Err(_) => {
            return None;
        }
    };

    // Detect `G::parse::empty-file`: a file with no significant tokens beyond
    // `Eof` (the tokenizer skips blank and comment-only lines). Per
    // `diagnostics.md` §Span Semantics, this is the canonical synthetic-fallback
    // option (3) — "diagnostics whose provenance is genuinely the file as a
    // whole" — so the reported span is `(1, 1) .. (1, 1)`.
    if tokens.len() == 1 && matches!(tokens[0].kind, TokenKind::Eof) {
        let span = Span::new(file_id, 0, 0);
        bag.push(
            Diagnostic::error(
                "G::parse::empty-file",
                "source file has no declarations",
                SourceSpan::from_byte_span(file_label, span, line_index),
            ),
            span,
        );
        return None;
    }

    let mut p = Parser { tokens: &tokens, pos: 0 };
    let file = match p.parse_file() {
        Ok(f) => f,
        Err(_e) => {
            // Other parse errors are not yet wired to structured diagnostic IDs
            // in this slice. The caller (compile_source) handles `None` by
            // surfacing the bag — which will be empty — and returning a
            // CompileError::Parse via the legacy path. For slice 2 we only
            // need empty-file and empty-flow to flow through the bag.
            return None;
        }
    };

    // Detect `G::parse::empty-flow`: a skill whose `flow:` sub-section is
    // syntactically present (parser already consumed it; not detectable here)
    // but contains zero statements. The parser tracks `flow:` presence via
    // `Skill.flow_present`, set when the `flow:` keyword is seen.
    for decl in &file.decls {
        if let Decl::Skill(spanned_skill) = decl {
            let s = &spanned_skill.node;
            if s.flow_present && s.flow.is_empty() {
                let span = spanned_skill.span;
                bag.push(
                    Diagnostic::error(
                        "G::parse::empty-flow",
                        "`flow:` sub-section is present but contains zero statements",
                        SourceSpan::from_byte_span(file_label, span, line_index),
                    ),
                    span,
                );
            }
        }
    }
    if bag.has_error() || bag.has_repairable() {
        return None;
    }

    Some(file)
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn bump(&mut self) -> &Token {
        let t = &self.tokens[self.pos];
        self.pos += 1;
        t
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    /// Returns the indent of the current line if positioned at a LineStart, else None.
    fn current_line_indent(&self) -> Option<u32> {
        match &self.peek().kind {
            TokenKind::LineStart { indent } => Some(*indent),
            _ => None,
        }
    }

    fn expect_line_start(&mut self) -> Result<u32, ParseError> {
        match &self.peek().kind {
            TokenKind::LineStart { indent } => {
                let n = *indent;
                self.pos += 1;
                Ok(n)
            }
            _ => Err(ParseError::Unexpected {
                span: self.peek().span,
                message: "expected start of line".into(),
            }),
        }
    }

    fn expect_ident(&mut self, expected: Option<&str>) -> Result<(String, Span), ParseError> {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::Ident(s) => {
                if let Some(e) = expected {
                    if s != e {
                        return Err(ParseError::Unexpected {
                            span: tok.span,
                            message: format!("expected `{}`, found `{}`", e, s),
                        });
                    }
                }
                self.pos += 1;
                Ok((s, tok.span))
            }
            _ => Err(ParseError::Unexpected {
                span: tok.span,
                message: "expected identifier".into(),
            }),
        }
    }

    fn expect(&mut self, kind: &TokenKind) -> Result<Span, ParseError> {
        if std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind) {
            let span = self.peek().span;
            self.pos += 1;
            Ok(span)
        } else {
            Err(ParseError::Unexpected {
                span: self.peek().span,
                message: format!("expected token {:?}, found {:?}", kind, self.peek().kind),
            })
        }
    }

    fn parse_file(&mut self) -> Result<SourceFile, ParseError> {
        let mut decls = Vec::new();
        loop {
            // Skip any leading LineStart with indent 0 plus advance to a top-level decl keyword.
            if self.at_eof() {
                break;
            }
            // Top-level declarations all begin at indent 0.
            let indent = self.expect_line_start()?;
            if indent != 0 {
                return Err(ParseError::Unexpected {
                    span: self.tokens[self.pos.saturating_sub(1)].span,
                    message: format!("top-level declaration must be at indent 0, got {}", indent),
                });
            }
            let kw = match &self.peek().kind {
                TokenKind::Ident(s) => s.clone(),
                _ => {
                    return Err(ParseError::Unexpected {
                        span: self.peek().span,
                        message: "expected top-level declaration keyword".into(),
                    });
                }
            };
            match kw.as_str() {
                "skill" => {
                    let d = self.parse_skill()?;
                    decls.push(Decl::Skill(d));
                }
                "text" => {
                    let d = self.parse_text_decl()?;
                    decls.push(Decl::Text(d));
                }
                other => {
                    return Err(ParseError::Unexpected {
                        span: self.peek().span,
                        message: format!("unsupported top-level declaration `{}`", other),
                    });
                }
            }
        }
        Ok(SourceFile { decls })
    }

    fn parse_skill(&mut self) -> Result<Spanned<Skill>, ParseError> {
        let (_, kw_span) = self.expect_ident(Some("skill"))?;
        let (name, _) = self.expect_ident(None)?;
        self.expect(&TokenKind::Lparen)?;
        // Walking skeleton: parameterless only.
        self.expect(&TokenKind::Rparen)?;

        let mut description: Option<String> = None;
        let mut body_constraints: Vec<ConstraintMarker> = Vec::new();
        let mut effects: Vec<String> = Vec::new();
        let mut flow: Vec<FlowStmt> = Vec::new();
        let mut flow_present = false;

        // Parse body lines at indent 1.
        loop {
            match self.current_line_indent() {
                Some(1) => {
                    self.parse_skill_body_line(
                        &mut description,
                        &mut body_constraints,
                        &mut effects,
                        &mut flow,
                        &mut flow_present,
                    )?;
                }
                _ => break,
            }
        }

        let end_span = if self.pos > 0 {
            self.tokens[self.pos - 1].span
        } else {
            kw_span
        };
        let span = Span::new(kw_span.file_id, kw_span.start, end_span.end);

        Ok(Spanned::new(
            Skill {
                name,
                description,
                body_constraints,
                effects,
                flow,
                flow_present,
            },
            span,
        ))
    }

    fn parse_skill_body_line(
        &mut self,
        description: &mut Option<String>,
        body_constraints: &mut Vec<ConstraintMarker>,
        effects: &mut Vec<String>,
        flow: &mut Vec<FlowStmt>,
        flow_present: &mut bool,
    ) -> Result<(), ParseError> {
        // Already at LineStart with indent 1.
        self.expect_line_start()?;
        let (kw, kw_span) = match &self.peek().kind {
            TokenKind::Ident(s) => (s.clone(), self.peek().span),
            _ => {
                return Err(ParseError::Unexpected {
                    span: self.peek().span,
                    message: "expected keyword in skill body".into(),
                });
            }
        };

        match kw.as_str() {
            "description" => {
                self.pos += 1;
                self.expect(&TokenKind::Colon)?;
                let s = self.consume_string_after_colon()?;
                if description.is_some() {
                    return Err(ParseError::Unexpected {
                        span: kw_span,
                        message: "duplicate `description:` in skill body".into(),
                    });
                }
                *description = Some(s);
            }
            "effects" => {
                self.pos += 1;
                self.expect(&TokenKind::Colon)?;
                // Walking skeleton: short form only — comma-separated idents on the same line.
                loop {
                    let (eff, _) = self.expect_ident(None)?;
                    effects.push(eff);
                    match &self.peek().kind {
                        TokenKind::Comma => {
                            self.pos += 1;
                        }
                        _ => break,
                    }
                }
            }
            "require" | "avoid" | "must" => {
                self.pos += 1;
                let kind = match kw.as_str() {
                    "require" => ConstraintMarkerKind::Require,
                    "avoid" => ConstraintMarkerKind::Avoid,
                    "must" => {
                        // Could be `must avoid <name>` — peek next ident.
                        if let TokenKind::Ident(next) = &self.peek().kind {
                            if next == "avoid" {
                                self.pos += 1;
                                ConstraintMarkerKind::MustAvoid
                            } else {
                                ConstraintMarkerKind::Must
                            }
                        } else {
                            ConstraintMarkerKind::Must
                        }
                    }
                    _ => unreachable!(),
                };
                let (name, _) = self.expect_ident(None)?;
                body_constraints.push(ConstraintMarker { marker: kind, name });
            }
            "flow" => {
                self.pos += 1;
                self.expect(&TokenKind::Colon)?;
                *flow_present = true;
                // Body at indent 2.
                loop {
                    match self.current_line_indent() {
                        Some(2) => {
                            self.expect_line_start()?;
                            // Walking skeleton: each flow line is exactly one StringLit.
                            match &self.peek().kind {
                                TokenKind::StringLit(s) => {
                                    let v = s.clone();
                                    self.pos += 1;
                                    flow.push(FlowStmt::InlineString(v));
                                }
                                _ => {
                                    return Err(ParseError::Unexpected {
                                        span: self.peek().span,
                                        message: "walking-skeleton flow only supports inline strings".into(),
                                    });
                                }
                            }
                        }
                        _ => break,
                    }
                }
            }
            other => {
                return Err(ParseError::Unexpected {
                    span: kw_span,
                    message: format!("unsupported skill body keyword `{}`", other),
                });
            }
        }
        Ok(())
    }

    /// After a `:`, consume the rest of the line as a single string literal.
    fn consume_string_after_colon(&mut self) -> Result<String, ParseError> {
        match &self.peek().kind {
            TokenKind::StringLit(s) => {
                let v = s.clone();
                self.pos += 1;
                Ok(v)
            }
            _ => Err(ParseError::Unexpected {
                span: self.peek().span,
                message: "expected string literal after `:`".into(),
            }),
        }
    }

    fn parse_text_decl(&mut self) -> Result<Spanned<TextDecl>, ParseError> {
        let (_, kw_span) = self.expect_ident(Some("text"))?;
        let (name, _) = self.expect_ident(None)?;
        self.expect(&TokenKind::Equals)?;
        let value = match &self.peek().kind {
            TokenKind::StringLit(s) => {
                let v = s.clone();
                self.pos += 1;
                v
            }
            _ => {
                return Err(ParseError::Unexpected {
                    span: self.peek().span,
                    message: "expected string literal as `text` value".into(),
                });
            }
        };
        let end_span = self.tokens[self.pos - 1].span;
        let span = Span::new(kw_span.file_id, kw_span.start, end_span.end);
        Ok(Spanned::new(TextDecl { name, value }, span))
    }
}
