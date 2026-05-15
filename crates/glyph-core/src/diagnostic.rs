//! Diagnostic shape, classification, accumulator, and exit-code mapping.
//!
//! Implements the structured diagnostic contract from `docs/reference/diagnostics.md`
//! and the exit-code rules from `docs/adr/` §A6.
//!
//! Key invariants:
//! - `SourceSpan` uses 1-indexed `(line, col)` with **inclusive** end semantics
//!   (single-character span has `start == end`), per `diagnostics.md` §Span Semantics.
//! - Maps in serialized output use `BTreeMap` (none in this module today, but
//!   any future map field MUST follow the JSON-determinism rule from
//!   `build-foundation.md` §JSON Determinism).
//! - Diagnostics are sorted on output by `(file, span.start.byte, id)` — see
//!   `DiagBag::into_sorted_with_byte_offsets` and `pretty::render`.
//! - Classification → exit code: `error` → 1, `repairable` → 2, `warning` → 0;
//!   `1` wins over `2` when both present (see `Classification::worst_exit_code`).

use serde::Serialize;

use crate::span::{LineIndex, Span};

/// Stable diagnostic id for the warning fired when an author writes one of
/// the banned generic type names (e.g. `String`, `Int`, `List`) in type
/// position in a `.glyph` source file.
///
/// Warning tier — non-blocking; compilation continues. Closest neighbor in
/// classification + phase is `G::analyze::effects-over-declared`. See
/// `docs/reference/diagnostics.md` §Classification.
pub const GENERIC_TYPE_NAME_DIAG_ID: &str = "G::analyze::generic-type-name";

/// Type identifiers must be strict PascalCase. Emitted by
/// `analyze::validate_identifier_case` for type-position identifiers that
/// don't satisfy `name_kind::is_pascal_case`.
pub const TYPE_CASE_VIOLATION_DIAG_ID: &str = "G::analyze::type-case-violation";

/// Value identifiers must be strict snake_case. Emitted by
/// `analyze::validate_identifier_case` for value-position identifiers that
/// don't satisfy `name_kind::is_snake_case`.
pub const VALUE_CASE_VIOLATION_DIAG_ID: &str = "G::analyze::value-case-violation";

/// Two raw spellings of the same canonical type (`-> LinkMode` then
/// `-> Linkmode`) registered against one another. Warning-tier.
pub const INCONSISTENT_TYPE_SPELLING_DIAG_ID: &str = "G::analyze::inconsistent-type-spelling";

/// Two `type` declarations with the same D6 canonical key. Emitted by
/// `analyze::register_type_use` when called twice with `TypeUseKind::ExplicitDecl`.
pub const DUPLICATE_TYPE_DECL_DIAG_ID: &str = "G::analyze::duplicate-type-decl";

/// An imported `.glyph` file resolves outside the canonicalized input root used
/// by `--out-dir`. The file is written in-place rather than mirrored under
/// `--out-dir`. Emitted once per outside-root file per build. Warning-tier.
pub const IMPORT_OUTSIDE_OUT_DIR_DIAG_ID: &str = "G::build::import-outside-out-dir";

/// The three trust tiers from `pipeline.md` Phase 2.
///
/// Stable serialization: lowercase string for JSON output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Classification {
    /// Hard stop, no repair possible.
    Error,
    /// Phase 3 (Repair) can likely fix this.
    Repairable,
    /// Non-blocking observation.
    Warning,
}

impl Classification {
    /// Exit-code contribution for a single diagnostic.
    ///
    /// Per `build-foundation.md` §A6:
    ///   `error`      → 1
    ///   `repairable` → 2
    ///   `warning`    → 0
    pub fn exit_code(self) -> u8 {
        match self {
            Classification::Error => 1,
            Classification::Repairable => 2,
            Classification::Warning => 0,
        }
    }
}

/// Position in a source file, 1-indexed line + column.
///
/// Glyph rejects tabs in source, so column counts bytes from start of line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

/// A source location attached to a diagnostic.
///
/// Spans are 1-indexed and **inclusive** at both ends. A single-character span
/// has `start == end`. Multi-line spans are legal (e.g., unterminated string).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceSpan {
    pub file: String,
    pub start: LineCol,
    pub end: LineCol,
}

impl SourceSpan {
    /// Build a `SourceSpan` from a half-open byte `Span` and a `LineIndex`.
    ///
    /// Internal half-open byte spans become inclusive 1-indexed line/col pairs:
    /// a span with `end == start + 1` (single character) lands as `start == end`
    /// (per `diagnostics.md` §Span Semantics).
    pub fn from_byte_span(file: impl Into<String>, span: Span, line_index: &LineIndex) -> Self {
        let (s_line, s_col) = line_index.line_col(span.start);
        // Inclusive end: convert exclusive byte `end` to its inclusive counterpart by
        // mapping `end - 1` (when the span has any width). For a zero-width fallback
        // span (rare; only synthetic-diagnostic option (3) per `diagnostics.md`),
        // collapse end onto start.
        let (e_line, e_col) = if span.end > span.start {
            line_index.line_col(span.end - 1)
        } else {
            (s_line, s_col)
        };
        Self {
            file: file.into(),
            start: LineCol {
                line: s_line,
                col: s_col,
            },
            end: LineCol {
                line: e_line,
                col: e_col,
            },
        }
    }
}

/// One structured diagnostic.
///
/// Shape matches `docs/reference/diagnostics.md`. The `id` is `G::<phase>::<name>` and
/// is stable across compiler versions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub id: String,
    pub classification: Classification,
    pub message: String,
    pub span: SourceSpan,
    /// Other locations that contribute (e.g., the other side of a name collision).
    /// Omitted from JSON when empty.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub related: Vec<SourceSpan>,
    /// Actionable suggestions for the author. Omitted from JSON when empty.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub hints: Vec<String>,
}

impl Diagnostic {
    /// Construct an `error` diagnostic with no related spans or hints.
    pub fn error(id: impl Into<String>, message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            id: id.into(),
            classification: Classification::Error,
            message: message.into(),
            span,
            related: Vec::new(),
            hints: Vec::new(),
        }
    }
}

/// Accumulates diagnostics across pipeline phases (per `build-foundation.md` §A6).
///
/// Holds each diagnostic together with the originating byte span — the byte
/// offset is the canonical sort key for stable output ordering, even after
/// the public `SourceSpan` has lossily collapsed to `(line, col)`.
#[derive(Debug, Default)]
pub struct DiagBag {
    entries: Vec<DiagEntry>,
}

#[derive(Clone, Debug)]
struct DiagEntry {
    diag: Diagnostic,
    /// Original half-open byte span. Used as the canonical sort key.
    byte_start: u32,
}

impl DiagBag {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// True iff there are zero diagnostics (regardless of classification).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Push a diagnostic together with its originating byte span.
    pub fn push(&mut self, diag: Diagnostic, byte_span: Span) {
        self.entries.push(DiagEntry {
            diag,
            byte_start: byte_span.start,
        });
    }

    /// All diagnostics in insertion order (no sort).
    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.entries.iter().map(|e| &e.diag)
    }

    /// Drain `other` into `self`, preserving every entry's original byte_start
    /// so subsequent `sorted()` calls produce the same ordering as if the
    /// diagnostics had all been pushed into one bag from the start.
    ///
    /// Used by the multi-file LSP entry points (`check_source_with_imports`,
    /// the legacy `check_file` back-compat shim) which collect per-file bags
    /// during the import walk and may need to merge them.
    pub fn merge(&mut self, mut other: DiagBag) {
        self.entries.append(&mut other.entries);
    }

    /// Returns true if any diagnostic has classification `Error`.
    pub fn has_error(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.diag.classification == Classification::Error)
    }

    /// Returns true if any diagnostic has classification `Repairable`.
    pub fn has_repairable(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.diag.classification == Classification::Repairable)
    }

    /// Compute the build's exit code per `build-foundation.md` §A6.
    ///
    /// `error` → 1, `repairable` → 2, `warning`/none → 0.
    /// `1` wins over `2` when both classifications are present.
    pub fn exit_code(&self) -> u8 {
        if self.has_error() {
            1
        } else if self.has_repairable() {
            2
        } else {
            0
        }
    }

    /// Return diagnostics sorted by `(file, byte_start, id)` per
    /// `build-foundation.md` §JSON Determinism.
    pub fn sorted(&self) -> Vec<Diagnostic> {
        let mut entries = self.entries.clone();
        entries.sort_by(|a, b| {
            a.diag
                .span
                .file
                .cmp(&b.diag.span.file)
                .then(a.byte_start.cmp(&b.byte_start))
                .then(a.diag.id.cmp(&b.diag.id))
        });
        entries.into_iter().map(|e| e.diag).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_index(src: &str) -> LineIndex {
        LineIndex::new(src)
    }

    #[test]
    fn classification_exit_codes() {
        assert_eq!(Classification::Error.exit_code(), 1);
        assert_eq!(Classification::Repairable.exit_code(), 2);
        assert_eq!(Classification::Warning.exit_code(), 0);
    }

    #[test]
    fn diagbag_one_wins_over_two() {
        // Single error: exit 1.
        let mut bag = DiagBag::new();
        bag.push(
            Diagnostic::error(
                "G::test::e",
                "boom",
                SourceSpan {
                    file: "f".into(),
                    start: LineCol { line: 1, col: 1 },
                    end: LineCol { line: 1, col: 1 },
                },
            ),
            Span::new(0, 0, 1),
        );
        assert_eq!(bag.exit_code(), 1);

        // Single repairable: exit 2.
        let mut bag = DiagBag::new();
        bag.push(
            Diagnostic {
                id: "G::test::r".into(),
                classification: Classification::Repairable,
                message: "fixme".into(),
                span: SourceSpan {
                    file: "f".into(),
                    start: LineCol { line: 1, col: 1 },
                    end: LineCol { line: 1, col: 1 },
                },
                related: Vec::new(),
                hints: Vec::new(),
            },
            Span::new(0, 0, 1),
        );
        assert_eq!(bag.exit_code(), 2);

        // Both: 1 wins over 2.
        bag.push(
            Diagnostic::error(
                "G::test::e",
                "boom",
                SourceSpan {
                    file: "f".into(),
                    start: LineCol { line: 1, col: 1 },
                    end: LineCol { line: 1, col: 1 },
                },
            ),
            Span::new(0, 0, 1),
        );
        assert_eq!(bag.exit_code(), 1);
    }

    #[test]
    fn diagbag_warning_only_is_zero() {
        let mut bag = DiagBag::new();
        bag.push(
            Diagnostic {
                id: "G::test::w".into(),
                classification: Classification::Warning,
                message: "hmm".into(),
                span: SourceSpan {
                    file: "f".into(),
                    start: LineCol { line: 1, col: 1 },
                    end: LineCol { line: 1, col: 1 },
                },
                related: Vec::new(),
                hints: Vec::new(),
            },
            Span::new(0, 0, 1),
        );
        assert_eq!(bag.exit_code(), 0);
        assert!(!bag.is_empty());
    }

    #[test]
    fn empty_bag_is_zero_exit() {
        let bag = DiagBag::new();
        assert!(bag.is_empty());
        assert_eq!(bag.exit_code(), 0);
    }

    #[test]
    fn sort_by_file_then_byte_then_id() {
        let mk = |file: &str, byte_start: u32, id: &str| {
            (
                Diagnostic::error(
                    id,
                    "msg",
                    SourceSpan {
                        file: file.into(),
                        start: LineCol { line: 1, col: 1 },
                        end: LineCol { line: 1, col: 1 },
                    },
                ),
                Span::new(0, byte_start, byte_start + 1),
            )
        };

        let mut bag = DiagBag::new();
        // Insert in deliberately unsorted order.
        for (d, s) in [
            mk("b.glyph", 5, "G::a::z"),
            mk("a.glyph", 10, "G::a::a"),
            mk("a.glyph", 5, "G::a::z"),
            mk("a.glyph", 5, "G::a::a"),
        ] {
            bag.push(d, s);
        }

        let sorted = bag.sorted();
        let order: Vec<(&str, &str)> = sorted
            .iter()
            .map(|d| (d.span.file.as_str(), d.id.as_str()))
            .collect();
        assert_eq!(
            order,
            vec![
                ("a.glyph", "G::a::a"), // byte 5, id a
                ("a.glyph", "G::a::z"), // byte 5, id z
                ("a.glyph", "G::a::a"), // byte 10
                ("b.glyph", "G::a::z"), // file b last
            ]
        );
    }

    #[test]
    fn source_span_from_byte_span_is_inclusive() {
        let src = "abc\ndef\n";
        let li = line_index(src);
        // "abc" — bytes [0, 3). Inclusive end should be (line 1, col 3), not col 4.
        let s = SourceSpan::from_byte_span("f.glyph", Span::new(0, 0, 3), &li);
        assert_eq!(s.start, LineCol { line: 1, col: 1 });
        assert_eq!(s.end, LineCol { line: 1, col: 3 });

        // single char "c": [2, 3) → start == end == (1, 3)
        let s = SourceSpan::from_byte_span("f.glyph", Span::new(0, 2, 3), &li);
        assert_eq!(s.start, s.end);
    }

    #[test]
    fn diagnostic_serializes_with_lowercase_classification() {
        let d = Diagnostic::error(
            "G::parse::empty-file",
            "source has no declarations",
            SourceSpan {
                file: "f.glyph".into(),
                start: LineCol { line: 1, col: 1 },
                end: LineCol { line: 1, col: 1 },
            },
        );
        let s = serde_json::to_string(&d).unwrap();
        assert!(s.contains("\"classification\":\"error\""));
        assert!(s.contains("\"id\":\"G::parse::empty-file\""));
        // No related / hints fields when empty.
        assert!(!s.contains("\"related\""));
        assert!(!s.contains("\"hints\""));
    }
}
