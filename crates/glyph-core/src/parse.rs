//! Phase 1 parser — hand-rolled recursive descent over the tokenizer's output.
//!
//! Walking-skeleton scope: parses exactly the constructs needed for
//! `update_docs.glyph.md` per `design/mvp-acceptance.md` §1.

use crate::ast::{
    BlockDecl, ConstDecl, ConstKind, ConstraintMarker, ConstraintMarkerKind, ContextEntry, Decl,
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
        enable_effects: true,
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
    parse_with_diagnostics_opts(source, file_id, file_label, line_index, bag, false)
}

/// Diagnostic-aware Phase 1 entry point with effects gate.
///
/// When `enable_effects` is false, any `effects:` sub-section on `skill`,
/// `block`, or `export block` declarations produces a `G::parse::effects-disabled`
/// error diagnostic and parsing halts.
pub fn parse_with_diagnostics_opts(
    source: &str,
    file_id: u32,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    enable_effects: bool,
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
        enable_effects,
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
    enable_effects: bool,
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
                "const" => {
                    let d = self.parse_const_decl()?;
                    decls.push(Decl::Const(d));
                }
                "block" => {
                    let d = self.parse_block_decl()?;
                    decls.push(Decl::Block(d));
                }
                "import" => {
                    let d = self.parse_import()?;
                    decls.push(Decl::Import(d));
                }
                "export" => {
                    // Peek at the word after `export` to decide: `export block` or `export const`.
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
                        legacy @ ("text" | "int" | "float") => {
                            return Err(ParseError::Unexpected {
                                span: self.peek().span,
                                message: format!(
                                    "`export {}` is no longer supported; use `export const` instead",
                                    legacy
                                ),
                            });
                        }
                        _ => {
                            return Err(ParseError::Unexpected {
                                span: self.peek().span,
                                message: format!("expected `block` or `const` after `export`, found `{}`", next_kw),
                            });
                        }
                    }
                }
                "generated" => {
                    // Peek at the word after `generated` to support `generated const`
                    // (the only generated-form value binding). `generated block` is
                    // a separate slice and not yet implemented in the parser.
                    let saved = self.pos;
                    self.pos += 1; // skip `generated`
                    let next_kw = match &self.peek().kind {
                        TokenKind::Ident(s) => s.clone(),
                        _ => {
                            return Err(ParseError::Unexpected {
                                span: self.peek().span,
                                message: "expected `const` after `generated`".into(),
                            });
                        }
                    };
                    self.pos = saved; // restore
                    match next_kw.as_str() {
                        "const" => {
                            let d = self.parse_generated_const()?;
                            decls.push(Decl::Const(d));
                        }
                        legacy @ ("text" | "int" | "float") => {
                            return Err(ParseError::Unexpected {
                                span: self.peek().span,
                                message: format!(
                                    "`generated {}` is no longer supported; use `generated const` instead",
                                    legacy
                                ),
                            });
                        }
                        _ => {
                            return Err(ParseError::Unexpected {
                                span: self.peek().span,
                                message: format!("expected `const` after `generated`, found `{}`", next_kw),
                            });
                        }
                    }
                }
                legacy @ ("text" | "int" | "float") => {
                    return Err(ParseError::Unexpected {
                        span: self.peek().span,
                        message: format!(
                            "`{}` is no longer a value-binding keyword; use `const` instead",
                            legacy
                        ),
                    });
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

    /// Parse `export const <name> = "<value>"`.
    fn parse_export_const(&mut self) -> Result<Spanned<ConstDecl>, ParseError> {
        let (_, kw_span) = self.expect_ident(Some("export"))?;
        let (_, _) = self.expect_ident(Some("const"))?;
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
                    message: "expected string literal as `export const` value".into(),
                });
            }
        };
        let end_span = self.tokens[self.pos - 1].span;
        let span = Span::new(kw_span.file_id, kw_span.start, end_span.end);
        Ok(Spanned::new(
            ConstDecl { name, value, kind: ConstKind::String, exported: true },
            span,
        ))
    }

    /// Parse `generated const <name> = "<value>"`.
    fn parse_generated_const(&mut self) -> Result<Spanned<ConstDecl>, ParseError> {
        let (_, kw_span) = self.expect_ident(Some("generated"))?;
        let (_, _) = self.expect_ident(Some("const"))?;
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
                    message: "expected string literal as `generated const` value".into(),
                });
            }
        };
        let end_span = self.tokens[self.pos - 1].span;
        let span = Span::new(kw_span.file_id, kw_span.start, end_span.end);
        Ok(Spanned::new(
            ConstDecl { name, value, kind: ConstKind::String, exported: false },
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
        // Also collect bare-name references for closure checking and word count.
        // Additionally capture description, effects, and flow strings for Tier 3 emission.
        let mut has_return = false;
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
            "const",
        ];
        loop {
            match self.current_line_indent() {
                Some(n) if n > 0 => {
                    // Drop the LineStart and every token until the next LineStart or Eof.
                    self.pos += 1;
                    // Check if line starts with a sub-section keyword or `return`.
                    if let TokenKind::Ident(kw) = &self.peek().kind {
                        match kw.as_str() {
                            "return" => has_return = true,
                            "description" => { current_section = Some("description"); }
                            "effects" => {
                                if !self.enable_effects {
                                    let eff_span = self.peek().span;
                                    self.bag.push(
                                        Diagnostic::error(
                                            "G::parse::effects-disabled",
                                            "effects are not enabled; pass `--enable-effects` to use this feature",
                                            SourceSpan::from_byte_span(self.file_label, eff_span, self.line_index),
                                        ),
                                        eff_span,
                                    );
                                }
                                current_section = Some("effects");
                            }
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
            name, params, has_return, body_refs, body_word_count,
            description, effects, flow_strings,
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
                                    if !self.enable_effects {
                                        let eff_span = self.peek().span;
                                        self.bag.push(
                                            Diagnostic::error(
                                                "G::parse::effects-disabled",
                                                "effects are not enabled; pass `--enable-effects` to use this feature",
                                                SourceSpan::from_byte_span(self.file_label, eff_span, self.line_index),
                                            ),
                                            eff_span,
                                        );
                                        // Skip the rest of the line.
                                        while !self.at_eof()
                                            && !matches!(self.peek().kind, TokenKind::LineStart { .. })
                                        {
                                            self.pos += 1;
                                        }
                                    } else {
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
                if !self.enable_effects {
                    let span = kw_span;
                    self.bag.push(
                        Diagnostic::error(
                            "G::parse::effects-disabled",
                            "effects are not enabled; pass `--enable-effects` to use this feature",
                            SourceSpan::from_byte_span(self.file_label, span, self.line_index),
                        ),
                        span,
                    );
                    // Skip the rest of the line.
                    while !self.at_eof()
                        && !matches!(self.peek().kind, TokenKind::LineStart { .. })
                    {
                        self.pos += 1;
                    }
                } else {
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

    fn parse_const_decl(&mut self) -> Result<Spanned<ConstDecl>, ParseError> {
        let (_, kw_span) = self.expect_ident(Some("const"))?;
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
                    message: "expected string literal as `const` value".into(),
                });
            }
        };
        let end_span = self.tokens[self.pos - 1].span;
        let span = Span::new(kw_span.file_id, kw_span.start, end_span.end);
        Ok(Spanned::new(
            ConstDecl { name, value, kind: ConstKind::String, exported: false },
            span,
        ))
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
