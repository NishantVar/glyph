//! Diagnostic conversion: `glyph_core::diagnostic::Diagnostic` → `lsp_types::Diagnostic`.
//!
//! The mapping is specified in `design/glyph-lsp.md` §6. Two non-obvious
//! pieces:
//!
//! 1. **Span coordinate conversion (`design/glyph-lsp.md` §10.B).**
//!    `glyph-core` emits 1-indexed (line, col) with **inclusive end** semantics
//!    (per `docs/reference/diagnostics.md` §Span Semantics — a single-character span
//!    has `start == end`). LSP wants 0-indexed (line, character) with
//!    **exclusive end**. The `start` mapping subtracts 1 from both fields. The
//!    `end` mapping subtracts 1 from `line` (0-indexed) but **keeps `col` as-is**
//!    — the `-1` for 0-indexed conversion and the `+1` for inclusive→exclusive
//!    cancel.
//!
//! 2. **Severity mapping.** Glyph's `repairable` tier means "the agent will
//!    likely fix this" — analogous to a clippy lint that a tool can address.
//!    We map it to LSP `Warning` to match the mental model: not a hard error,
//!    but actionable. (The pretty-printer in `glyph-cli/src/main.rs` makes the
//!    same call.) Glyph `warning` (advisory, build still passes) maps to LSP
//!    `Information`.
//!
//! 3. **Hints.** LSP has no first-class "hint" field. We append each Glyph
//!    hint to `message` on a new line as `  hint: <hint>`. Post-MVP, surface
//!    them via code-actions.

use glyph_core::diagnostic::{Classification, Diagnostic as GlyphDiagnostic, LineCol, SourceSpan};
use glyph_core::span::{byte_col_to_utf16_col, LineIndex, Span};
use tower_lsp::lsp_types::{
    Diagnostic as LspDiagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location,
    NumberOrString, Position, Range, Url,
};

/// `source` field on every emitted LSP diagnostic. Matches the design contract
/// in §6.
pub const SOURCE: &str = "glyph";

/// Convert a `LineCol` start (1-indexed) into an LSP `Position` (0-indexed).
fn start_position(lc: &LineCol) -> Position {
    Position {
        line: lc.line.saturating_sub(1),
        character: lc.col.saturating_sub(1),
    }
}

/// Convert a `LineCol` end (1-indexed, inclusive) into an LSP `Position`
/// (0-indexed, exclusive). `line - 1` for the indexing convention; column
/// stays as-is because the off-by-ones cancel.
///
/// See module docs for the derivation.
fn end_position(lc: &LineCol) -> Position {
    Position {
        line: lc.line.saturating_sub(1),
        character: lc.col,
    }
}

/// Convert a `SourceSpan` (Glyph's diagnostic span) into an LSP `Range`.
fn source_span_to_range(span: &SourceSpan) -> Range {
    Range {
        start: start_position(&span.start),
        end: end_position(&span.end),
    }
}

/// Convert a Glyph classification tier into an LSP severity.
///
/// - `error`      → `Error`
/// - `repairable` → `Warning`  (the agent / `glyph fmt` will likely fix; not a hard fail)
/// - `warning`    → `Information`
fn severity(c: Classification) -> DiagnosticSeverity {
    match c {
        Classification::Error => DiagnosticSeverity::ERROR,
        Classification::Repairable => DiagnosticSeverity::WARNING,
        Classification::Warning => DiagnosticSeverity::INFORMATION,
    }
}

/// Render a Glyph hint list as a trailing block on the diagnostic message.
///
/// LSP has no first-class hint field; appending to `message` keeps the
/// information visible in every editor that renders diagnostics. Empty list
/// produces an empty string.
fn render_hints(hints: &[String]) -> String {
    if hints.is_empty() {
        return String::new();
    }
    let mut s = String::new();
    for h in hints {
        s.push('\n');
        s.push_str("  hint: ");
        s.push_str(h);
    }
    s
}

/// Convert a Glyph related-span (best-effort: no message attached, just a
/// pointer at the related location). The `file` on a `SourceSpan` is the
/// label string the compiler was invoked with (typically the path); we treat
/// it as the URI's path component when building the `Location`.
fn related_to_lsp(spans: &[SourceSpan]) -> Option<Vec<DiagnosticRelatedInformation>> {
    if spans.is_empty() {
        return None;
    }
    let out = spans
        .iter()
        .filter_map(|s| {
            let uri = file_label_to_url(&s.file)?;
            Some(DiagnosticRelatedInformation {
                location: Location {
                    uri,
                    range: source_span_to_range(s),
                },
                message: String::new(),
            })
        })
        .collect::<Vec<_>>();
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Best-effort conversion of a Glyph `SourceSpan.file` label to a `Url`.
///
/// Glyph uses the file label both as a path (when invoked on a real file) and
/// as a synthetic display string (e.g., `"<source>"` in tests). We try
/// `from_file_path` first and fall back to a `file://` URL constructed from a
/// canonicalised path; if both fail we drop the related entry. Dropping is
/// safe: the primary diagnostic still surfaces.
fn file_label_to_url(label: &str) -> Option<Url> {
    let path = std::path::Path::new(label);
    if path.is_absolute() {
        return Url::from_file_path(path).ok();
    }
    // Relative path: resolve against CWD if possible, otherwise drop.
    if let Ok(cwd) = std::env::current_dir() {
        let abs = cwd.join(path);
        return Url::from_file_path(abs).ok();
    }
    None
}

/// Convert a `glyph_core::span::Span` (byte offsets) into an LSP `Range`.
///
/// Used by the go-to-definition handler — Glyph's resolution table carries
/// raw byte spans, not the cooked `(file, line, col)` form Diagnostic spans
/// use. This helper bridges the two: walk both endpoints through `LineIndex`
/// to get 1-indexed line/col pairs, then subtract 1 from each to land on
/// LSP's 0-indexed coordinate system.
///
/// `Span.end` is **exclusive** (half-open `[start, end)`), so unlike the
/// diagnostic-span path (§10.B) there is no inclusive→exclusive bump — both
/// endpoints translate symmetrically.
pub fn byte_span_to_lsp_range(span: Span, line_index: &LineIndex, source: &str) -> Range {
    Range {
        start: byte_offset_to_lsp_position(span.start, line_index, source),
        end: byte_offset_to_lsp_position(span.end, line_index, source),
    }
}

/// Convert a single byte offset into the file to an LSP `Position`
/// (0-indexed line + UTF-16 character offset). LSP defaults to UTF-16
/// for `Position.character`; counting bytes here would mis-place the
/// cursor whenever the line contains non-ASCII characters before
/// `byte_offset`.
fn byte_offset_to_lsp_position(byte_offset: u32, line_index: &LineIndex, source: &str) -> Position {
    let (line_1, col_1_bytes) = line_index.line_col(byte_offset);
    let line_zero = line_1.saturating_sub(1);
    let byte_col_zero = col_1_bytes.saturating_sub(1);
    let line_text = source.lines().nth(line_zero as usize).unwrap_or("");
    let character = byte_col_to_utf16_col(line_text, byte_col_zero);
    Position {
        line: line_zero,
        character,
    }
}

/// Convert a Glyph `Diagnostic` to an LSP `Diagnostic`.
///
/// Pure: takes an immutable reference, allocates a fresh value. No I/O.
pub fn diagnostic_to_lsp(d: &GlyphDiagnostic) -> LspDiagnostic {
    let mut message = d.message.clone();
    message.push_str(&render_hints(&d.hints));

    LspDiagnostic {
        range: source_span_to_range(&d.span),
        severity: Some(severity(d.classification)),
        code: Some(NumberOrString::String(d.id.clone())),
        code_description: None,
        source: Some(SOURCE.to_string()),
        message,
        related_information: related_to_lsp(&d.related),
        tags: None,
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyph_core::diagnostic::{Classification, Diagnostic, LineCol, SourceSpan};

    /// Build a Glyph `SourceSpan` at the given inclusive 1-indexed coords.
    fn span(file: &str, sl: u32, sc: u32, el: u32, ec: u32) -> SourceSpan {
        SourceSpan {
            file: file.to_string(),
            start: LineCol { line: sl, col: sc },
            end: LineCol { line: el, col: ec },
        }
    }

    /// **Design §10.B lock-in.** A single-character span at line 5, col 7
    /// (inclusive end == start) must produce an LSP range `[5:6, 5:7)` —
    /// 0-indexed start, 0-indexed exclusive end.
    ///
    /// If this test ever fails, somebody changed the conversion and is about
    /// to ship a one-character-off-by-one bug to every Glyph user.
    #[test]
    fn single_character_span_end_col_conversion() {
        let s = span("f.glyph", 5, 7, 5, 7);
        let r = source_span_to_range(&s);
        assert_eq!(
            r.start,
            Position {
                line: 4,
                character: 6
            }
        );
        assert_eq!(
            r.end,
            Position {
                line: 4,
                character: 7
            }
        );
    }

    /// Multi-character single-line span: `foo` at line 3 cols 5..7 inclusive.
    /// LSP wants `[3:4, 3:7)`.
    #[test]
    fn multi_character_single_line_span() {
        let s = span("f.glyph", 3, 5, 3, 7);
        let r = source_span_to_range(&s);
        assert_eq!(
            r.start,
            Position {
                line: 2,
                character: 4
            }
        );
        assert_eq!(
            r.end,
            Position {
                line: 2,
                character: 7
            }
        );
    }

    /// Multi-line span: line 1 col 1 through line 3 col 4 inclusive.
    /// LSP: `[0:0, 2:4)`.
    #[test]
    fn multi_line_span() {
        let s = span("f.glyph", 1, 1, 3, 4);
        let r = source_span_to_range(&s);
        assert_eq!(
            r.start,
            Position {
                line: 0,
                character: 0
            }
        );
        assert_eq!(
            r.end,
            Position {
                line: 2,
                character: 4
            }
        );
    }

    /// Severity mapping is fixed by §6 and asserted explicitly.
    #[test]
    fn severity_mapping() {
        assert_eq!(severity(Classification::Error), DiagnosticSeverity::ERROR);
        assert_eq!(
            severity(Classification::Repairable),
            DiagnosticSeverity::WARNING
        );
        assert_eq!(
            severity(Classification::Warning),
            DiagnosticSeverity::INFORMATION
        );
    }

    /// Roundtrip test #1: a parse error becomes an LSP error with the
    /// correct code/source/range.
    #[test]
    fn parse_error_roundtrip() {
        let d = Diagnostic::error(
            "G::parse::tab-indent",
            "tab character used for indentation",
            span("f.glyph", 2, 1, 2, 1),
        );
        let lsp = diagnostic_to_lsp(&d);
        assert_eq!(
            lsp.code,
            Some(NumberOrString::String("G::parse::tab-indent".into()))
        );
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(lsp.source.as_deref(), Some("glyph"));
        assert_eq!(lsp.message, "tab character used for indentation");
        assert_eq!(
            lsp.range,
            Range {
                start: Position {
                    line: 1,
                    character: 0
                },
                end: Position {
                    line: 1,
                    character: 1
                }
            }
        );
        assert!(lsp.related_information.is_none());
    }

    /// Roundtrip test #2: an analyze error. Uses
    /// `G::analyze::undefined-name` as a representative live error ID so the
    /// test exercises the generic Error → LSP path without depending on any
    /// retired diagnostic.
    #[test]
    fn analyze_error_roundtrip() {
        let d = Diagnostic::error(
            "G::analyze::undefined-name",
            "`x` is not a declared `const` in this file",
            span("f.glyph", 10, 5, 10, 5),
        );
        let lsp = diagnostic_to_lsp(&d);
        assert_eq!(
            lsp.code,
            Some(NumberOrString::String("G::analyze::undefined-name".into()))
        );
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::ERROR));
        assert!(lsp.message.starts_with("`x` is not a declared"));
        // Inclusive single-char span: end.character == 5 (not 4, not 6).
        assert_eq!(lsp.range.end.character, 5);
    }

    /// PRD #103 / Slice 1 (#104): the new `G::analyze::missing-required-arg`
    /// diagnostic must round-trip through the convert layer with severity
    /// `Error` and `code` set to the diagnostic ID verbatim — no special-case
    /// mapping required.
    #[test]
    fn missing_required_arg_roundtrip() {
        let d = Diagnostic::error(
            "G::analyze::missing-required-arg",
            "call to `bar()` is missing required argument `x`",
            span("f.glyph", 4, 9, 4, 13),
        );
        let lsp = diagnostic_to_lsp(&d);
        assert_eq!(
            lsp.code,
            Some(NumberOrString::String(
                "G::analyze::missing-required-arg".into()
            ))
        );
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::ERROR));
    }

    /// Roundtrip test #3: a repairable warning with hints. Hints must be
    /// appended to the message; severity must be `Warning`.
    #[test]
    fn repairable_warning_with_hints_roundtrip() {
        let d = Diagnostic {
            id: "G::analyze::unused-import".into(),
            classification: Classification::Repairable,
            message: "imported name `foo` is never used".into(),
            span: span("f.glyph", 1, 1, 1, 20),
            related: Vec::new(),
            hints: vec!["remove the unused import".into()],
        };
        let lsp = diagnostic_to_lsp(&d);
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::WARNING));
        assert!(lsp.message.contains("imported name"));
        assert!(
            lsp.message.contains("hint: remove the unused import"),
            "expected hint to be appended; got: {:?}",
            lsp.message
        );
    }

    /// Byte-span → LSP range. Half-open input, half-open output.
    /// Pin the conversion so the go-to-def handler can rely on it.
    #[test]
    fn byte_span_to_range_basic() {
        // "abc\ndef" — bytes 4..7 covers `def` on line 2 (0-indexed line 1).
        let src = "abc\ndef";
        let li = LineIndex::new(src);
        let span = Span::new(0, 4, 7);
        let r = byte_span_to_lsp_range(span, &li, src);
        assert_eq!(
            r.start,
            Position {
                line: 1,
                character: 0
            }
        );
        assert_eq!(
            r.end,
            Position {
                line: 1,
                character: 3
            }
        );
    }

    /// Byte-span → LSP range when the use-site is at the very start of file.
    #[test]
    fn byte_span_to_range_at_origin() {
        let src = "abc";
        let li = LineIndex::new(src);
        let span = Span::new(0, 0, 3);
        let r = byte_span_to_lsp_range(span, &li, src);
        assert_eq!(
            r.start,
            Position {
                line: 0,
                character: 0
            }
        );
        assert_eq!(
            r.end,
            Position {
                line: 0,
                character: 3
            }
        );
    }

    /// LSP `Position.character` defaults to UTF-16 code units. A span whose
    /// byte start sits after a multi-byte UTF-8 character must be reported
    /// at the matching UTF-16 column, not the byte column.
    #[test]
    fn byte_span_to_range_uses_utf16_after_multibyte_char() {
        // "αbc" — α is 2 UTF-8 bytes, 1 UTF-16 code unit.
        // The span over "bc" is bytes 2..4 → UTF-16 chars 1..3.
        let src = "αbc";
        let li = LineIndex::new(src);
        let span = Span::new(0, 2, 4);
        let r = byte_span_to_lsp_range(span, &li, src);
        assert_eq!(
            r.start,
            Position {
                line: 0,
                character: 1
            }
        );
        assert_eq!(
            r.end,
            Position {
                line: 0,
                character: 3
            }
        );
    }

    /// Supplementary-plane characters (e.g. emoji) take 4 UTF-8 bytes but
    /// 2 UTF-16 code units (a surrogate pair). The conversion has to count
    /// the surrogate pair, not the byte pair.
    #[test]
    fn byte_span_to_range_uses_utf16_for_supplementary_chars() {
        // "🦀x" — the crab is 4 UTF-8 bytes / 2 UTF-16 units; "x" is at
        // byte 4, UTF-16 column 2.
        let src = "🦀x";
        let li = LineIndex::new(src);
        let span = Span::new(0, 4, 5);
        let r = byte_span_to_lsp_range(span, &li, src);
        assert_eq!(r.start.character, 2);
        assert_eq!(r.end.character, 3);
    }

    /// `related` spans flow through into LSP `related_information`.
    #[test]
    fn related_spans_become_related_information() {
        // Use absolute paths so `file_label_to_url` succeeds without filesystem prerequisites.
        let primary = SourceSpan {
            file: "/tmp/main.glyph".into(),
            start: LineCol { line: 1, col: 1 },
            end: LineCol { line: 1, col: 5 },
        };
        let other = SourceSpan {
            file: "/tmp/main.glyph".into(),
            start: LineCol { line: 5, col: 1 },
            end: LineCol { line: 5, col: 5 },
        };
        let d = Diagnostic {
            id: "G::analyze::name-collision".into(),
            classification: Classification::Error,
            message: "duplicate export name `foo`".into(),
            span: primary,
            related: vec![other],
            hints: Vec::new(),
        };
        let lsp = diagnostic_to_lsp(&d);
        let related = lsp
            .related_information
            .expect("related info should be present");
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].location.range.start.line, 4);
    }
}
