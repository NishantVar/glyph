//! Phase 1 parser — hand-rolled recursive descent over the tokenizer's output.
//!
//! Walking-skeleton scope: parses exactly the constructs needed for
//! `update_docs.glyph.md` per `design/mvp-acceptance.md` §1.

use crate::ast::{
    BlockDecl, ConstraintMarker, ConstraintMarkerKind, ContextEntry, Decl, ExportBlockDecl,
    FlowStmt, Param, ReturnExpr, Skill, SourceFile, TextDecl,
};
use crate::diagnostic::{Classification, DiagBag, Diagnostic, SourceSpan};
use crate::slot::scan_slots;
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
    // Build a throw-away diagnostic context for callers that don't need
    // structured diagnostics — the parser only writes to the bag for the
    // parameter/description slot rules; legacy callers don't exercise those
    // code paths since they were added in slice 4.
    let mut sink = DiagBag::new();
    let mut p = Parser {
        tokens: &tokens,
        pos: 0,
        file_id,
        file_label: "<source>",
        line_index: &line_index,
        bag: &mut sink,
    };
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

    let mut p = Parser {
        tokens: &tokens,
        pos: 0,
        file_id,
        file_label,
        line_index,
        bag,
    };
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
        // Check return-related diagnostics for skills.
        if let Decl::Skill(spanned_skill) = decl {
            check_return_rules(&spanned_skill.node.flow, spanned_skill.span, file_label, line_index, bag, false);
        }
        // Check return-related diagnostics for blocks.
        if let Decl::Block(spanned_block) = decl {
            check_return_rules(&spanned_block.node.flow, spanned_block.span, file_label, line_index, bag, false);
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
    file_id: u32,
    file_label: &'a str,
    line_index: &'a LineIndex,
    bag: &'a mut DiagBag,
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
                "block" => {
                    let d = self.parse_block_decl()?;
                    decls.push(Decl::Block(d));
                }
                "export" => {
                    // Slice 4 supports `export block <name>(<params>)` headers only.
                    // Body is skipped — full lowering ships in a later slice.
                    let d = self.parse_export_block()?;
                    decls.push(Decl::ExportBlock(d));
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
        let params = self.parse_param_list()?;
        self.expect(&TokenKind::Rparen)?;

        let mut description: Option<String> = None;
        let mut body_constraints: Vec<ConstraintMarker> = Vec::new();
        let mut body_context: Vec<ContextEntry> = Vec::new();
        let mut context_section: Vec<ContextEntry> = Vec::new();
        let mut effects: Vec<String> = Vec::new();
        let mut flow: Vec<FlowStmt> = Vec::new();
        let mut flow_present = false;
        let mut body_bare_names: Vec<String> = Vec::new();

        // Parse body lines at indent 1.
        loop {
            match self.current_line_indent() {
                Some(1) => {
                    self.parse_skill_body_line(
                        &mut description,
                        &mut body_constraints,
                        &mut body_context,
                        &mut context_section,
                        &mut effects,
                        &mut flow,
                        &mut flow_present,
                        &mut body_bare_names,
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
                params,
                body_constraints,
                body_context,
                context_section,
                effects,
                flow,
                flow_present,
                body_bare_names,
            },
            span,
        ))
    }

    /// Parse `export block <name>(<params>)` header only (slice 4 placeholder).
    /// Body lines (any indent > 0) are consumed but not stored — full
    /// `export block` lowering ships in a later slice.
    fn parse_export_block(&mut self) -> Result<Spanned<ExportBlockDecl>, ParseError> {
        let (_, kw_span) = self.expect_ident(Some("export"))?;
        let (_, _) = self.expect_ident(Some("block"))?;
        let (name, _) = self.expect_ident(None)?;
        self.expect(&TokenKind::Lparen)?;
        let params = self.parse_param_list()?;
        self.expect(&TokenKind::Rparen)?;

        // Skip body — every line whose LineStart indent is > 0.
        // Scan for `return` keyword to set has_return flag.
        let mut has_return = false;
        loop {
            match self.current_line_indent() {
                Some(n) if n > 0 => {
                    // Drop the LineStart and every token until the next LineStart or Eof.
                    self.pos += 1;
                    // Check if line starts with `return`.
                    if let TokenKind::Ident(kw) = &self.peek().kind {
                        if kw == "return" {
                            has_return = true;
                        }
                    }
                    while !self.at_eof()
                        && !matches!(self.peek().kind, TokenKind::LineStart { .. })
                    {
                        self.pos += 1;
                    }
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
        Ok(Spanned::new(ExportBlockDecl { name, params, has_return }, span))
    }

    /// Parse `block <name>(<params>)` with optional body (description, flow,
    /// single-string shorthand).
    fn parse_block_decl(&mut self) -> Result<Spanned<BlockDecl>, ParseError> {
        let (_, kw_span) = self.expect_ident(Some("block"))?;
        let (name, _) = self.expect_ident(None)?;
        self.expect(&TokenKind::Lparen)?;
        let params = self.parse_param_list()?;
        self.expect(&TokenKind::Rparen)?;

        let mut description: Option<String> = None;
        let mut effects: Vec<String> = Vec::new();
        let mut flow: Vec<FlowStmt> = Vec::new();

        // Parse body lines at indent 1.
        loop {
            match self.current_line_indent() {
                Some(1) => {
                    // Peek at the keyword on this line.
                    let saved_pos = self.pos;
                    self.expect_line_start()?;
                    match &self.peek().kind {
                        TokenKind::Ident(kw) => {
                            let kw = kw.clone();
                            match kw.as_str() {
                                "description" => {
                                    self.pos += 1;
                                    self.expect(&TokenKind::Colon)?;
                                    let s = self.consume_string_after_colon()?;
                                    description = Some(s);
                                }
                                "effects" => {
                                    self.pos += 1;
                                    let colon_span = self.expect(&TokenKind::Colon)?;
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
                                    // Validate `none` exclusivity for blocks too.
                                    if effects.contains(&"none".to_string()) && effects.len() > 1 {
                                        let span = Span::new(self.file_id, colon_span.start, colon_span.end);
                                        self.bag.push(
                                            Diagnostic::error(
                                                "G::parse::none-with-effects",
                                                "`effects: none` must not appear alongside other effect keywords",
                                                SourceSpan::from_byte_span(self.file_label, span, self.line_index),
                                            ),
                                            span,
                                        );
                                    }
                                }
                                "flow" => {
                                    self.pos += 1;
                                    self.expect(&TokenKind::Colon)?;
                                    // Body at indent 2.
                                    loop {
                                        match self.current_line_indent() {
                                            Some(2) => {
                                                self.expect_line_start()?;
                                                let stmt = self.parse_flow_stmt()?;
                                                flow.push(stmt);
                                            }
                                            _ => break,
                                        }
                                    }
                                }
                                _ => {
                                    // Unknown keyword at body level — skip the rest of the line.
                                    self.pos = saved_pos;
                                    self.expect_line_start()?;
                                    while !self.at_eof()
                                        && !matches!(self.peek().kind, TokenKind::LineStart { .. })
                                    {
                                        self.pos += 1;
                                    }
                                }
                            }
                        }
                        TokenKind::StringLit(s) => {
                            // Single-string shorthand: bare string at indent 1, no flow: header.
                            let v = s.clone();
                            self.pos += 1;
                            flow.push(FlowStmt::InlineString(v));
                        }
                        _ => {
                            // Skip unrecognised tokens on this line.
                            while !self.at_eof()
                                && !matches!(self.peek().kind, TokenKind::LineStart { .. })
                            {
                                self.pos += 1;
                            }
                        }
                    }
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
            BlockDecl {
                name,
                description,
                params,
                effects,
                flow,
            },
            span,
        ))
    }

    /// Parse a (possibly empty) comma-separated parameter list between the
    /// opening and closing parens of a header. Walking-skeleton scope: untyped,
    /// optional default of the form `= "literal"` (string only). Type
    /// annotations and non-string defaults are deferred to later slices.
    fn parse_param_list(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params: Vec<Param> = Vec::new();
        // Empty list?
        if matches!(self.peek().kind, TokenKind::Rparen) {
            return Ok(params);
        }
        loop {
            let (pname, name_span) = self.expect_ident(None)?;
            let mut default: Option<String> = None;
            let mut end_span = name_span;
            if matches!(self.peek().kind, TokenKind::Equals) {
                self.pos += 1;
                // Slice 4: only string-literal defaults are supported.
                match &self.peek().kind {
                    TokenKind::StringLit(s) => {
                        let raw = s.clone();
                        let lit_span = self.peek().span;
                        // Reject `{name}` slots inside parameter defaults
                        // (`G::parse::param-slot-in-non-instruction-string`,
                        // repairable per `design/diagnostics.md`).
                        if let Some(off) = crate::slot::first_slot_offset(&raw) {
                            // Map the in-content offset back to a source byte
                            // span. The literal starts with `"` so add 1 for the
                            // opening quote; only meaningful for ASCII content
                            // in the walking skeleton.
                            let span_start = lit_span.start + 1 + off as u32;
                            let span = Span::new(
                                self.file_id,
                                span_start,
                                span_start + 1,
                            );
                            self.bag.push(
                                Diagnostic {
                                    id: "G::parse::param-slot-in-non-instruction-string".into(),
                                    classification: Classification::Repairable,
                                    message: "parameter default is not an instruction-bearing string; `{name}` slots are not allowed here".into(),
                                    span: SourceSpan::from_byte_span(self.file_label, span, self.line_index),
                                    related: Vec::new(),
                                    hints: vec![
                                        "remove the braces or move the slot into an instruction string".into(),
                                    ],
                                },
                                span,
                            );
                        }
                        // Pre-render the default with surrounding quotes — see
                        // `Param.default` doc-comment.
                        default = Some(format!("\"{}\"", raw));
                        end_span = lit_span;
                        self.pos += 1;
                    }
                    _ => {
                        return Err(ParseError::Unexpected {
                            span: self.peek().span,
                            message: "parameter default must be a string literal in slice 4".into(),
                        });
                    }
                }
            }
            let span = Span::new(self.file_id, name_span.start, end_span.end);
            params.push(Param { name: pname, default, span });
            match &self.peek().kind {
                TokenKind::Comma => {
                    self.pos += 1;
                }
                _ => break,
            }
        }
        Ok(params)
    }

    fn parse_skill_body_line(
        &mut self,
        description: &mut Option<String>,
        body_constraints: &mut Vec<ConstraintMarker>,
        body_context: &mut Vec<ContextEntry>,
        context_section: &mut Vec<ContextEntry>,
        effects: &mut Vec<String>,
        flow: &mut Vec<FlowStmt>,
        flow_present: &mut bool,
        body_bare_names: &mut Vec<String>,
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
                // Capture the literal token span before consuming so we can
                // attribute a slot diagnostic to the offending position.
                let lit_span = self.peek().span;
                let s = self.consume_string_after_colon()?;
                // `description:` is a non-instruction-bearing string. Any
                // `{name}` slots inside fire `G::parse::param-slot-in-non-instruction-string`.
                for slot in scan_slots(&s) {
                    let span_start = lit_span.start + 1 + slot.start_in_content as u32;
                    let span = Span::new(self.file_id, span_start, span_start + 1);
                    self.bag.push(
                        Diagnostic {
                            id: "G::parse::param-slot-in-non-instruction-string".into(),
                            classification: Classification::Repairable,
                            message: format!(
                                "`{{{}}}` slot is not allowed in `description:` — descriptions are not instruction-bearing strings",
                                slot.name
                            ),
                            span: SourceSpan::from_byte_span(self.file_label, span, self.line_index),
                            related: Vec::new(),
                            hints: vec![
                                "remove the braces or move the slot into an instruction string".into(),
                            ],
                        },
                        span,
                    );
                }
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
                let colon_span = self.expect(&TokenKind::Colon)?;
                // Short form only — comma-separated idents on the same line.
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
                // Validate `none` exclusivity: `none` must not appear alongside
                // other effect keywords → G::parse::none-with-effects (error).
                if effects.contains(&"none".to_string()) && effects.len() > 1 {
                    let span = Span::new(self.file_id, colon_span.start, colon_span.end);
                    self.bag.push(
                        Diagnostic::error(
                            "G::parse::none-with-effects",
                            "`effects: none` must not appear alongside other effect keywords",
                            SourceSpan::from_byte_span(self.file_label, span, self.line_index),
                        ),
                        span,
                    );
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
            "context" => {
                self.pos += 1;
                // Two forms: `context:` (sub-section) or `context <name>` (body-level marker).
                if matches!(self.peek().kind, TokenKind::Colon) {
                    self.pos += 1;
                    // `context:` sub-section — body at indent 2.
                    // Short form: `context: "inline string"` on the same line.
                    if matches!(self.peek().kind, TokenKind::StringLit(_)) {
                        if let TokenKind::StringLit(s) = &self.peek().kind {
                            let lit_span = self.peek().span;
                            let v = s.clone();
                            // Check for {param} slots in context body.
                            for slot in scan_slots(&v) {
                                let span_start = lit_span.start + 1 + slot.start_in_content as u32;
                                let span = Span::new(self.file_id, span_start, span_start + 1);
                                self.bag.push(
                                    Diagnostic {
                                        id: "G::parse::param-slot-in-non-instruction-string".into(),
                                        classification: Classification::Repairable,
                                        message: format!(
                                            "`{{{}}}` slot is not allowed in `context:` — context is not instruction-bearing",
                                            slot.name
                                        ),
                                        span: SourceSpan::from_byte_span(self.file_label, span, self.line_index),
                                        related: Vec::new(),
                                        hints: vec![
                                            "remove the braces or move the slot into an instruction string".into(),
                                        ],
                                    },
                                    span,
                                );
                            }
                            self.pos += 1;
                            context_section.push(ContextEntry::InlineString(v));
                        }
                    }
                    // Long form: indented entries at indent 2.
                    loop {
                        match self.current_line_indent() {
                            Some(2) => {
                                self.expect_line_start()?;
                                match &self.peek().kind {
                                    TokenKind::StringLit(s) => {
                                        let lit_span = self.peek().span;
                                        let v = s.clone();
                                        for slot in scan_slots(&v) {
                                            let span_start = lit_span.start + 1 + slot.start_in_content as u32;
                                            let span = Span::new(self.file_id, span_start, span_start + 1);
                                            self.bag.push(
                                                Diagnostic {
                                                    id: "G::parse::param-slot-in-non-instruction-string".into(),
                                                    classification: Classification::Repairable,
                                                    message: format!(
                                                        "`{{{}}}` slot is not allowed in `context:` — context is not instruction-bearing",
                                                        slot.name
                                                    ),
                                                    span: SourceSpan::from_byte_span(self.file_label, span, self.line_index),
                                                    related: Vec::new(),
                                                    hints: vec![
                                                        "remove the braces or move the slot into an instruction string".into(),
                                                    ],
                                                },
                                                span,
                                            );
                                        }
                                        self.pos += 1;
                                        context_section.push(ContextEntry::InlineString(v));
                                    }
                                    TokenKind::Ident(name) => {
                                        let v = name.clone();
                                        self.pos += 1;
                                        context_section.push(ContextEntry::NameRef(v));
                                    }
                                    _ => {
                                        return Err(ParseError::Unexpected {
                                            span: self.peek().span,
                                            message: "expected string literal or name in `context:` body".into(),
                                        });
                                    }
                                }
                            }
                            _ => break,
                        }
                    }
                } else {
                    // Body-level `context <name>` or `context "string"` marker.
                    match &self.peek().kind {
                        TokenKind::Ident(name) => {
                            let v = name.clone();
                            self.pos += 1;
                            body_context.push(ContextEntry::NameRef(v));
                        }
                        TokenKind::StringLit(s) => {
                            let lit_span = self.peek().span;
                            let v = s.clone();
                            for slot in scan_slots(&v) {
                                let span_start = lit_span.start + 1 + slot.start_in_content as u32;
                                let span = Span::new(self.file_id, span_start, span_start + 1);
                                self.bag.push(
                                    Diagnostic {
                                        id: "G::parse::param-slot-in-non-instruction-string".into(),
                                        classification: Classification::Repairable,
                                        message: format!(
                                            "`{{{}}}` slot is not allowed in `context` — context is not instruction-bearing",
                                            slot.name
                                        ),
                                        span: SourceSpan::from_byte_span(self.file_label, span, self.line_index),
                                        related: Vec::new(),
                                        hints: vec![
                                            "remove the braces or move the slot into an instruction string".into(),
                                        ],
                                    },
                                    span,
                                );
                            }
                            self.pos += 1;
                            body_context.push(ContextEntry::InlineString(v));
                        }
                        _ => {
                            return Err(ParseError::Unexpected {
                                span: self.peek().span,
                                message: "expected name or string after `context`".into(),
                            });
                        }
                    }
                }
            }
            "constraints" => {
                self.pos += 1;
                self.expect(&TokenKind::Colon)?;
                // `constraints:` sub-section — body at indent 2.
                loop {
                    match self.current_line_indent() {
                        Some(2) => {
                            self.expect_line_start()?;
                            match &self.peek().kind {
                                TokenKind::Ident(kw) => {
                                    let kw = kw.clone();
                                    self.pos += 1;
                                    let kind = match kw.as_str() {
                                        "require" => ConstraintMarkerKind::Require,
                                        "avoid" => ConstraintMarkerKind::Avoid,
                                        "must" => {
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
                                        _ => {
                                            return Err(ParseError::Unexpected {
                                                span: self.peek().span,
                                                message: format!("expected constraint keyword (`require`, `avoid`, `must`), found `{}`", kw),
                                            });
                                        }
                                    };
                                    let (name, _) = self.expect_ident(None)?;
                                    body_constraints.push(ConstraintMarker { marker: kind, name });
                                }
                                _ => {
                                    return Err(ParseError::Unexpected {
                                        span: self.peek().span,
                                        message: "expected constraint marker in `constraints:` body".into(),
                                    });
                                }
                            }
                        }
                        _ => break,
                    }
                }
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
                            let stmt = self.parse_flow_stmt()?;
                            flow.push(stmt);
                        }
                        _ => break,
                    }
                }
            }
            _other => {
                // Bare name at body level — not a recognized keyword.
                // Store it for analyze to check `G::analyze::ambiguous-role`.
                self.pos += 1;
                body_bare_names.push(kw.clone());
            }
        }
        Ok(())
    }

    /// Parse a single flow statement (already past LineStart).
    /// Handles inline strings, constraint/context markers, calls, and bare names.
    fn parse_flow_stmt(&mut self) -> Result<FlowStmt, ParseError> {
        match &self.peek().kind {
            TokenKind::StringLit(s) => {
                let v = s.clone();
                self.pos += 1;
                Ok(FlowStmt::InlineString(v))
            }
            TokenKind::Ident(kw) => {
                let kw_val = kw.clone();
                match kw_val.as_str() {
                    "require" | "avoid" | "must" => {
                        self.pos += 1;
                        let kind = match kw_val.as_str() {
                            "require" => ConstraintMarkerKind::Require,
                            "avoid" => ConstraintMarkerKind::Avoid,
                            "must" => {
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
                        Ok(FlowStmt::ConstraintMarker(ConstraintMarker {
                            marker: kind,
                            name,
                        }))
                    }
                    "return" => {
                        self.pos += 1;
                        // Parse the return expression.
                        let expr = match &self.peek().kind {
                            TokenKind::LineStart { .. } | TokenKind::Eof => {
                                // Bare `return` with no expression = return none.
                                ReturnExpr::None
                            }
                            TokenKind::Ident(name) if name == "none" => {
                                self.pos += 1;
                                ReturnExpr::None
                            }
                            TokenKind::Ident(name) => {
                                let name = name.clone();
                                self.pos += 1;
                                // Check if it's a call: name(args).
                                if matches!(self.peek().kind, TokenKind::Lparen) {
                                    self.pos += 1; // consume `(`
                                    let mut args: Vec<String> = Vec::new();
                                    if !matches!(self.peek().kind, TokenKind::Rparen) {
                                        loop {
                                            match &self.peek().kind {
                                                TokenKind::Ident(a) => {
                                                    args.push(a.clone());
                                                    self.pos += 1;
                                                }
                                                TokenKind::StringLit(a) => {
                                                    args.push(a.clone());
                                                    self.pos += 1;
                                                }
                                                _ => {
                                                    return Err(ParseError::Unexpected {
                                                        span: self.peek().span,
                                                        message: "expected argument in return call".into(),
                                                    });
                                                }
                                            }
                                            match &self.peek().kind {
                                                TokenKind::Comma => { self.pos += 1; }
                                                _ => break,
                                            }
                                        }
                                    }
                                    self.expect(&TokenKind::Rparen)?;
                                    ReturnExpr::Call { target: name, args }
                                } else {
                                    ReturnExpr::Name(name)
                                }
                            }
                            _ => {
                                return Err(ParseError::Unexpected {
                                    span: self.peek().span,
                                    message: "expected identifier, call, or `none` after `return`".into(),
                                });
                            }
                        };
                        Ok(FlowStmt::Return(expr))
                    }
                    "context" => {
                        self.pos += 1;
                        match &self.peek().kind {
                            TokenKind::Ident(name) => {
                                let v = name.clone();
                                self.pos += 1;
                                Ok(FlowStmt::ContextMarker(ContextEntry::NameRef(v)))
                            }
                            TokenKind::StringLit(s) => {
                                let v = s.clone();
                                self.pos += 1;
                                Ok(FlowStmt::ContextMarker(ContextEntry::InlineString(v)))
                            }
                            _ => Err(ParseError::Unexpected {
                                span: self.peek().span,
                                message: "expected name or string after `context` in flow".into(),
                            }),
                        }
                    }
                    _ => {
                        // Could be a call (name followed by `(`) or a bare name.
                        self.pos += 1;
                        if matches!(self.peek().kind, TokenKind::Lparen) {
                            // Call expression: name(args)
                            self.pos += 1; // consume `(`
                            let mut args: Vec<String> = Vec::new();
                            if !matches!(self.peek().kind, TokenKind::Rparen) {
                                loop {
                                    // Positional args: identifiers or string literals.
                                    match &self.peek().kind {
                                        TokenKind::Ident(a) => {
                                            args.push(a.clone());
                                            self.pos += 1;
                                        }
                                        TokenKind::StringLit(a) => {
                                            args.push(a.clone());
                                            self.pos += 1;
                                        }
                                        _ => {
                                            return Err(ParseError::Unexpected {
                                                span: self.peek().span,
                                                message: "expected argument in call".into(),
                                            });
                                        }
                                    }
                                    match &self.peek().kind {
                                        TokenKind::Comma => {
                                            self.pos += 1;
                                        }
                                        _ => break,
                                    }
                                }
                            }
                            self.expect(&TokenKind::Rparen)?;
                            Ok(FlowStmt::Call {
                                target: kw_val,
                                args,
                            })
                        } else {
                            Ok(FlowStmt::BareName(kw_val))
                        }
                    }
                }
            }
            _ => Err(ParseError::Unexpected {
                span: self.peek().span,
                message: "expected string, keyword, or name in flow body".into(),
            }),
        }
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

/// Check return-related structural rules on a flow statement list.
///
/// - `G::parse::return-not-terminal` — `return` is not the last statement.
/// - `G::parse::multiple-returns` — more than one `return`.
/// - `G::parse::return-in-branch` — `return` inside a branch body (when `in_branch` is true).
pub(crate) fn check_return_rules(
    flow: &[FlowStmt],
    span: Span,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    in_branch: bool,
) {
    let return_positions: Vec<usize> = flow
        .iter()
        .enumerate()
        .filter_map(|(i, stmt)| matches!(stmt, FlowStmt::Return(_)).then_some(i))
        .collect();

    if return_positions.is_empty() {
        return;
    }

    // G::parse::return-in-branch — return inside a branch body.
    if in_branch {
        bag.push(
            Diagnostic::error(
                "G::parse::return-in-branch",
                "`return` is not allowed inside an `if`/`elif`/`else` branch",
                SourceSpan::from_byte_span(file_label, span, line_index),
            ),
            span,
        );
        return; // Don't fire other return diagnostics for in-branch returns.
    }

    // G::parse::multiple-returns — more than one return.
    if return_positions.len() > 1 {
        bag.push(
            Diagnostic::error(
                "G::parse::multiple-returns",
                "more than one `return` statement in `flow:`",
                SourceSpan::from_byte_span(file_label, span, line_index),
            ),
            span,
        );
        return; // Don't also fire return-not-terminal for multi-return.
    }

    // G::parse::return-not-terminal — single return not at the end.
    let pos = return_positions[0];
    if pos != flow.len() - 1 {
        bag.push(
            Diagnostic::error(
                "G::parse::return-not-terminal",
                "`return` must be the last statement in `flow:`",
                SourceSpan::from_byte_span(file_label, span, line_index),
            ),
            span,
        );
    }
}
