//! Semantic-token collector.
//!
//! Walks the lex stream + parsed AST to produce a sorted, dense list of
//! `(line, col, length, token-type, modifiers)` tuples. The LSP layer
//! delta-encodes these into the LSP `textDocument/semanticTokens/full`
//! reply. Token types and modifiers are mapped onto the LSP standard set
//! (Keyword, Function, Method, Variable, …) plus a Glyph-specific set
//! (GlyphFlowString, GlyphContextString, GlyphBlockCall, …) introduced by
//! issue #93 so VS Code can ship default colors that visually distinguish
//! the four core Glyph primitives — `flow:` content, `context:` content,
//! `text` declarations, and `block` / `export block` declarations.
//!
//! # Coordinate system
//!
//! Each `RawSemToken` carries:
//! - `line`: 0-indexed line number (LSP convention).
//! - `start`: 0-indexed byte column from the start of the line.
//!   Glyph rejects tabs and non-ASCII identifiers, so byte == utf-16 code
//!   unit for everything except string-literal contents. Pure-ASCII source
//!   is exact; string literals containing non-ASCII may produce slightly
//!   off LSP ranges.
//! - `length`: byte length of the token.
//! - `token_type`: index into [`SemTokenType::legend`].
//! - `modifiers`: bitset over [`SemTokenModifier::legend`].
//!
//! # Strategy
//!
//! Three passes that produce candidate tokens, then sort + dedup by start
//! position (last-write-wins for exact overlaps so the AST pass refines
//! the lex pass):
//!
//! 1. **Lex pass** — re-tokenize the source. Track current
//!    section state (None / Description / Context / Constraints / Flow /
//!    Effects) by watching for `<label>:` tokens at indent 1 and
//!    top-level decl keywords at indent 0. Classify identifiers (hard
//!    keywords, types, section labels with per-section `Glyph*` types)
//!    and string literals (Glyph flow / context strings vs plain).
//!    Inside flow strings, scan for `{name}` interpolations.
//! 2. **Comment pass** — scan source bytes for `//` comments (the lexer
//!    drops them), emit [`SemTokenType::Comment`] for each.
//! 3. **AST pass** — parse the source (errors are silenced — partial AST
//!    is fine here). For each declaration, classify the bound name plus
//!    any references reachable from the declaration body. Flow `Call`
//!    sites are emitted as `GlyphBlockCall` when the target is a same-file
//!    `block` / `export block` decl, otherwise `GlyphFlowCall`. Context
//!    bare-name references are emitted as `GlyphContextNameRef`.

use crate::ast::{
    BlockDecl, ConstDecl, ConstValue, ContextEntry, Decl, ExportBlockDecl, FlowStmt, ImportDecl,
    ImportKind, ReturnExpr, Skill, SourceFile,
};
use crate::condition::tokenize_condition;
use crate::diagnostic::DiagBag;
use crate::parse::parse_with_diagnostics_opts;
use crate::span::{LineIndex, Span, Spanned};
use crate::tokenize::{tokenize, Token, TokenKind};
use std::collections::HashSet;

/// LSP semantic token types we emit. Index in the legend == enum
/// discriminant. The first 11 are LSP-standard names; the remainder
/// (`Glyph*`) are issue-#93 additions that the VS Code extension defaults
/// colors for and the tree-sitter grammar mirrors as
/// `@glyph.<area>.<role>` captures.
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
    /// Inline string literal inside a `flow:` body. Loud / saturated by
    /// default in VS Code so the instructional payload stands out.
    GlyphFlowString = 11,
    /// Inline string literal inside a `context:` body. Muted-italic by
    /// default so background knowledge reads quieter than flow content.
    GlyphContextString = 12,
    /// Call site inside `flow:` whose target is *not* a same-file `block`
    /// or `export block` (e.g. stdlib calls, or unresolved names).
    GlyphFlowCall = 13,
    /// Call site inside `flow:` whose target *is* a same-file `block`
    /// or `export block` declaration.
    GlyphBlockCall = 14,
    /// Bare-name reference inside `context:` (or a body-level `context`
    /// marker) — typically points to a `text` declaration.
    GlyphContextNameRef = 15,
    /// `description:` section header.
    GlyphSectionDescription = 16,
    /// `context:` section header.
    GlyphSectionContext = 17,
    /// `constraints:` section header.
    GlyphSectionConstraints = 18,
    /// `flow:` section header.
    GlyphSectionFlow = 19,
    /// `{name}` interpolation slot inside a flow inline string.
    GlyphInterpolation = 20,
    /// A predicate token in an `if`/`elif` condition position. Covers all
    /// three predicate forms: `BLOCKNAME.applies()`, a bare string-kinded
    /// `const` name, and an inline string literal `"..."`. Distinct from
    /// `GlyphFlowString` (instructional payload) — predicates are logic
    /// guards, not prose emitted to the agent.
    GlyphPredicate = 21,
}

impl SemTokenType {
    /// Names in the LSP `semanticTokensProvider.legend.tokenTypes` array.
    pub const fn legend() -> [&'static str; 22] {
        [
            "keyword",                 // 0
            "type",                    // 1
            "function",                // 2
            "method",                  // 3
            "parameter",               // 4
            "variable",                // 5
            "property",                // 6
            "string",                  // 7
            "namespace",               // 8
            "number",                  // 9
            "comment",                 // 10
            "glyphFlowString",         // 11
            "glyphContextString",      // 12
            "glyphFlowCall",           // 13
            "glyphBlockCall",          // 14
            "glyphContextNameRef",     // 15
            "glyphSectionDescription", // 16
            "glyphSectionContext",     // 17
            "glyphSectionConstraints", // 18
            "glyphSectionFlow",        // 19
            "glyphInterpolation",      // 20
            "glyphPredicate",          // 21
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

    // Pass 1: lex tokens (section-aware).
    let lex_tokens = tokenize(source, file_id)
        .map(|(t, _)| t)
        .unwrap_or_default();
    classify_lex_tokens(source, &lex_tokens, &line_index, &mut tokens);

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
        let block_names = collect_block_decl_names(&ast);
        classify_ast(
            &ast,
            &block_names,
            source,
            file_id,
            &line_index,
            &mut tokens,
        );
    }

    sort_and_dedup(&mut tokens);
    tokens
}

// -- Lex pass -------------------------------------------------------------

/// Currently active section in the lex pass — drives string-literal
/// classification and interpolation scanning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    None,
    Description,
    Context,
    Constraints,
    Flow,
    Effects,
}

fn classify_lex_tokens(
    source: &str,
    lex_tokens: &[Token],
    line_index: &LineIndex,
    out: &mut Vec<RawSemToken>,
) {
    let mut section = Section::None;
    let mut current_indent: u32 = 0;

    for (i, tok) in lex_tokens.iter().enumerate() {
        match &tok.kind {
            TokenKind::LineStart { indent } => {
                current_indent = *indent;
            }
            TokenKind::Ident(text) => {
                // Top-level decl keyword at indent 0 → reset section.
                if current_indent == 0 && is_top_level_decl_keyword(text.as_str()) {
                    section = Section::None;
                }
                // Section labels: only when followed by `:`.
                let next_is_colon = lex_tokens
                    .get(i + 1)
                    .map(|t| matches!(t.kind, TokenKind::Colon))
                    .unwrap_or(false);
                if next_is_colon {
                    if let Some((sec, ttype)) = section_for_label(text.as_str()) {
                        section = sec;
                        push_span(out, tok.span, ttype, SemTokenModifier::NONE, line_index);
                        continue;
                    }
                }
                // Hard keywords / type keywords.
                if let Some(ttype) = classify_keyword_text(text.as_str()) {
                    push_span(out, tok.span, ttype, SemTokenModifier::NONE, line_index);
                }
            }
            TokenKind::StringLit(_) => {
                let ttype = match section {
                    Section::Flow => SemTokenType::GlyphFlowString,
                    Section::Context => SemTokenType::GlyphContextString,
                    _ => SemTokenType::String,
                };
                push_span(out, tok.span, ttype, SemTokenModifier::NONE, line_index);
                if section == Section::Flow {
                    classify_interpolations(source, tok.span, line_index, out);
                }
            }
            _ => {}
        }
    }
}

/// Top-level declaration keywords that reset section state when seen at
/// indent 0.
fn is_top_level_decl_keyword(s: &str) -> bool {
    matches!(
        s,
        "skill" | "block" | "export" | "generated" | "const" | "import"
    )
}

/// Hard keywords + builtin types — always classified, regardless of
/// surrounding tokens. `description`, `context`, `constraints`, `flow`,
/// `effects` are *not* here; they're handled by `section_for_label` when
/// followed by `:`. `context` falls through to keyword when not followed
/// by `:` (e.g. body-level `context name_ref`).
fn classify_keyword_text(s: &str) -> Option<SemTokenType> {
    match s {
        // Declaration / control flow / logical operators / markers.
        "skill" | "block" | "const" | "export" | "generated" | "import" | "as" | "return"
        | "if" | "elif" | "else" | "and" | "or" | "not" | "require" | "avoid" | "must"
        | "context" | "with" | "none" => Some(SemTokenType::Keyword),
        // Builtin types.
        "int" | "float" => Some(SemTokenType::Type),
        _ => None,
    }
}

/// Map a section-label identifier to its `(section, header-token-type)`
/// pair. Only valid when the identifier is immediately followed by `:`.
fn section_for_label(s: &str) -> Option<(Section, SemTokenType)> {
    match s {
        "description" => Some((Section::Description, SemTokenType::GlyphSectionDescription)),
        "context" => Some((Section::Context, SemTokenType::GlyphSectionContext)),
        "constraints" => Some((Section::Constraints, SemTokenType::GlyphSectionConstraints)),
        "flow" => Some((Section::Flow, SemTokenType::GlyphSectionFlow)),
        // `effects:` keeps the plain Keyword classification — PRD #93 does
        // not single it out for a per-section color.
        "effects" => Some((Section::Effects, SemTokenType::Keyword)),
        _ => None,
    }
}

/// Scan inside a flow-string literal span for `{name}` interpolation
/// slots and emit a `GlyphInterpolation` token for each (covering the
/// braces + the inner identifier).
///
/// `string_span` covers the whole literal *including* the surrounding
/// quotes — we skip the open quote, search forward, and bail at the close
/// quote.
fn classify_interpolations(
    source: &str,
    string_span: Span,
    line_index: &LineIndex,
    out: &mut Vec<RawSemToken>,
) {
    // String literals always include quotes in their span; skip them.
    let s = string_span.start as usize + 1;
    let e = (string_span.end as usize)
        .saturating_sub(1)
        .min(source.len());
    if e <= s {
        return;
    }
    let bytes = source.as_bytes();
    let mut p = s;
    while p < e {
        if bytes[p] == b'{' {
            // Find matching `}` within the string, but only accept a
            // simple `{ident}` shape.
            let inner_start = p + 1;
            let mut q = inner_start;
            while q < e && bytes[q] != b'}' {
                q += 1;
            }
            if q < e && bytes[q] == b'}' {
                // Validate inner is a non-empty identifier (allow leading
                // whitespace tolerance to keep this lenient).
                let inner = &source[inner_start..q];
                let trimmed = inner.trim();
                if !trimmed.is_empty()
                    && trimmed
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    && trimmed
                        .bytes()
                        .next()
                        .map(|b| b.is_ascii_alphabetic() || b == b'_')
                        .unwrap_or(false)
                {
                    let span = Span::new(string_span.file_id, p as u32, (q + 1) as u32);
                    push_span(
                        out,
                        span,
                        SemTokenType::GlyphInterpolation,
                        SemTokenModifier::NONE,
                        line_index,
                    );
                }
                p = q + 1;
                continue;
            }
        }
        p += 1;
    }
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
            push_span(
                out,
                span,
                SemTokenType::Comment,
                SemTokenModifier::NONE,
                line_index,
            );
            p = q;
            continue;
        }
        p += 1;
    }
}

// -- AST pass -------------------------------------------------------------

/// Collect the names of every same-file `block` / `export block`
/// declaration. Flow `Call` sites whose target matches a name in this set
/// are classified as `GlyphBlockCall` (vs `GlyphFlowCall` for everything
/// else). Cross-file imported blocks are *not* included — keeping this
/// local avoids an analyze pass dependency.
fn collect_block_decl_names(ast: &SourceFile) -> HashSet<String> {
    let mut names = HashSet::new();
    for decl in &ast.decls {
        match decl {
            Decl::Block(b) => {
                names.insert(b.node.name.clone());
            }
            Decl::ExportBlock(eb) => {
                names.insert(eb.node.name.clone());
            }
            _ => {}
        }
    }
    names
}

/// Collect the names of every same-file **string-bodied** `const`
/// declaration. Used to identify bare-name predicate tokens in `if`/`elif`
/// conditions — only string-kinded consts qualify per the design spec
/// (§Condition Expressions / Predicate forms), so bool / int / float consts
/// are intentionally excluded. A boolean const in condition position is a
/// boolean condition, not a predicate, and must NOT receive the
/// `GlyphPredicate` highlight.
fn collect_const_decl_names(ast: &SourceFile) -> HashSet<String> {
    let mut names = HashSet::new();
    for decl in &ast.decls {
        if let Decl::Const(c) = decl {
            if matches!(c.node.value, ConstValue::String(_)) {
                names.insert(c.node.name.clone());
            }
        }
    }
    names
}

fn classify_ast(
    ast: &SourceFile,
    block_names: &HashSet<String>,
    source: &str,
    file_id: u32,
    line_index: &LineIndex,
    out: &mut Vec<RawSemToken>,
) {
    let const_names = collect_const_decl_names(ast);
    for decl in &ast.decls {
        match decl {
            Decl::Skill(s) => classify_skill(s, block_names, &const_names, source, file_id, line_index, out),
            Decl::Block(b) => classify_block(b, block_names, &const_names, source, file_id, line_index, out),
            Decl::ExportBlock(eb) => classify_export_block(eb, source, file_id, line_index, out),
            Decl::Const(c) => classify_const(c, source, file_id, line_index, out),
            Decl::Import(i) => classify_import(i, source, file_id, line_index, out),
        }
    }
}

fn classify_skill(
    spanned: &Spanned<Skill>,
    block_names: &HashSet<String>,
    const_names: &HashSet<String>,
    source: &str,
    file_id: u32,
    line_index: &LineIndex,
    out: &mut Vec<RawSemToken>,
) {
    if let Some(name_span) = find_name_after_keywords(source, file_id, spanned.span, &["skill"]) {
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
    // body_bare_names are plain Strings without span info (M2 upgrade pending);
    // skip semantic token emission for them.
    for stmt in &spanned.node.flow {
        classify_flow_stmt(stmt, block_names, const_names, source, file_id, line_index, out);
    }
}

fn classify_block(
    spanned: &Spanned<BlockDecl>,
    block_names: &HashSet<String>,
    const_names: &HashSet<String>,
    source: &str,
    file_id: u32,
    line_index: &LineIndex,
    out: &mut Vec<RawSemToken>,
) {
    if let Some(name_span) = find_name_after_keywords(source, file_id, spanned.span, &["block"]) {
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
        classify_flow_stmt(stmt, block_names, const_names, source, file_id, line_index, out);
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
    // ExportBlockDecl doesn't carry FlowStmt — `body_refs` is a flat list
    // of identifier strings without spans, so we don't get call-site
    // classifications here. Acceptable for the highlight pass.
}

fn classify_const(
    spanned: &Spanned<ConstDecl>,
    source: &str,
    file_id: u32,
    line_index: &LineIndex,
    out: &mut Vec<RawSemToken>,
) {
    let kws: &[&str] = if spanned.node.generated {
        &["generated", "const"]
    } else if spanned.node.exported {
        &["export", "const"]
    } else {
        &["const"]
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
                    if let Some(alias_span) =
                        find_alias_span(source, file_id, n.name.span.end, spanned.span.end, alias)
                    {
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
            if let Some(alias_span) =
                find_alias_span(source, file_id, spanned.span.start, spanned.span.end, alias)
            {
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
        push_span(
            out,
            cm.name.span,
            SemTokenType::Variable,
            SemTokenModifier::NONE,
            line_index,
        );
    }
}

fn classify_context_entries(
    entries: &[ContextEntry],
    line_index: &LineIndex,
    out: &mut Vec<RawSemToken>,
) {
    for entry in entries {
        if let ContextEntry::NameRef(n) = entry {
            push_span(
                out,
                n.span,
                SemTokenType::GlyphContextNameRef,
                SemTokenModifier::NONE,
                line_index,
            );
        }
    }
}

fn classify_flow_stmt(
    stmt: &FlowStmt,
    block_names: &HashSet<String>,
    const_names: &HashSet<String>,
    source: &str,
    file_id: u32,
    line_index: &LineIndex,
    out: &mut Vec<RawSemToken>,
) {
    match stmt {
        FlowStmt::Call { target, .. } => {
            let ttype = if block_names.contains(&target.node) {
                SemTokenType::GlyphBlockCall
            } else {
                SemTokenType::GlyphFlowCall
            };
            push_span(out, target.span, ttype, SemTokenModifier::NONE, line_index);
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
            push_span(
                out,
                n.span,
                SemTokenType::GlyphContextNameRef,
                SemTokenModifier::NONE,
                line_index,
            );
        }
        FlowStmt::ContextMarker(ContextEntry::InlineString(_)) => {}
        FlowStmt::BareName(n) => {
            push_span(
                out,
                n.span,
                SemTokenType::Variable,
                SemTokenModifier::NONE,
                line_index,
            );
        }
        FlowStmt::Return(ReturnExpr::Call { target, .. }) => {
            let ttype = if block_names.contains(&target.node) {
                SemTokenType::GlyphBlockCall
            } else {
                SemTokenType::GlyphFlowCall
            };
            push_span(out, target.span, ttype, SemTokenModifier::NONE, line_index);
        }
        FlowStmt::Return(ReturnExpr::Name(n)) => {
            push_span(
                out,
                n.span,
                SemTokenType::Variable,
                SemTokenModifier::NONE,
                line_index,
            );
        }
        FlowStmt::Return(_) => {}
        FlowStmt::Branch {
            condition,
            then_body,
            elif_branches,
            else_body,
            ..
        } => {
            // Emit GlyphPredicate tokens for predicate-form `if` conditions.
            emit_predicate_tokens(
                source,
                file_id,
                condition,
                const_names,
                "if",
                line_index,
                out,
            );
            for s in then_body {
                classify_flow_stmt(s, block_names, const_names, source, file_id, line_index, out);
            }
            for elif in elif_branches {
                // Emit GlyphPredicate tokens for predicate-form `elif` conditions.
                emit_predicate_tokens(
                    source,
                    file_id,
                    &elif.condition,
                    const_names,
                    "elif",
                    line_index,
                    out,
                );
                for s in &elif.body {
                    classify_flow_stmt(s, block_names, const_names, source, file_id, line_index, out);
                }
            }
            if let Some(eb) = else_body {
                for s in eb {
                    classify_flow_stmt(s, block_names, const_names, source, file_id, line_index, out);
                }
            }
        }
        FlowStmt::InlineString(_) => {}
    }
}

// -- Predicate span resolution -------------------------------------------

/// Scan `source` for all occurrences of `<kw> <condition_text>:` (where `kw`
/// is `"if"` or `"elif"`), then — for each occurrence — re-tokenize the
/// condition text and emit a [`SemTokenType::GlyphPredicate`] token for each
/// predicate-kind token using a syntactic-only classifier:
///
/// - Token that starts with `"` → `PredicateLiteral`
/// - Token that contains `.applies()` → `PredicateApplies`
/// - Token that matches a name in `const_names` → `PredicateConst`
/// - Everything else → not a predicate (skipped)
///
/// **Why scan all occurrences?** The `FlowStmt::Branch` AST node does not
/// carry a source span for itself or its condition expression. Scanning every
/// matching occurrence handles files where the same condition text appears in
/// multiple branches (each position gets highlighted correctly). The
/// `sort_and_dedup` pass removes genuine duplicates.
///
/// **Why syntactic-only?** The semantic token collector runs only the parse
/// pass, not the analyze pass, so `ConditionClassification` slots in the AST
/// are always `None` here. The `const_names` set passed in is restricted by
/// `collect_const_decl_names` to *string-bodied* consts only, so bool / int /
/// float consts in condition position correctly do NOT get highlighted as
/// predicates (per spec §3 — only string-kinded consts are predicate forms).
fn emit_predicate_tokens(
    source: &str,
    file_id: u32,
    condition_text: &str,
    const_names: &HashSet<String>,
    kw: &str, // "if" or "elif"
    line_index: &LineIndex,
    out: &mut Vec<RawSemToken>,
) {
    // Strip trailing ` :` that the parser may append to condition strings.
    let condition_text = condition_text.trim().trim_end_matches(':').trim_end();
    if condition_text.is_empty() {
        return;
    }

    let condition_tokens = tokenize_condition(condition_text);

    // Quick exit: nothing to highlight if none of the tokens are predicate-like.
    let has_predicate = condition_tokens.iter().any(|ct| {
        ct.starts_with('"') || ct.contains(".applies()") || const_names.contains(ct.as_str())
    });
    if !has_predicate {
        return;
    }

    let bytes = source.as_bytes();
    let src_len = bytes.len();
    let kw_bytes = kw.as_bytes();
    let kw_len = kw.len();
    let cond_bytes = condition_text.as_bytes();

    let mut search_pos = 0usize;
    while search_pos + kw_len <= src_len {
        // Match the keyword as a standalone word.
        if &bytes[search_pos..search_pos + kw_len] != kw_bytes {
            search_pos += 1;
            continue;
        }
        let preceded_ok = search_pos == 0 || !is_ident_continue(bytes[search_pos - 1]);
        let followed_ok = search_pos + kw_len >= src_len
            || !is_ident_continue(bytes[search_pos + kw_len]);
        if !preceded_ok || !followed_ok {
            search_pos += 1;
            continue;
        }

        // Skip whitespace after the keyword.
        let mut p = search_pos + kw_len;
        while p < src_len && (bytes[p] == b' ' || bytes[p] == b'\t') {
            p += 1;
        }
        let cond_start = p;

        // Check verbatim match of condition text.
        if cond_start + cond_bytes.len() > src_len
            || &bytes[cond_start..cond_start + cond_bytes.len()] != cond_bytes
        {
            search_pos += 1;
            continue;
        }
        // Verify the byte right after the condition text is a terminator.
        let after = cond_start + cond_bytes.len();
        if after < src_len
            && bytes[after] != b':'
            && bytes[after] != b' '
            && bytes[after] != b'\t'
            && bytes[after] != b'\n'
        {
            search_pos += 1;
            continue;
        }

        // Walk condition_tokens, tracking byte offset inside condition_text.
        let mut tok_offset = 0usize;
        for ct in &condition_tokens {
            // Skip whitespace to reach this token's start in condition_text.
            while tok_offset < cond_bytes.len()
                && (cond_bytes[tok_offset] == b' ' || cond_bytes[tok_offset] == b'\t')
            {
                tok_offset += 1;
            }
            let tok_len = ct.len();
            if tok_offset + tok_len > cond_bytes.len() {
                break;
            }

            let is_predicate = ct.starts_with('"')
                || ct.contains(".applies()")
                || const_names.contains(ct.as_str());
            if is_predicate {
                let abs_start = (cond_start + tok_offset) as u32;
                let abs_end = abs_start + tok_len as u32;
                let span = Span::new(file_id, abs_start, abs_end);
                push_span(
                    out,
                    span,
                    SemTokenType::GlyphPredicate,
                    SemTokenModifier::NONE,
                    line_index,
                );
            }
            tok_offset += tok_len;
        }

        search_pos = cond_start + cond_bytes.len();
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
    let (eline, _) = line_index.line_col(span.end.saturating_sub(1));

    if sline == eline {
        out.push(RawSemToken {
            line: sline.saturating_sub(1),
            start: scol.saturating_sub(1),
            length: span.end - span.start,
            token_type: ttype as u32,
            modifiers,
        });
        return;
    }

    // Multi-line span (e.g. triple-quoted block strings): split into one
    // token per line so VS Code doesn't silently drop the whole token.
    let ttype_u32 = ttype as u32;
    for l in sline..=eline {
        let line_start = line_index.byte_offset(l, 1);
        let line_end_excl = line_index.byte_offset(l + 1, 1); // start of next line (after \n)
        let seg_start = if l == sline { span.start } else { line_start };
        let seg_end = if l == eline {
            span.end
        } else {
            // exclude the \n itself
            line_end_excl.saturating_sub(1)
        };
        if seg_end <= seg_start {
            continue;
        }
        let col = if l == sline {
            scol.saturating_sub(1)
        } else {
            0
        };
        out.push(RawSemToken {
            line: l.saturating_sub(1),
            start: col,
            length: seg_end - seg_start,
            token_type: ttype_u32,
            modifiers,
        });
    }
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
                    return Some(Span::new(file_id, (s + alias_start) as u32, (s + q) as u32));
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
        assert_eq!(SemTokenType::legend().len(), 22);
        assert_eq!(SemTokenModifier::legend().len(), 4);
    }

    #[test]
    fn keyword_and_string_classification() {
        let src = "skill main()\n    description: \"main.\"\n    flow:\n        \"hi\"\n";
        let tokens = collect_semantic_tokens(src, 0);
        let skill = find_token(&tokens, 0, "skill", src).expect("`skill` keyword");
        assert_eq!(skill.token_type, SemTokenType::Keyword as u32);
        // `description:` is now its own per-section header type.
        let desc = find_token(&tokens, 1, "description", src).expect("`description:` header");
        assert_eq!(
            desc.token_type,
            SemTokenType::GlyphSectionDescription as u32
        );
        // `flow:` is its own per-section header type.
        let flow = find_token(&tokens, 2, "flow", src).expect("`flow:` header");
        assert_eq!(flow.token_type, SemTokenType::GlyphSectionFlow as u32);
        // The description body string is plain String (not flow/context).
        let main_str = tokens
            .iter()
            .find(|t| t.line == 1 && t.token_type == SemTokenType::String as u32)
            .expect("string on description line");
        assert_eq!(main_str.length, 7);
    }

    #[test]
    fn flow_inline_string_is_glyph_flow_string() {
        let src = "skill main()\n    description: \"d\"\n    flow:\n        \"do work\"\n";
        let tokens = collect_semantic_tokens(src, 0);
        let s = tokens
            .iter()
            .find(|t| t.line == 3 && t.token_type == SemTokenType::GlyphFlowString as u32)
            .expect("flow string");
        assert_eq!(s.length, "\"do work\"".len() as u32);
        // And it is *not* classified as plain String.
        let plain_strs = tokens
            .iter()
            .filter(|t| t.line == 3 && t.token_type == SemTokenType::String as u32)
            .count();
        assert_eq!(plain_strs, 0, "no plain String on the flow line");
    }

    #[test]
    fn context_inline_string_is_glyph_context_string() {
        let src = "skill main()\n    description: \"d\"\n    context:\n        \"background note\"\n    flow:\n        \"act\"\n";
        let tokens = collect_semantic_tokens(src, 0);
        let s = tokens
            .iter()
            .find(|t| t.line == 3 && t.token_type == SemTokenType::GlyphContextString as u32)
            .expect("context string");
        assert_eq!(s.length, "\"background note\"".len() as u32);
        // And it is *not* classified as a flow string.
        assert!(
            !tokens
                .iter()
                .any(|t| t.line == 3 && t.token_type == SemTokenType::GlyphFlowString as u32),
            "context string must not be flow-classified",
        );
    }

    #[test]
    fn context_name_ref_in_context_section() {
        let src = "skill main()\n    description: \"d\"\n    context:\n        project_conventions\n    flow:\n        \"hi\"\n\nconst project_conventions = \"use kebab-case.\"\n";
        let tokens = collect_semantic_tokens(src, 0);
        let nref = find_token(&tokens, 3, "project_conventions", src).expect("context name ref");
        assert_eq!(nref.token_type, SemTokenType::GlyphContextNameRef as u32);
    }

    #[test]
    fn block_call_distinguished_from_flow_call() {
        let src = "skill main()\n    description: \"d\"\n    flow:\n        run_block()\n        send(\"x\")\n\nblock run_block()\n    \"do it.\"\n";
        let tokens = collect_semantic_tokens(src, 0);
        let block_call = find_token(&tokens, 3, "run_block", src).expect("`run_block` call");
        assert_eq!(block_call.token_type, SemTokenType::GlyphBlockCall as u32);
        // `send` is a stdlib call (no same-file `block send`), so it gets the
        // generic flow-call type.
        let stdlib_call = find_token(&tokens, 4, "send", src).expect("`send` call");
        assert_eq!(stdlib_call.token_type, SemTokenType::GlyphFlowCall as u32);
    }

    #[test]
    fn each_section_header_has_its_own_type() {
        let src = "skill main()\n    description: \"d\"\n    context:\n        \"k\"\n    constraints:\n        require accuracy\n    flow:\n        \"hi\"\n\ntext accuracy = \"a\"\n";
        let tokens = collect_semantic_tokens(src, 0);
        let desc = find_token(&tokens, 1, "description", src).expect("description");
        assert_eq!(
            desc.token_type,
            SemTokenType::GlyphSectionDescription as u32
        );
        let ctx = find_token(&tokens, 2, "context", src).expect("context");
        assert_eq!(ctx.token_type, SemTokenType::GlyphSectionContext as u32);
        let cons = find_token(&tokens, 4, "constraints", src).expect("constraints");
        assert_eq!(
            cons.token_type,
            SemTokenType::GlyphSectionConstraints as u32
        );
        let flow = find_token(&tokens, 6, "flow", src).expect("flow");
        assert_eq!(flow.token_type, SemTokenType::GlyphSectionFlow as u32);
    }

    #[test]
    fn flow_string_interpolation_classified() {
        let src = "skill main(scope = \".\")\n    description: \"d\"\n    flow:\n        \"inspect {scope} now\"\n";
        let tokens = collect_semantic_tokens(src, 0);
        let interp = tokens
            .iter()
            .find(|t| t.line == 3 && t.token_type == SemTokenType::GlyphInterpolation as u32)
            .expect("interpolation");
        // `{scope}` is 7 bytes including braces.
        assert_eq!(interp.length, "{scope}".len() as u32);
    }

    #[test]
    fn context_keyword_without_colon_stays_keyword() {
        // Body-level `context name_ref` (no `:`) — `context` should stay
        // a plain keyword (not GlyphSectionContext).
        let src = "skill main()\n    description: \"d\"\n    context project_conventions\n    flow:\n        \"hi\"\n\ntext project_conventions = \"x\"\n";
        let tokens = collect_semantic_tokens(src, 0);
        let ctx = find_token(&tokens, 2, "context", src).expect("context kw");
        assert_eq!(ctx.token_type, SemTokenType::Keyword as u32);
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
        // `run` in the call site (line 3) is now a Glyph block call.
        let run_call = find_token(&tokens, 3, "run", src).expect("`run` call site");
        assert_eq!(run_call.token_type, SemTokenType::GlyphBlockCall as u32);
    }

    #[test]
    fn text_decl_is_readonly_variable() {
        let src = "skill main()\n    description: \"d\"\n    require accuracy\n    flow:\n        \"hi\"\n\nconst accuracy = \"be accurate.\"\n";
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
        let src =
            "// header comment\nskill main()\n    description: \"d\"\n    flow:\n        \"hi\"\n";
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
        let src =
            "skill main()\n    description: \"contains // text\"\n    flow:\n        \"hi\"\n";
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
    fn import_specifier_is_variable() {
        let src = "import \"./other.glyph\" { helper }\n";
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
        let src = "import \"./other.glyph\" as helpers\n";
        let tokens = collect_semantic_tokens(src, 0);
        let alias = find_token(&tokens, 0, "helpers", src).expect("alias");
        assert_eq!(alias.token_type, SemTokenType::Namespace as u32);
        assert!(alias.modifiers & SemTokenModifier::DECLARATION != 0);
    }

    #[test]
    fn dedup_keeps_ast_classification_over_lex() {
        // `description` would be picked up by the section-label lex rule
        // because it's followed by `:`. Ensure the result is still a
        // single `description` token (no duplicates from the
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
    fn triple_quoted_string_splits_into_per_line_tokens() {
        // A multi-line block string must produce one token per source line so
        // VS Code doesn't silently drop the single over-long token.
        let src =
            "skill s()\n    flow:\n        \"\"\"\n        hello\n        world\n        \"\"\"\n";
        let tokens = collect_semantic_tokens(src, 0);
        // All tokens must live on a single line (length never crosses a \n).
        for t in &tokens {
            assert!(
                t.length < 200,
                "suspiciously large token — probably not split: {:?}",
                t
            );
        }
        // The four source lines of the triple-quoted string (opening """,
        // content line 1, content line 2, closing """) must each produce a
        // GlyphFlowString token.
        let flow_string_type = SemTokenType::GlyphFlowString as u32;
        let flow_lines: Vec<u32> = tokens
            .iter()
            .filter(|t| t.token_type == flow_string_type)
            .map(|t| t.line)
            .collect();
        // Lines 2,3,4,5 (0-indexed) should all have a flow-string token.
        assert!(
            flow_lines.contains(&2),
            "line 2 (opening \"\"\") missing: {:?}",
            flow_lines
        );
        assert!(
            flow_lines.contains(&3),
            "line 3 (hello) missing: {:?}",
            flow_lines
        );
        assert!(
            flow_lines.contains(&4),
            "line 4 (world) missing: {:?}",
            flow_lines
        );
        assert!(
            flow_lines.contains(&5),
            "line 5 (closing \"\"\") missing: {:?}",
            flow_lines
        );
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

    // -- Predicate-form highlighting tests (Task 5.4) ---------------------

    #[test]
    fn predicate_const_form_emits_glyph_predicate() {
        // `if my_const:` — bare const name in condition position.
        // `my_const` is a same-file const decl, so it is identified as a
        // predicate token and gets GlyphPredicate highlighting.
        let src = concat!(
            "skill main()\n",
            "    description: \"d\"\n",
            "    flow:\n",
            "        if my_const:\n",
            "            \"do it\"\n",
            "\n",
            "const my_const = \"the user opted in\"\n",
        );
        let tokens = collect_semantic_tokens(src, 0);
        let pred = find_token(&tokens, 3, "my_const", src)
            .expect("`my_const` GlyphPredicate token on if line");
        assert_eq!(
            pred.token_type,
            SemTokenType::GlyphPredicate as u32,
            "expected GlyphPredicate for bare const name in condition; got token: {:?}",
            pred
        );
    }

    #[test]
    fn predicate_literal_form_emits_glyph_predicate() {
        // `if "the user opted in":` — inline string literal in condition position.
        let src = concat!(
            "skill main()\n",
            "    description: \"d\"\n",
            "    flow:\n",
            "        if \"the user opted in\":\n",
            "            \"do it\"\n",
        );
        let tokens = collect_semantic_tokens(src, 0);
        let pred = find_token(&tokens, 3, "\"the user opted in\"", src)
            .expect("`\"the user opted in\"` GlyphPredicate token on if line");
        assert_eq!(
            pred.token_type,
            SemTokenType::GlyphPredicate as u32,
            "expected GlyphPredicate for string literal in condition; got token: {:?}",
            pred
        );
    }

    #[test]
    fn predicate_applies_form_emits_glyph_predicate() {
        // `if my_block.applies():` — BLOCKNAME.applies() in condition position.
        // The entire `my_block.applies()` span is highlighted as GlyphPredicate.
        let src = concat!(
            "skill main()\n",
            "    description: \"d\"\n",
            "    flow:\n",
            "        if my_block.applies():\n",
            "            \"do it\"\n",
            "\n",
            "block my_block()\n",
            "    \"step\"\n",
        );
        let tokens = collect_semantic_tokens(src, 0);
        let pred = find_token(&tokens, 3, "my_block.applies()", src)
            .expect("`my_block.applies()` GlyphPredicate token on if line");
        assert_eq!(
            pred.token_type,
            SemTokenType::GlyphPredicate as u32,
            "expected GlyphPredicate for applies() form in condition; got token: {:?}",
            pred
        );
    }

    #[test]
    fn predicate_const_form_skips_bool_consts() {
        // `const flag = true` (bool body) — `if flag:` must NOT highlight `flag`
        // as GlyphPredicate. Predicate highlighting is reserved for
        // string-kinded consts per spec §3; a boolean const in condition
        // position is a boolean condition, not a predicate.
        let src = concat!(
            "skill main()\n",
            "    description: \"d\"\n",
            "    flow:\n",
            "        if flag:\n",
            "            \"do it\"\n",
            "\n",
            "const flag = true\n",
        );
        let tokens = collect_semantic_tokens(src, 0);
        let predicate_tokens = tokens
            .iter()
            .filter(|t| t.token_type == SemTokenType::GlyphPredicate as u32)
            .count();
        assert_eq!(
            predicate_tokens, 0,
            "bool const must not be highlighted as predicate; got tokens: {:?}",
            tokens
        );
    }

    #[test]
    fn predicate_literal_in_elif_emits_glyph_predicate() {
        // String literal in an `elif` condition — must be highlighted as
        // GlyphPredicate symmetrically with the `if` arm.
        let src = concat!(
            "skill main()\n",
            "    description: \"d\"\n",
            "    flow:\n",
            "        if true:\n",
            "            \"a\"\n",
            "        elif \"the user opted out\":\n",
            "            \"b\"\n",
        );
        let tokens = collect_semantic_tokens(src, 0);
        let predicate_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == SemTokenType::GlyphPredicate as u32)
            .collect();
        assert_eq!(
            predicate_tokens.len(),
            1,
            "elif literal predicate should be highlighted; got tokens: {:?}",
            predicate_tokens
        );
    }
}
