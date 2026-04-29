//! glyph-core: deterministic compiler phases for the Glyph language.
//!
//! Walking-skeleton scope (slice 1): minimum viable Phase 1 / 2 / 4 / 5 / 6-Step1 / 7
//! that produces a byte-identical golden snapshot for `update_docs.glyph.md` per
//! `design/mvp-acceptance.md` §1.

pub mod analyze;
pub mod ast;
pub mod diagnostic;
pub mod emit;
pub mod expand;
pub mod ir;
pub mod lower;
pub mod parse;
pub mod slot;
pub mod span;
pub mod tokenize;
pub mod validate;

use std::path::Path;

use crate::diagnostic::DiagBag;
use crate::span::LineIndex;

#[derive(Debug)]
pub enum CompileError {
    Read { path: String, source: std::io::Error },
    Parse(parse::ParseError),
    Lower(lower::LowerError),
    Validate(validate::ValidateError),
    Write { path: String, source: std::io::Error },
}

/// Outcome of compiling a single source file.
///
/// Either:
/// - `Compiled { markdown, diagnostics }` — Phases 1–7 ran clean; `diagnostics`
///   carries any non-blocking warnings.
/// - `Diagnostics(diag_bag)` — diagnostics-only result (errors or repairables).
///   The pipeline halted; no Markdown was produced.
///
/// The CLI maps this onto exit codes via `DiagBag::exit_code()` and the `1`-wins-over-`2`
/// rule in `design/build-foundation.md` §A6.
#[derive(Debug)]
pub enum CompileOutcome {
    Compiled { markdown: String, diagnostics: DiagBag },
    Diagnostics(DiagBag),
}

/// Run all walking-skeleton phases and return either the compiled Markdown or
/// a `DiagBag` of structured diagnostics.
///
/// `file_label` is recorded into every emitted `Diagnostic.span.file` so JSON
/// output is meaningful regardless of where the source string came from.
///
/// Phases: 1 (Parse) → 2 (Analyze) → 4 (Lower) → 5 (Validate) → 6-Step1 (Expand) → 7 (Emit).
pub fn compile_source(
    source: &str,
    file_id: u32,
    file_label: &str,
) -> Result<CompileOutcome, CompileError> {
    let mut bag = DiagBag::new();

    // Build a line index up front for diagnostic span conversion. The parser
    // builds its own when there is no diagnostic; on the diagnostic path we
    // recompute here to avoid plumbing an extra return value out of `parse`.
    let line_index = LineIndex::new(source);

    let parsed = parse::parse_with_diagnostics(source, file_id, file_label, &line_index, &mut bag);
    if !bag.is_empty() && (bag.has_error() || bag.has_repairable()) {
        // Diagnostics block compilation. Surface and stop.
        return Ok(CompileOutcome::Diagnostics(bag));
    }

    let file = match parsed {
        Some(file) => file,
        None => {
            // Defensive: parse_with_diagnostics returned None without producing a
            // blocking diagnostic. Treat as error (should not happen with current
            // implementation; a missing AST without diagnostics is a compiler bug).
            return Err(CompileError::Parse(parse::ParseError::Eof {
                message: "parser returned no AST and no diagnostics".into(),
            }));
        }
    };

    let file = analyze::analyze_with_diagnostics(file, file_id, file_label, &line_index, &mut bag);
    if bag.has_error() || bag.has_repairable() {
        return Ok(CompileOutcome::Diagnostics(bag));
    }
    let arena = lower::lower(&file).map_err(CompileError::Lower)?;
    validate::validate(&arena).map_err(CompileError::Validate)?;
    let arena = expand::expand_step1(arena);
    let markdown = emit::emit(&arena);
    Ok(CompileOutcome::Compiled { markdown, diagnostics: bag })
}

/// Run only Phase 1 (Parse) and Phase 2 (Analyze) and return the populated
/// `DiagBag`. No output files are produced; the pipeline never enters
/// Lower/Validate/Expand/Emit.
///
/// This is the engine behind the `glyph check` subcommand (`design/cli.md`
/// §`glyph check`). The returned bag may carry zero diagnostics (clean source),
/// errors, repairables, warnings, or any combination. The caller maps the bag
/// onto an exit code via `DiagBag::exit_code()` (1-wins-over-2 rule honoured).
pub fn check_source(source: &str, file_id: u32, file_label: &str) -> DiagBag {
    let mut bag = DiagBag::new();
    let line_index = LineIndex::new(source);

    let parsed = parse::parse_with_diagnostics(source, file_id, file_label, &line_index, &mut bag);

    // Phase 2 (Analyze) — slice 4 adds the parameter-related diagnostics
    // (`G::analyze::unknown-param-slot`, `G::analyze::missing-param-default`).
    if let Some(file) = parsed {
        let _ = analyze::analyze_with_diagnostics(file, file_id, file_label, &line_index, &mut bag);
    }

    bag
}

/// End-to-end file-driven compile.
///
/// Reads `<name>.glyph.md`, runs the pipeline, and (on the success path) writes
/// `<name>.md` next to the source file. The returned `CompileOutcome` carries
/// either the compiled output or a `DiagBag`; the CLI is responsible for
/// rendering and exit-code mapping.
pub fn compile_file(path: &Path) -> Result<CompileOutcome, CompileError> {
    let source = std::fs::read_to_string(path).map_err(|e| CompileError::Read {
        path: path.display().to_string(),
        source: e,
    })?;
    let label = path.display().to_string();
    let outcome = compile_source(&source, 0, &label)?;
    if let CompileOutcome::Compiled { ref markdown, .. } = outcome {
        let out_path = compiled_output_path(path);
        std::fs::write(&out_path, markdown).map_err(|e| CompileError::Write {
            path: out_path.display().to_string(),
            source: e,
        })?;
    }
    Ok(outcome)
}

/// Map `foo.glyph.md` → `foo.md` next to the source file.
fn compiled_output_path(input: &Path) -> std::path::PathBuf {
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let file_name = input.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let stem = file_name.strip_suffix(".glyph.md").unwrap_or(
        file_name
            .strip_suffix(".md")
            .unwrap_or(file_name),
    );
    parent.join(format!("{}.md", stem))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_path_strips_glyph_md() {
        let p = compiled_output_path(Path::new("tests/corpus/valid/update_docs.glyph.md"));
        assert_eq!(p, Path::new("tests/corpus/valid/update_docs.md"));
    }

    #[test]
    fn check_source_returns_empty_bag_on_empty_file_repairs_skipped() {
        // An empty file produces `G::parse::empty-file` (error). check_source
        // surfaces it and exits without running later phases.
        let bag = check_source("", 0, "empty.glyph.md");
        assert!(!bag.is_empty());
        assert_eq!(bag.exit_code(), 1);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"G::parse::empty-file"), "ids: {:?}", ids);
    }

    #[test]
    fn check_source_flags_tab_indent_as_repairable() {
        // Tab-indented source surfaces a `repairable` diagnostic, not an error.
        let src = "skill foo()\n\tflow:\n\t\t\"bar\"\n";
        let bag = check_source(src, 0, "tab.glyph.md");
        assert_eq!(bag.exit_code(), 2, "expected exit 2 for tab indent");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"G::parse::tab-indent"), "ids: {:?}", ids);
    }
}
