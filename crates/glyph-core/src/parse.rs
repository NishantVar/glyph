//! Phase 1 parser — hand-rolled recursive descent over the tokenizer's output.
//!
//! Walking-skeleton scope: parses exactly the constructs needed for
//! `update_docs.glyph` per `design/mvp-acceptance.md` §1.

use crate::ast::{
    BlockDecl, ConstDecl, ConstValue, ConstraintMarker, ConstraintMarkerKind, ContextEntry, Decl,
    DuplicateSubsection, ElifBranch, ExportBlockDecl, FlowStmt, ImportDecl, ImportKind, ImportName,
    Param, ReturnExpr, Skill, SourceFile,
};
use crate::diagnostic::{Classification, DiagBag, Diagnostic, SourceSpan};
use crate::output_target::{OutputTargetExpr, OutputTargetParseError};
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
        source,
        consumed_arrow_offsets: Vec::new(),
        consumed_output_target_offsets: Vec::new(),
        enable_effects: false,
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
                    message: "tab character used for indentation; Glyph requires 4-space indents"
                        .into(),
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

    // Run the parser. We intentionally hold its result before dropping the
    // borrow on `bag` so we can scan for stray `Arrow` tokens against
    // `consumed_arrow_offsets` below — and we want the post-parse scan to
    // run whether `parse_file` succeeded or failed (a generic `ParseError`
    // can leave a stray `->` in the stream that would otherwise be silently
    // dropped, regressing the pre-#82-chunk-2 `G::parse::operator-in-expression`
    // diagnostic that the byte-scan path used to emit on bare `-`).
    let (parsed_result, consumed_arrows, consumed_output_targets) = {
        let mut p = Parser {
            tokens: &tokens,
            pos: 0,
            file_id,
            file_label,
            line_index,
            bag,
            source,
            consumed_arrow_offsets: Vec::new(),
            consumed_output_target_offsets: Vec::new(),
            enable_effects,
        };
        let parsed = p.parse_file();
        (
            parsed,
            std::mem::take(&mut p.consumed_arrow_offsets),
            std::mem::take(&mut p.consumed_output_target_offsets),
        )
    };

    // Cascade-gate (issue #119). `parse_file` has no per-declaration
    // recovery: the first structural error returns out of the whole call,
    // leaving any tokens past the failure point unconsumed. The two
    // post-parse leftover-token sweeps below (`Arrow` and `LAngle`) attribute
    // those unreached tokens to the author, producing a screen of false
    // positives that hide the real structural error. Skip both sweeps
    // entirely when the parse failed; the structured error still surfaces
    // via the legacy `CompileError::Parse` path. Standard compiler UX: one
    // structural error at a time. After the author fixes the first, the
    // sweeps run again and surface the next problem.
    if parsed_result.is_ok() {
        // Post-parse Arrow scan. Any `Arrow` token whose start offset is NOT
        // in `consumed_arrows` is a stray `->` in an expression position
        // (`return x -> y`, `const a = b -> c`, `if x -> y`, etc.) — the
        // parser could not legitimately use it. Emit
        // `G::parse::operator-in-expression` Repairable per
        // `design/language-surface.md` §3 (the `->` arrow is only valid as a
        // return-type annotation on a declaration header) so callers see the
        // same structured diagnostic that fired pre-#82-chunk-2 when the
        // tokenizer flagged `-` as `UnexpectedChar`.
        for tok in tokens.iter() {
            if matches!(tok.kind, TokenKind::Arrow) && !consumed_arrows.contains(&tok.span.start) {
                let span = tok.span;
                bag.push(
                    Diagnostic {
                        id: "G::parse::operator-in-expression".into(),
                        classification: Classification::Repairable,
                        message:
                            "operator `->` is not supported in expressions; MVP Glyph has no value-level operators"
                                .into(),
                        span: SourceSpan::from_byte_span(file_label, span, line_index),
                        related: Vec::new(),
                        hints: vec![
                            "the `->` arrow is only valid as a return-type annotation on a declaration header (e.g. `block foo() -> Path`); rewrite or remove it here"
                                .into(),
                        ],
                    },
                    span,
                );
            }
        }

        // Post-parse output-target scan. Any `<` token that was not consumed
        // as part of a return-position output target candidate is outside
        // the only MVP-legal slot (`return <name>` as the terminal flow
        // statement).
        for tok in tokens.iter() {
            if !matches!(tok.kind, TokenKind::LAngle) {
                continue;
            }
            if consumed_output_targets.contains(&tok.span.start) {
                continue;
            }
            let span = tok.span;
            bag.push(
                Diagnostic::error(
                    "G::parse::output-target-outside-return",
                    "output targets are only allowed as the terminal `return <name>` expression",
                    SourceSpan::from_byte_span(file_label, span, line_index),
                ),
                span,
            );
        }
    }

    let file = match parsed_result {
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
            check_return_rules(
                &spanned_skill.node.flow,
                spanned_skill.span,
                file_label,
                line_index,
                bag,
                false,
            );
        }
        // Check return-related diagnostics for blocks.
        if let Decl::Block(spanned_block) = decl {
            check_return_rules(
                &spanned_block.node.flow,
                spanned_block.span,
                file_label,
                line_index,
                bag,
                false,
            );
        }
    }
    // Detect `G::parse::multiple-skills`: more than one `skill` per file.
    {
        let skill_count = file
            .decls
            .iter()
            .filter(|d| matches!(d, Decl::Skill(_)))
            .count();
        if skill_count > 1 {
            let span = file
                .decls
                .iter()
                .filter_map(|d| match d {
                    Decl::Skill(s) => Some(s.span),
                    _ => None,
                })
                .nth(1)
                .unwrap();
            bag.push(
                Diagnostic::error(
                    "G::parse::multiple-skills",
                    "a `.glyph` file may contain at most one `skill` declaration",
                    SourceSpan::from_byte_span(file_label, span, line_index),
                ),
                span,
            );
        }
    }

    // Issue #109 codex pass-3 finding 8: gate AST suppression on tier
    // alone — repairable-only bags flow through; any error suppresses.
    // The principle: any combination of repairables is itself repairable,
    // and downstream consumers (`glyph fmt`, agent repair loop) need the
    // AST to operate on. Pre-#109 the gate was tier-only too; the brief
    // window where it was scoped to a single ID was a defensive narrowing
    // for chunk 2 and is no longer needed once all repairable IDs have
    // been audited to confirm none of them produce a structurally-broken
    // AST that would crash later phases (audit done in pass-3 review).
    if bag.has_error() {
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
    /// Original source text. Needed by issue-#85 chunk 3 to slice the
    /// `<IDENT>` byte range covered by an `LAngle`/`RAngle` token pair and
    /// hand it to `output_target::parse_output_target` (which validates the
    /// inner identifier without re-tokenizing).
    source: &'a str,
    /// Byte-offset (`Span.start`) of every `Arrow` token the parser has
    /// legitimately consumed via `try_parse_return_type`. After parsing,
    /// `parse_with_diagnostics` scans the token stream for any `Arrow`
    /// whose offset is NOT in this set and emits the structured
    /// `G::parse::operator-in-expression` Repairable diagnostic — the
    /// post-#82-chunk-2 substitute for the previous byte-scan path that
    /// fired on stray `-` characters before the tokenizer learned the
    /// `Arrow` token.
    consumed_arrow_offsets: Vec<u32>,
    /// Byte-offset (`Span.start`) of every `<` token the parser consumed as a
    /// return-position output target candidate. The post-parse scan uses this
    /// to reject all other angle-bracket output-target forms with the
    /// structured `G::parse::output-target-outside-return` diagnostic.
    consumed_output_target_offsets: Vec<u32>,
    /// When `false`, parsing an `effects:` sub-section emits
    /// `G::parse::effects-disabled` and the section is skipped.
    enable_effects: bool,
}

/// Issue #109: commit any pending duplicate sub-section scratch from the
/// section we are leaving in `parse_export_block` into `extra_subsections`,
/// then reset the scratch state. Called on every section transition and once
/// more at end-of-body.
fn flush_dup_export_block(
    extras: &mut Vec<DuplicateSubsection>,
    current_dup_kind: &mut Option<&'static str>,
    dup_description: &mut Option<String>,
    dup_effects: &mut Vec<String>,
    dup_flow_strings: &mut Vec<String>,
) {
    match *current_dup_kind {
        Some("description") => {
            // `description:` always carries an inline string; if the
            // duplicate body parsed successfully, scratch is `Some`.
            if let Some(s) = dup_description.take() {
                extras.push(DuplicateSubsection::Description(s));
            }
        }
        Some("effects") => {
            extras.push(DuplicateSubsection::Effects(std::mem::take(dup_effects)));
        }
        Some("flow") => {
            // The export-block parser only captures inline-string flow
            // bodies (per `flow_strings` shape). Map them back into
            // `FlowStmt::InlineString` so the duplicate carries the same
            // shape `Flow(Vec<FlowStmt>)` expects.
            let stmts = std::mem::take(dup_flow_strings)
                .into_iter()
                .map(FlowStmt::InlineString)
                .collect();
            extras.push(DuplicateSubsection::Flow(stmts));
        }
        _ => {}
    }
    *current_dup_kind = None;
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

    /// Advance `self.pos` past consecutive `LineStart` tokens.
    ///
    /// Used by callers that delimit a construct with a brace pair and treat
    /// inner whitespace as non-significant. Today the only caller is the
    /// selective-import branch of `parse_import` (issue #117). Items inside
    /// such a construct remain atomic; the helper is called only between
    /// items, never inside one. Safe at EOF: `peek()` returns the EOF
    /// sentinel (not `LineStart`), so the loop terminates.
    fn skip_line_starts(&mut self) {
        while matches!(self.peek().kind, TokenKind::LineStart { .. }) {
            self.pos += 1;
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
                // The `Arrow` was consumed above but the next token is not
                // an `Ident` (e.g. `block foo() ->` with nothing after, or
                // `skill foo() -> "Path"` with a string literal). Bail with
                // `ParseError::Unexpected` and intentionally do NOT record
                // the Arrow in `consumed_arrow_offsets` — that way the
                // post-parse Arrow scan in `parse_with_diagnostics` still
                // surfaces the structured `G::parse::operator-in-expression`
                // (Repairable) diagnostic, restoring the pre-#82-chunk-2
                // diagnostic quality on incomplete header arrows.
                return Err(ParseError::Unexpected {
                    span: self.peek().span,
                    message: "expected return-type name after `->`".into(),
                });
            }
        };

        // Record this Arrow as legitimately consumed in a header
        // return-type slot so the post-parse scan in
        // `parse_with_diagnostics` does NOT flag it as a stray
        // expression-position `->`. Recorded only after the trailing
        // `Ident` is validated; an incomplete `->` (no ident, or non-ident
        // token) leaves the offset out so the scan emits
        // `G::parse::operator-in-expression`.
        self.consumed_arrow_offsets.push(arrow_span.start);

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

    fn emit_malformed_output_target(&mut self, form_span: Span, err: OutputTargetParseError) {
        let detail = match err {
            OutputTargetParseError::MissingOpenBracket => {
                "output target must start with `<`".to_string()
            }
            OutputTargetParseError::UnclosedBracket => {
                "output target is missing its closing `>`".to_string()
            }
            OutputTargetParseError::TrailingChars { .. } => {
                "output target must contain exactly one `<name>` form".to_string()
            }
            OutputTargetParseError::Empty => "output target identifier is empty".to_string(),
            OutputTargetParseError::InvalidIdentStart { ch, .. } if ch.is_whitespace() => {
                "output target identifiers must not contain whitespace".to_string()
            }
            OutputTargetParseError::InvalidIdentChar { ch, .. } if ch.is_whitespace() => {
                "output target identifiers must not contain whitespace".to_string()
            }
            OutputTargetParseError::InvalidIdentStart { ch, .. } => {
                format!(
                    "output target identifier must start with a letter or `_`, found `{}`",
                    ch
                )
            }
            OutputTargetParseError::InvalidIdentChar { ch, .. } => {
                format!(
                    "output target identifier may only contain letters, digits, or `_`, found `{}`",
                    ch
                )
            }
            OutputTargetParseError::EmptyDescription => {
                "descriptive output target must not be empty; write `return <\"description\">`"
                    .to_string()
            }
            OutputTargetParseError::UnterminatedDescription { .. } => {
                "descriptive output target is missing its closing `\"`; write `return <\"description\">`"
                    .to_string()
            }
        };
        self.bag.push(
            Diagnostic {
                id: "G::parse::malformed-output-target".into(),
                classification: Classification::Error,
                message: format!("{detail}; write `return <name>`"),
                span: SourceSpan::from_byte_span(self.file_label, form_span, self.line_index),
                related: Vec::new(),
                hints: vec![
                    "`return <name>` accepts only identifier-shaped names like `current_branch`"
                        .into(),
                ],
            },
            form_span,
        );
    }

    fn emit_output_target_outside_return(&mut self, span: Span) {
        self.bag.push(
            Diagnostic::error(
                "G::parse::output-target-outside-return",
                "output targets are only allowed as the terminal `return <name>` expression",
                SourceSpan::from_byte_span(self.file_label, span, self.line_index),
            ),
            span,
        );
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
                    // language-surface.md §3.6 line 342 / §3.7 line 375 (all
                    // `generated const` / `generated block` decls must appear
                    // after all non-generated top-level decls).
                    //
                    // Peek the token after `generated` to dispatch:
                    // `generated const` (§3.6) vs `generated block` (§3.7).
                    let saved = self.pos;
                    self.pos += 1; // skip `generated`
                    let next_kw = match &self.peek().kind {
                        TokenKind::Ident(s) => s.clone(),
                        _ => {
                            return Err(ParseError::Unexpected {
                                span: self.peek().span,
                                message: "expected `const` or `block` after `generated`".into(),
                            });
                        }
                    };
                    self.pos = saved; // restore
                    match next_kw.as_str() {
                        "const" => {
                            let d = self.parse_generated_const()?;
                            decls.push(Decl::Const(d));
                        }
                        "block" => {
                            let d = self.parse_generated_block()?;
                            decls.push(Decl::Block(d));
                        }
                        _ => {
                            return Err(ParseError::Unexpected {
                                span: self.peek().span,
                                message: format!(
                                    "expected `const` or `block` after `generated`, found `{}`",
                                    next_kw
                                ),
                            });
                        }
                    }
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
                                message: format!(
                                    "expected `block` or `const` after `export`, found `{}`",
                                    next_kw
                                ),
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
        let mut extra_subsections: Vec<DuplicateSubsection> = Vec::new();
        // Per-kind presence tracking for duplicate detection (issue #109).
        // Booleans (rather than `is_empty()`) so a *legitimately empty*
        // sub-section still counts as "seen" — second occurrence then
        // recovers to extras instead of merging.
        let mut context_section_present = false;
        let mut effects_present = false;
        let mut constraints_section_present = false;

        // Parse body lines at indent 1.
        loop {
            match self.current_line_indent() {
                Some(1) => {
                    self.parse_skill_body_line(
                        &mut description,
                        &mut body_constraints,
                        &mut body_context,
                        &mut context_section,
                        &mut context_section_present,
                        &mut effects,
                        &mut effects_present,
                        &mut flow,
                        &mut flow_present,
                        &mut constraints_section_present,
                        &mut body_bare_names,
                        &mut extra_subsections,
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
                constraints_section: Vec::new(),
                effects,
                flow,
                flow_present,
                body_bare_names,
                return_type,
                extra_subsections,
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
                // Selective import: `{ name1, name2 as alias2 }`.
                //
                // Whitespace inside `{ … }` is non-significant: line breaks and
                // indentation between import items are allowed; the brace pair is
                // the sole delimiter (`design/language-surface.md` §3.5). Items
                // (`name`, optional `as <alias>`) must stay on a single line —
                // `skip_line_starts` is intentionally NOT called inside an item.
                self.pos += 1; // consume `{`
                self.skip_line_starts();
                let mut names = Vec::new();
                if !matches!(self.peek().kind, TokenKind::Rbrace) {
                    loop {
                        let (name, name_span) = self.expect_ident(None)?;
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
                        names.push(ImportName {
                            name: Spanned::new(name, name_span),
                            alias,
                        });
                        match &self.peek().kind {
                            TokenKind::Comma => {
                                self.pos += 1;
                                self.skip_line_starts();
                                // Trailing comma before `}` (same- or different-line).
                                if matches!(self.peek().kind, TokenKind::Rbrace) {
                                    break;
                                }
                            }
                            _ => break,
                        }
                    }
                }
                self.skip_line_starts();
                // Replaces the prior `self.expect(&TokenKind::Rbrace)?` with a
                // peek-and-match that emits a clearer diagnostic when the user
                // forgets a separator (e.g. two names on adjacent lines, no comma).
                if matches!(self.peek().kind, TokenKind::Rbrace) {
                    self.pos += 1;
                } else {
                    return Err(ParseError::Unexpected {
                        span: self.peek().span,
                        message: "expected ',' or '}' after import name".into(),
                    });
                }
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
        // Issue #85 chunk 4b (D4): last-write-wins capture of the
        // structurally-parsed return expression. See
        // `ExportBlockDecl::terminal_return` for the language invariant.
        let mut terminal_return: Option<ReturnExpr> = None;
        let mut flow_item_count: usize = 0;
        let mut root_flow_output_targets: Vec<(usize, Span)> = Vec::new();
        // Track which sub-section we are currently in.
        let mut current_section: Option<&'static str> = None;
        // Issue #109: per-kind presence flags + scratch buffers for duplicate
        // sub-section bodies. When a duplicate header is encountered, body
        // tokens are routed to the scratch buffer and committed to
        // `extra_subsections` on section transition (or end-of-body) so that
        // `glyph fmt` can splice them back into the singletons later.
        let mut description_present = false;
        let mut effects_present = false;
        let mut flow_present = false;
        // Issue #109 codex pass-3 finding 9: `context:` and `constraints:`
        // are valid sub-sections on `export block` per
        // `design/language-surface.md` §2.5 ("Inside a `skill`, `block`, or
        // `export block` body…"). The export-block flat scanner does not
        // structurally parse their bodies into AST fields, but we still
        // need to detect duplicates and surface them via
        // `extra_subsections` with an empty body so `glyph fmt` can lift
        // the merge / dedup work back into the source-text stratum.
        let mut context_present = false;
        let mut constraints_present = false;
        let mut current_dup_kind: Option<&'static str> = None;
        let mut dup_description: Option<String> = None;
        let mut dup_effects: Vec<String> = Vec::new();
        let mut dup_flow_strings: Vec<String> = Vec::new();
        let mut extra_subsections: Vec<DuplicateSubsection> = Vec::new();
        let body_keywords: &[&str] = &[
            "flow",
            "return",
            "description",
            "effects",
            "constraints",
            "context",
            "require",
            "avoid",
            "must",
            "if",
            "elif",
            "else",
            "none",
            "with",
            "as",
            "import",
            "export",
            "block",
            "skill",
            "int",
            "float",
        ];
        loop {
            match self.current_line_indent() {
                Some(n) if n > 0 => {
                    let line_indent = n;
                    // Drop the LineStart and every token until the next LineStart or Eof.
                    self.pos += 1;
                    let mut line_is_section_header = false;
                    let mut output_target_return_span: Option<Span> = None;
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
                                    Some(TokenKind::Ident(s)) if s.eq_ignore_ascii_case("none") => {
                                        false
                                    }
                                    _ => true,
                                };
                                if is_meaningful {
                                    has_meaningful_return = true;
                                }
                                // Issue #85 chunk 4b (D4): structurally
                                // parse the return expression for
                                // `terminal_return`. Save pos so the body-
                                // walking loop below still observes the
                                // expression tokens for body_refs /
                                // body_word_count accumulation. Last-write-
                                // wins: a flow with multiple `return`s
                                // (illegal but tolerated upstream) keeps
                                // the most recent one.
                                let saved_for_body_walk = self.pos;
                                self.pos += 1; // consume `return`
                                let expr = self.parse_return_expr()?;
                                if let ReturnExpr::OutputTarget(OutputTargetExpr::Identifier(id)) =
                                    &expr
                                {
                                    output_target_return_span = Some(id.span);
                                }
                                terminal_return = Some(expr);
                                self.pos = saved_for_body_walk;
                            }
                            "description" => {
                                line_is_section_header = true;
                                let kw_tok_span = self.peek().span;
                                // Flush any pending duplicate scratch from
                                // the section we are leaving.
                                flush_dup_export_block(
                                    &mut extra_subsections,
                                    &mut current_dup_kind,
                                    &mut dup_description,
                                    &mut dup_effects,
                                    &mut dup_flow_strings,
                                );
                                if description_present {
                                    // Duplicate `description:` — route body
                                    // captures into scratch.
                                    current_dup_kind = Some("description");
                                    self.bag.push(
                                        Diagnostic {
                                            id: "G::parse::duplicate-subsection".into(),
                                            classification: Classification::Repairable,
                                            message: "duplicate `description:` sub-section in export block body".into(),
                                            span: SourceSpan::from_byte_span(
                                                self.file_label,
                                                kw_tok_span,
                                                self.line_index,
                                            ),
                                            related: Vec::new(),
                                            hints: vec![
                                                "remove the duplicate or merge contents into one `description:`".into(),
                                            ],
                                        },
                                        kw_tok_span,
                                    );
                                } else {
                                    description_present = true;
                                }
                                current_section = Some("description");
                            }
                            "effects" => {
                                line_is_section_header = true;
                                let kw_tok_span = self.peek().span;
                                flush_dup_export_block(
                                    &mut extra_subsections,
                                    &mut current_dup_kind,
                                    &mut dup_description,
                                    &mut dup_effects,
                                    &mut dup_flow_strings,
                                );
                                if effects_present {
                                    current_dup_kind = Some("effects");
                                    self.bag.push(
                                        Diagnostic {
                                            id: "G::parse::duplicate-subsection".into(),
                                            classification: Classification::Repairable,
                                            message: "duplicate `effects:` sub-section in export block body".into(),
                                            span: SourceSpan::from_byte_span(
                                                self.file_label,
                                                kw_tok_span,
                                                self.line_index,
                                            ),
                                            related: Vec::new(),
                                            hints: vec![
                                                "remove the duplicate or merge contents into one `effects:`".into(),
                                            ],
                                        },
                                        kw_tok_span,
                                    );
                                } else {
                                    effects_present = true;
                                }
                                current_section = Some("effects");
                            }
                            "flow" => {
                                line_is_section_header = true;
                                let kw_tok_span = self.peek().span;
                                flush_dup_export_block(
                                    &mut extra_subsections,
                                    &mut current_dup_kind,
                                    &mut dup_description,
                                    &mut dup_effects,
                                    &mut dup_flow_strings,
                                );
                                if flow_present {
                                    current_dup_kind = Some("flow");
                                    self.bag.push(
                                        Diagnostic {
                                            id: "G::parse::duplicate-subsection".into(),
                                            classification: Classification::Repairable,
                                            message: "duplicate `flow:` sub-section in export block body".into(),
                                            span: SourceSpan::from_byte_span(
                                                self.file_label,
                                                kw_tok_span,
                                                self.line_index,
                                            ),
                                            related: Vec::new(),
                                            hints: vec![
                                                "remove the duplicate or merge contents into one `flow:`".into(),
                                            ],
                                        },
                                        kw_tok_span,
                                    );
                                } else {
                                    flow_present = true;
                                }
                                current_section = Some("flow");
                            }
                            "context" => {
                                line_is_section_header = true;
                                let kw_tok_span = self.peek().span;
                                flush_dup_export_block(
                                    &mut extra_subsections,
                                    &mut current_dup_kind,
                                    &mut dup_description,
                                    &mut dup_effects,
                                    &mut dup_flow_strings,
                                );
                                if context_present {
                                    // Issue #109 codex pass-3 finding 9:
                                    // duplicate `context:` is repairable.
                                    // The flat scanner doesn't structurally
                                    // capture context entries, so push an
                                    // empty `Vec<ContextEntry>` — fmt's
                                    // source-text stratum is responsible
                                    // for the actual merge.
                                    self.bag.push(
                                        Diagnostic {
                                            id: "G::parse::duplicate-subsection".into(),
                                            classification: Classification::Repairable,
                                            message: "duplicate `context:` sub-section in export block body".into(),
                                            span: SourceSpan::from_byte_span(
                                                self.file_label,
                                                kw_tok_span,
                                                self.line_index,
                                            ),
                                            related: Vec::new(),
                                            hints: vec![
                                                "remove the duplicate or merge contents into one `context:`".into(),
                                            ],
                                        },
                                        kw_tok_span,
                                    );
                                    extra_subsections
                                        .push(DuplicateSubsection::Context(Vec::new()));
                                } else {
                                    context_present = true;
                                }
                                current_section = Some("other");
                            }
                            "constraints" => {
                                line_is_section_header = true;
                                let kw_tok_span = self.peek().span;
                                flush_dup_export_block(
                                    &mut extra_subsections,
                                    &mut current_dup_kind,
                                    &mut dup_description,
                                    &mut dup_effects,
                                    &mut dup_flow_strings,
                                );
                                if constraints_present {
                                    // Issue #109 codex pass-3 finding 9:
                                    // duplicate `constraints:` is repairable.
                                    self.bag.push(
                                        Diagnostic {
                                            id: "G::parse::duplicate-subsection".into(),
                                            classification: Classification::Repairable,
                                            message: "duplicate `constraints:` sub-section in export block body".into(),
                                            span: SourceSpan::from_byte_span(
                                                self.file_label,
                                                kw_tok_span,
                                                self.line_index,
                                            ),
                                            related: Vec::new(),
                                            hints: vec![
                                                "remove the duplicate or merge contents into one `constraints:`".into(),
                                            ],
                                        },
                                        kw_tok_span,
                                    );
                                    extra_subsections
                                        .push(DuplicateSubsection::Constraints(Vec::new()));
                                } else {
                                    constraints_present = true;
                                }
                                current_section = Some("other");
                            }
                            _ => {}
                        }
                    }
                    if current_section == Some("flow") && !line_is_section_header {
                        let item_index = flow_item_count;
                        flow_item_count += 1;
                        if let Some(span) = output_target_return_span {
                            if line_indent == 2 {
                                root_flow_output_targets.push((item_index, span));
                            } else {
                                self.emit_output_target_outside_return(span);
                            }
                        }
                    } else if let Some(span) = output_target_return_span {
                        self.emit_output_target_outside_return(span);
                    }
                    while !self.at_eof() && !matches!(self.peek().kind, TokenKind::LineStart { .. })
                    {
                        match &self.peek().kind {
                            TokenKind::Ident(ident) => {
                                if !body_keywords.contains(&ident.as_str()) {
                                    body_refs.push(ident.clone());
                                    // Capture effect names
                                    if current_section == Some("effects") {
                                        if current_dup_kind == Some("effects") {
                                            dup_effects.push(ident.clone());
                                        } else {
                                            effects.push(ident.clone());
                                        }
                                    }
                                }
                                body_word_count += 1;
                            }
                            TokenKind::StringLit(s) => {
                                body_word_count += s.split_whitespace().count();
                                // Capture description and flow strings
                                match current_section {
                                    Some("description") => {
                                        if current_dup_kind == Some("description") {
                                            dup_description = Some(s.clone());
                                        } else {
                                            description = Some(s.clone());
                                        }
                                    }
                                    Some("flow") => {
                                        if current_dup_kind == Some("flow") {
                                            dup_flow_strings.push(s.clone());
                                        } else {
                                            flow_strings.push(s.clone());
                                        }
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

        for (item_index, span) in root_flow_output_targets {
            if item_index + 1 != flow_item_count {
                self.emit_output_target_outside_return(span);
            }
        }

        // Final flush: commit any pending duplicate scratch left over from
        // the last duplicate sub-section in the body.
        flush_dup_export_block(
            &mut extra_subsections,
            &mut current_dup_kind,
            &mut dup_description,
            &mut dup_effects,
            &mut dup_flow_strings,
        );

        let end_span = if self.pos > 0 {
            self.tokens[self.pos - 1].span
        } else {
            kw_span
        };
        let span = Span::new(kw_span.file_id, kw_span.start, end_span.end);
        Ok(Spanned::new(
            ExportBlockDecl {
                name,
                params,
                has_return,
                has_meaningful_return,
                body_refs,
                body_word_count,
                description,
                effects,
                flow_strings,
                return_type,
                terminal_return,
                extra_subsections,
            },
            span,
        ))
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
        let mut effects_present = false;
        let mut flow: Vec<FlowStmt> = Vec::new();
        let mut flow_present = false;
        // Issue #109 codex pass-3 finding 9: `BlockDecl` has no canonical
        // `context_section` / `body_constraints` AST fields, so the first
        // occurrence of `context:` / `constraints:` is silently consumed
        // (the body parses but its content is discarded). Subsequent
        // occurrences emit `G::parse::duplicate-subsection` (Repairable)
        // and route the body intact into `extra_subsections` so `glyph
        // fmt` can splice them back later.
        let mut context_present = false;
        let mut constraints_present = false;
        // Issue #109: track duplicate sub-section bodies so `glyph fmt` can
        // splice them back into the canonical singletons instead of dropping
        // them silently.
        let mut extra_subsections: Vec<DuplicateSubsection> = Vec::new();

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
                            let kw_tok_span = self.peek().span;
                            match kw.as_str() {
                                "description" => {
                                    self.pos += 1;
                                    self.expect(&TokenKind::Colon)?;
                                    let s = self.consume_string_after_colon()?;
                                    if description.is_some() {
                                        // Issue #109: duplicate `description:` on a block.
                                        let span = kw_tok_span;
                                        self.bag.push(
                                            Diagnostic {
                                                id: "G::parse::duplicate-subsection".into(),
                                                classification: Classification::Repairable,
                                                message: "duplicate `description:` sub-section in block body".into(),
                                                span: SourceSpan::from_byte_span(
                                                    self.file_label,
                                                    span,
                                                    self.line_index,
                                                ),
                                                related: Vec::new(),
                                                hints: vec![
                                                    "remove the duplicate or merge contents into one `description:`".into(),
                                                ],
                                            },
                                            span,
                                        );
                                        extra_subsections
                                            .push(DuplicateSubsection::Description(s));
                                    } else {
                                        description = Some(s);
                                    }
                                }
                                "effects" => {
                                    if !self.enable_effects {
                                        let eff_span = kw_tok_span;
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
                                        // Gather into a local so duplicate `effects:`
                                        // can be recovered intact (issue #109).
                                        let mut local_effects: Vec<String> = Vec::new();
                                        loop {
                                            let (eff, _) = self.expect_ident(None)?;
                                            local_effects.push(eff);
                                            match &self.peek().kind {
                                                TokenKind::Comma => {
                                                    self.pos += 1;
                                                }
                                                _ => break,
                                            }
                                        }
                                        // Validate `none` exclusivity for blocks too.
                                        if local_effects.contains(&"none".to_string())
                                            && local_effects.len() > 1
                                        {
                                            let span = Span::new(
                                                self.file_id,
                                                colon_span.start,
                                                colon_span.end,
                                            );
                                            self.bag.push(
                                                Diagnostic::error(
                                                    "G::parse::none-with-effects",
                                                    "`effects: none` must not appear alongside other effect keywords",
                                                    SourceSpan::from_byte_span(self.file_label, span, self.line_index),
                                                ),
                                                span,
                                            );
                                        }
                                        if effects_present {
                                            let span = kw_tok_span;
                                            self.bag.push(
                                                Diagnostic {
                                                    id: "G::parse::duplicate-subsection".into(),
                                                    classification: Classification::Repairable,
                                                    message: "duplicate `effects:` sub-section in block body".into(),
                                                    span: SourceSpan::from_byte_span(
                                                        self.file_label,
                                                        span,
                                                        self.line_index,
                                                    ),
                                                    related: Vec::new(),
                                                    hints: vec![
                                                        "remove the duplicate or merge contents into one `effects:`".into(),
                                                    ],
                                                },
                                                span,
                                            );
                                            extra_subsections
                                                .push(DuplicateSubsection::Effects(local_effects));
                                        } else {
                                            effects_present = true;
                                            effects.extend(local_effects);
                                        }
                                    }
                                }
                                "flow" => {
                                    self.pos += 1;
                                    self.expect(&TokenKind::Colon)?;
                                    // Gather body into a local so a duplicate
                                    // `flow:` can be recovered intact (issue #109).
                                    let mut local_flow: Vec<FlowStmt> = Vec::new();
                                    // Body at indent 2.
                                    loop {
                                        match self.current_line_indent() {
                                            Some(2) => {
                                                self.expect_line_start()?;
                                                let stmt = self.parse_flow_stmt(2)?;
                                                local_flow.push(stmt);
                                            }
                                            _ => break,
                                        }
                                    }
                                    if flow_present {
                                        let span = kw_tok_span;
                                        self.bag.push(
                                            Diagnostic {
                                                id: "G::parse::duplicate-subsection".into(),
                                                classification: Classification::Repairable,
                                                message: "duplicate `flow:` sub-section in block body".into(),
                                                span: SourceSpan::from_byte_span(
                                                    self.file_label,
                                                    span,
                                                    self.line_index,
                                                ),
                                                related: Vec::new(),
                                                hints: vec![
                                                    "remove the duplicate or merge contents into one `flow:`".into(),
                                                ],
                                            },
                                            span,
                                        );
                                        extra_subsections.push(DuplicateSubsection::Flow(
                                            local_flow,
                                        ));
                                    } else {
                                        flow_present = true;
                                        flow.extend(local_flow);
                                    }
                                }
                                "context" => {
                                    // Issue #109 codex pass-3 finding 9:
                                    // `context:` is a valid sub-section on
                                    // `block` per `design/language-surface.md`
                                    // §2.5. `BlockDecl` has no canonical
                                    // field for it, so the first occurrence
                                    // is silently absorbed; subsequent
                                    // occurrences land in `extra_subsections`.
                                    self.pos += 1;
                                    self.expect(&TokenKind::Colon)?;
                                    let mut local_entries: Vec<ContextEntry> = Vec::new();
                                    // Short form: `context: "inline"` on the same line.
                                    if let TokenKind::StringLit(s) = &self.peek().kind {
                                        let lit_span = self.peek().span;
                                        let v = s.clone();
                                        for slot in scan_slots(&v) {
                                            let span_start =
                                                lit_span.start + 1 + slot.start_in_content as u32;
                                            let span =
                                                Span::new(self.file_id, span_start, span_start + 1);
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
                                        local_entries.push(ContextEntry::InlineString(v));
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
                                                            let span_start = lit_span.start
                                                                + 1
                                                                + slot.start_in_content as u32;
                                                            let span = Span::new(
                                                                self.file_id,
                                                                span_start,
                                                                span_start + 1,
                                                            );
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
                                                        local_entries
                                                            .push(ContextEntry::InlineString(v));
                                                    }
                                                    TokenKind::Ident(name) => {
                                                        let v = name.clone();
                                                        let name_span = self.peek().span;
                                                        self.pos += 1;
                                                        local_entries.push(ContextEntry::NameRef(
                                                            Spanned::new(v, name_span),
                                                        ));
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
                                    if context_present {
                                        let span = kw_tok_span;
                                        self.bag.push(
                                            Diagnostic {
                                                id: "G::parse::duplicate-subsection".into(),
                                                classification: Classification::Repairable,
                                                message: "duplicate `context:` sub-section in block body".into(),
                                                span: SourceSpan::from_byte_span(
                                                    self.file_label,
                                                    span,
                                                    self.line_index,
                                                ),
                                                related: Vec::new(),
                                                hints: vec![
                                                    "remove the duplicate or merge contents into one `context:`".into(),
                                                ],
                                            },
                                            span,
                                        );
                                        extra_subsections
                                            .push(DuplicateSubsection::Context(local_entries));
                                    } else {
                                        context_present = true;
                                        // First occurrence: silently
                                        // discard. `BlockDecl` has no
                                        // canonical context_section field.
                                    }
                                }
                                "constraints" => {
                                    // Issue #109 codex pass-3 finding 9.
                                    self.pos += 1;
                                    self.expect(&TokenKind::Colon)?;
                                    let mut local_markers: Vec<ConstraintMarker> = Vec::new();
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
                                                                if let TokenKind::Ident(next) =
                                                                    &self.peek().kind
                                                                {
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
                                                        let (name, name_span) =
                                                            self.expect_ident(None)?;
                                                        local_markers.push(ConstraintMarker {
                                                            marker: kind,
                                                            name: Spanned::new(name, name_span),
                                                        });
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
                                    if constraints_present {
                                        let span = kw_tok_span;
                                        self.bag.push(
                                            Diagnostic {
                                                id: "G::parse::duplicate-subsection".into(),
                                                classification: Classification::Repairable,
                                                message: "duplicate `constraints:` sub-section in block body".into(),
                                                span: SourceSpan::from_byte_span(
                                                    self.file_label,
                                                    span,
                                                    self.line_index,
                                                ),
                                                related: Vec::new(),
                                                hints: vec![
                                                    "remove the duplicate or merge contents into one `constraints:`".into(),
                                                ],
                                            },
                                            span,
                                        );
                                        extra_subsections
                                            .push(DuplicateSubsection::Constraints(local_markers));
                                    } else {
                                        constraints_present = true;
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
                generated: false,
                extra_subsections,
            },
            span,
        ))
    }

    /// Parse a (possibly empty) comma-separated parameter list between the
    /// opening and closing parens of a header. Slice 4 supports the bare
    /// `name`, optional `name: Type` annotation (issue #119 / Phase 0), and
    /// optional `= "literal"` string default. The annotation reserves the
    /// syntactic position only — no resolution, no validation, no
    /// module-qualified or generic forms (per `design/types.md`).
    fn parse_param_list(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params: Vec<Param> = Vec::new();
        // Empty list?
        if matches!(self.peek().kind, TokenKind::Rparen) {
            return Ok(params);
        }
        loop {
            let (pname, name_span) = self.expect_ident(None)?;
            let mut type_annotation: Option<Spanned<String>> = None;
            let mut default: Option<String> = None;
            let mut end_span = name_span;
            // Optional `: Type` annotation — issue #119. Bare ident only;
            // any malformed shape (`x:`, `x: 123`) reuses the generic
            // `expect_ident` error per the PRD.
            if matches!(self.peek().kind, TokenKind::Colon) {
                self.pos += 1;
                let (tname, tspan) = self.expect_ident(None)?;
                type_annotation = Some(Spanned::new(tname, tspan));
                end_span = tspan;
            }
            let mut description: Option<Spanned<String>> = None;
            if matches!(self.peek().kind, TokenKind::Equals) {
                self.pos += 1;
                // Slice 4: only string-literal defaults are supported.
                // Phase A.2 (issue #119): the `=` slot now also accepts a
                // descriptive form `<"…">` either standalone or trailing a
                // string-literal default.
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
                            let span = Span::new(self.file_id, span_start, span_start + 1);
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

                        // Combo form: `name = "default" <"description">`.
                        // Adjacency is unambiguous because `<` is not legal
                        // anywhere else in param-default position.
                        if matches!(self.peek().kind, TokenKind::LAngle) {
                            let d = self.parse_param_description()?;
                            end_span = d.span;
                            description = Some(d);
                        }
                    }
                    TokenKind::LAngle => {
                        // Standalone descriptive form: `name = <"description">`.
                        let d = self.parse_param_description()?;
                        end_span = d.span;
                        description = Some(d);
                    }
                    _ => {
                        return Err(ParseError::Unexpected {
                            span: self.peek().span,
                            message:
                                "parameter default must be a string literal or `<\"…\">` description"
                                    .into(),
                        });
                    }
                }
            }
            let span = Span::new(self.file_id, name_span.start, end_span.end);
            params.push(Param {
                name: pname,
                default,
                type_annotation,
                description,
                span,
            });
            match &self.peek().kind {
                TokenKind::Comma => {
                    self.pos += 1;
                }
                _ => break,
            }
        }
        Ok(params)
    }

    /// Parse a `<"…">` (or `<"""…""">`) descriptive form in param-default position.
    /// Consumes `LAngle StringLit RAngle` from the token stream and returns the
    /// description content with a span covering the full form (brackets included).
    ///
    /// The `<` byte offset is registered in `consumed_output_target_offsets` so the
    /// post-parse `<` sweep does not double-fire on a `<` that is already part of a
    /// valid param description.
    ///
    /// Block-string content (`<"""…""">`) is delivered by the tokenizer as a single
    /// `StringLit` with dedent already applied (see `tokenize.rs::scan_triple_string`),
    /// so this helper does not distinguish inline vs block form.
    fn parse_param_description(&mut self) -> Result<Spanned<String>, ParseError> {
        let langle_span = self.peek().span;
        self.consumed_output_target_offsets.push(langle_span.start);
        self.pos += 1;

        let content = match &self.peek().kind {
            TokenKind::StringLit(s) => s.clone(),
            _ => {
                return Err(ParseError::Unexpected {
                    span: self.peek().span,
                    message: "expected a quoted string inside `<…>` param description".into(),
                });
            }
        };
        self.pos += 1;

        if !matches!(self.peek().kind, TokenKind::RAngle) {
            return Err(ParseError::Unexpected {
                span: self.peek().span,
                message: "expected `>` to close param description".into(),
            });
        }
        let end_span = self.peek().span;
        self.pos += 1;

        let span = Span::new(self.file_id, langle_span.start, end_span.end);
        Ok(Spanned::new(content, span))
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_skill_body_line(
        &mut self,
        description: &mut Option<String>,
        body_constraints: &mut Vec<ConstraintMarker>,
        body_context: &mut Vec<ContextEntry>,
        context_section: &mut Vec<ContextEntry>,
        context_section_present: &mut bool,
        effects: &mut Vec<String>,
        effects_present: &mut bool,
        flow: &mut Vec<FlowStmt>,
        flow_present: &mut bool,
        constraints_section_present: &mut bool,
        body_bare_names: &mut Vec<String>,
        extra_subsections: &mut Vec<DuplicateSubsection>,
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
                            span: SourceSpan::from_byte_span(
                                self.file_label,
                                span,
                                self.line_index,
                            ),
                            related: Vec::new(),
                            hints: vec![
                                "remove the duplicate or merge contents into one `description:`"
                                    .into(),
                            ],
                        },
                        span,
                    );
                    // Issue #109: keep the duplicate body for `glyph fmt` to
                    // splice back into the singleton later, instead of
                    // silently dropping it.
                    extra_subsections.push(DuplicateSubsection::Description(s));
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
                    // Gather the body's keywords into a local so a duplicate
                    // `effects:` can be recovered into `extra_subsections`
                    // intact (issue #109) without polluting `effects`.
                    let mut local_effects: Vec<String> = Vec::new();
                    // Short form only — comma-separated idents on the same line.
                    loop {
                        let (eff, _) = self.expect_ident(None)?;
                        local_effects.push(eff);
                        match &self.peek().kind {
                            TokenKind::Comma => {
                                self.pos += 1;
                            }
                            _ => break,
                        }
                    }
                    // Validate `none` exclusivity: `none` must not appear alongside
                    // other effect keywords → G::parse::none-with-effects (error).
                    // Run on the just-parsed body (matches pre-#109 behavior
                    // when this was the singleton body).
                    if local_effects.contains(&"none".to_string()) && local_effects.len() > 1 {
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
                    if *effects_present {
                        // Issue #109: duplicate `effects:`. Capture intact.
                        let span = kw_span;
                        self.bag.push(
                            Diagnostic {
                                id: "G::parse::duplicate-subsection".into(),
                                classification: Classification::Repairable,
                                message: "duplicate `effects:` sub-section in skill body".into(),
                                span: SourceSpan::from_byte_span(
                                    self.file_label,
                                    span,
                                    self.line_index,
                                ),
                                related: Vec::new(),
                                hints: vec![
                                    "remove the duplicate or merge contents into one `effects:`"
                                        .into(),
                                ],
                            },
                            span,
                        );
                        extra_subsections.push(DuplicateSubsection::Effects(local_effects));
                    } else {
                        *effects_present = true;
                        effects.extend(local_effects);
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
                let (name, name_span) = self.expect_ident(None)?;
                body_constraints.push(ConstraintMarker { marker: kind, name: Spanned::new(name, name_span) });
            }
            "context" => {
                self.pos += 1;
                // Two forms: `context:` (sub-section) or `context <name>` (body-level marker).
                if matches!(self.peek().kind, TokenKind::Colon) {
                    self.pos += 1;
                    // Gather the body's entries into a local so a duplicate
                    // `context:` can be recovered into `extra_subsections`
                    // intact (issue #109) without polluting `context_section`.
                    let mut local_entries: Vec<ContextEntry> = Vec::new();
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
                            local_entries.push(ContextEntry::InlineString(v));
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
                                            let span_start =
                                                lit_span.start + 1 + slot.start_in_content as u32;
                                            let span =
                                                Span::new(self.file_id, span_start, span_start + 1);
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
                                        local_entries.push(ContextEntry::InlineString(v));
                                    }
                                    TokenKind::Ident(name) => {
                                        let v = name.clone();
                                        let name_span = self.peek().span;
                                        self.pos += 1;
                                        local_entries.push(ContextEntry::NameRef(Spanned::new(v, name_span)));
                                    }
                                    _ => {
                                        return Err(ParseError::Unexpected {
                                            span: self.peek().span,
                                            message:
                                                "expected string literal or name in `context:` body"
                                                    .into(),
                                        });
                                    }
                                }
                            }
                            _ => break,
                        }
                    }
                    if *context_section_present {
                        // Issue #109: duplicate `context:`. Capture intact.
                        let span = kw_span;
                        self.bag.push(
                            Diagnostic {
                                id: "G::parse::duplicate-subsection".into(),
                                classification: Classification::Repairable,
                                message: "duplicate `context:` sub-section in skill body".into(),
                                span: SourceSpan::from_byte_span(
                                    self.file_label,
                                    span,
                                    self.line_index,
                                ),
                                related: Vec::new(),
                                hints: vec![
                                    "remove the duplicate or merge contents into one `context:`"
                                        .into(),
                                ],
                            },
                            span,
                        );
                        extra_subsections.push(DuplicateSubsection::Context(local_entries));
                    } else {
                        *context_section_present = true;
                        context_section.extend(local_entries);
                    }
                } else {
                    // Body-level `context <name>` or `context "string"` marker.
                    match &self.peek().kind {
                        TokenKind::Ident(name) => {
                            let v = name.clone();
                            let name_span = self.peek().span;
                            self.pos += 1;
                            body_context.push(ContextEntry::NameRef(Spanned::new(v, name_span)));
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
                // Gather the body's markers into a local so a duplicate
                // sub-section can be recovered into `extra_subsections`
                // intact (issue #109) without polluting `body_constraints`.
                let mut local_markers: Vec<ConstraintMarker> = Vec::new();
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
                                    let (name, name_span) = self.expect_ident(None)?;
                                    local_markers.push(ConstraintMarker {
                                        marker: kind,
                                        name: Spanned::new(name, name_span),
                                    });
                                }
                                _ => {
                                    return Err(ParseError::Unexpected {
                                        span: self.peek().span,
                                        message:
                                            "expected constraint marker in `constraints:` body"
                                                .into(),
                                    });
                                }
                            }
                        }
                        _ => break,
                    }
                }
                if *constraints_section_present {
                    // Issue #109: duplicate `constraints:`. Capture the body
                    // intact into extras and emit
                    // `G::parse::duplicate-subsection` (Repairable) on the
                    // duplicate header.
                    let span = kw_span;
                    self.bag.push(
                        Diagnostic {
                            id: "G::parse::duplicate-subsection".into(),
                            classification: Classification::Repairable,
                            message: "duplicate `constraints:` sub-section in skill body".into(),
                            span: SourceSpan::from_byte_span(
                                self.file_label,
                                span,
                                self.line_index,
                            ),
                            related: Vec::new(),
                            hints: vec![
                                "remove the duplicate or merge contents into one `constraints:`"
                                    .into(),
                            ],
                        },
                        span,
                    );
                    extra_subsections.push(DuplicateSubsection::Constraints(local_markers));
                } else {
                    *constraints_section_present = true;
                    body_constraints.extend(local_markers);
                }
            }
            "flow" => {
                self.pos += 1;
                self.expect(&TokenKind::Colon)?;
                let was_present = *flow_present;
                *flow_present = true;
                // Gather the body's statements into a local so a duplicate
                // `flow:` can be recovered into `extra_subsections` intact
                // (issue #109) without polluting `flow`.
                let mut local_flow: Vec<FlowStmt> = Vec::new();
                loop {
                    match self.current_line_indent() {
                        Some(2) => {
                            self.expect_line_start()?;
                            let stmt = self.parse_flow_stmt(2)?;
                            local_flow.push(stmt);
                        }
                        _ => break,
                    }
                }
                if was_present {
                    let span = kw_span;
                    self.bag.push(
                        Diagnostic {
                            id: "G::parse::duplicate-subsection".into(),
                            classification: Classification::Repairable,
                            message: "duplicate `flow:` sub-section in skill body".into(),
                            span: SourceSpan::from_byte_span(
                                self.file_label,
                                span,
                                self.line_index,
                            ),
                            related: Vec::new(),
                            hints: vec![
                                "remove the duplicate or merge contents into one `flow:`".into(),
                            ],
                        },
                        span,
                    );
                    extra_subsections.push(DuplicateSubsection::Flow(local_flow));
                } else {
                    flow.extend(local_flow);
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

    /// Parse a return expression. Caller must have consumed the `return`
    /// keyword; this method consumes the expression tokens and returns the
    /// parsed `ReturnExpr`.
    ///
    /// Used by:
    ///   - the canonical `parse_flow_stmt` `"return"` arm (skill / private
    ///     block flows);
    ///   - the `parse_export_block` flat scanner (issue #85 chunk 4b),
    ///     which save-then-parse-then-restore-pos's so the body-walking
    ///     loop still observes the same expression tokens for
    ///     `body_refs` / `body_word_count` accumulation.
    fn parse_return_expr(&mut self) -> Result<ReturnExpr, ParseError> {
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
                let name_span = self.peek().span;
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
                                TokenKind::Comma => {
                                    self.pos += 1;
                                }
                                _ => break,
                            }
                        }
                    }
                    self.expect(&TokenKind::Rparen)?;
                    ReturnExpr::Call { target: Spanned::new(name, name_span), args }
                } else {
                    ReturnExpr::Name(Spanned::new(name, name_span))
                }
            }
            TokenKind::StringLit(s) => {
                let s = s.clone();
                self.pos += 1;
                ReturnExpr::Inline(s)
            }
            TokenKind::LAngle => {
                // Issue #85: output-target identifier form
                // `return <IDENT>`. Hand the byte slice from `<` through the
                // end of the logical line to the chunk-1 deep parser so
                // trailing text like `return <foo>bar` is diagnosed as a
                // malformed output target instead of becoming an opaque parse
                // failure after the valid-looking `<foo>` prefix.
                let langle_span = self.peek().span;
                self.consumed_output_target_offsets.push(langle_span.start);
                self.pos += 1;
                // Scan to the matching `RAngle` on the same logical line.
                // Stop on `LineStart` or `Eof` (unclosed form).
                let mut rangle_end: Option<u32> = None;
                let mut candidate_end = langle_span.end;
                while !matches!(
                    self.peek().kind,
                    TokenKind::LineStart { .. } | TokenKind::Eof
                ) {
                    candidate_end = self.peek().span.end;
                    if matches!(self.peek().kind, TokenKind::RAngle) {
                        rangle_end = Some(self.peek().span.end);
                        self.pos += 1;
                        while !matches!(
                            self.peek().kind,
                            TokenKind::LineStart { .. } | TokenKind::Eof
                        ) {
                            candidate_end = self.peek().span.end;
                            self.pos += 1;
                        }
                        break;
                    }
                    self.pos += 1;
                }
                match rangle_end {
                    Some(e) => e,
                    None => {
                        self.emit_malformed_output_target(
                            langle_span,
                            OutputTargetParseError::UnclosedBracket,
                        );
                        return Err(ParseError::Unexpected {
                            span: langle_span,
                            message: "unclosed `<` in `return <IDENT>` output-target form".into(),
                        });
                    }
                };
                let form_span = Span::new(self.file_id, langle_span.start, candidate_end);
                let slice = &self.source[langle_span.start as usize..candidate_end as usize];
                match crate::output_target::parse_output_target(slice, form_span) {
                    Ok(expr) => ReturnExpr::OutputTarget(expr),
                    Err(e) => {
                        self.emit_malformed_output_target(form_span, e);
                        return Err(ParseError::Unexpected {
                            span: form_span,
                            message: "malformed `<IDENT>` output-target form after `return`".into(),
                        });
                    }
                }
            }
            _ => {
                return Err(ParseError::Unexpected {
                    span: self.peek().span,
                    message: "expected identifier, call, string, or `none` after `return`".into(),
                });
            }
        };
        Ok(expr)
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
                let kw_val_span = self.peek().span;
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
                        let (name, name_span) = self.expect_ident(None)?;
                        Ok(FlowStmt::ConstraintMarker(ConstraintMarker {
                            marker: kind,
                            name: Spanned::new(name, name_span),
                        }))
                    }
                    "return" => {
                        self.pos += 1;
                        let expr = self.parse_return_expr()?;
                        Ok(FlowStmt::Return(expr))
                    }
                    "context" => {
                        self.pos += 1;
                        match &self.peek().kind {
                            TokenKind::Ident(name) => {
                                let v = name.clone();
                                let name_span = self.peek().span;
                                self.pos += 1;
                                Ok(FlowStmt::ContextMarker(ContextEntry::NameRef(Spanned::new(v, name_span))))
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
                        Ok(FlowStmt::BareName(Spanned::new(kw_val, kw_val_span)))
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
                                            elif_branches.push(ElifBranch {
                                                condition: cond,
                                                body,
                                            });
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
                                target: Spanned::new(kw_val, kw_val_span),
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
                                        while !matches!(
                                            self.peek().kind,
                                            TokenKind::Rparen
                                                | TokenKind::Eof
                                                | TokenKind::LineStart { .. }
                                        ) {
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
                                    Ok(FlowStmt::BareName(Spanned::new(kw_val, kw_val_span)))
                                } else {
                                    Err(ParseError::Unexpected {
                                        span: dot_span,
                                        message: format!(
                                            "unexpected `.{}` after `{}`",
                                            method, kw_val
                                        ),
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
                                    SourceSpan::from_byte_span(
                                        self.file_label,
                                        span,
                                        self.line_index,
                                    ),
                                ),
                                span,
                            );
                            // Consume `with` and its string to avoid parse cascade.
                            self.pos += 1;
                            if matches!(self.peek().kind, TokenKind::StringLit(_)) {
                                self.pos += 1;
                            }
                            Ok(FlowStmt::BareName(Spanned::new(kw_val, kw_val_span)))
                        } else {
                            Ok(FlowStmt::BareName(Spanned::new(kw_val, kw_val_span)))
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
                    if ident == "applies"
                        && !parts.is_empty()
                        && parts.last() == Some(&".".to_string())
                    {
                        // Check if followed by `(` — if not, it's applies-no-parens.
                        if !matches!(self.peek().kind, TokenKind::Lparen) {
                            let span = ident_span;
                            self.bag.push(
                                Diagnostic::error(
                                    "G::parse::applies-no-parens",
                                    "`.applies` must be followed by `()` — write `.applies()`",
                                    SourceSpan::from_byte_span(
                                        self.file_label,
                                        span,
                                        self.line_index,
                                    ),
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
                                        SourceSpan::from_byte_span(
                                            self.file_label,
                                            span,
                                            self.line_index,
                                        ),
                                    ),
                                    span,
                                );
                                // Skip args until `)`.
                                while !self.at_eof()
                                    && !matches!(
                                        self.peek().kind,
                                        TokenKind::Rparen | TokenKind::LineStart { .. }
                                    )
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
                    // inside a branch condition. Stop scanning the
                    // condition WITHOUT consuming the `Arrow` so the
                    // post-parse Arrow scan in `parse_with_diagnostics`
                    // surfaces the structured `G::parse::operator-in-expression`
                    // (Repairable) diagnostic the pre-#82-chunk-2 byte-scan
                    // path used to emit. Returning a hard `ParseError`
                    // here would short-circuit the scan into a generic
                    // exit-1 failure with no structured ID.
                    break;
                }
                TokenKind::LAngle | TokenKind::RAngle => {
                    // `<`/`>` are only legal in the output-target form
                    // `<IDENT>` after `return` (issue #85). MVP has no
                    // value-level `<` operator (`values-and-names.md` §No
                    // Value-Level Operators 47–55), so they have no meaning
                    // inside a branch condition. Mirror the `Arrow` arm:
                    // break without consuming so an outer scan surfaces a
                    // structured diagnostic instead of a generic exit-1.
                    break;
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
            if i > 0
                && part != "."
                && part != "("
                && part != ")"
                && part != ","
                && parts[i - 1] != "."
                && parts[i - 1] != "("
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
            ConstDecl {
                name,
                value,
                exported: false,
                generated: false,
            },
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
            ConstDecl {
                name,
                value,
                exported: true,
                generated: false,
            },
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
            ConstDecl {
                name,
                value,
                exported: false,
                generated: true,
            },
            span,
        ))
    }

    /// Parse `generated block <name>(<params>) <body>` per
    /// `design/language-surface.md` §3.7. Reuses `parse_block_decl` for the
    /// header and body grammar (header shape is identical to private `block`)
    /// and flips the `generated` flag on the resulting AST node. Span is
    /// extended back to the `generated` keyword so go-to-def lands on the
    /// declaration head.
    ///
    /// Per §3.7 a `generated block` admits no return type. Authors who need
    /// one should promote to a hand-authored `block`. Body shape (single
    /// inline/block string vs. multi-statement `flow:`) is not enforced
    /// here — repair emits a single string body, and §3.7 placement-order
    /// enforcement is deferred alongside §3.6.
    fn parse_generated_block(&mut self) -> Result<Spanned<BlockDecl>, ParseError> {
        let (_, gen_span) = self.expect_ident(Some("generated"))?;
        let mut decl = self.parse_block_decl()?;
        if let Some(rt) = &decl.node.return_type {
            return Err(ParseError::Unexpected {
                span: rt.span,
                message: "`generated block` does not admit a return type \
                          (see design/language-surface.md §3.7); \
                          promote to a hand-authored `block` if one is needed"
                    .to_string(),
            });
        }
        decl.node.generated = true;
        decl.span = Span::new(gen_span.file_id, gen_span.start, decl.span.end);
        Ok(decl)
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
        if let FlowStmt::Branch {
            then_body,
            elif_branches,
            else_body,
            ..
        } = stmt
        {
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

    let only_return = &flow[return_positions[0]];
    let is_output_target_return =
        matches!(only_return, FlowStmt::Return(ReturnExpr::OutputTarget(_)));

    // G::parse::return-in-branch — return inside a branch body. Output targets
    // use the issue-#85-specific diagnostic because they are only legal as a
    // terminal root-flow return.
    if in_branch {
        if is_output_target_return {
            bag.push(
                Diagnostic::error(
                    "G::parse::output-target-outside-return",
                    "output targets are only allowed as the terminal `return <name>` expression",
                    SourceSpan::from_byte_span(file_label, span, line_index),
                ),
                span,
            );
        } else {
            bag.push(
                Diagnostic::error(
                    "G::parse::return-in-branch",
                    "`return` is not allowed inside an `if`/`elif`/`else` branch",
                    SourceSpan::from_byte_span(file_label, span, line_index),
                ),
                span,
            );
        }
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
        if is_output_target_return {
            bag.push(
                Diagnostic::error(
                    "G::parse::output-target-outside-return",
                    "output targets are only allowed as the terminal `return <name>` expression",
                    SourceSpan::from_byte_span(file_label, span, line_index),
                ),
                span,
            );
        } else {
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

    // -- `generated block` form (per language-surface.md §3.7) --

    #[test]
    fn generated_block_parses() {
        // Single-string shorthand body, with a parameter — minimal repair
        // shape per language-surface.md §3.7.
        let src = "\
generated block inspect_failure(area)
    \"Inspect the failure area and report findings.\"
";
        let (file, _) = parse(src, 0).expect("generated block should parse");
        assert_eq!(file.decls.len(), 1);
        match &file.decls[0] {
            Decl::Block(b) => {
                assert_eq!(b.node.name, "inspect_failure");
                assert!(b.node.generated, "generated flag must be set");
                assert_eq!(b.node.params.len(), 1);
                assert_eq!(b.node.params[0].name, "area");
                assert_eq!(b.node.flow.len(), 1);
            }
            other => panic!("expected Decl::Block, got {:?}", other),
        }
    }

    #[test]
    fn generated_block_no_params() {
        let src = "\
generated block summarize_changes()
    flow:
        \"Summarize the changes.\"
";
        let (file, _) = parse(src, 0).expect("generated block (zero params) should parse");
        match &file.decls[0] {
            Decl::Block(b) => {
                assert!(b.node.generated);
                assert!(b.node.params.is_empty());
            }
            other => panic!("expected Decl::Block, got {:?}", other),
        }
    }

    #[test]
    fn generated_block_rejects_return_type() {
        // §3.7: `generated block` does not admit a return-type slot.
        let err = parse(
            "generated block fix() -> Report\n    \"do thing\"\n",
            0,
        )
        .err();
        match err {
            Some(ParseError::Unexpected { ref message, .. }) => {
                assert!(
                    message.contains("return type"),
                    "expected return-type rejection, got: {}",
                    message
                );
            }
            other => panic!("expected ParseError::Unexpected, got {:?}", other),
        }
    }

    #[test]
    fn generated_rejects_unknown_keyword() {
        // Anything other than `const` or `block` after `generated` is an error.
        let err = parse("generated widget x = 1\n", 0).err();
        match err {
            Some(ParseError::Unexpected { ref message, .. }) => {
                assert!(
                    message.contains("`const` or `block`"),
                    "expected dispatch-error message, got: {}",
                    message
                );
            }
            other => panic!("expected ParseError::Unexpected, got {:?}", other),
        }
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
        let (ids, code, _failed) = run_full(src);
        (ids, code)
    }

    /// Run `parse_with_diagnostics` and return (ids, exit_code, parse_failed).
    /// `parse_failed` is `true` when the parser returned `None` (i.e. an
    /// unrecoverable structural error). Cascade-gate suppression tests use
    /// this to lock in BOTH invariants: (a) the parser actually failed on
    /// the malformed input, and (b) no false-positive sweeps fired on
    /// downstream tokens.
    fn run_full(src: &str) -> (Vec<String>, u8, bool) {
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let result = parse_with_diagnostics(src, 0, "t.glyph", &line_index, &mut bag);
        let ids: Vec<String> = bag.iter().map(|d| d.id.clone()).collect();
        (ids, bag.exit_code(), result.is_none())
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
        assert!(toks
            .iter()
            .any(|t| matches!(&t.kind, crate::tokenize::TokenKind::Ident(s) if s == "return")));
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

    // --- Issue #82 codex-pass-2 JOB A: stray `->` in expression positions ---
    //
    // Pre-#82-chunk-2, `-` was tokenized as `UnexpectedChar` and the
    // parse_with_diagnostics tokenize-error arm (lines ~111–139) emitted
    // `G::parse::operator-in-expression`. After chunk 2 promoted `->` to a
    // real `Arrow` token, expression-position `->` was silently dropped by
    // the parser body walkers, regressing the diagnostic. A post-parse
    // Arrow scan was introduced to restore the structured diagnostic via
    // `consumed_arrow_offsets`.
    //
    // Issue #119 (Phase 0) refines the contract: when the parser produces
    // any structural error, BOTH leftover-token sweeps are skipped so the
    // author sees the real structural error first rather than a screen of
    // false positives on unreached downstream tokens. The structured
    // diagnostic still fires when the parser succeeds and the `->` survives
    // unconsumed; it does not fire when the parser stops at the `->` and
    // the legacy `CompileError::Parse` path delivers the structural error
    // instead.

    #[test]
    fn parse_arrow_in_flow_return_expression_is_suppressed_on_parse_error() {
        // Issue #119 cascade-gate: `return x -> y` aborts the flow parser.
        // The post-parse Arrow sweep is skipped on parse error so the
        // author sees the real structural diagnostic (delivered via the
        // legacy `CompileError::Parse` path) instead of an
        // `operator-in-expression` mis-attribution.
        let src = "\
skill foo()
    description: \"d\"
    flow:
        return x -> y
";
        let (ids, _code, parse_failed) = run_full(src);
        assert!(
            parse_failed,
            "this input must trigger a parse failure for the cascade-gate to be relevant; got ids: {:?}",
            ids
        );
        assert!(
            !ids.iter().any(|s| s == "G::parse::operator-in-expression"),
            "cascade-gate must suppress operator-in-expression on parse error, got: {:?}",
            ids
        );
    }

    #[test]
    fn parse_arrow_in_const_rhs_expression_is_suppressed_on_parse_error() {
        // Issue #119 cascade-gate: `const a = b -> c` causes a parse
        // failure (the Arrow appears where a newline is expected). The
        // post-parse Arrow sweep is skipped; the legacy `CompileError::Parse`
        // path surfaces the structural error.
        let src = "const a = b -> c\n";
        let (ids, _code, parse_failed) = run_full(src);
        assert!(
            parse_failed,
            "this input must trigger a parse failure for the cascade-gate to be relevant; got ids: {:?}",
            ids
        );
        assert!(
            !ids.iter().any(|s| s == "G::parse::operator-in-expression"),
            "cascade-gate must suppress operator-in-expression on parse error, got: {:?}",
            ids
        );
    }

    #[test]
    fn parse_does_not_fire_arrow_diag_on_valid_header_return_type() {
        // Regression guard: a valid `block foo() -> Path` header consumes
        // the Arrow via `try_parse_return_type` (which records the offset
        // in `consumed_arrow_offsets`), so the post-parse scan must NOT
        // emit `G::parse::operator-in-expression` for it.
        let src = "\
block foo() -> Path
    description: \"d\"
    flow:
        return \"x\"
";
        let (ids, _) = run(src);
        assert!(
            !ids.iter().any(|s| s == "G::parse::operator-in-expression"),
            "must NOT fire operator-in-expression for valid header `-> Path`, got: {:?}",
            ids
        );
    }

    // --- Issue #82 codex-pass-3 P2-A: `try_parse_return_type` must not
    // record the Arrow before validating the trailing Ident, so the
    // post-parse scan still flags incomplete header arrows.

    #[test]
    fn parse_incomplete_header_arrow_is_suppressed_on_parse_error() {
        // Issue #119 cascade-gate: `block foo() ->` (no trailing ident)
        // makes the parser fail. The post-parse Arrow sweep is skipped on
        // parse error so the author sees the structural error from the
        // legacy `CompileError::Parse` path instead of an
        // `operator-in-expression` mis-attribution.
        let src = "\
block foo() ->
    description: \"d\"
    flow:
        \"x\"
";
        let (ids, _code, parse_failed) = run_full(src);
        assert!(
            parse_failed,
            "this input must trigger a parse failure for the cascade-gate to be relevant; got ids: {:?}",
            ids
        );
        assert!(
            !ids.iter().any(|s| s == "G::parse::operator-in-expression"),
            "cascade-gate must suppress operator-in-expression on parse error, got: {:?}",
            ids
        );
    }

    #[test]
    fn parse_header_arrow_followed_by_string_literal_is_suppressed_on_parse_error() {
        // Issue #119 cascade-gate: `skill foo() -> "Path"` (string literal
        // where an Ident is required) makes the parser fail. The post-parse
        // Arrow sweep is skipped so the structural error surfaces via the
        // legacy `CompileError::Parse` path instead of being shadowed by
        // an `operator-in-expression` mis-attribution.
        let src = "\
skill foo() -> \"Path\"
    description: \"d\"
    flow:
        \"x\"
";
        let (ids, _code, parse_failed) = run_full(src);
        assert!(
            parse_failed,
            "this input must trigger a parse failure for the cascade-gate to be relevant; got ids: {:?}",
            ids
        );
        assert!(
            !ids.iter().any(|s| s == "G::parse::operator-in-expression"),
            "cascade-gate must suppress operator-in-expression on parse error, got: {:?}",
            ids
        );
    }

    // --- Issue #82 codex-pass-3 P2-B: branch-condition Arrow must fall
    // through to the post-parse scan rather than short-circuit into a
    // generic `ParseError::Unexpected`.

    #[test]
    fn parse_arrow_in_branch_condition_is_suppressed_on_parse_error() {
        // Issue #119 cascade-gate: `if cond -> other` raises a parse
        // error from `parse_branch_condition`. The post-parse Arrow
        // sweep is skipped so the structural error from the legacy
        // `CompileError::Parse` path surfaces instead of an
        // `operator-in-expression` mis-attribution.
        let src = "\
skill foo()
    description: \"d\"
    flow:
        if cond -> other
            \"yes\"
";
        let (ids, _code, parse_failed) = run_full(src);
        assert!(
            parse_failed,
            "this input must trigger a parse failure for the cascade-gate to be relevant; got ids: {:?}",
            ids
        );
        assert!(
            !ids.iter().any(|s| s == "G::parse::operator-in-expression"),
            "cascade-gate must suppress operator-in-expression on parse error, got: {:?}",
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
        assert!(
            eb2.has_return,
            "has_return should be true for `return none`"
        );
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
        assert!(
            eb.has_return,
            "has_return should be true for `return \"result\"`"
        );
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

#[cfg(test)]
mod output_target_return_tests {
    //! Issue #85 chunk 3 — wire the output-target identifier form
    //! `return <IDENT>` into the main parser. The AST gains a new
    //! `ReturnExpr::OutputTarget(...)` variant carrying chunk 1's
    //! `OutputTargetExpr`. Diagnostic-ID surfacing for malformed forms is
    //! deferred to chunk 8; chunk 3 only needs the parse path to round-trip
    //! the identifier form and to *reject* malformed forms with an
    //! unstructured `ParseError::Unexpected` for now.
    use super::*;
    use crate::ast::{Decl, FlowStmt};
    use crate::output_target::OutputTargetExpr;

    fn first_skill_flow(src: &str) -> Vec<FlowStmt> {
        let (file, _) = parse(src, 0).expect("parse should succeed");
        file.decls
            .into_iter()
            .find_map(|d| match d {
                Decl::Skill(s) => Some(s.node.flow),
                _ => None,
            })
            .expect("expected a skill declaration")
    }

    #[test]
    fn parse_return_output_target_identifier_tracer() {
        let src = "\
skill foo()
    flow:
        return <thing>
";
        let flow = first_skill_flow(src);
        match flow.last().expect("expected at least one flow stmt") {
            FlowStmt::Return(ReturnExpr::OutputTarget(OutputTargetExpr::Identifier(id))) => {
                assert_eq!(id.name, "thing");
            }
            other => panic!("expected Return(OutputTarget(Identifier)), got {:?}", other),
        }
    }

    #[test]
    fn parse_return_output_target_rejects_malformed_inner_dot() {
        // Chunk 8 will surface a structured diagnostic. Chunk 3 only
        // promises a parser error — never silently produce an
        // `Identifier { name: "a.b" }` and never crash.
        let src = "\
skill foo()
    flow:
        return <a.b>
";
        let err = parse(src, 0)
            .err()
            .expect("expected parse error for `<a.b>`");
        assert!(
            matches!(err, ParseError::Unexpected { .. }),
            "expected ParseError::Unexpected, got {:?}",
            err
        );
    }

    #[test]
    fn parse_return_output_target_rejects_unclosed_bracket() {
        // `return <foo` (no `>`, EOL/EOF) must error rather than scan
        // past the line. Chunk 8 will assign this its own diagnostic ID.
        let src = "\
skill foo()
    flow:
        return <foo
";
        let err = parse(src, 0)
            .err()
            .expect("expected parse error for unclosed `<foo`");
        assert!(
            matches!(err, ParseError::Unexpected { .. }),
            "expected ParseError::Unexpected, got {:?}",
            err
        );
    }

    fn diagnostic_ids(src: &str) -> Vec<String> {
        let (ids, _failed) = diagnostic_ids_full(src);
        ids
    }

    /// Like `diagnostic_ids`, but also returns whether the parser failed
    /// (i.e. `parse_with_diagnostics` returned `None`). Cascade-gate
    /// suppression tests use this to lock in BOTH invariants: (a) the
    /// parser actually failed on the malformed input, and (b) no
    /// false-positive sweeps fired on downstream tokens.
    fn diagnostic_ids_full(src: &str) -> (Vec<String>, bool) {
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let result = parse_with_diagnostics(src, 0, "t.glyph", &line_index, &mut bag);
        let ids: Vec<String> = bag.iter().map(|d| d.id.clone()).collect();
        (ids, result.is_none())
    }

    #[test]
    fn malformed_output_target_surfaces_structured_diagnostic() {
        let src = "\
skill foo()
    flow:
        return <a.b>
";
        let ids = diagnostic_ids(src);
        assert!(
            ids.iter()
                .any(|id| id == "G::parse::malformed-output-target"),
            "expected malformed-output-target diagnostic, got {ids:?}"
        );
    }

    #[test]
    fn trailing_text_after_output_target_surfaces_structured_diagnostic() {
        let src = "\
skill foo()
    flow:
        return <thing>bar
";
        let ids = diagnostic_ids(src);
        assert!(
            ids.iter()
                .any(|id| id == "G::parse::malformed-output-target"),
            "expected malformed-output-target diagnostic, got {ids:?}"
        );
    }

    #[test]
    fn output_target_outside_terminal_return_surfaces_structured_diagnostic() {
        let src = "\
skill foo()
    flow:
        return <thing>
        \"continue\"
";
        let ids = diagnostic_ids(src);
        assert!(
            ids.iter()
                .any(|id| id == "G::parse::output-target-outside-return"),
            "expected output-target-outside-return diagnostic, got {ids:?}"
        );
    }

    /// Issue #119 Phase 0: typed parameters now parse cleanly (the
    /// `: TypeName` slot is recognized syntactically by the parser).
    /// A descriptive return in terminal position should not surface
    /// `output-target-outside-return`. This is the historical "parser
    /// fails, sweep mis-fires on the descriptive return's `<`" scenario,
    /// now resolved by both Phase 0 changes: typed params parse, and
    /// the cascade-gate would suppress sweeps on any parse error anyway.
    #[test]
    fn parse_typed_param_with_descriptive_return_emits_no_output_target_diagnostic() {
        let src = "\
skill foo(a: Path)
    description: \"test\"
    flow:
        \"do x\"
        return <\"the result\">
";
        let ids = diagnostic_ids(src);
        assert!(
            !ids.iter()
                .any(|id| id == "G::parse::output-target-outside-return"),
            "typed param + terminal descriptive return must not fire output-target-outside-return, got: {ids:?}"
        );
    }

    /// Issue #119 cascade-gate: a stray `<` at statement position causes
    /// a parse failure. Both leftover-token sweeps (Arrow scan and `<`
    /// scan) are now suppressed on any parse error, so the structured
    /// `output-target-outside-return` does NOT fire here. The author
    /// sees the structural error from the legacy `CompileError::Parse`
    /// path instead — that is the rejected mis-attribution scenario.
    #[test]
    fn parse_failure_at_stray_langle_is_suppressed_on_parse_error() {
        let src = "\
skill foo()
    flow:
        < bar
";
        let (ids, parse_failed) = diagnostic_ids_full(src);
        assert!(
            parse_failed,
            "this input must trigger a parse failure for the cascade-gate to be relevant; got ids: {ids:?}"
        );
        assert!(
            !ids.iter()
                .any(|id| id == "G::parse::output-target-outside-return"),
            "cascade-gate must suppress output-target-outside-return on parse error, got: {ids:?}"
        );
    }

    #[test]
    fn parse_return_output_target_identifier_span_covers_whole_form() {
        // The Identifier.span produced by chunk 1 includes the brackets.
        // The parser must propagate that contract: its computed `form_span`
        // must equal `<…>` byte-for-byte (start at the `<`, end after `>`).
        // Chunk 8 relies on this span when surfacing structured diagnostics.
        let src = "\
skill foo()
    flow:
        return <bar>
";
        let flow = first_skill_flow(src);
        let id = match flow.last().expect("expected a flow stmt") {
            FlowStmt::Return(ReturnExpr::OutputTarget(OutputTargetExpr::Identifier(id))) => {
                id.clone()
            }
            other => panic!("expected Return(OutputTarget(Identifier)), got {:?}", other),
        };
        let start = id.span.start as usize;
        let end = id.span.end as usize;
        assert_eq!(&src[start..end], "<bar>", "span must cover `<bar>` exactly");
    }

    #[test]
    fn parse_return_output_target_in_private_block() {
        let src = "\
block helper() -> Path
    flow:
        return <output>
";
        let (file, _) = parse(src, 0).expect("parse should succeed");
        let block_flow = file
            .decls
            .into_iter()
            .find_map(|d| match d {
                Decl::Block(b) => Some(b.node.flow),
                _ => None,
            })
            .expect("expected a private-block declaration");
        match block_flow.last().expect("expected a flow stmt") {
            FlowStmt::Return(ReturnExpr::OutputTarget(OutputTargetExpr::Identifier(id))) => {
                assert_eq!(id.name, "output");
            }
            other => panic!("expected Return(OutputTarget(Identifier)), got {:?}", other),
        }
    }

    #[test]
    fn descriptive_output_target_parses_in_terminal_return() {
        let src = "\
block diagnose() -> Diagnosis
    flow:
        return <\"root cause analysis\">
";
        let (file, _) = parse(src, 0).expect("parse should succeed");
        let block_flow = file
            .decls
            .into_iter()
            .find_map(|d| match d {
                Decl::Block(b) => Some(b.node.flow),
                _ => None,
            })
            .expect("expected a private-block declaration");
        match block_flow.last().expect("expected a flow stmt") {
            FlowStmt::Return(ReturnExpr::OutputTarget(OutputTargetExpr::Description(d))) => {
                assert_eq!(d.content, "root cause analysis");
            }
            other => panic!("expected Return(OutputTarget(Description)), got {:?}", other),
        }
    }
}

#[cfg(test)]
mod export_block_terminal_return_tests {
    //! Issue #85 chunk 4b (D4) — `ExportBlockDecl.terminal_return` field
    //! captures the structurally-parsed `ReturnExpr` from the body's
    //! `return ...` line. AST-only per D4 — IR symmetry for export blocks
    //! is deferred to a follow-up issue.
    use super::*;
    use crate::ast::{Decl, ExportBlockDecl, ReturnExpr};
    use crate::output_target::OutputTargetExpr;

    fn first_export_block(src: &str) -> ExportBlockDecl {
        let (file, _) = parse(src, 0).expect("parse should succeed");
        file.decls
            .into_iter()
            .find_map(|d| match d {
                Decl::ExportBlock(b) => Some(b.node),
                _ => None,
            })
            .expect("expected an export block declaration")
    }

    #[test]
    fn export_block_terminal_return_output_target_tracer() {
        let src = "\
export block foo() -> Report
    flow:
        \"x\"
        return <result>
";
        let eb = first_export_block(src);
        match eb.terminal_return {
            Some(ReturnExpr::OutputTarget(OutputTargetExpr::Identifier(id))) => {
                assert_eq!(id.name, "result");
            }
            other => panic!(
                "expected Some(Return(OutputTarget(Identifier))), got {:?}",
                other
            ),
        }
    }

    #[test]
    fn export_block_terminal_return_name_variant() {
        // `return some_name` → ReturnExpr::Name (matches canonical skill arm).
        let src = "\
export block foo() -> SomeType
    flow:
        \"x\"
        return result
";
        let eb = first_export_block(src);
        match eb.terminal_return {
            Some(ReturnExpr::Name(ref n)) => assert_eq!(n.node, "result"),
            other => panic!("expected Some(Return(Name)), got {:?}", other),
        }
    }

    #[test]
    fn export_block_terminal_return_inline_string_variant() {
        // `return "literal"` → ReturnExpr::Inline.
        let src = "\
export block foo()
    flow:
        \"x\"
        return \"literal payload\"
";
        let eb = first_export_block(src);
        match eb.terminal_return {
            Some(ReturnExpr::Inline(ref s)) => assert_eq!(s, "literal payload"),
            other => panic!("expected Some(Return(Inline)), got {:?}", other),
        }
    }

    #[test]
    fn export_block_terminal_return_none_lowercase_variant() {
        // `return none` → ReturnExpr::None (lowercase consumed by canonical arm).
        let src = "\
export block foo()
    flow:
        \"x\"
        return none
";
        let eb = first_export_block(src);
        assert!(
            matches!(eb.terminal_return, Some(ReturnExpr::None)),
            "expected Some(Return(None)), got {:?}",
            eb.terminal_return
        );
    }

    #[test]
    fn export_block_terminal_return_last_write_wins() {
        // Two `return` lines → terminal_return holds the last one.
        // (The language requires exactly one per data-flow.md §Return
        // Semantics line 401-403; this guard documents the parser behavior
        // when authors break the rule.)
        let src = "\
export block foo() -> SomeType
    flow:
        return first
        return last
";
        let eb = first_export_block(src);
        match eb.terminal_return {
            Some(ReturnExpr::Name(ref n)) => assert_eq!(n.node, "last"),
            other => panic!("expected Some(Return(Name(\"last\"))), got {:?}", other),
        }
    }

    #[test]
    fn export_block_terminal_return_none_when_body_has_no_return() {
        // No `return` line at all → terminal_return stays None.
        // (`G::analyze::missing-return` covers the user-facing diagnostic
        // via `has_return: bool`; this assertion just pins the field.)
        let src = "\
export block foo()
    flow:
        \"x\"
";
        let eb = first_export_block(src);
        assert!(
            eb.terminal_return.is_none(),
            "expected None when body has no return, got {:?}",
            eb.terminal_return
        );
    }

    #[test]
    fn export_block_output_target_must_be_terminal_flow_item() {
        let src = "\
export block foo() -> Report
    flow:
        return <result>
        \"continue\"
";
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let _ = parse_with_diagnostics(src, 0, "t.glyph", &line_index, &mut bag);
        let ids: Vec<_> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.iter()
                .any(|id| *id == "G::parse::output-target-outside-return"),
            "expected output-target-outside-return diagnostic, got {ids:?}"
        );
    }
}

#[cfg(test)]
mod duplicate_subsection_recovery_tests {
    //! Issue #109 chunk 2 — duplicate `description:` / `context:` / `flow:` /
    //! `effects:` / `constraints:` sub-sections under one declaration are
    //! recovered: the first occurrence populates the canonical singleton
    //! field, every subsequent occurrence's body lands in
    //! `extra_subsections`, and `G::parse::duplicate-subsection` fires once
    //! per duplicate header (classification `Repairable`).
    //!
    //! Tests target `Skill` because it is the only declaration that exposes
    //! all five sub-section kinds today (`BlockDecl` / `ExportBlockDecl`
    //! parse only `description` / `effects` / `flow`).
    use super::*;
    use crate::ast::{
        ConstraintMarkerKind, ContextEntry, Decl, DuplicateSubsection, FlowStmt, Skill,
    };
    use crate::diagnostic::{Classification, DiagBag};

    /// Parse a source containing exactly one `skill` decl, returning the
    /// `Skill` node together with the diagnostic bag accumulated during
    /// parse. Effects are enabled so `effects:` sub-section tests work.
    fn parse_first_skill_with_bag(src: &str) -> (Skill, DiagBag) {
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let file = match parse_with_diagnostics_opts(
            src,
            0,
            "dup.glyph",
            &line_index,
            &mut bag,
            true,
        ) {
            Some(f) => f,
            None => {
                // Surface the legacy parse error to make AC4 failures
                // (parser returning None when only duplicate-subsection
                // diagnostics fire) actionable.
                let legacy = parse(src, 0).err();
                let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
                panic!(
                    "parser returned None; bag ids: {:?}; legacy parse err: {:?}",
                    ids, legacy
                );
            }
        };
        let skill = file
            .decls
            .into_iter()
            .find_map(|d| match d {
                Decl::Skill(spanned) => Some(spanned.node),
                _ => None,
            })
            .expect("expected one skill decl");
        (skill, bag)
    }

    fn duplicate_subsection_diags(bag: &DiagBag) -> Vec<&crate::diagnostic::Diagnostic> {
        bag.iter()
            .filter(|d| d.id == "G::parse::duplicate-subsection")
            .collect()
    }

    #[test]
    fn skill_two_descriptions_first_wins_second_in_extras() {
        // Two `description:` under one `skill` — first body stays in the
        // singleton, second body lands in `extra_subsections`. One
        // `G::parse::duplicate-subsection` diagnostic fires (repairable).
        let src = "\
skill foo()
    description: \"First.\"
    description: \"Second.\"
    flow:
        \"Do something.\"
";
        let (skill, bag) = parse_first_skill_with_bag(src);

        assert_eq!(
            skill.description.as_deref(),
            Some("First."),
            "first `description:` body must stay in the singleton field (first-wins)"
        );
        assert_eq!(
            skill.extra_subsections.len(),
            1,
            "second `description:` body must be captured in extras (got {:?})",
            skill.extra_subsections
        );
        match &skill.extra_subsections[0] {
            DuplicateSubsection::Description(s) => {
                assert_eq!(s, "Second.", "extras[0] should hold the second body");
            }
            other => panic!("expected DuplicateSubsection::Description, got {:?}", other),
        }

        let dups = duplicate_subsection_diags(&bag);
        assert_eq!(dups.len(), 1, "exactly one duplicate-subsection diagnostic");
        assert_eq!(dups[0].classification, Classification::Repairable);
    }

    #[test]
    fn skill_two_constraints_first_wins_second_in_extras() {
        // Two `constraints:` sub-sections under one `skill`. The parser
        // routes `constraints:` markers into `body_constraints` (not the
        // dormant `constraints_section`), so the recovery contract is:
        //   - first body's markers stay in `body_constraints`
        //   - second body's markers land in
        //     `extra_subsections` as `DuplicateSubsection::Constraints(...)`
        //   - second body's markers MUST NOT flow into `body_constraints`
        let src = "\
skill foo()
    constraints:
        require accuracy
    constraints:
        avoid stale_references
    flow:
        \"Do something.\"
";
        let (skill, bag) = parse_first_skill_with_bag(src);

        // First body's markers landed in body_constraints exactly once.
        assert_eq!(
            skill.body_constraints.len(),
            1,
            "body_constraints should hold exactly the first `constraints:` body's markers (got {:?})",
            skill.body_constraints
        );
        assert_eq!(skill.body_constraints[0].marker, ConstraintMarkerKind::Require);
        assert_eq!(skill.body_constraints[0].name.node, "accuracy");

        // Second body recovered as a single Constraints variant in extras.
        assert_eq!(
            skill.extra_subsections.len(),
            1,
            "second `constraints:` body must be captured in extras"
        );
        match &skill.extra_subsections[0] {
            DuplicateSubsection::Constraints(markers) => {
                assert_eq!(markers.len(), 1);
                assert_eq!(markers[0].marker, ConstraintMarkerKind::Avoid);
                assert_eq!(markers[0].name.node, "stale_references");
            }
            other => panic!("expected DuplicateSubsection::Constraints, got {:?}", other),
        }

        let dups = duplicate_subsection_diags(&bag);
        assert_eq!(dups.len(), 1, "exactly one duplicate-subsection diagnostic");
        assert_eq!(dups[0].classification, Classification::Repairable);
    }

    #[test]
    fn skill_triple_constraints_extras_in_source_order() {
        // Three `constraints:` sub-sections. First body's markers stay in
        // `body_constraints`; the second and third bodies land in
        // `extra_subsections` in source order. Two duplicate-subsection
        // diagnostics fire (one per duplicate header).
        let src = "\
skill foo()
    constraints:
        require accuracy
    constraints:
        avoid stale_references
    constraints:
        must clarity
    flow:
        \"Do something.\"
";
        let (skill, bag) = parse_first_skill_with_bag(src);

        assert_eq!(skill.body_constraints.len(), 1, "first body wins on body_constraints");
        assert_eq!(skill.body_constraints[0].name.node, "accuracy");

        assert_eq!(skill.extra_subsections.len(), 2, "two extras for the 2nd + 3rd body");
        match (&skill.extra_subsections[0], &skill.extra_subsections[1]) {
            (DuplicateSubsection::Constraints(m1), DuplicateSubsection::Constraints(m2)) => {
                assert_eq!(m1.len(), 1);
                assert_eq!(m1[0].marker, ConstraintMarkerKind::Avoid);
                assert_eq!(m1[0].name.node, "stale_references");
                assert_eq!(m2.len(), 1);
                assert_eq!(m2[0].marker, ConstraintMarkerKind::Must);
                assert_eq!(m2[0].name.node, "clarity");
            }
            other => panic!("expected two Constraints extras in source order, got {:?}", other),
        }

        let dups = duplicate_subsection_diags(&bag);
        assert_eq!(dups.len(), 2, "one diagnostic per duplicate header");
        for d in &dups {
            assert_eq!(d.classification, Classification::Repairable);
        }
    }

    #[test]
    fn skill_two_contexts_first_wins_second_in_extras() {
        // Two `context:` sub-sections. First body's entries stay in
        // `context_section`; second body's entries land in extras.
        let src = "\
skill foo()
    context:
        \"first ctx\"
    context:
        \"second ctx\"
    flow:
        \"Do something.\"
";
        let (skill, bag) = parse_first_skill_with_bag(src);

        assert_eq!(
            skill.context_section.len(),
            1,
            "context_section should hold exactly the first body's entries"
        );
        match &skill.context_section[0] {
            ContextEntry::InlineString(s) => assert_eq!(s, "first ctx"),
            other => panic!("expected first body to be InlineString, got {:?}", other),
        }

        assert_eq!(skill.extra_subsections.len(), 1);
        match &skill.extra_subsections[0] {
            DuplicateSubsection::Context(entries) => {
                assert_eq!(entries.len(), 1);
                match &entries[0] {
                    ContextEntry::InlineString(s) => assert_eq!(s, "second ctx"),
                    other => panic!("expected second body InlineString, got {:?}", other),
                }
            }
            other => panic!("expected DuplicateSubsection::Context, got {:?}", other),
        }

        let dups = duplicate_subsection_diags(&bag);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].classification, Classification::Repairable);
    }

    #[test]
    fn skill_two_effects_first_wins_second_in_extras() {
        // Two `effects:` sub-sections. First body's keywords stay in
        // `effects`; second body's keywords land in extras.
        let src = "\
skill foo()
    effects: reads_files
    effects: writes_files
    flow:
        \"Do something.\"
";
        let (skill, bag) = parse_first_skill_with_bag(src);

        assert_eq!(skill.effects, vec!["reads_files".to_string()]);
        assert_eq!(skill.extra_subsections.len(), 1);
        match &skill.extra_subsections[0] {
            DuplicateSubsection::Effects(items) => {
                assert_eq!(items, &vec!["writes_files".to_string()]);
            }
            other => panic!("expected DuplicateSubsection::Effects, got {:?}", other),
        }

        let dups = duplicate_subsection_diags(&bag);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].classification, Classification::Repairable);
    }

    #[test]
    fn skill_two_flows_first_wins_second_in_extras() {
        // Two `flow:` sub-sections. First body's statements stay in `flow`;
        // second body's statements land in extras as `Flow(...)`.
        let src = "\
skill foo()
    description: \"Has two flows.\"
    flow:
        \"first stmt\"
    flow:
        \"second stmt\"
";
        let (skill, bag) = parse_first_skill_with_bag(src);

        assert!(skill.flow_present);
        assert_eq!(skill.flow.len(), 1, "first body wins on flow");
        match &skill.flow[0] {
            FlowStmt::InlineString(s) => assert_eq!(s, "first stmt"),
            other => panic!("expected first flow stmt InlineString, got {:?}", other),
        }

        assert_eq!(skill.extra_subsections.len(), 1);
        match &skill.extra_subsections[0] {
            DuplicateSubsection::Flow(stmts) => {
                assert_eq!(stmts.len(), 1);
                match &stmts[0] {
                    FlowStmt::InlineString(s) => assert_eq!(s, "second stmt"),
                    other => panic!("expected second flow stmt InlineString, got {:?}", other),
                }
            }
            other => panic!("expected DuplicateSubsection::Flow, got {:?}", other),
        }

        let dups = duplicate_subsection_diags(&bag);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].classification, Classification::Repairable);
    }

    #[test]
    fn skill_no_duplicates_extras_empty_no_diagnostic() {
        // Spot-check baseline: a well-formed skill with one of every
        // sub-section produces no extras and no duplicate-subsection
        // diagnostic.
        let src = "\
skill foo()
    description: \"All distinct.\"
    context:
        \"ctx\"
    constraints:
        require accuracy
    effects: reads_files
    flow:
        \"Do something.\"
";
        let (skill, bag) = parse_first_skill_with_bag(src);

        assert!(
            skill.extra_subsections.is_empty(),
            "no duplicates → extras must be empty (got {:?})",
            skill.extra_subsections
        );
        let dups = duplicate_subsection_diags(&bag);
        assert!(
            dups.is_empty(),
            "no duplicates → no duplicate-subsection diagnostic (ids: {:?})",
            bag.iter().map(|d| d.id.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn skill_two_kinds_duplicated_in_one_body() {
        // Sanity: independent duplicates of two different kinds in one
        // declaration produce two extras and two diagnostics; first
        // occurrences of each kind populate their singleton fields.
        let src = "\
skill foo()
    description: \"first desc\"
    description: \"second desc\"
    effects: reads_files
    effects: writes_files
    flow:
        \"Do something.\"
";
        let (skill, bag) = parse_first_skill_with_bag(src);

        assert_eq!(skill.description.as_deref(), Some("first desc"));
        assert_eq!(skill.effects, vec!["reads_files".to_string()]);

        assert_eq!(skill.extra_subsections.len(), 2);
        // Order: the description duplicate header appears before the
        // effects duplicate header in source order.
        match &skill.extra_subsections[0] {
            DuplicateSubsection::Description(s) => assert_eq!(s, "second desc"),
            other => panic!("expected first extra Description, got {:?}", other),
        }
        match &skill.extra_subsections[1] {
            DuplicateSubsection::Effects(items) => {
                assert_eq!(items, &vec!["writes_files".to_string()]);
            }
            other => panic!("expected second extra Effects, got {:?}", other),
        }

        let dups = duplicate_subsection_diags(&bag);
        assert_eq!(dups.len(), 2);
        for d in &dups {
            assert_eq!(d.classification, Classification::Repairable);
        }
    }

    // --- Issue #109 codex-pass-2 findings 4 & 5 ---
    //
    // `BlockDecl` and `ExportBlockDecl` carry the same `extra_subsections`
    // field as `Skill`, and Analyze checks them, but the chunk-2 parser
    // changes only landed for `parse_skill_body_line`. These tests pin the
    // recovery contract for the other two declaration kinds.

    fn parse_first_block_with_bag(src: &str) -> (crate::ast::BlockDecl, DiagBag) {
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let file = parse_with_diagnostics_opts(src, 0, "dup.glyph", &line_index, &mut bag, true)
            .expect("parser must recover and return Some(file)");
        let block = file
            .decls
            .into_iter()
            .find_map(|d| match d {
                Decl::Block(b) => Some(b.node),
                _ => None,
            })
            .expect("expected a block decl");
        (block, bag)
    }

    fn parse_first_export_block_with_bag(
        src: &str,
    ) -> (crate::ast::ExportBlockDecl, DiagBag) {
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let file = parse_with_diagnostics_opts(src, 0, "dup.glyph", &line_index, &mut bag, true)
            .expect("parser must recover and return Some(file)");
        let eb = file
            .decls
            .into_iter()
            .find_map(|d| match d {
                Decl::ExportBlock(e) => Some(e.node),
                _ => None,
            })
            .expect("expected an export block decl");
        (eb, bag)
    }

    /// Finding 5 — duplicate `description:` in a `block` is recovered into
    /// `extra_subsections` and surfaces a single repairable
    /// `G::parse::duplicate-subsection` diagnostic.
    #[test]
    fn block_two_descriptions_first_wins_second_in_extras() {
        let src = "\
block foo()
    description: \"First.\"
    description: \"Second.\"
    flow:
        \"Do something.\"
";
        let (block, bag) = parse_first_block_with_bag(src);
        assert_eq!(
            block.description.as_deref(),
            Some("First."),
            "first `description:` body must stay in the singleton (first-wins)"
        );
        assert_eq!(
            block.extra_subsections.len(),
            1,
            "second body must land in extras, got {:?}",
            block.extra_subsections
        );
        match &block.extra_subsections[0] {
            DuplicateSubsection::Description(s) => assert_eq!(s, "Second."),
            other => panic!("expected DuplicateSubsection::Description, got {:?}", other),
        }
        let dups = duplicate_subsection_diags(&bag);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].classification, Classification::Repairable);
    }

    /// Finding 5 — duplicate `effects:` in a `block` is recovered.
    #[test]
    fn block_two_effects_first_wins_second_in_extras() {
        let src = "\
block foo()
    effects: reads_files
    effects: writes_files
    flow:
        \"Do something.\"
";
        let (block, bag) = parse_first_block_with_bag(src);
        assert_eq!(
            block.effects,
            vec!["reads_files".to_string()],
            "first `effects:` body must stay in the canonical Vec"
        );
        assert_eq!(block.extra_subsections.len(), 1);
        match &block.extra_subsections[0] {
            DuplicateSubsection::Effects(items) => {
                assert_eq!(items, &vec!["writes_files".to_string()]);
            }
            other => panic!("expected DuplicateSubsection::Effects, got {:?}", other),
        }
        let dups = duplicate_subsection_diags(&bag);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].classification, Classification::Repairable);
    }

    /// Finding 5 — duplicate `flow:` in a `block` is recovered. First body's
    /// statements stay in `flow`; second body's statements land in extras.
    #[test]
    fn block_two_flows_first_wins_second_in_extras() {
        let src = "\
block foo()
    flow:
        \"first stmt\"
    flow:
        \"second stmt\"
";
        let (block, bag) = parse_first_block_with_bag(src);
        assert_eq!(block.flow.len(), 1, "first body wins on `flow`");
        match &block.flow[0] {
            FlowStmt::InlineString(s) => assert_eq!(s, "first stmt"),
            other => panic!("expected first body InlineString, got {:?}", other),
        }
        assert_eq!(block.extra_subsections.len(), 1);
        match &block.extra_subsections[0] {
            DuplicateSubsection::Flow(stmts) => {
                assert_eq!(stmts.len(), 1);
                match &stmts[0] {
                    FlowStmt::InlineString(s) => assert_eq!(s, "second stmt"),
                    other => panic!("expected second body InlineString, got {:?}", other),
                }
            }
            other => panic!("expected DuplicateSubsection::Flow, got {:?}", other),
        }
        let dups = duplicate_subsection_diags(&bag);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].classification, Classification::Repairable);
    }

    /// Finding 4 — duplicate `description:` in an `export block` is
    /// recovered into `extra_subsections`.
    #[test]
    fn export_block_two_descriptions_first_wins_second_in_extras() {
        let src = "\
export block foo() -> Report
    description: \"First.\"
    description: \"Second.\"
    flow:
        \"Do something.\"
        return <result>
";
        let (eb, bag) = parse_first_export_block_with_bag(src);
        assert_eq!(
            eb.description.as_deref(),
            Some("First."),
            "first `description:` body must stay in the singleton (first-wins)"
        );
        assert_eq!(
            eb.extra_subsections.len(),
            1,
            "second body must land in extras, got {:?}",
            eb.extra_subsections
        );
        match &eb.extra_subsections[0] {
            DuplicateSubsection::Description(s) => assert_eq!(s, "Second."),
            other => panic!("expected DuplicateSubsection::Description, got {:?}", other),
        }
        let dups = duplicate_subsection_diags(&bag);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].classification, Classification::Repairable);
    }

    /// Finding 4 — duplicate `effects:` in an `export block` is recovered.
    #[test]
    fn export_block_two_effects_first_wins_second_in_extras() {
        let src = "\
export block foo() -> Report
    effects: reads_files
    effects: writes_files
    flow:
        \"Do something.\"
        return <result>
";
        let (eb, bag) = parse_first_export_block_with_bag(src);
        assert_eq!(eb.effects, vec!["reads_files".to_string()]);
        assert_eq!(eb.extra_subsections.len(), 1);
        match &eb.extra_subsections[0] {
            DuplicateSubsection::Effects(items) => {
                assert_eq!(items, &vec!["writes_files".to_string()]);
            }
            other => panic!("expected DuplicateSubsection::Effects, got {:?}", other),
        }
        let dups = duplicate_subsection_diags(&bag);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].classification, Classification::Repairable);
    }

    /// Finding 4 — duplicate `flow:` in an `export block` is recovered.
    /// First body's strings stay in `flow_strings`; second body's strings
    /// land in extras.
    #[test]
    fn export_block_two_flows_first_wins_second_in_extras() {
        let src = "\
export block foo() -> Report
    flow:
        \"first stmt\"
    flow:
        \"second stmt\"
        return <result>
";
        let (eb, bag) = parse_first_export_block_with_bag(src);
        assert_eq!(
            eb.flow_strings,
            vec!["first stmt".to_string()],
            "first body wins on `flow_strings`"
        );
        assert_eq!(eb.extra_subsections.len(), 1);
        match &eb.extra_subsections[0] {
            DuplicateSubsection::Flow(stmts) => {
                assert_eq!(stmts.len(), 1);
                match &stmts[0] {
                    FlowStmt::InlineString(s) => assert_eq!(s, "second stmt"),
                    other => panic!("expected second body InlineString, got {:?}", other),
                }
            }
            other => panic!("expected DuplicateSubsection::Flow, got {:?}", other),
        }
        let dups = duplicate_subsection_diags(&bag);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].classification, Classification::Repairable);
    }

    // --- Issue #109 codex pass-3 finding 8 ---
    //
    // The AST-suppression gate at `parse_with_diagnostics_opts` originally
    // returned `None` whenever the bag contained any repairable other than
    // `G::parse::duplicate-subsection`. That broke fmt's recovery contract
    // for mixed-diagnostic inputs: a file with a duplicate sub-section AND
    // any other repairable (e.g., a `{slot}` inside a `context:` body
    // emitting `G::parse::param-slot-in-non-instruction-string`) had its
    // AST suppressed, the merge never fired, and the duplicate body was
    // lost. Fix: gate on tier alone — repairable-only bags flow through;
    // any error/fatal still suppresses. The principle is "any combination
    // of repairables is repairable" — that's what the tier means.

    /// Finding 8 — a file that emits both `G::parse::duplicate-subsection`
    /// AND another repairable diagnostic of an unrelated kind must still
    /// return a `Some(file)` from the parser so fmt can merge the
    /// duplicate. The unrelated repairable used here is
    /// `G::parse::param-slot-in-non-instruction-string` from a `{name}`
    /// slot inside a `context:` body — orthogonal to duplicate-subsection
    /// recovery but sharing the repairable tier.
    #[test]
    fn ast_flows_through_for_mixed_repairables() {
        let src = "\
skill foo()
    context:
        \"ctx with {slot}\"
    context:
        \"second ctx\"
    flow:
        \"do work\"
";
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let file = parse_with_diagnostics_opts(
            src,
            0,
            "dup.glyph",
            &line_index,
            &mut bag,
            true,
        );

        // Pin the bag shape: at least one duplicate-subsection AND at
        // least one param-slot diagnostic, both repairable.
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.iter().any(|id| *id == "G::parse::duplicate-subsection"),
            "expected duplicate-subsection in bag, got {:?}",
            ids
        );
        assert!(
            ids.iter()
                .any(|id| *id == "G::parse::param-slot-in-non-instruction-string"),
            "expected param-slot-in-non-instruction-string in bag, got {:?}",
            ids
        );

        // Both diagnostics must be repairable (no errors); the AST must
        // flow through so fmt can later splice the duplicate body back
        // into the singleton.
        let any_error = bag
            .iter()
            .any(|d| matches!(d.classification, Classification::Error));
        assert!(
            !any_error,
            "all diagnostics in this scenario should be repairable; bag: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.classification))
                .collect::<Vec<_>>()
        );

        let file = file.expect(
            "AST must flow through when bag is repairable-only (any combination); \
             gate suppressed it",
        );

        // The recovered duplicate must land in `extra_subsections` so fmt
        // can pick it up.
        let skill = file
            .decls
            .into_iter()
            .find_map(|d| match d {
                Decl::Skill(s) => Some(s.node),
                _ => None,
            })
            .expect("expected a skill decl");
        assert_eq!(
            skill.extra_subsections.len(),
            1,
            "duplicate context body must land in extras"
        );
    }

    // --- Issue #109 codex pass-3 finding 9 ---
    //
    // Finding 4's `parse_export_block` recovery only covered
    // `description:` / `effects:` / `flow:`. `context:` and `constraints:`
    // also pass through `parse_export_block` (`design/language-surface.md`
    // §2.5: "Inside a `skill`, `block`, or `export block` body…"), but
    // duplicates of those kinds were silently dropped. These tests pin
    // the recovery contract for both kinds on `export block` AND `block`.

    /// Finding 9 — duplicate `context:` in an `export block` is recovered.
    #[test]
    fn export_block_two_contexts_first_wins_second_in_extras() {
        let src = "\
export block foo() -> Report
    context:
        \"first ctx\"
    context:
        \"second ctx\"
    flow:
        \"Do something.\"
        return <result>
";
        let (eb, bag) = parse_first_export_block_with_bag(src);
        assert_eq!(
            eb.extra_subsections.len(),
            1,
            "duplicate context body must land in extras; got {:?}",
            eb.extra_subsections
        );
        match &eb.extra_subsections[0] {
            DuplicateSubsection::Context(_) => {}
            other => panic!("expected DuplicateSubsection::Context, got {:?}", other),
        }
        let dups = duplicate_subsection_diags(&bag);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].classification, Classification::Repairable);
    }

    /// Finding 9 — duplicate `constraints:` in an `export block` is recovered.
    #[test]
    fn export_block_two_constraints_first_wins_second_in_extras() {
        let src = "\
export block foo() -> Report
    constraints:
        require accuracy
    constraints:
        avoid stale_references
    flow:
        \"Do something.\"
        return <result>
";
        let (eb, bag) = parse_first_export_block_with_bag(src);
        assert_eq!(
            eb.extra_subsections.len(),
            1,
            "duplicate constraints body must land in extras; got {:?}",
            eb.extra_subsections
        );
        match &eb.extra_subsections[0] {
            DuplicateSubsection::Constraints(_) => {}
            other => panic!("expected DuplicateSubsection::Constraints, got {:?}", other),
        }
        let dups = duplicate_subsection_diags(&bag);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].classification, Classification::Repairable);
    }

    /// Finding 9 (block coverage) — duplicate `context:` in a `block` is
    /// recovered. Mirrors the export-block test for the inner `block`
    /// declaration kind.
    #[test]
    fn block_two_contexts_first_wins_second_in_extras() {
        let src = "\
block foo()
    context:
        \"first ctx\"
    context:
        \"second ctx\"
    flow:
        \"Do something.\"
";
        let (block, bag) = parse_first_block_with_bag(src);
        assert_eq!(
            block.extra_subsections.len(),
            1,
            "duplicate context body must land in extras; got {:?}",
            block.extra_subsections
        );
        match &block.extra_subsections[0] {
            DuplicateSubsection::Context(_) => {}
            other => panic!("expected DuplicateSubsection::Context, got {:?}", other),
        }
        let dups = duplicate_subsection_diags(&bag);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].classification, Classification::Repairable);
    }

    /// Finding 9 (block coverage) — duplicate `constraints:` in a `block`
    /// is recovered.
    #[test]
    fn block_two_constraints_first_wins_second_in_extras() {
        let src = "\
block foo()
    constraints:
        require accuracy
    constraints:
        avoid stale_references
    flow:
        \"Do something.\"
";
        let (block, bag) = parse_first_block_with_bag(src);
        assert_eq!(
            block.extra_subsections.len(),
            1,
            "duplicate constraints body must land in extras; got {:?}",
            block.extra_subsections
        );
        match &block.extra_subsections[0] {
            DuplicateSubsection::Constraints(_) => {}
            other => panic!("expected DuplicateSubsection::Constraints, got {:?}", other),
        }
        let dups = duplicate_subsection_diags(&bag);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].classification, Classification::Repairable);
    }
}

#[cfg(test)]
mod import_decl_tests {
    //! Issue #116 / #117 — selective-import brace list may span multiple lines.
    //!
    //! Verifies: the helper `Parser::skip_line_starts` is called at three
    //! positions inside the `TokenKind::Lbrace` arm of `parse_import`
    //! (after `{`, after each `,`, before the closing `}` check), and that
    //! items (`name`, optional `as <alias>`) remain atomic. Tests drive
    //! external parser behavior via `parse(...)` — they do not assert on
    //! token positions, byte ranges, or helper call counts.

    use super::*;
    use crate::ast::{Decl, ImportDecl, ImportKind};

    /// Parse `src` and return the first decl as an `ImportDecl`. Panics if
    /// the source fails to parse or the first decl isn't an import.
    fn parse_first_import(src: &str) -> ImportDecl {
        let (file, _) = parse(src, 0).expect("source should parse");
        match file.decls.into_iter().next().expect("expected one decl") {
            Decl::Import(spanned) => spanned.node,
            other => panic!("expected Decl::Import, got {:?}", other),
        }
    }

    /// Project a selective `ImportDecl` to `(path, [(name, alias), …])` so
    /// equivalence between single-line and multi-line forms can be asserted
    /// without coupling to outer-span byte ranges or future fields on
    /// `ImportName`.
    fn extract(d: ImportDecl) -> (String, Vec<(String, Option<String>)>) {
        let names = match d.kind {
            ImportKind::Selective(ns) => ns.into_iter().map(|n| (n.name.node, n.alias)).collect(),
            other => panic!("expected ImportKind::Selective, got {:?}", other),
        };
        (d.path, names)
    }

    #[test]
    fn multi_line_with_trailing_comma_equals_single_line() {
        let multi = "import \"./x.glyph\" {\n    a,\n    b,\n    c,\n}\n";
        let single = "import \"./x.glyph\" { a, b, c }\n";
        assert_eq!(
            extract(parse_first_import(multi)),
            extract(parse_first_import(single)),
        );
    }

    #[test]
    fn multi_line_without_trailing_comma_parses() {
        let src = "import \"./x.glyph\" {\n    a,\n    b,\n    c\n}\n";
        let (path, names) = extract(parse_first_import(src));
        assert_eq!(path, "./x.glyph");
        let bare: Vec<&str> = names.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(bare, vec!["a", "b", "c"]);
        assert!(names.iter().all(|(_, alias)| alias.is_none()));
    }

    #[test]
    fn multi_line_mixed_layout_parses() {
        // Some names on the header line, more on subsequent lines, `}` on
        // its own line. Asserts the parser does not require a uniform layout.
        let src = "import \"./x.glyph\" { a, b,\n    c,\n    d,\n}\n";
        let (_, names) = extract(parse_first_import(src));
        let bare: Vec<&str> = names.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(bare, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn multi_line_aliases_across_lines_parse() {
        // Items themselves stay on a single line; line breaks between items
        // are exercised. Both aliases survive.
        let src = "import \"./x.glyph\" {\n    foo as f,\n    bar as b,\n}\n";
        let (_, names) = extract(parse_first_import(src));
        assert_eq!(names.len(), 2);
        assert_eq!(names[0].0, "foo");
        assert_eq!(names[0].1.as_deref(), Some("f"));
        assert_eq!(names[1].0, "bar");
        assert_eq!(names[1].1.as_deref(), Some("b"));
    }

    #[test]
    fn multi_line_missing_comma_between_names_diagnostic() {
        // `b` on a new line without a comma after `a`. The diagnostic must
        // mention both `,` and `}` and pin the span to the `b` token, not
        // to a `LineStart`.
        let src = "import \"./x.glyph\" { a\n    b\n}\n";
        let err = parse(src, 0).err().expect("expected ParseError");
        match err {
            ParseError::Unexpected { ref message, span } => {
                assert!(
                    message.contains(',') && message.contains('}'),
                    "message should mention both `,` and `}}`, got: {:?}",
                    message
                );
                // Span must sit on the `b` token. Extract it from the source.
                let snippet = &src[span.start as usize..span.end as usize];
                assert_eq!(snippet, "b", "span should cover `b`, got {:?}", snippet);
            }
            other => panic!("expected ParseError::Unexpected, got {:?}", other),
        }
    }

    #[test]
    fn multi_line_with_comments_parses() {
        // A comment-only line between names + a trailing `// …` after a name.
        // Both should be invisible to the parser by the time it sees the
        // brace list, so the import parses cleanly.
        let src = "\
import \"./x.glyph\" {
    // explanatory note
    a, // why we need a
    b,
}
";
        let (_, names) = extract(parse_first_import(src));
        let bare: Vec<&str> = names.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(bare, vec!["a", "b"]);
    }

    #[test]
    fn multi_line_import_does_not_cascade_to_arrow_or_output_target() {
        // Reduced inline fixture (do NOT reference any authoring file):
        //   * multi-line selective import (the previously breaking shape)
        //   * later `-> Path` return-type annotation (legit; would mis-fire
        //     `G::parse::operator-in-expression` if parse_import bails)
        //   * later `<output_target>` site (legit; would mis-fire
        //     `G::parse::output-target-outside-return` pre-PR-#140)
        //
        // After the fix, parse_import succeeds, both Arrow and `<` tokens
        // are consumed legitimately, and neither cascade triggers.
        let src = "\
import \"./other.glyph\" {
    foo,
    bar,
}

skill main() -> Path
    flow:
        return <output_target>
";
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let _ = parse_with_diagnostics(src, 0, "t.glyph", &line_index, &mut bag);
        let ids: Vec<String> = bag.iter().map(|d| d.id.clone()).collect();
        assert!(
            !ids.iter().any(|s| s == "G::parse::operator-in-expression"),
            "must not fire operator-in-expression after multi-line-import fix; got: {:?}",
            ids
        );
        assert!(
            !ids.iter()
                .any(|s| s == "G::parse::output-target-outside-return"),
            "must not fire output-target-outside-return after multi-line-import fix; got: {:?}",
            ids
        );
    }
}

#[cfg(test)]
mod typed_param_tests {
    //! Issue #119 Phase 0 — parser support for `name: Type` parameter
    //! slots. Phase 0 is purely syntactic: the type ident is recorded as
    //! `Param.type_annotation` and surfaced as `SemTokenType::Type`. There
    //! is no resolution, no validation, and no semantic interpretation;
    //! later phases extend this slot with descriptions and TypeRegistry
    //! wiring.
    use super::*;
    use crate::ast::{BlockDecl, Decl, ExportBlockDecl, Skill};
    use crate::span::LineIndex;

    fn first_skill(src: &str) -> Skill {
        let (file, _diags) = parse(src, 0).expect("parse should succeed");
        file.decls
            .into_iter()
            .find_map(|d| match d {
                Decl::Skill(s) => Some(s.node),
                _ => None,
            })
            .expect("expected a skill declaration")
    }

    fn first_block(src: &str) -> BlockDecl {
        let (file, _diags) = parse(src, 0).expect("parse should succeed");
        file.decls
            .into_iter()
            .find_map(|d| match d {
                Decl::Block(b) => Some(b.node),
                _ => None,
            })
            .expect("expected a block declaration")
    }

    fn first_export_block(src: &str) -> ExportBlockDecl {
        let (file, _diags) = parse(src, 0).expect("parse should succeed");
        file.decls
            .into_iter()
            .find_map(|d| match d {
                Decl::ExportBlock(b) => Some(b.node),
                _ => None,
            })
            .expect("expected an export block declaration")
    }

    #[test]
    fn skill_typed_param_records_annotation() {
        let src = "\
skill foo(a: Path)
    description: \"d\"
    flow:
        \"x\"
";
        let skill = first_skill(src);
        let p = &skill.params[0];
        assert_eq!(p.name, "a");
        assert!(
            p.default.is_none(),
            "no default expected, got: {:?}",
            p.default
        );
        let t = p
            .type_annotation
            .as_ref()
            .expect("type_annotation should be populated");
        assert_eq!(t.node, "Path");
    }

    #[test]
    fn skill_typed_param_with_default_records_both() {
        let src = "\
skill foo(a: Path = \"./out\")
    description: \"d\"
    flow:
        \"x\"
";
        let skill = first_skill(src);
        let p = &skill.params[0];
        assert_eq!(p.name, "a");
        assert_eq!(p.default.as_deref(), Some("\"./out\""));
        let t = p
            .type_annotation
            .as_ref()
            .expect("type_annotation should be populated");
        assert_eq!(t.node, "Path");
    }

    #[test]
    fn skill_untyped_param_records_no_annotation() {
        let src = "\
skill foo(a)
    description: \"d\"
    flow:
        \"x\"
";
        let skill = first_skill(src);
        let p = &skill.params[0];
        assert_eq!(p.name, "a");
        assert!(
            p.type_annotation.is_none(),
            "untyped param must have no annotation, got: {:?}",
            p.type_annotation
        );
    }

    #[test]
    fn skill_mixed_param_list_typed_and_untyped() {
        let src = "\
skill foo(a, b: Path, c = \"x\", d: Path = \"y\")
    description: \"d\"
    flow:
        \"x\"
";
        let skill = first_skill(src);
        assert_eq!(skill.params.len(), 4);

        // a — untyped, no default
        assert_eq!(skill.params[0].name, "a");
        assert!(skill.params[0].type_annotation.is_none());
        assert!(skill.params[0].default.is_none());

        // b — typed, no default
        assert_eq!(skill.params[1].name, "b");
        assert_eq!(
            skill.params[1]
                .type_annotation
                .as_ref()
                .map(|t| t.node.as_str()),
            Some("Path")
        );
        assert!(skill.params[1].default.is_none());

        // c — untyped, with default
        assert_eq!(skill.params[2].name, "c");
        assert!(skill.params[2].type_annotation.is_none());
        assert_eq!(skill.params[2].default.as_deref(), Some("\"x\""));

        // d — typed, with default
        assert_eq!(skill.params[3].name, "d");
        assert_eq!(
            skill.params[3]
                .type_annotation
                .as_ref()
                .map(|t| t.node.as_str()),
            Some("Path")
        );
        assert_eq!(skill.params[3].default.as_deref(), Some("\"y\""));
    }

    #[test]
    fn skill_typed_param_span_covers_type_ident() {
        let src = "\
skill foo(a: Path)
    description: \"d\"
    flow:
        \"x\"
";
        let skill = first_skill(src);
        let t = skill.params[0]
            .type_annotation
            .as_ref()
            .expect("type_annotation populated");
        let start = t.span.start as usize;
        let end = t.span.end as usize;
        assert_eq!(
            &src[start..end],
            "Path",
            "type ident span must cover `Path`"
        );
    }

    #[test]
    fn block_typed_param_records_annotation() {
        let src = "\
block helper(a: Path)
    description: \"d\"
    flow:
        \"x\"
";
        let block = first_block(src);
        let t = block.params[0]
            .type_annotation
            .as_ref()
            .expect("type_annotation should be populated");
        assert_eq!(t.node, "Path");
    }

    #[test]
    fn export_block_typed_param_records_annotation() {
        let src = "\
export block helper(a: Path) -> Path
    description: \"d\"
    flow:
        return \"x\"
";
        let eb = first_export_block(src);
        let t = eb.params[0]
            .type_annotation
            .as_ref()
            .expect("type_annotation should be populated");
        assert_eq!(t.node, "Path");
    }

    #[test]
    fn malformed_typed_param_missing_type_name_is_parse_error() {
        // `a:` with nothing after it must surface a generic ParseError
        // (per the PRD: malformed shapes reuse `expect_ident`'s error).
        let src = "\
skill foo(a:)
    description: \"d\"
    flow:
        \"x\"
";
        let res = parse(src, 0);
        assert!(
            res.is_err(),
            "expected ParseError for `a:` (missing type ident), got Ok"
        );
    }

    #[test]
    fn malformed_typed_param_non_ident_after_colon_is_parse_error() {
        // `a: 123` must surface a generic ParseError — the slot only
        // accepts a bare identifier in Phase 0.
        let src = "\
skill foo(a: 123)
    description: \"d\"
    flow:
        \"x\"
";
        let res = parse(src, 0);
        assert!(
            res.is_err(),
            "expected ParseError for `a: 123` (non-ident type), got Ok"
        );
    }

    #[test]
    fn cascade_gate_suppresses_both_sweeps_on_parse_error() {
        // Issue #119 cascade-gate: a single parse error suppresses BOTH
        // leftover-token sweeps (Arrow scan and `<` scan). Construct a
        // source whose parse failure lands BEFORE both a downstream `->`
        // and a downstream `<`; without the gate, the post-parse sweeps
        // would mis-attribute one or both of these as repairable
        // diagnostics. With the gate, neither fires.
        let src = "\
skill foo(a: 123)
    description: \"d\"
    flow:
        \"x\" -> bar
        return <output>
";
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let _ = parse_with_diagnostics(src, 0, "t.glyph", &line_index, &mut bag);
        let ids: Vec<String> = bag.iter().map(|d| d.id.clone()).collect();
        assert!(
            !ids.iter().any(|s| s == "G::parse::operator-in-expression"),
            "cascade-gate must suppress operator-in-expression on parse error, got: {:?}",
            ids
        );
        assert!(
            !ids.iter()
                .any(|s| s == "G::parse::output-target-outside-return"),
            "cascade-gate must suppress output-target-outside-return on parse error, got: {:?}",
            ids
        );
    }
}

#[cfg(test)]
mod param_description_tests {
    //! Issue #119 Phase A.2 — parser support for `<"…">` per-param
    //! descriptions. The descriptive form sits in the `=` slot alongside
    //! (or in place of) a default literal. Stored as `Param.description`
    //! (a `Spanned<String>` covering the full `<…>` form). Phase A.2 is
    //! syntactic only: emitter wiring lands in Phase A.4.
    use super::*;
    use crate::ast::{Decl, Skill};
    use crate::span::LineIndex;

    fn first_skill(src: &str) -> Skill {
        let (file, _diags) = parse(src, 0).expect("parse should succeed");
        file.decls
            .into_iter()
            .find_map(|d| match d {
                Decl::Skill(s) => Some(s.node),
                _ => None,
            })
            .expect("expected a skill declaration")
    }

    #[test]
    fn parse_param_description_inline_form() {
        let src = r#"skill test_skill(x = <"the description">)
    description: "test"
    flow:
        "step"
"#;
        let skill = first_skill(src);
        let p = &skill.params[0];
        assert_eq!(p.name, "x");
        assert_eq!(p.default, None);
        let desc = p.description.as_ref().expect("description should be Some");
        assert_eq!(desc.node, "the description");
    }

    #[test]
    fn parse_param_combo_default_and_description() {
        let src = r#"skill test_skill(risk = "medium" <"raise to high if auth">)
    description: "test"
    flow:
        "step"
"#;
        let skill = first_skill(src);
        let p = &skill.params[0];
        assert_eq!(p.default.as_deref(), Some("\"medium\""));
        let desc = p.description.as_ref().expect("description should be Some");
        assert_eq!(desc.node, "raise to high if auth");
    }

    #[test]
    fn parse_param_description_block_string() {
        let src = "\
skill test_skill(x = <\"\"\"line1\nline2\"\"\">)
    description: \"test\"
    flow:
        \"step\"
";
        let skill = first_skill(src);
        let p = &skill.params[0];
        let desc = p.description.as_ref().expect("description should be Some");
        assert_eq!(desc.node, "line1\nline2");
    }

    #[test]
    fn param_description_does_not_trigger_output_target_sweep() {
        // The `<` consumed inside `parse_param_description` must be
        // registered in `consumed_output_target_offsets` so the post-parse
        // LAngle sweep (which surfaces `G::parse::output-target-outside-return`)
        // does not double-fire on a `<` that was a legitimate part of a
        // valid param description.
        let src = r#"skill test_skill(x = <"desc">)
    description: "test"
    flow:
        "step"
"#;
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let result = parse_with_diagnostics(src, 0, "t.glyph", &line_index, &mut bag);
        assert!(result.is_some(), "valid source should parse");
        let ids: Vec<String> = bag.iter().map(|d| d.id.clone()).collect();
        assert!(
            !ids.iter()
                .any(|s| s == "G::parse::output-target-outside-return"),
            "param description must register `<` as consumed; got: {:?}",
            ids
        );
    }
}
