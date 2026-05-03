//! Semantic-token collector (M3).
//!
//! Walks the lex stream + parsed AST to produce a sorted, dense list of
//! `(line, col, length, token-type, modifiers)` tuples. The LSP layer
//! delta-encodes these into the LSP `textDocument/semanticTokens/full`
//! reply. Token types and modifiers are mapped onto the LSP standard set
//! per the tree-sitter `highlights.scm` grammar (kept in
//! `feature/tree-sitter-grammar`) so cross-editor highlighting stays in
//! sync between tree-sitter editors (Helix, Zed, Neovim treesitter) and
//! pure-LSP editors (VS Code, basic nvim-lspconfig).
//!
//! # Coordinate system
//!
//! Each `RawSemToken` carries:
//! - `line`: 0-indexed line number (LSP convention).
//! - `start`: 0-indexed byte column from the start of the line.
//!   Glyph rejects tabs and non-ASCII identifiers, so byte == utf-16 code
//!   unit for everything except string-literal contents. M3 accepts the
//!   small imprecision that string literals containing non-ASCII may
//!   produce slightly off LSP ranges; pure-ASCII source is exact.
//! - `length`: byte length of the token.
//! - `token_type`: index into [`SemTokenType::legend`].
//! - `modifiers`: bitset over [`SemTokenModifier::legend`].
//!
//! # Strategy
//!
//! Two passes that produce candidate tokens, then sort + dedup by start
//! position (last-write-wins for exact overlaps so the AST pass refines
//! the lex pass):
//!
//! 1. **Lex pass** — re-tokenize the source. Map identifier tokens whose
//!    text matches a hard keyword (`skill`, `block`, `if`, …) to
//!    [`SemTokenType::Keyword`]; map type identifiers (`text`, `int`,
//!    `float`) to [`SemTokenType::Type`]; map section labels
//!    (`description`, `flow`, …) to keyword **only when followed by a
//!    colon** (so they remain plain identifiers when used elsewhere).
//!    String literals become [`SemTokenType::String`].
//! 2. **Comment pass** — scan source bytes for `//` comments (the lexer
//!    drops them), emit [`SemTokenType::Comment`] for each.
//! 3. **AST pass** — parse the source (errors are silenced — partial AST
//!    is fine here). For each declaration, classify the bound name plus
//!    any references reachable from the declaration body.

use crate::ast::{
    BlockDecl, ContextEntry, Decl, ExportBlockDecl, FlowStmt, ImportDecl, ImportKind,
    ReturnExpr, Skill, SourceFile, TextDecl,
};
use crate::diagnostic::DiagBag;
use crate::parse::parse_with_diagnostics_opts;
use crate::span::{LineIndex, Span, Spanned};
use crate::tokenize::{tokenize, Token, TokenKind};

/// LSP standard semantic token types we emit. Index in the legend ==
/// enum discriminant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum SemTokenType {
    Keyword = 0,
    Type = 1,
    Function = 2,
    Method = 3,
    Parameter = 4,
    Variable = 5,
    Property = 6,
    String = 7,
    Namespace = 8,
    Number = 9,
    Comment = 10,
}

impl SemTokenType {
    /// Names in the LSP `semanticTokensProvider.legend.tokenTypes` array.
    pub const fn legend() -> [&'static str; 11] {
        [
            "keyword",   // 0
            "type",      // 1
            "function",  // 2
            "method",    // 3
            "parameter", // 4
            "variable",  // 5
            "property",  // 6
            "string",    // 7
            "namespace", // 8
            "number",    // 9
            "comment",   // 10
        ]
    }
}

/// LSP standard semantic token modifiers as bit flags. Bit position
/// matches the index into [`SemTokenModifier::legend`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemTokenModifier;

impl SemTokenModifier {
    pub const NONE: u32 = 0;
    pub const DECLARATION: u32 = 1 << 0;
    pub const DEFINITION: u32 = 1 << 1;
    pub const READONLY: u32 = 1 << 2;
    pub const DEFAULT_LIBRARY: u32 = 1 << 3;

    /// Names in the LSP `semanticTokensProvider.legend.tokenModifiers` array.
    pub const fn legend() -> [&'static str; 4] {
        ["declaration", "definition", "readonly", "defaultLibrary"]
    }
}

/// One semantic token in absolute (line, start) coordinates. The LSP
/// transport delta-encodes a sorted list of these into the `data: Vec<u32>`
/// shape on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawSemToken {
    /// 0-indexed line number.
    pub line: u32,
    /// 0-indexed column (bytes from line start).
    pub start: u32,
    /// Byte length of the token.
    pub length: u32,
    /// Token type — discriminant value of [`SemTokenType`].
    pub token_type: u32,
    /// Bitset of [`SemTokenModifier`] flags.
    pub modifiers: u32,
}

/// Run the lex + comment + AST passes and return a sorted, dense list of
/// semantic tokens for the whole source.
///
/// Errors during tokenize or parse are swallowed — partial AST is still
/// useful for highlighting (we just emit fewer AST-pass tokens).
pub fn collect_semantic_tokens(source: &str, file_id: u32) -> Vec<RawSemToken> {
    let line_index = LineIndex::new(source);
    let mut tokens: Vec<RawSemToken> = Vec::new();

    // Pass 1: lex tokens.
    let lex_tokens = tokenize(source, file_id).map(|(t, _)| t).unwrap_or_default();
    classify_lex_tokens(&lex_tokens, &line_index, &mut tokens);

    // Pass 2: line comments (lexer strips them).
    classify_comments(source, &line_index, &mut tokens);

    // Pass 3: AST-driven identifier classification.
    let mut bag = DiagBag::new();
    if let Some(ast) = parse_with_diagnostics_opts(
        source,
        file_id,
        "<semantic-tokens>",
        &line_index,
        &mut bag,
        true,
    ) {
        classify_ast(&ast, source, file_id, &line_index, &mut tokens);
    }

    sort_and_dedup(&mut tokens);
    tokens
}

// -- Lex pass -------------------------------------------------------------

fn classify_lex_tokens(
    lex_tokens: &[Token],
    line_index: &LineIndex,
    out: &mut Vec<RawSemToken>,
) {
    for (i, tok) in lex_tokens.iter().enumerate() {
        match &tok.kind {
            TokenKind::Ident(text) => {
                if let Some(ttype) = classify_keyword_text(text.as_str()) {
                    push_span(out, tok.span, ttype, SemTokenModifier::NONE, line_index);
                } else if is_section_label(text.as_str()) {
                    // Section labels (`description`, `flow`, …) are
                    // keywords only when immediately followed by `:`.
                    let next_is_colon = lex_tokens
                        .get(i + 1)
                        .map(|t| matches!(t.kind, TokenKind::Colon))
                        .unwrap_or(false);
                    if next_is_colon {
                        push_span(
                            out,
                            tok.span,
                            SemTokenType::Keyword,
                            SemTokenModifier::NONE,
                            line_index,
                        );
                    }
                }
            }
            TokenKind::StringLit(_) => {
                push_span(out, tok.span, SemTokenType::String, SemTokenModifier::NONE, line_index);
            }
            _ => {}
        }
    }
}

/// Hard keywords + builtin types — always classified, regardless of
/// surrounding tokens.
fn classify_keyword_text(s: &str) -> Option<SemTokenType> {
    match s {
        // Declaration / control flow / logical operators / markers.
        "skill"
        | "block"
        | "export"
        | "generated"
        | "import"
        | "as"
        | "return"
        | "if"
        | "elif"
        | "else"
        | "and"
        | "or"
        | "not"
        | "require"
        | "avoid"
        | "must"
        | "context"
        | "with"
        | "none" => Some(SemTokenType::Keyword),
        // Builtin types.
        "text" | "int" | "float" => Some(SemTokenType::Type),
        _ => None,
    }
}

/// Section-label words — keywords *only* when followed by `:` (so they
/// don't false-positive when used as identifiers elsewhere).
fn is_section_label(s: &str) -> bool {
    matches!(s, "description" | "effects" | "constraints" | "flow")
}

// -- Comment pass ---------------------------------------------------------

/// Scan source bytes for `//` line comments and emit a
/// [`SemTokenType::Comment`] for each (`//` to end-of-line).
///
/// We mirror the tokenizer's `strip_trailing_comment` logic: track an
/// in-string flag so `//` inside a string literal is not mistaken for a
/// comment opener.
fn classify_comments(source: &str, line_index: &LineIndex, out: &mut Vec<RawSemToken>) {
    let bytes = source.as_bytes();
    let mut p = 0;
    let mut in_string = false;
    while p < bytes.len() {
        let b = bytes[p];
        if b == b'\n' {
            in_string = false;
            p += 1;
            continue;
        }
        if b == b'"' {
            in_string = !in_string;
            p += 1;
            continue;
        }
        if !in_string && b == b'/' && p + 1 < bytes.len() && bytes[p + 1] == b'/' {
            // Find end of line.
            let mut q = p;
            while q < bytes.len() && bytes[q] != b'\n' {
                q += 1;
            }
            let span = Span::new(0, p as u32, q as u32);
            push_span(out, span, SemTokenType::Comment, SemTokenModifier::NONE, line_index);
            p = q;
            continue;
        }
        p += 1;
    }
}

// -- AST pass -------------------------------------------------------------

fn classify_ast(
    ast: &SourceFile,
    source: &str,
    file_id: u32,
    line_index: &LineIndex,
    out: &mut Vec<RawSemToken>,
) {
    for decl in &ast.decls {
        match decl {
            Decl::Skill(s) => classify_skill(s, source, file_id, line_index, out),
            Decl::Block(b) => classify_block(b, source, file_id, line_index, out),
            Decl::ExportBlock(eb) => {
                classify_export_block(eb, source, file_id, line_index, out)
            }
            Decl::Text(t) => classify_text(t, source, file_id, line_index, out),
            Decl::Import(i) => classify_import(i, source, file_id, line_index, out),
        }
    }
}

fn classify_skill(
    spanned: &Spanned<Skill>,
    source: &str,
    file_id: u32,
    line_index: &LineIndex,
    out: &mut Vec<RawSemToken>,
) {
    if let Some(name_span) =
        find_name_after_keywords(source, file_id, spanned.span, &["skill"])
    {
        push_span(
            out,
            name_span,
            SemTokenType::Function,
            SemTokenModifier::DECLARATION | SemTokenModifier::DEFINITION,
            line_index,
        );
    }
    classify_params(&spanned.node.params, line_index, out);
    classify_constraints(&spanned.node.body_constraints, line_index, out);
    classify_context_entries(&spanned.node.body_context, line_index, out);
    classify_context_entries(&spanned.node.context_section, line_index, out);
    for n in &spanned.node.body_bare_names {
        push_span(out, n.span, SemTokenType::Variable, SemTokenModifier::NONE, line_index);
    }
    for stmt in &spanned.node.flow {
        classify_flow_stmt(stmt, line_index, out);
    }
}

fn classify_block(
    spanned: &Spanned<BlockDecl>,
    source: &str,
    file_id: u32,
    line_index: &LineIndex,
    out: &mut Vec<RawSemToken>,
) {
    if let Some(name_span) =
        find_name_after_keywords(source, file_id, spanned.span, &["block"])
    {
        push_span(
            out,
            name_span,
            SemTokenType::Method,
            SemTokenModifier::DECLARATION | SemTokenModifier::DEFINITION,
            line_index,
        );
    }
    classify_params(&spanned.node.params, line_index, out);
    for stmt in &spanned.node.flow {
        classify_flow_stmt(stmt, line_index, out);
    }
}

fn classify_export_block(
    spanned: &Spanned<ExportBlockDecl>,
    source: &str,
    file_id: u32,
    line_index: &LineIndex,
    out: &mut Vec<RawSemToken>,
) {
    if let Some(name_span) =
        find_name_after_keywords(source, file_id, spanned.span, &["export", "block"])
    {
        push_span(
            out,
            name_span,
            SemTokenType::Method,
            SemTokenModifier::DECLARATION | SemTokenModifier::DEFINITION,
            line_index,
        );
    }
    classify_params(&spanned.node.params, line_index, out);
    // Slice 4 doesn't lower export-block flow into FlowStmt — `body_refs` is
    // a flat list of identifier strings without spans, so we don't get
    // call-site classifications here. Acceptable for M3.
}

fn classify_text(
    spanned: &Spanned<TextDecl>,
    source: &str,
    file_id: u32,
    line_index: &LineIndex,
    out: &mut Vec<RawSemToken>,
) {
    let kws: &[&str] = if spanned.node.exported {
        &["export", "text"]
    } else {
        &["text"]
    };
    if let Some(name_span) = find_name_after_keywords(source, file_id, spanned.span, kws) {
        push_span(
            out,
            name_span,
            SemTokenType::Variable,
            SemTokenModifier::DECLARATION
                | SemTokenModifier::DEFINITION
                | SemTokenModifier::READONLY,
            line_index,
        );
    }
}

fn classify_import(
    spanned: &Spanned<ImportDecl>,
    source: &str,
    file_id: u32,
    line_index: &LineIndex,
    out: &mut Vec<RawSemToken>,
) {
    match &spanned.node.kind {
        ImportKind::Selective(names) => {
            for n in names {
                push_span(
                    out,
                    n.name.span,
                    SemTokenType::Variable,
                    SemTokenModifier::NONE,
                    line_index,
                );
                // Aliases (`name as alias`) lack spans on the alias token in
                // the AST. Locate by scanning forward from the name span end
                // for ` as <ident>` within the decl span.
                if let Some(alias) = &n.alias {
                    if let Some(alias_span) = find_alias_span(
                        source,
                        file_id,
                        n.name.span.end,
                        spanned.span.end,
                        alias,
                    ) {
                        push_span(
                            out,
                            alias_span,
                            SemTokenType::Namespace,
                            SemTokenModifier::DECLARATION,
                            line_index,
                        );
                    }
                }
            }
        }
        ImportKind::WholeModule { alias } => {
            // Whole-module imports look like `import "<path>" as <alias>`.
            // The decl span starts at `import`; the alias sits after a
            // string literal and the `as` keyword. Scan for ` as <alias>`
            // anywhere in the decl slice.
            if let Some(alias_span) = find_alias_span(
                source,
                file_id,
                spanned.span.start,
                spanned.span.end,
                alias,
            ) {
                push_span(
                    out,
                    alias_span,
                    SemTokenType::Namespace,
                    SemTokenModifier::DECLARATION,
                    line_index,
                );
            }
        }
    }
}

fn classify_params(
    params: &[crate::ast::Param],
    line_index: &LineIndex,
    out: &mut Vec<RawSemToken>,
) {
    for p in params {
        let name_span = Span::new(
            p.span.file_id,
            p.span.start,
            p.span.start + p.name.len() as u32,
        );
        push_span(
            out,
            name_span,
            SemTokenType::Parameter,
            SemTokenModifier::DECLARATION,
            line_index,
        );
    }
}

fn classify_constraints(
    cms: &[crate::ast::ConstraintMarker],
    line_index: &LineIndex,
    out: &mut Vec<RawSemToken>,
) {
    for cm in cms {
        push_span(out, cm.name.span, SemTokenType::Variable, SemTokenModifier::NONE, line_index);
    }
}

fn classify_context_entries(
    entries: &[ContextEntry],
    line_index: &LineIndex,
    out: &mut Vec<RawSemToken>,
) {
    for entry in entries {
        if let ContextEntry::NameRef(n) = entry {
            push_span(out, n.span, SemTokenType::Variable, SemTokenModifier::NONE, line_index);
        }
    }
}

fn classify_flow_stmt(stmt: &FlowStmt, line_index: &LineIndex, out: &mut Vec<RawSemToken>) {
    match stmt {
        FlowStmt::Call { target, .. } => {
            push_span(
                out,
                target.span,
                SemTokenType::Function,
                SemTokenModifier::NONE,
                line_index,
            );
        }
        FlowStmt::ConstraintMarker(cm) => {
            push_span(
                out,
                cm.name.span,
                SemTokenType::Variable,
                SemTokenModifier::NONE,
                line_index,
            );
        }
        FlowStmt::ContextMarker(ContextEntry::NameRef(n)) => {
            push_span(out, n.span, SemTokenType::Variable, SemTokenModifier::NONE, line_index);
        }
        FlowStmt::ContextMarker(ContextEntry::InlineString(_)) => {}
        FlowStmt::BareName(n) => {
            push_span(out, n.span, SemTokenType::Variable, SemTokenModifier::NONE, line_index);
        }
        FlowStmt::Return(ReturnExpr::Call { target, .. }) => {
            push_span(
                out,
                target.span,
                SemTokenType::Function,
                SemTokenModifier::NONE,
                line_index,
            );
        }
        FlowStmt::Return(ReturnExpr::Name(n)) => {
            push_span(out, n.span, SemTokenType::Variable, SemTokenModifier::NONE, line_index);
        }
        FlowStmt::Return(_) => {}
        FlowStmt::Branch { then_body, elif_branches, else_body, .. } => {
            for s in then_body {
                classify_flow_stmt(s, line_index, out);
            }
            for elif in elif_branches {
                for s in &elif.body {
                    classify_flow_stmt(s, line_index, out);
                }
            }
            if let Some(eb) = else_body {
                for s in eb {
                    classify_flow_stmt(s, line_index, out);
                }
            }
        }
        FlowStmt::InlineString(_) => {}
    }
}

// -- Helpers --------------------------------------------------------------

fn push_span(
    out: &mut Vec<RawSemToken>,
    span: Span,
    ttype: SemTokenType,
    modifiers: u32,
    line_index: &LineIndex,
) {
    if span.end <= span.start {
        return;
    }
    let (sline, scol) = line_index.line_col(span.start);
    let length = span.end - span.start;
    out.push(RawSemToken {
        line: sline.saturating_sub(1),
        start: scol.saturating_sub(1),
        length,
        token_type: ttype as u32,
        modifiers,
    });
}

/// Sort tokens by `(line, start)`, then dedup exact-overlap entries by
/// keeping the *last* push. This lets the AST pass refine a lex-pass
/// classification (e.g. plain `Ident` → `Function` for a call site).
fn sort_and_dedup(tokens: &mut Vec<RawSemToken>) {
    tokens.sort_by_key(|t| (t.line, t.start));
    let mut deduped: Vec<RawSemToken> = Vec::with_capacity(tokens.len());
    for t in tokens.drain(..) {
        if let Some(last) = deduped.last_mut() {
            if last.line == t.line && last.start == t.start && last.length == t.length {
                // Exact overlap — keep the newer (AST-pass) classification.
                *last = t;
                continue;
            }
        }
        deduped.push(t);
    }
    *tokens = deduped;
}

/// Find the absolute byte span of the identifier that follows the given
/// keyword sequence inside a decl's source slice.
///
/// Used to recover the name span for `Skill`, `BlockDecl`, etc., which
/// store `name: String` rather than `Spanned<String>`. `decl_span` is the
/// declaration's full span; `keywords` is the prefix to skip
/// (e.g. `&["export", "block"]`).
fn find_name_after_keywords(
    source: &str,
    file_id: u32,
    decl_span: Span,
    keywords: &[&str],
) -> Option<Span> {
    let s = decl_span.start as usize;
    let e = decl_span.end as usize;
    let slice = source.get(s..e)?;
    let bytes = slice.as_bytes();
    let mut p = 0;
    skip_inline_ws(bytes, &mut p);
    for kw in keywords {
        if !slice.get(p..).map(|t| t.starts_with(kw)).unwrap_or(false) {
            return None;
        }
        p += kw.len();
        skip_inline_ws(bytes, &mut p);
    }
    let name_start = p;
    while p < bytes.len() && (bytes[p].is_ascii_alphanumeric() || bytes[p] == b'_') {
        p += 1;
    }
    if p == name_start {
        return None;
    }
    Some(Span::new(file_id, (s + name_start) as u32, (s + p) as u32))
}

/// Find the span of the alias name in `... as <alias>` within the
/// `[search_start, search_end)` slice of `source`. Used for both
/// selective-import aliases (search starts after the imported name's
/// span) and whole-module aliases (search starts at the decl).
///
/// Scans forward for a standalone `as` keyword (preceded by whitespace
/// or at the slice start, followed by whitespace), then captures the
/// next identifier token. Returns its span if it matches `alias`.
fn find_alias_span(
    source: &str,
    file_id: u32,
    search_start: u32,
    search_end: u32,
    alias: &str,
) -> Option<Span> {
    let s = search_start as usize;
    let e = (search_end as usize).min(source.len());
    let slice = source.get(s..e)?;
    let bytes = slice.as_bytes();
    // Walk byte-by-byte looking for a standalone `as` token.
    let mut p = 0;
    while p < bytes.len() {
        let ch = bytes[p];
        if (ch == b'a')
            && p + 1 < bytes.len()
            && bytes[p + 1] == b's'
            && (p == 0 || !is_ident_continue(bytes[p - 1]))
            && (p + 2 >= bytes.len() || !is_ident_continue(bytes[p + 2]))
        {
            let mut q = p + 2;
            skip_inline_ws(bytes, &mut q);
            let alias_start = q;
            while q < bytes.len() && is_ident_continue(bytes[q]) {
                q += 1;
            }
            if q > alias_start {
                let found = &slice[alias_start..q];
                if found == alias {
                    return Some(Span::new(
                        file_id,
                        (s + alias_start) as u32,
                        (s + q) as u32,
                    ));
                }
            }
        }
        p += 1;
    }
    None
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn skip_inline_ws(bytes: &[u8], p: &mut usize) {
    while *p < bytes.len() && (bytes[*p] == b' ' || bytes[*p] == b'\t' || bytes[*p] == b'\n') {
        *p += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_token<'a>(
        tokens: &'a [RawSemToken],
        line: u32,
        col_substr: &str,
        source: &str,
    ) -> Option<&'a RawSemToken> {
        // Locate the substring position to drive the assertion lookup.
        let line_start: usize = source
            .split('\n')
            .take(line as usize)
            .map(|l| l.len() + 1)
            .sum();
        let after_line_start = source.get(line_start..)?;
        let in_line = after_line_start.split('\n').next()?;
        let col = in_line.find(col_substr)? as u32;
        tokens
            .iter()
            .find(|t| t.line == line && t.start == col && t.length == col_substr.len() as u32)
    }

    #[test]
    fn legend_lengths_match_discriminants() {
        assert_eq!(SemTokenType::legend().len(), 11);
        assert_eq!(SemTokenModifier::legend().len(), 4);
    }

    #[test]
    fn keyword_and_string_classification() {
        let src = "skill main()\n    description: \"main.\"\n    flow:\n        \"hi\"\n";
        let tokens = collect_semantic_tokens(src, 0);
        let skill = find_token(&tokens, 0, "skill", src).expect("`skill` keyword");
        assert_eq!(skill.token_type, SemTokenType::Keyword as u32);
        let desc = find_token(&tokens, 1, "description", src).expect("`description:` keyword");
        assert_eq!(desc.token_type, SemTokenType::Keyword as u32);
        let flow = find_token(&tokens, 2, "flow", src).expect("`flow:` keyword");
        assert_eq!(flow.token_type, SemTokenType::Keyword as u32);
        // String literals: `"main."` on line 1, `"hi"` on line 3.
        let main_str = tokens
            .iter()
            .find(|t| t.line == 1 && t.token_type == SemTokenType::String as u32)
            .expect("string on description line");
        // The lex pass returns a span for the *full literal*, including the
        // surrounding quotes. Length 7 → "main.".
        assert_eq!(main_str.length, 7);
    }

    #[test]
    fn skill_name_classified_as_function_decl() {
        let src = "skill main()\n    description: \"d\"\n    flow:\n        \"hi\"\n";
        let tokens = collect_semantic_tokens(src, 0);
        let main = find_token(&tokens, 0, "main", src).expect("`main` skill name");
        assert_eq!(main.token_type, SemTokenType::Function as u32);
        assert!(main.modifiers & SemTokenModifier::DECLARATION != 0);
        assert!(main.modifiers & SemTokenModifier::DEFINITION != 0);
    }

    #[test]
    fn block_name_classified_as_method() {
        let src = "skill main()\n    description: \"d\"\n    flow:\n        run()\n\nblock run()\n    \"do it.\"\n";
        let tokens = collect_semantic_tokens(src, 0);
        // `run` on the block declaration line is the method declaration.
        let run_decl = find_token(&tokens, 5, "run", src).expect("`run` block name");
        assert_eq!(run_decl.token_type, SemTokenType::Method as u32);
        assert!(run_decl.modifiers & SemTokenModifier::DECLARATION != 0);
        // `run` in the call site (line 3) is a function reference.
        let run_call = find_token(&tokens, 3, "run", src).expect("`run` call site");
        assert_eq!(run_call.token_type, SemTokenType::Function as u32);
    }

    #[test]
    fn text_decl_is_readonly_variable() {
        let src = "skill main()\n    description: \"d\"\n    require accuracy\n    flow:\n        \"hi\"\n\ntext accuracy = \"be accurate.\"\n";
        let tokens = collect_semantic_tokens(src, 0);
        let decl = find_token(&tokens, 6, "accuracy", src).expect("text decl name");
        assert_eq!(decl.token_type, SemTokenType::Variable as u32);
        assert!(decl.modifiers & SemTokenModifier::READONLY != 0);
        assert!(decl.modifiers & SemTokenModifier::DECLARATION != 0);
        // The use site after `require` is a Variable reference (no
        // declaration modifier).
        let use_site = find_token(&tokens, 2, "accuracy", src).expect("use site");
        assert_eq!(use_site.token_type, SemTokenType::Variable as u32);
        assert_eq!(use_site.modifiers & SemTokenModifier::DECLARATION, 0);
    }

    #[test]
    fn line_comment_classified() {
        let src = "// header comment\nskill main()\n    description: \"d\"\n    flow:\n        \"hi\"\n";
        let tokens = collect_semantic_tokens(src, 0);
        let comment = tokens
            .iter()
            .find(|t| t.line == 0 && t.token_type == SemTokenType::Comment as u32)
            .expect("comment on line 0");
        assert_eq!(comment.start, 0);
        assert_eq!(comment.length, "// header comment".len() as u32);
    }

    #[test]
    fn comment_inside_string_is_not_classified() {
        let src = "skill main()\n    description: \"contains // text\"\n    flow:\n        \"hi\"\n";
        let tokens = collect_semantic_tokens(src, 0);
        // No comment token should have been emitted.
        assert!(
            tokens
                .iter()
                .all(|t| t.token_type != SemTokenType::Comment as u32),
            "no comment expected; got tokens: {:?}",
            tokens
        );
    }

    #[test]
    fn parameter_classified() {
        let src = "skill main(scope = \".\")\n    description: \"d\"\n    flow:\n        \"hi\"\n";
        let tokens = collect_semantic_tokens(src, 0);
        let scope = find_token(&tokens, 0, "scope", src).expect("param `scope`");
        assert_eq!(scope.token_type, SemTokenType::Parameter as u32);
        assert!(scope.modifiers & SemTokenModifier::DECLARATION != 0);
    }

    #[test]
    fn type_keyword_classified_as_type() {
        let src = "text greeting = \"hi\"\n";
        let tokens = collect_semantic_tokens(src, 0);
        let text_kw = find_token(&tokens, 0, "text", src).expect("`text` keyword");
        assert_eq!(text_kw.token_type, SemTokenType::Type as u32);
    }

    #[test]
    fn import_specifier_is_variable() {
        let src = "import \"./other.glyph.md\" { helper }\n";
        let tokens = collect_semantic_tokens(src, 0);
        // `import` is a keyword.
        let kw = find_token(&tokens, 0, "import", src).expect("import kw");
        assert_eq!(kw.token_type, SemTokenType::Keyword as u32);
        // `helper` is a Variable (the imported name).
        let name = find_token(&tokens, 0, "helper", src).expect("imported name");
        assert_eq!(name.token_type, SemTokenType::Variable as u32);
    }

    #[test]
    fn whole_module_import_alias_is_namespace() {
        let src = "import \"./other.glyph.md\" as helpers\n";
        let tokens = collect_semantic_tokens(src, 0);
        let alias = find_token(&tokens, 0, "helpers", src).expect("alias");
        assert_eq!(alias.token_type, SemTokenType::Namespace as u32);
        assert!(alias.modifiers & SemTokenModifier::DECLARATION != 0);
    }

    #[test]
    fn dedup_keeps_ast_classification_over_lex() {
        // `description` would be picked up by the section-label lex rule
        // because it's followed by `:`. Ensure the result is still a
        // single `description` keyword token (no duplicates from the
        // sort/dedup step).
        let src = "skill main()\n    description: \"d\"\n    flow:\n        \"hi\"\n";
        let tokens = collect_semantic_tokens(src, 0);
        let count = tokens
            .iter()
            .filter(|t| t.line == 1 && t.length == "description".len() as u32)
            .count();
        assert_eq!(count, 1, "exactly one description token; got {:?}", tokens);
    }

    #[test]
    fn tokens_sorted_by_line_then_start() {
        let src = "skill main()\n    description: \"d\"\n    flow:\n        \"hi\"\n";
        let tokens = collect_semantic_tokens(src, 0);
        for w in tokens.windows(2) {
            let (a, b) = (w[0], w[1]);
            assert!(
                (a.line, a.start) <= (b.line, b.start),
                "tokens out of order: {:?} then {:?}",
                a,
                b
            );
        }
    }
}
