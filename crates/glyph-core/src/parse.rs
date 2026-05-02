//! Phase 1 parser — hand-rolled recursive descent over the tokenizer's output.
//!
//! Walking-skeleton scope: parses exactly the constructs needed for
//! `update_docs.glyph.md` per `design/mvp-acceptance.md` §1.

use crate::ast::{
    BlockDecl, ConstDecl, ConstValue, ConstraintMarker, ConstraintMarkerKind, ContextEntry, Decl,
    ElifBranch, ExportBlockDecl, FlowStmt, ImportDecl, ImportKind, ImportName, Param, ReturnExpr,
    Skill, SourceFile,
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
        Err(TokenizeError::UnexpectedChar { byte_offset, ch })
            if ch == '+' || ch == '-' || ch == '*' || ch == '/' =>
        {
            // Note (#82 chunk 2): the prior byte-scan that detected `-> none`
            // here has been deleted now that the tokenizer emits a real
            // `Arrow` token. The `-> None` rejection lives in
            // `Parser::try_parse_return_type` and fires the same
            // `G::parse::none-as-return-type` diagnostic from the parser
            // proper. Stray `-` (e.g., `5 - 2`) still falls through to
            // `G::parse::operator-in-expression` below.
            let span = Span::new(file_id, byte_offset, byte_offset + 1);
            bag.push(
                Diagnostic {
                    id: "G::parse::operator-in-expression".into(),
                    classification: Classification::Repairable,
                    message: format!(
                        "operator `{}` is not supported in expressions; MVP Glyph has no value-level operators",
                        ch
                    ),
                    span: SourceSpan::from_byte_span(file_label, span, line_index),
                    related: Vec::new(),
                    hints: vec![
                        "rewrite using a call expression or inline instruction string".into(),
                    ],
                },
                span,
            );
            return None;
        }
        Err(TokenizeError::LeadingZeroNumeric { byte_offset }) => {
            let span = Span::new(file_id, byte_offset, byte_offset + 1);
            bag.push(
                Diagnostic {
                    id: "G::parse::leading-zero-numeric".into(),
                    classification: Classification::Repairable,
                    message: "numeric literal has a leading zero; per `design/values-and-names.md` §Integers, leading zeros are not allowed on integers or float integer parts".into(),
                    span: SourceSpan::from_byte_span(file_label, span, line_index),
                    related: Vec::new(),
                    hints: vec![
                        "drop the leading zero(s) — write `3` instead of `03`, or `1.5` instead of `01.5`".into(),
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
    // Detect `G::parse::multiple-skills`: more than one `skill` per file.
    {
        let skill_count = file.decls.iter().filter(|d| matches!(d, Decl::Skill(_))).count();
        if skill_count > 1 {
            let span = file.decls.iter().filter_map(|d| match d {
                Decl::Skill(s) => Some(s.span),
                _ => None,
            }).nth(1).unwrap();
            bag.push(
                Diagnostic::error(
                    "G::parse::multiple-skills",
                    "a `.glyph.md` file may contain at most one `skill` declaration",
                    SourceSpan::from_byte_span(file_label, span, line_index),
                ),
                span,
            );
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

    /// Optionally consume a header return-type annotation `-> DomainType`.
    ///
    /// Shared by `parse_skill`, `parse_block_decl`, and `parse_export_block`
    /// per the uniform-grammar decision for issue #82 (the `->`-optional rule
    /// applies to all three kinds — see `design/language-surface.md` §3.1
    /// line 161, §3.2 line 198, §3.3 lines 224/227/230).
    ///
    /// Returns:
    /// - `Ok(None)` if no `Arrow` is at peek (no annotation),
    ///   OR the annotation was `-> none` (case-insensitive) and we emitted
    ///   the repairable `G::parse::none-as-return-type` diagnostic per
    ///   `design/types.md` §none Value lines 81–96 / `design/values-and-names.md`
    ///   §None — in that case we consume the bogus `Arrow Ident` so the parse
    ///   continues, and the outer `parse_with_diagnostics` halts on
    ///   `bag.has_repairable()`.
    /// - `Ok(Some(Spanned<String>))` if `-> Ident` was consumed and the
    ///   ident is a real domain-type name (anything other than `none`).
    fn try_parse_return_type(&mut self) -> Result<Option<Spanned<String>>, ParseError> {
        if !matches!(self.peek().kind, TokenKind::Arrow) {
            return Ok(None);
        }
        let arrow_span = self.peek().span;
        self.pos += 1; // consume `->`

        let (name, name_span) = match &self.peek().kind {
            TokenKind::Ident(s) => {
                let s = s.clone();
                let span = self.peek().span;
                self.pos += 1;
                (s, span)
            }
            _ => {
                return Err(ParseError::Unexpected {
                    span: self.peek().span,
                    message: "expected return-type name after `->`".into(),
                });
            }
        };

        // Reject `-> none` (case-insensitive) per `design/types.md` §none
        // Value lines 81–96 ("`None` as a type annotation (`-> None`) is
        // dropped"). Source-side case-insensitivity is per
        // `design/values-and-names.md` §None. Same ID/tier/message as the
        // pre-Chunk-2 byte-scan path; this is just the relocated detection
        // site.
        if name.eq_ignore_ascii_case("none") {
            let span = Span::new(self.file_id, arrow_span.start, name_span.end);
            self.bag.push(
                Diagnostic {
                    id: "G::parse::none-as-return-type".into(),
                    classification: Classification::Repairable,
                    message: "`-> None` is not a valid return-type annotation; a block with no meaningful return value omits `->` entirely from its header".into(),
                    span: SourceSpan::from_byte_span(self.file_label, span, self.line_index),
                    related: Vec::new(),
                    hints: vec![
                        "drop the `-> None` from the header — Glyph has no `None` type annotation; the absence of `->` already means \"no meaningful return value\"".into(),
                    ],
                },
                span,
            );
            return Ok(None);
        }

        let span = Span::new(self.file_id, arrow_span.start, name_span.end);
        Ok(Some(Spanned::new(name, span)))
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
                "block" => {
                    let d = self.parse_block_decl()?;
                    decls.push(Decl::Block(d));
                }
                "import" => {
                    let d = self.parse_import()?;
                    decls.push(Decl::Import(d));
                }
                "const" => {
                    let d = self.parse_const_decl()?;
                    decls.push(Decl::Const(d));
                }
                "generated" => {
                    // TODO(#81 follow-up): enforce placement order per
                    // language-surface.md §3.6 line 342 (all `generated const`
                    // decls must appear after all non-generated top-level decls).
                    let d = self.parse_generated_const()?;
                    decls.push(Decl::Const(d));
                }
                "export" => {
                    // Peek at the word after `export` to decide:
                    // `export block` | `export const`.
                    let saved = self.pos;
                    self.pos += 1; // skip `export`
                    let next_kw = match &self.peek().kind {
                        TokenKind::Ident(s) => s.clone(),
                        _ => {
                            return Err(ParseError::Unexpected {
                                span: self.peek().span,
                                message: "expected `block` or `const` after `export`".into(),
                            });
                        }
                    };
                    self.pos = saved; // restore
                    match next_kw.as_str() {
                        "block" => {
                            let d = self.parse_export_block()?;
                            decls.push(Decl::ExportBlock(d));
                        }
                        "const" => {
                            let d = self.parse_export_const()?;
                            decls.push(Decl::Const(d));
                        }
                        _ => {
                            return Err(ParseError::Unexpected {
                                span: self.peek().span,
                                message: format!("expected `block` or `const` after `export`, found `{}`", next_kw),
                            });
                        }
                    }
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
        let return_type = self.try_parse_return_type()?;

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
                return_type,
            },
            span,
        ))
    }

    /// Parse `import "<path>" { name1, name2 as alias }` or `import "<path>" as <alias>`.
    fn parse_import(&mut self) -> Result<Spanned<ImportDecl>, ParseError> {
        let (_, kw_span) = self.expect_ident(Some("import"))?;
        // Path must be a string literal.
        let path = match &self.peek().kind {
            TokenKind::StringLit(s) => {
                let v = s.clone();
                self.pos += 1;
                v
            }
            _ => {
                return Err(ParseError::Unexpected {
                    span: self.peek().span,
                    message: "expected string literal (path) after `import`".into(),
                });
            }
        };

        let kind = match &self.peek().kind {
            TokenKind::Lbrace => {
                // Selective import: `{ name1, name2 as alias2 }`
                self.pos += 1; // consume `{`
                let mut names = Vec::new();
                if !matches!(self.peek().kind, TokenKind::Rbrace) {
                    loop {
                        let (name, _) = self.expect_ident(None)?;
                        let alias = if let TokenKind::Ident(kw) = &self.peek().kind {
                            if kw == "as" {
                                self.pos += 1;
                                let (alias_name, _) = self.expect_ident(None)?;
                                Some(alias_name)
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        names.push(ImportName { name, alias });
                        match &self.peek().kind {
                            TokenKind::Comma => {
                                self.pos += 1;
                                // Allow trailing comma before `}`.
                                if matches!(self.peek().kind, TokenKind::Rbrace) {
                                    break;
                                }
                            }
                            _ => break,
                        }
                    }
                }
                self.expect(&TokenKind::Rbrace)?;
                ImportKind::Selective(names)
            }
            TokenKind::Ident(kw) if kw == "as" => {
                // Whole-module import: `as <alias>`
                self.pos += 1;
                let (alias, _) = self.expect_ident(None)?;
                ImportKind::WholeModule { alias }
            }
            _ => {
                return Err(ParseError::Unexpected {
                    span: self.peek().span,
                    message: "expected `{` (selective import) or `as` (whole-module import) after import path".into(),
                });
            }
        };

        let end_span = self.tokens[self.pos.saturating_sub(1)].span;
        let span = Span::new(kw_span.file_id, kw_span.start, end_span.end);
        Ok(Spanned::new(ImportDecl { path, kind }, span))
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
        let return_type = self.try_parse_return_type()?;

        // Skip body — every line whose LineStart indent is > 0.
        // Scan for `return` keyword to set has_return flag, and look at the
        // immediate next token to compute `has_meaningful_return` per
        // issue #82 AC2 (true iff `return <expr>` where `<expr>` is not the
        // `none` value-keyword and not a bare/empty return).
        // Also collect bare-name references for closure checking and word count.
        // Additionally capture description, effects, and flow strings for Tier 3 emission.
        let mut has_return = false;
        let mut has_meaningful_return = false;
        let mut body_refs: Vec<String> = Vec::new();
        let mut body_word_count: usize = 0;
        let mut description: Option<String> = None;
        let mut effects: Vec<String> = Vec::new();
        let mut flow_strings: Vec<String> = Vec::new();
        // Track which sub-section we are currently in.
        let mut current_section: Option<&'static str> = None;
        let body_keywords: &[&str] = &[
            "flow", "return", "description", "effects", "constraints",
            "context", "require", "avoid", "must", "if", "elif", "else",
            "none", "with", "as", "import", "export", "block", "skill",
            "text", "int", "float",
        ];
        loop {
            match self.current_line_indent() {
                Some(n) if n > 0 => {
                    // Drop the LineStart and every token until the next LineStart or Eof.
                    self.pos += 1;
                    // Check if line starts with a sub-section keyword or `return`.
                    if let TokenKind::Ident(kw) = &self.peek().kind {
                        match kw.as_str() {
                            "return" => {
                                has_return = true;
                                // Distinguish meaningful (`return foo`,
                                // `return some_call()`, `return "lit"`) from
                                // non-meaningful (bare `return`,
                                // `return none`). The token after `return`
                                // sits at `self.tokens[self.pos + 1]`.
                                //
                                // The `none` value-keyword is
                                // case-insensitive on the source side
                                // (`design/values-and-names.md` §None;
                                // mirrors the case-insensitive `-> None`
                                // parse rejection at line 380), so
                                // `return None` and `return NONE` are
                                // semantically identical to `return none`
                                // and must NOT count as meaningful.
                                let next = self.tokens.get(self.pos + 1).map(|t| &t.kind);
                                let is_meaningful = match next {
                                    None
                                    | Some(TokenKind::LineStart { .. })
                                    | Some(TokenKind::Eof) => false,
                                    Some(TokenKind::Ident(s)) if s.eq_ignore_ascii_case("none") => false,
                                    _ => true,
                                };
                                if is_meaningful {
                                    has_meaningful_return = true;
                                }
                            }
                            "description" => { current_section = Some("description"); }
                            "effects" => { current_section = Some("effects"); }
                            "flow" => { current_section = Some("flow"); }
                            "constraints" | "context" => { current_section = Some("other"); }
                            _ => {}
                        }
                    }
                    while !self.at_eof()
                        && !matches!(self.peek().kind, TokenKind::LineStart { .. })
                    {
                        match &self.peek().kind {
                            TokenKind::Ident(ident) => {
                                if !body_keywords.contains(&ident.as_str()) {
                                    body_refs.push(ident.clone());
                                    // Capture effect names
                                    if current_section == Some("effects") {
                                        effects.push(ident.clone());
                                    }
                                }
                                body_word_count += 1;
                            }
                            TokenKind::StringLit(s) => {
                                body_word_count += s.split_whitespace().count();
                                // Capture description and flow strings
                                match current_section {
                                    Some("description") => {
                                        description = Some(s.clone());
                                    }
                                    Some("flow") => {
                                        flow_strings.push(s.clone());
                                    }
                                    _ => {}
                                }
                            }
                            _ => {}
                        }
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
        Ok(Spanned::new(ExportBlockDecl {
            name, params, has_return, has_meaningful_return, body_refs, body_word_count,
            description, effects, flow_strings, return_type,
        }, span))
    }

    /// Parse `block <name>(<params>)` with optional body (description, flow,
    /// single-string shorthand).
    fn parse_block_decl(&mut self) -> Result<Spanned<BlockDecl>, ParseError> {
        let (_, kw_span) = self.expect_ident(Some("block"))?;
        let (name, _) = self.expect_ident(None)?;
        self.expect(&TokenKind::Lparen)?;
        let params = self.parse_param_list()?;
        self.expect(&TokenKind::Rparen)?;
        let return_type = self.try_parse_return_type()?;

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
                                                let stmt = self.parse_flow_stmt(2)?;
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
                return_type,
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
                    let span = kw_span;
                    self.bag.push(
                        Diagnostic {
                            id: "G::parse::duplicate-subsection".into(),
                            classification: Classification::Repairable,
                            message: "duplicate `description:` sub-section in skill body".into(),
                            span: SourceSpan::from_byte_span(self.file_label, span, self.line_index),
                            related: Vec::new(),
                            hints: vec![
                                "remove the duplicate or merge contents into one `description:`".into(),
                            ],
                        },
                        span,
                    );
                } else {
                    *description = Some(s);
                }
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
                            let stmt = self.parse_flow_stmt(2)?;
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

    /// Parse a sequence of flow statements at a given indent level.
    /// Returns the collected statements.
    fn parse_flow_body(&mut self, indent: u32) -> Result<Vec<FlowStmt>, ParseError> {
        let mut stmts = Vec::new();
        loop {
            match self.current_line_indent() {
                Some(n) if n == indent => {
                    self.expect_line_start()?;
                    let stmt = self.parse_flow_stmt(indent)?;
                    stmts.push(stmt);
                }
                _ => break,
            }
        }
        Ok(stmts)
    }

    /// Parse a single flow statement (already past LineStart).
    /// Handles inline strings, constraint/context markers, calls, bare names,
    /// and if/elif/else branches. `current_indent` is the indent level of the
    /// line we just consumed (used to determine branch body indent).
    fn parse_flow_stmt(&mut self, current_indent: u32) -> Result<FlowStmt, ParseError> {
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
                            TokenKind::StringLit(s) => {
                                let s = s.clone();
                                self.pos += 1;
                                ReturnExpr::Inline(s)
                            }
                            _ => {
                                return Err(ParseError::Unexpected {
                                    span: self.peek().span,
                                    message: "expected identifier, call, string, or `none` after `return`".into(),
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
                    "flow" => {
                        // `flow:` inside `flow:` is illegal — G::parse::nested-flow.
                        let span = self.peek().span;
                        self.pos += 1;
                        // Consume the colon if present to avoid parse cascade.
                        if matches!(self.peek().kind, TokenKind::Colon) {
                            self.pos += 1;
                        }
                        // Skip any remaining tokens on this line plus indented body.
                        while !self.at_eof()
                            && !matches!(self.peek().kind, TokenKind::LineStart { .. })
                        {
                            self.pos += 1;
                        }
                        // Skip indented body lines (indent > current_indent).
                        loop {
                            match self.current_line_indent() {
                                Some(n) if n > current_indent => {
                                    self.pos += 1; // skip LineStart
                                    while !self.at_eof()
                                        && !matches!(self.peek().kind, TokenKind::LineStart { .. })
                                    {
                                        self.pos += 1;
                                    }
                                }
                                _ => break,
                            }
                        }
                        self.bag.push(
                            Diagnostic::error(
                                "G::parse::nested-flow",
                                "`flow:` inside `flow:` is not allowed",
                                SourceSpan::from_byte_span(self.file_label, span, self.line_index),
                            ),
                            span,
                        );
                        Ok(FlowStmt::BareName(kw_val))
                    }
                    "if" => {
                        self.pos += 1;
                        let condition = self.parse_branch_condition()?;
                        let body_indent = current_indent + 1;
                        let then_body = self.parse_flow_body(body_indent)?;

                        let mut elif_branches: Vec<ElifBranch> = Vec::new();
                        let mut else_body: Option<Vec<FlowStmt>> = None;

                        // Look for elif / else arms at the same indent as `if`.
                        loop {
                            match self.current_line_indent() {
                                Some(n) if n == current_indent => {
                                    // Peek at keyword without consuming LineStart yet.
                                    let saved = self.pos;
                                    self.expect_line_start()?;
                                    match &self.peek().kind {
                                        TokenKind::Ident(kw) if kw == "elif" => {
                                            self.pos += 1;
                                            let cond = self.parse_branch_condition()?;
                                            let body = self.parse_flow_body(body_indent)?;
                                            elif_branches.push(ElifBranch { condition: cond, body });
                                        }
                                        TokenKind::Ident(kw) if kw == "else" => {
                                            self.pos += 1;
                                            let body = self.parse_flow_body(body_indent)?;
                                            else_body = Some(body);
                                            break; // else is always last
                                        }
                                        _ => {
                                            // Not elif/else — put back the LineStart.
                                            self.pos = saved;
                                            break;
                                        }
                                    }
                                }
                                _ => break,
                            }
                        }

                        Ok(FlowStmt::Branch {
                            condition,
                            then_body,
                            elif_branches,
                            else_body,
                        })
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
                            // Check for optional `with "modifier"`.
                            let site_modifier = self.try_parse_with_modifier()?;
                            Ok(FlowStmt::Call {
                                target: kw_val,
                                args,
                                site_modifier,
                            })
                        } else if matches!(self.peek().kind, TokenKind::Dot) {
                            // Detect `name.applies()` used outside a branch condition.
                            let dot_span = self.peek().span;
                            self.pos += 1; // consume `.`
                            if let TokenKind::Ident(method) = &self.peek().kind {
                                if method == "applies" {
                                    // Skip the rest of the `.applies(...)` tokens.
                                    self.pos += 1; // consume `applies`
                                    if matches!(self.peek().kind, TokenKind::Lparen) {
                                        self.pos += 1; // consume `(`
                                        // Skip args until `)`.
                                        while !matches!(self.peek().kind, TokenKind::Rparen | TokenKind::Eof | TokenKind::LineStart { .. }) {
                                            self.pos += 1;
                                        }
                                        if matches!(self.peek().kind, TokenKind::Rparen) {
                                            self.pos += 1;
                                        }
                                    }
                                    self.bag.push(
                                        Diagnostic::error(
                                            "G::parse::applies-outside-condition",
                                            format!("`{}.applies()` can only be used inside an `if`/`elif` condition", kw_val),
                                            SourceSpan::from_byte_span(self.file_label, dot_span, self.line_index),
                                        ),
                                        dot_span,
                                    );
                                    // Return a BareName so parsing can continue.
                                    Ok(FlowStmt::BareName(kw_val))
                                } else {
                                    Err(ParseError::Unexpected {
                                        span: dot_span,
                                        message: format!("unexpected `.{}` after `{}`", method, kw_val),
                                    })
                                }
                            } else {
                                Err(ParseError::Unexpected {
                                    span: dot_span,
                                    message: "unexpected `.` in flow statement".into(),
                                })
                            }
                        } else if matches!(&self.peek().kind, TokenKind::Ident(w) if w == "with") {
                            // `bare_name with "..."` — `with` only attaches to calls.
                            let span = self.peek().span;
                            self.bag.push(
                                Diagnostic::error(
                                    "G::parse::with-on-bare-name",
                                    "`with` modifier requires a call expression (add parentheses)",
                                    SourceSpan::from_byte_span(self.file_label, span, self.line_index),
                                ),
                                span,
                            );
                            // Consume `with` and its string to avoid parse cascade.
                            self.pos += 1;
                            if matches!(self.peek().kind, TokenKind::StringLit(_)) {
                                self.pos += 1;
                            }
                            Ok(FlowStmt::BareName(kw_val))
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

    /// Parse a branch condition: consume all tokens until the next LineStart or Eof.
    /// Returns the condition as a reconstructed string.
    /// Validates applies() syntax: no-parens and with-args diagnostics.
    fn parse_branch_condition(&mut self) -> Result<String, ParseError> {
        let mut parts: Vec<String> = Vec::new();
        loop {
            match &self.peek().kind {
                TokenKind::LineStart { .. } | TokenKind::Eof => break,
                TokenKind::Ident(s) => {
                    let ident = s.clone();
                    let ident_span = self.peek().span;
                    self.pos += 1;

                    // Check for `.applies` pattern.
                    if ident == "applies" && !parts.is_empty() && parts.last() == Some(&".".to_string()) {
                        // Check if followed by `(` — if not, it's applies-no-parens.
                        if !matches!(self.peek().kind, TokenKind::Lparen) {
                            let span = ident_span;
                            self.bag.push(
                                Diagnostic::error(
                                    "G::parse::applies-no-parens",
                                    "`.applies` must be followed by `()` — write `.applies()`",
                                    SourceSpan::from_byte_span(self.file_label, span, self.line_index),
                                ),
                                span,
                            );
                        } else {
                            // Consume `(`
                            self.pos += 1;
                            // Check for args — if next is not `)`, it's applies-with-args.
                            if !matches!(self.peek().kind, TokenKind::Rparen) {
                                let span = ident_span;
                                self.bag.push(
                                    Diagnostic::error(
                                        "G::parse::applies-with-args",
                                        "`.applies()` must not be called with arguments",
                                        SourceSpan::from_byte_span(self.file_label, span, self.line_index),
                                    ),
                                    span,
                                );
                                // Skip args until `)`.
                                while !self.at_eof()
                                    && !matches!(self.peek().kind, TokenKind::Rparen | TokenKind::LineStart { .. })
                                {
                                    self.pos += 1;
                                }
                            }
                            if matches!(self.peek().kind, TokenKind::Rparen) {
                                self.pos += 1;
                            }
                            parts.push(ident);
                            parts.push("(".into());
                            parts.push(")".into());
                            continue;
                        }
                    }

                    parts.push(ident);
                }
                TokenKind::StringLit(s) => {
                    parts.push(format!("\"{}\"", s));
                    self.pos += 1;
                }
                TokenKind::DoubleEquals => {
                    parts.push("==".into());
                    self.pos += 1;
                }
                TokenKind::Dot => {
                    parts.push(".".into());
                    self.pos += 1;
                }
                TokenKind::Lparen => {
                    parts.push("(".into());
                    self.pos += 1;
                }
                TokenKind::Rparen => {
                    parts.push(")".into());
                    self.pos += 1;
                }
                TokenKind::Comma => {
                    parts.push(",".into());
                    self.pos += 1;
                }
                TokenKind::Colon => {
                    parts.push(":".into());
                    self.pos += 1;
                }
                TokenKind::Equals => {
                    parts.push("=".into());
                    self.pos += 1;
                }
                TokenKind::Lbrace => {
                    parts.push("{".into());
                    self.pos += 1;
                }
                TokenKind::Rbrace => {
                    parts.push("}".into());
                    self.pos += 1;
                }
                TokenKind::NumericLit(s) => {
                    parts.push(s.clone());
                    self.pos += 1;
                }
                TokenKind::Arrow => {
                    // `->` is only valid as a header return-type arrow per
                    // `design/language-surface.md` §3; it has no meaning
                    // inside a branch condition.
                    return Err(ParseError::Unexpected {
                        span: self.peek().span,
                        message: "`->` is not allowed inside a branch condition".into(),
                    });
                }
            }
        }
        if parts.is_empty() {
            return Err(ParseError::Unexpected {
                span: self.peek().span,
                message: "expected branch condition after `if` or `elif`".into(),
            });
        }
        // Reconstruct with smart spacing: no space before/after `.`, `(`, `)`.
        let mut result = String::new();
        for (i, part) in parts.iter().enumerate() {
            if i > 0 && part != "." && part != "(" && part != ")" && part != ","
                && parts[i - 1] != "." && parts[i - 1] != "("
            {
                result.push(' ');
            }
            result.push_str(part);
        }
        Ok(result)
    }

    /// Try to parse `with "modifier text"` after a call expression.
    /// Returns `Some(text)` if found, `None` otherwise.
    /// Emits `G::parse::multiple-with` if a second `with` clause follows.
    fn try_parse_with_modifier(&mut self) -> Result<Option<String>, ParseError> {
        if let TokenKind::Ident(kw) = &self.peek().kind {
            if kw == "with" {
                let _with_span = self.peek().span;
                self.pos += 1;
                let modifier = match &self.peek().kind {
                    TokenKind::StringLit(s) => {
                        let v = s.clone();
                        self.pos += 1;
                        v
                    }
                    _ => {
                        return Err(ParseError::Unexpected {
                            span: self.peek().span,
                            message: "expected string literal after `with`".into(),
                        });
                    }
                };
                // Check for chained `with` — `G::parse::multiple-with`.
                if let TokenKind::Ident(kw2) = &self.peek().kind {
                    if kw2 == "with" {
                        let span = self.peek().span;
                        self.bag.push(
                            Diagnostic::error(
                                "G::parse::multiple-with",
                                "only one `with` modifier is allowed per call site",
                                SourceSpan::from_byte_span(self.file_label, span, self.line_index),
                            ),
                            span,
                        );
                        // Consume the second `with` and its string to avoid parse errors.
                        self.pos += 1;
                        if matches!(self.peek().kind, TokenKind::StringLit(_)) {
                            self.pos += 1;
                        }
                    }
                }
                Ok(Some(modifier))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
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

    /// Parse `const NAME = <literal>` where `<literal>` is one of:
    /// String, Int, Float, or Bool — per `design/language-surface.md` §3.4
    /// and the issue #81 type-system slate.
    ///
    /// Bare-name and qualified-name RHS forms are out of scope for #81 and
    /// rejected here with a `ParseError::Unexpected`.
    fn parse_const_decl(&mut self) -> Result<Spanned<ConstDecl>, ParseError> {
        let (_, kw_span) = self.expect_ident(Some("const"))?;
        let (name, _) = self.expect_ident(None)?;
        self.expect(&TokenKind::Equals)?;
        let value = self.parse_const_literal_rhs()?;
        let end_span = self.tokens[self.pos - 1].span;
        let span = Span::new(kw_span.file_id, kw_span.start, end_span.end);
        Ok(Spanned::new(
            ConstDecl { name, value, exported: false, generated: false },
            span,
        ))
    }

    /// Parse `export const NAME = <literal>`.
    fn parse_export_const(&mut self) -> Result<Spanned<ConstDecl>, ParseError> {
        let (_, kw_span) = self.expect_ident(Some("export"))?;
        let (_, _) = self.expect_ident(Some("const"))?;
        let (name, _) = self.expect_ident(None)?;
        self.expect(&TokenKind::Equals)?;
        let value = self.parse_const_literal_rhs()?;
        // Sanity assertion: `export generated const` is invalid grammar
        // (no path produces both flags). Defensive null-check; reaching here
        // with `generated == true` would be a parser bug.
        let end_span = self.tokens[self.pos - 1].span;
        let span = Span::new(kw_span.file_id, kw_span.start, end_span.end);
        Ok(Spanned::new(
            ConstDecl { name, value, exported: true, generated: false },
            span,
        ))
    }

    /// Parse `generated const NAME = "<string>"` — string-only RHS per
    /// `design/language-surface.md` §3.6 (line 324).
    ///
    /// Rejects int/float/bool RHS with a parse error citing the §3.6 rule.
    fn parse_generated_const(&mut self) -> Result<Spanned<ConstDecl>, ParseError> {
        let (_, kw_span) = self.expect_ident(Some("generated"))?;
        let (_, _) = self.expect_ident(Some("const"))?;
        let (name, _) = self.expect_ident(None)?;
        self.expect(&TokenKind::Equals)?;
        // String-only RHS: peek the next token and reject anything but StringLit.
        let value = match &self.peek().kind {
            TokenKind::StringLit(s) => {
                let v = s.clone();
                self.pos += 1;
                ConstValue::String(v)
            }
            _ => {
                return Err(ParseError::Unexpected {
                    span: self.peek().span,
                    message:
                        "expected string literal as `generated const` value (string-only RHS per language-surface.md §3.6)"
                            .into(),
                });
            }
        };
        let end_span = self.tokens[self.pos - 1].span;
        let span = Span::new(kw_span.file_id, kw_span.start, end_span.end);
        Ok(Spanned::new(
            ConstDecl { name, value, exported: false, generated: true },
            span,
        ))
    }

    /// Shared literal-RHS reader for `const` and `export const`. Accepts the
    /// four primitive literal kinds; rejects bare/qualified names and any
    /// other token kind.
    fn parse_const_literal_rhs(&mut self) -> Result<ConstValue, ParseError> {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::StringLit(s) => {
                self.pos += 1;
                Ok(ConstValue::String(s))
            }
            TokenKind::NumericLit(s) => {
                self.pos += 1;
                if s.contains('.') {
                    Ok(ConstValue::Float(s))
                } else {
                    Ok(ConstValue::Int(s))
                }
            }
            TokenKind::Ident(ref s) => {
                // Bool literals tokenize as Ident; case-insensitive on input
                // per `design/values-and-names.md` §Booleans.
                let lower = s.to_ascii_lowercase();
                if lower == "true" || lower == "false" {
                    self.pos += 1;
                    Ok(ConstValue::Bool(s.clone()))
                } else {
                    Err(ParseError::Unexpected {
                        span: tok.span,
                        message: format!(
                            "expected literal RHS (string / number / `true` / `false`) for `const`, found `{}` (bare-name and qualified-name RHS are out of scope for #81)",
                            s
                        ),
                    })
                }
            }
            _ => Err(ParseError::Unexpected {
                span: tok.span,
                message: "expected literal RHS (string / number / `true` / `false`) for `const`"
                    .into(),
            }),
        }
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
    // Recurse into branch bodies to check for return-in-branch.
    for stmt in flow {
        if let FlowStmt::Branch { then_body, elif_branches, else_body, .. } = stmt {
            check_return_rules(then_body, span, file_label, line_index, bag, true);
            for elif in elif_branches {
                check_return_rules(&elif.body, span, file_label, line_index, bag, true);
            }
            if let Some(eb) = else_body {
                check_return_rules(eb, span, file_label, line_index, bag, true);
            }
        }
    }

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

#[cfg(test)]
mod const_decl_tests {
    //! Issue #81 chunk 2 — `Decl::Const` parser coverage.
    //!
    //! Cases follow planner brief: three forms (`const`, `export const`,
    //! `generated const`) × four primitive kinds (String, Int, Float, Bool),
    //! with `generated const × non-string` rejected as a parse error per
    //! `design/language-surface.md` §3.6.

    use super::*;
    use crate::ast::{ConstValue, Decl};

    /// Helper: parse a source string and return the first decl, expecting it
    /// to be a `Decl::Const`. Panics on parse failure or wrong variant.
    fn parse_first_const(src: &str) -> ConstDecl {
        let (file, _) = parse(src, 0).expect("source should parse");
        match file.decls.into_iter().next().expect("expected one decl") {
            Decl::Const(spanned) => spanned.node,
            other => panic!("expected Decl::Const, got {:?}", other),
        }
    }

    // -- `const` form × 4 kinds --

    #[test]
    fn const_string_literal() {
        let d = parse_first_const("const greeting = \"hello\"\n");
        assert_eq!(d.name, "greeting");
        assert!(matches!(&d.value, ConstValue::String(s) if s == "hello"));
        assert!(!d.exported && !d.generated);
    }

    #[test]
    fn const_int_literal() {
        let d = parse_first_const("const max = 3\n");
        assert_eq!(d.name, "max");
        assert!(matches!(&d.value, ConstValue::Int(s) if s == "3"));
        assert!(!d.exported && !d.generated);
    }

    #[test]
    fn const_float_literal() {
        let d = parse_first_const("const ratio = 3.14\n");
        assert_eq!(d.name, "ratio");
        assert!(matches!(&d.value, ConstValue::Float(s) if s == "3.14"));
    }

    #[test]
    fn const_bool_true_literal() {
        let d = parse_first_const("const flag = true\n");
        assert!(matches!(&d.value, ConstValue::Bool(s) if s == "true"));
    }

    // -- `export const` form × 4 kinds --

    #[test]
    fn export_const_string_literal() {
        let d = parse_first_const("export const greeting = \"world\"\n");
        assert_eq!(d.name, "greeting");
        assert!(matches!(&d.value, ConstValue::String(s) if s == "world"));
        assert!(d.exported && !d.generated);
    }

    #[test]
    fn export_const_int_literal() {
        let d = parse_first_const("export const answer = 42\n");
        assert!(matches!(&d.value, ConstValue::Int(s) if s == "42"));
        assert!(d.exported);
    }

    #[test]
    fn export_const_float_literal() {
        let d = parse_first_const("export const zero = 0.0\n");
        assert!(matches!(&d.value, ConstValue::Float(s) if s == "0.0"));
        assert!(d.exported);
    }

    #[test]
    fn export_const_bool_false_literal() {
        let d = parse_first_const("export const off = false\n");
        assert!(matches!(&d.value, ConstValue::Bool(s) if s == "false"));
        assert!(d.exported);
    }

    // -- `generated const` form (string-only RHS positive) --

    #[test]
    fn generated_const_string_literal() {
        let d = parse_first_const("generated const summary = \"auto\"\n");
        assert!(matches!(&d.value, ConstValue::String(s) if s == "auto"));
        assert!(d.generated && !d.exported);
    }

    // -- `generated const × non-string` negative cases (string-only per §3.6) --

    #[test]
    fn generated_const_rejects_int_rhs() {
        let err = parse("generated const x = 3\n", 0).err();
        match err {
            Some(ParseError::Unexpected { ref message, .. }) => {
                assert!(
                    message.contains("string"),
                    "expected message to cite string-only rule, got: {}",
                    message
                );
            }
            other => panic!("expected ParseError::Unexpected, got {:?}", other),
        }
    }

    #[test]
    fn generated_const_rejects_float_rhs() {
        assert!(matches!(
            parse("generated const x = 3.14\n", 0),
            Err(ParseError::Unexpected { .. })
        ));
    }

    #[test]
    fn generated_const_rejects_bool_rhs() {
        assert!(matches!(
            parse("generated const x = true\n", 0),
            Err(ParseError::Unexpected { .. })
        ));
    }

    // -- `const NAME = name_ref` negative (bare-name RHS deferred) --

    #[test]
    fn const_rejects_name_ref_rhs() {
        // `const x = other_binding` — name-ref RHS is out of scope for #81.
        let err = parse("const x = other_binding\n", 0).err();
        match err {
            Some(ParseError::Unexpected { ref message, .. }) => {
                assert!(message.contains("literal"));
            }
            other => panic!("expected ParseError::Unexpected, got {:?}", other),
        }
    }

    // -- Bool case-insensitive on input per `values-and-names.md` §Booleans --

    #[test]
    fn const_bool_uppercase_preserved_in_ast() {
        let d = parse_first_const("const flag = TRUE\n");
        // AST preserves authored casing; lowercase normalization is downstream.
        assert!(matches!(&d.value, ConstValue::Bool(s) if s == "TRUE"));
    }

    // -- Multi-decl file: const + skill coexist --

    #[test]
    fn const_alongside_skill_in_same_file() {
        let src = "\
const greeting = \"hi\"
skill demo()
    flow:
        \"do work\"
";
        let (file, _) = parse(src, 0).expect("should parse");
        // Decl 0: Const, Decl 1: Skill.
        assert!(matches!(&file.decls[0], Decl::Const(_)));
        assert!(matches!(&file.decls[1], Decl::Skill(_)));
    }
}

#[cfg(test)]
mod none_return_tests {
    //! Issue #82 chunk 1 — `G::parse::none-as-return-type`.
    //!
    //! Per `design/types.md` §none Value (No `None` Type Annotation), the
    //! `-> None` return-type annotation is dropped: a block with no
    //! meaningful return value simply omits `->` from its header. Per
    //! `design/values-and-names.md` §None, source is case-insensitive on the
    //! `none` keyword. Per `design/language-surface.md` §3, the rule applies
    //! uniformly to `skill` (§3.1), private `block` (§3.2), and
    //! `export block` (§3.3); `generated block` (§3.7) has no return-type
    //! slot.
    //!
    //! Classification: `repairable` — Phase 3 Repair drops the `-> None`.
    //! Per #82 AC1/AC4, this diagnostic must fire on all three block kinds
    //! that admit a header arrow, and case variants must all be rejected
    //! with the same ID.
    //!
    //! Negative regression: `return none` in a block body (with no `->` on
    //! the header) is the value-position keyword and must continue to parse
    //! cleanly — see `parse_accepts_return_none_in_body_no_arrow`.
    use super::*;
    use crate::span::LineIndex;
    use crate::tokenize::tokenize;

    /// Run `parse_with_diagnostics` and return (ids, exit_code).
    fn run(src: &str) -> (Vec<String>, u8) {
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let _ = parse_with_diagnostics(src, 0, "t.glyph.md", &line_index, &mut bag);
        let ids: Vec<String> = bag.iter().map(|d| d.id.clone()).collect();
        (ids, bag.exit_code())
    }

    #[test]
    fn parse_rejects_arrow_none_on_skill() {
        // AC4(a): `skill foo() -> None` — repairable G::parse::none-as-return-type.
        let src = "skill foo() -> None\n    flow:\n        \"x\"\n";
        let (ids, code) = run(src);
        assert!(
            ids.iter().any(|s| s == "G::parse::none-as-return-type"),
            "expected G::parse::none-as-return-type, got: {:?}",
            ids
        );
        assert!(
            !ids.iter().any(|s| s == "G::parse::operator-in-expression"),
            "must NOT also fire operator-in-expression, got: {:?}",
            ids
        );
        assert_eq!(code, 2, "none-as-return-type is repairable (exit 2)");
    }

    #[test]
    fn parse_rejects_arrow_none_on_block() {
        // AC4(b): `block foo() -> None`.
        let src = "block foo() -> None\n    description: \"d\"\n";
        let (ids, code) = run(src);
        assert!(
            ids.iter().any(|s| s == "G::parse::none-as-return-type"),
            "expected G::parse::none-as-return-type, got: {:?}",
            ids
        );
        assert_eq!(code, 2);
    }

    #[test]
    fn parse_rejects_arrow_none_on_export_block() {
        // AC4(c): `export block foo() -> None`.
        let src = "export block foo() -> None\n    description: \"d\"\n";
        let (ids, code) = run(src);
        assert!(
            ids.iter().any(|s| s == "G::parse::none-as-return-type"),
            "expected G::parse::none-as-return-type, got: {:?}",
            ids
        );
        assert_eq!(code, 2);
    }

    #[test]
    fn parse_rejects_arrow_lowercase_none() {
        // AC4(d): `-> none` (lowercase) — same diagnostic.
        let src = "skill foo() -> none\n    flow:\n        \"x\"\n";
        let (ids, _) = run(src);
        assert!(
            ids.iter().any(|s| s == "G::parse::none-as-return-type"),
            "expected G::parse::none-as-return-type for `-> none`, got: {:?}",
            ids
        );
    }

    #[test]
    fn parse_rejects_arrow_uppercase_none() {
        // AC4(d): `-> NONE` (all-caps) — same diagnostic.
        let src = "skill foo() -> NONE\n    flow:\n        \"x\"\n";
        let (ids, _) = run(src);
        assert!(
            ids.iter().any(|s| s == "G::parse::none-as-return-type"),
            "expected G::parse::none-as-return-type for `-> NONE`, got: {:?}",
            ids
        );
    }

    #[test]
    fn parse_rejects_arrow_none_with_extra_spaces() {
        // The `none` ident may be separated from `->` by whitespace; the
        // detection must be insensitive to a single or multiple spaces.
        let src = "skill foo() ->   None\n    flow:\n        \"x\"\n";
        let (ids, _) = run(src);
        assert!(
            ids.iter().any(|s| s == "G::parse::none-as-return-type"),
            "expected G::parse::none-as-return-type for `->   None`, got: {:?}",
            ids
        );
    }

    #[test]
    fn parse_accepts_return_none_in_body_no_arrow() {
        // AC4(e) regression: `return none` in body with NO `->` on header
        // must continue to parse cleanly. The `none` value-position keyword
        // is unaffected by issue #82.
        let src = "\
skill foo()
    flow:
        return none
";
        // Tokenize must succeed (no `-` at all).
        let (toks, _) = tokenize(src, 0).expect("tokenize should succeed");
        assert!(toks.iter().any(
            |t| matches!(&t.kind, crate::tokenize::TokenKind::Ident(s) if s == "return")
        ));
        // And parse_with_diagnostics must NOT raise none-as-return-type.
        let (ids, _) = run(src);
        assert!(
            !ids.iter().any(|s| s == "G::parse::none-as-return-type"),
            "must NOT fire none-as-return-type for `return none` body, got: {:?}",
            ids
        );
    }

    #[test]
    fn parse_arrow_followed_by_non_none_ident_does_not_fire_this_id() {
        // Negative: `-> SomeOtherIdent` should NOT match this diagnostic
        // (it falls through to the existing operator-in-expression path,
        // since real return-type parsing is out of scope for this chunk).
        let src = "skill foo() -> SomeType\n    flow:\n        \"x\"\n";
        let (ids, _) = run(src);
        assert!(
            !ids.iter().any(|s| s == "G::parse::none-as-return-type"),
            "must NOT fire none-as-return-type for `-> SomeType`, got: {:?}",
            ids
        );
    }

    #[test]
    fn parse_arrow_followed_by_none_prefix_does_not_misfire() {
        // Ident-boundary check: `-> nonexistent` must NOT match `none`
        // (the `none` slice is a prefix of the longer ident).
        let src = "skill foo() -> nonexistent\n    flow:\n        \"x\"\n";
        let (ids, _) = run(src);
        assert!(
            !ids.iter().any(|s| s == "G::parse::none-as-return-type"),
            "must NOT fire none-as-return-type for `-> nonexistent`, got: {:?}",
            ids
        );
    }

    // --- Issue #82 chunk 2: AST `return_type` field is populated ---

    #[test]
    fn parse_skill_return_type_populates_ast() {
        // AC9: `skill foo() -> SomeType` parses cleanly with `return_type`
        // populated on the `Skill` AST node.
        let src = "skill foo() -> SomeType\n    flow:\n        \"x\"\n";
        let (file, _) = parse(src, 0).expect("parse should succeed");
        let skill = file
            .decls
            .iter()
            .find_map(|d| match d {
                crate::ast::Decl::Skill(s) => Some(&s.node),
                _ => None,
            })
            .expect("expected a skill declaration");
        let rt = skill
            .return_type
            .as_ref()
            .expect("expected return_type to be populated");
        assert_eq!(rt.node, "SomeType");
    }

    #[test]
    fn parse_block_return_type_populates_ast() {
        // AC9: `block foo() -> SomeType` parses cleanly with `return_type`
        // populated on the `BlockDecl` AST node.
        let src = "block foo() -> SomeType\n    description: \"d\"\n";
        let (file, _) = parse(src, 0).expect("parse should succeed");
        let block = file
            .decls
            .iter()
            .find_map(|d| match d {
                crate::ast::Decl::Block(b) => Some(&b.node),
                _ => None,
            })
            .expect("expected a block declaration");
        let rt = block
            .return_type
            .as_ref()
            .expect("expected return_type to be populated");
        assert_eq!(rt.node, "SomeType");
    }

    #[test]
    fn parse_export_block_return_type_populates_ast() {
        // AC9: `export block foo() -> SomeType` parses cleanly with
        // `return_type` populated on the `ExportBlockDecl` AST node.
        let src = "export block foo() -> SomeType\n    flow:\n        \"x\"\n        return none\n";
        let (file, _) = parse(src, 0).expect("parse should succeed");
        let eb = file
            .decls
            .iter()
            .find_map(|d| match d {
                crate::ast::Decl::ExportBlock(b) => Some(&b.node),
                _ => None,
            })
            .expect("expected an export block declaration");
        let rt = eb
            .return_type
            .as_ref()
            .expect("expected return_type to be populated");
        assert_eq!(rt.node, "SomeType");
    }

    #[test]
    fn parse_export_block_has_meaningful_return_tracking() {
        // AC2 prerequisite: `has_meaningful_return` is `true` when the body
        // contains `return <expr>` with `<expr>` not the `none` keyword.
        let src = "export block foo() -> SomeType\n    flow:\n        \"x\"\n        return x\n";
        let (file, _) = parse(src, 0).expect("parse should succeed");
        let eb = file
            .decls
            .iter()
            .find_map(|d| match d {
                crate::ast::Decl::ExportBlock(b) => Some(&b.node),
                _ => None,
            })
            .expect("expected an export block declaration");
        assert!(eb.has_return, "has_return should be true");
        assert!(
            eb.has_meaningful_return,
            "has_meaningful_return should be true for `return x`"
        );

        // And `return none` should set has_return but NOT has_meaningful_return.
        let src2 = "export block foo()\n    flow:\n        \"x\"\n        return none\n";
        let (file2, _) = parse(src2, 0).expect("parse should succeed");
        let eb2 = file2
            .decls
            .iter()
            .find_map(|d| match d {
                crate::ast::Decl::ExportBlock(b) => Some(&b.node),
                _ => None,
            })
            .expect("expected an export block declaration");
        assert!(eb2.has_return, "has_return should be true for `return none`");
        assert!(
            !eb2.has_meaningful_return,
            "has_meaningful_return should be false for `return none`"
        );
    }

    // --- Codex pass 1 P2: `return none` detection is case-insensitive ---
    //
    // The `none` value-keyword is case-insensitive on the source side per
    // `design/values-and-names.md` §None (same as the `-> None` parse rule
    // at parse.rs:380). `return None` / `return NONE` must be treated
    // identically to `return none` and NOT count as meaningful, otherwise
    // the analyze rule `G::analyze::export-missing-return-type` would
    // falsely fire on `export block foo() ... return None` without arrow.
    //
    // Tests below pin `has_meaningful_return` directly. The corresponding
    // analyze fire/no-fire behavior is exercised end-to-end through the
    // `G::analyze::export-missing-return-type` integration tests in
    // `crates/glyph-core/src/lib.rs` (issue-#82 chunk 2 site).

    fn first_export_block(src: &str) -> crate::ast::ExportBlockDecl {
        let (file, _diags) = parse(src, 0).expect("parse should succeed");
        file.decls
            .iter()
            .find_map(|d| match d {
                crate::ast::Decl::ExportBlock(b) => Some(b.node.clone()),
                _ => None,
            })
            .expect("expected an export block declaration")
    }

    #[test]
    fn parse_export_block_return_none_pascal_is_not_meaningful() {
        // `return None` (PascalCase) must be treated as no-meaningful-return.
        let src = "export block foo()\n    flow:\n        \"x\"\n        return None\n";
        let eb = first_export_block(src);
        assert!(eb.has_return, "has_return should be true for `return None`");
        assert!(
            !eb.has_meaningful_return,
            "has_meaningful_return should be false for `return None` (case-insensitive `none`)"
        );
    }

    #[test]
    fn parse_export_block_return_none_uppercase_is_not_meaningful() {
        // `return NONE` (all-caps) must be treated as no-meaningful-return.
        let src = "export block foo()\n    flow:\n        \"x\"\n        return NONE\n";
        let eb = first_export_block(src);
        assert!(eb.has_return, "has_return should be true for `return NONE`");
        assert!(
            !eb.has_meaningful_return,
            "has_meaningful_return should be false for `return NONE` (case-insensitive `none`)"
        );
    }

    #[test]
    fn parse_export_block_return_string_literal_without_arrow_is_meaningful() {
        // Regression: a meaningful return (`return "result"`) WITHOUT a
        // `-> DomainType` annotation must still be flagged as meaningful,
        // so the analyze rule `G::analyze::export-missing-return-type`
        // continues to fire for this case.
        let src = "export block foo()\n    flow:\n        \"x\"\n        return \"result\"\n";
        let eb = first_export_block(src);
        assert!(eb.has_return, "has_return should be true for `return \"result\"`");
        assert!(
            eb.has_meaningful_return,
            "has_meaningful_return must remain true for `return \"result\"` without `->`"
        );
        assert!(
            eb.return_type.is_none(),
            "return_type must be None when the header omits `->` — got {:?}",
            eb.return_type
        );
    }
}
