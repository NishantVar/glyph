//! Glyph CLI binary.
//!
//! Usage:
//!   glyph compile <path-to.glyph.md> [--format pretty|json]
//!   glyph check   <path-or-dir>      [--format pretty|json]
//!
//! Exit codes (per `design/build-foundation.md` §A6):
//!   0 — success (Markdown emitted, or `check` clean)
//!   1 — hard errors (compilation cannot proceed)
//!   2 — repairable diagnostics only
//!   3 — invocation error (bad flags, missing path, IO failure)
//!
//! `--format pretty` (default) renders diagnostics to **stderr** via
//! `codespan-reporting` (span + caret + message). `--format json` writes one
//! JSON diagnostic per line (NDJSON) to **stdout**.

use clap::{Parser, Subcommand, ValueEnum};
use codespan_reporting::diagnostic::{Diagnostic as CrDiag, Label, Severity};
use codespan_reporting::files::SimpleFiles;
use codespan_reporting::term::termcolor::{ColorChoice, StandardStream};
use glyph_core::diagnostic::{Classification, DiagBag, Diagnostic};
use glyph_core::CompileOutcome;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(name = "glyph", about = "Glyph compiler", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Compile `.glyph.md` source file(s) into Markdown next to the source.
    ///
    /// Accepts a single file or a directory. When given a directory, all
    /// `.glyph.md` files are compiled in topological order with partial failure
    /// (skip-dependents, leave stale `.md`, exit 1 if any file fails).
    Compile {
        /// Path to the source file or directory.
        path: PathBuf,
        /// Diagnostic output format. `pretty` (default) renders to stderr with
        /// codespan-reporting; `json` emits one NDJSON diagnostic per line on stdout.
        #[arg(long, value_enum, default_value_t = OutputFormat::Pretty)]
        format: OutputFormat,
        /// Emit the post-Step-1 resolved IR as a sidecar JSON file (`foo.ir.json`)
        /// next to the compiled `.md`. See `design/ir-json-schema.md`.
        #[arg(long)]
        emit_ir: bool,
    },
    /// Run Phases 1 (Parse) and 2 (Analyze) only — fast lint mode.
    ///
    /// Reports all diagnostics (errors / repairable / warnings) without continuing
    /// to Lower/Validate/Expand/Emit. **Writes no output files.** Accepts either a
    /// single `.glyph.md` file or a directory (recursively walked for `*.glyph.md`).
    /// See `design/cli.md` §`glyph check`.
    Check {
        /// Path to the source file or directory.
        path: PathBuf,
        /// Diagnostic output format. `pretty` (default) renders to stderr with
        /// codespan-reporting; `json` emits one NDJSON diagnostic per line on stdout.
        #[arg(long, value_enum, default_value_t = OutputFormat::Pretty)]
        format: OutputFormat,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum OutputFormat {
    Pretty,
    Json,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Compile { path, format, emit_ir } => run_compile(path, format, emit_ir),
        Command::Check { path, format } => run_check(path, format),
    }
}

/// Run `glyph check <path>` over `path`. If `path` is a directory, walks it
/// recursively for `*.glyph.md` files (sorted by path for byte-stable output)
/// and processes each one. The aggregate exit code follows the same
/// `1`-wins-over-`2` rule as a single-file check (per `design/cli.md`
/// §Multi-File Behavior).
///
/// Never writes output files, regardless of outcome. Diagnostics are rendered
/// per the requested `--format`.
fn run_check(path: PathBuf, format: OutputFormat) -> ExitCode {
    let files = match collect_glyph_sources(&path) {
        Ok(v) => v,
        Err(code) => return code,
    };

    if files.is_empty() {
        // Directory with no `.glyph.md` files inside — nothing to check, exit
        // cleanly. (A missing single file would have errored in
        // `collect_glyph_sources`.)
        return ExitCode::from(0);
    }

    // Aggregate exit code across files: 1 wins over 2 wins over 0.
    let mut worst: u8 = 0;
    for file in files {
        let source = match std::fs::read_to_string(&file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("glyph: cannot read `{}`: {}", file.display(), e);
                return ExitCode::from(3);
            }
        };
        let label = file.display().to_string();
        // Use import-aware check when the file path is available.
        let bag = glyph_core::check_file(&file);
        emit_diagnostics(&bag, &label, &source, format);
        let code = bag.exit_code();
        worst = combine_exit_codes(worst, code);
    }

    ExitCode::from(worst)
}

/// Combine two exit codes per the `1`-wins-over-`2` rule
/// (`design/build-foundation.md` §A6).
fn combine_exit_codes(a: u8, b: u8) -> u8 {
    match (a, b) {
        (1, _) | (_, 1) => 1,
        (2, _) | (_, 2) => 2,
        _ => 0,
    }
}

/// Collect every `.glyph.md` source file under `path`.
///
/// - If `path` is a single file, returns `vec![path]` (regardless of extension —
///   surfacing extension issues is the parser's job, not the CLI's).
/// - If `path` is a directory, walks it recursively and returns every entry
///   ending in `.glyph.md`, sorted by path for deterministic output.
/// - Anything else (missing path, IO error) returns `Err(ExitCode 3)`.
fn collect_glyph_sources(path: &std::path::Path) -> Result<Vec<PathBuf>, ExitCode> {
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("glyph: cannot stat `{}`: {}", path.display(), e);
            return Err(ExitCode::from(3));
        }
    };

    if metadata.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }

    if metadata.is_dir() {
        let mut out: Vec<PathBuf> = Vec::new();
        let mut stack: Vec<PathBuf> = vec![path.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("glyph: cannot read directory `{}`: {}", dir.display(), e);
                    return Err(ExitCode::from(3));
                }
            };
            for entry in entries {
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        eprintln!("glyph: cannot read directory entry under `{}`: {}", dir.display(), e);
                        return Err(ExitCode::from(3));
                    }
                };
                let p = entry.path();
                let ft = match entry.file_type() {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("glyph: cannot stat `{}`: {}", p.display(), e);
                        return Err(ExitCode::from(3));
                    }
                };
                if ft.is_dir() {
                    stack.push(p);
                } else if ft.is_file() {
                    if p.to_string_lossy().ends_with(".glyph.md") {
                        out.push(p);
                    }
                }
            }
        }
        out.sort();
        return Ok(out);
    }

    eprintln!("glyph: `{}` is neither a regular file nor a directory", path.display());
    Err(ExitCode::from(3))
}

fn run_compile(path: PathBuf, format: OutputFormat, emit_ir: bool) -> ExitCode {
    let metadata = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("glyph: cannot stat `{}`: {}", path.display(), e);
            return ExitCode::from(3);
        }
    };

    if metadata.is_dir() {
        return run_compile_directory(path, format);
    }

    // Single-file compile (existing behavior).
    let source = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("glyph: cannot read `{}`: {}", path.display(), e);
            return ExitCode::from(3);
        }
    };

    let label = path.display().to_string();
    let outcome = match glyph_core::compile_source(&source, 0, &label) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("glyph: compile failed: {:?}", e);
            return ExitCode::from(1);
        }
    };

    match outcome {
        CompileOutcome::Compiled { markdown, diagnostics, arena } => {
            let out_path = compiled_output_path(&path);
            if let Err(e) = glyph_core::atomic_write(&out_path, &markdown) {
                eprintln!("glyph: cannot write `{}`: {}", out_path.display(), e);
                return ExitCode::from(3);
            }
            if emit_ir {
                let source_file = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                if let Some(ir_json) = glyph_core::emit_ir::serialize_ir_json(&arena, source_file) {
                    let ir_path = ir_json_output_path(&path);
                    if let Err(e) = glyph_core::atomic_write(&ir_path, &ir_json) {
                        eprintln!("glyph: cannot write `{}`: {}", ir_path.display(), e);
                        return ExitCode::from(3);
                    }
                }
            }
            emit_diagnostics(&diagnostics, &label, &source, format);
            ExitCode::from(diagnostics.exit_code())
        }
        CompileOutcome::Diagnostics(bag) => {
            let code = bag.exit_code();
            emit_diagnostics(&bag, &label, &source, format);
            ExitCode::from(code)
        }
    }
}

/// Directory-mode compile: collect all `.glyph.md` files, build DAG, compile
/// in topological order with partial failure.
fn run_compile_directory(path: PathBuf, format: OutputFormat) -> ExitCode {
    let files = match collect_glyph_sources(&path) {
        Ok(v) => v,
        Err(code) => return code,
    };

    if files.is_empty() {
        return ExitCode::from(0);
    }

    let result = glyph_core::compile_directory(&files);

    // Emit diagnostics and stderr notes for each file outcome.
    for (file_path, outcome) in &result.outcomes {
        match outcome {
            glyph_core::FileOutcome::Compiled { diagnostics } => {
                if !diagnostics.is_empty() {
                    let source = std::fs::read_to_string(file_path).unwrap_or_default();
                    let label = file_path.display().to_string();
                    emit_diagnostics(diagnostics, &label, &source, format);
                }
            }
            glyph_core::FileOutcome::Failed { diagnostics } => {
                let source = std::fs::read_to_string(file_path).unwrap_or_default();
                let label = file_path.display().to_string();
                emit_diagnostics(diagnostics, &label, &source, format);
            }
            glyph_core::FileOutcome::Skipped { failed_dep } => {
                let file_name = file_path.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                let out_name = file_name.strip_suffix(".glyph.md")
                    .map(|s| format!("{}.md", s))
                    .unwrap_or_else(|| file_name.to_string());
                eprintln!(
                    "warning[G::build::skipped-due-to-failed-import]: `{}` skipped because `{}` failed",
                    file_path.display(),
                    failed_dep.display(),
                );
                // Stale .md note (per pipeline.md §Partial Failure Policy §3).
                let stale_path = file_path.parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join(&out_name);
                if stale_path.exists() {
                    eprintln!(
                        "note: `{}` was not regenerated; the on-disk version reflects the previous successful build of `{}` and may be out of sync.",
                        out_name,
                        file_name,
                    );
                }
            }
        }
    }

    ExitCode::from(result.exit_code)
}

fn emit_diagnostics(bag: &DiagBag, file_label: &str, source: &str, format: OutputFormat) {
    if bag.is_empty() {
        return;
    }
    let sorted = bag.sorted();
    match format {
        OutputFormat::Json => render_ndjson(&sorted),
        OutputFormat::Pretty => render_pretty(&sorted, file_label, source),
    }
}

/// Emit one JSON object per line (NDJSON) to stdout.
///
/// Diagnostics are pre-sorted by `(file, byte_start, id)` per
/// `build-foundation.md` §JSON Determinism, so the output is byte-stable
/// across runs over identical input.
fn render_ndjson(diags: &[Diagnostic]) {
    let mut out = std::io::stdout().lock();
    use std::io::Write as _;
    for d in diags {
        // serde_json::to_string is deterministic for a fixed Diagnostic shape:
        // SourceSpan/LineCol have stable field orders and there are no map fields.
        match serde_json::to_string(d) {
            Ok(s) => {
                let _ = writeln!(out, "{}", s);
            }
            Err(e) => {
                // Should not happen: Diagnostic is purely owned-string + numeric data.
                eprintln!("glyph: failed to serialize diagnostic {}: {}", d.id, e);
            }
        }
    }
}

/// Pretty-print diagnostics to stderr using codespan-reporting.
fn render_pretty(diags: &[Diagnostic], file_label: &str, source: &str) {
    let mut files = SimpleFiles::new();
    let file_id = files.add(file_label.to_string(), source.to_string());

    let writer = StandardStream::stderr(ColorChoice::Auto);
    let config = codespan_reporting::term::Config::default();

    for d in diags {
        let severity = match d.classification {
            Classification::Error => Severity::Error,
            Classification::Repairable => Severity::Warning,
            Classification::Warning => Severity::Warning,
        };

        // Convert our 1-indexed inclusive (line, col) span back to a byte range
        // for codespan-reporting. We do this by walking lines in `source`.
        let range = byte_range_from_linecol(source, &d.span.start, &d.span.end);

        let cr = CrDiag::new(severity)
            .with_code(d.id.clone())
            .with_message(d.message.clone())
            .with_labels(vec![Label::primary(file_id, range)]);

        let _ = codespan_reporting::term::emit(&mut writer.lock(), &config, &files, &cr);
    }
}

/// Convert a 1-indexed inclusive `(line, col)` start/end pair back into a
/// half-open byte range `[start_byte, end_byte)` over `source`. Inclusive end
/// (line, col) → exclusive byte index by adding 1 to the byte offset of the
/// final character.
fn byte_range_from_linecol(
    source: &str,
    start: &glyph_core::diagnostic::LineCol,
    end: &glyph_core::diagnostic::LineCol,
) -> std::ops::Range<usize> {
    let start_byte = locate_byte(source, start.line, start.col);
    let end_byte = locate_byte(source, end.line, end.col).saturating_add(1);
    start_byte..end_byte.max(start_byte)
}

fn locate_byte(source: &str, line: u32, col: u32) -> usize {
    // Walk line-by-line. col is 1-indexed bytes from start of line.
    let mut current_line: u32 = 1;
    let mut line_start: usize = 0;
    for (idx, b) in source.bytes().enumerate() {
        if current_line == line {
            return line_start + (col.saturating_sub(1) as usize).min(idx + 1 - line_start);
        }
        if b == b'\n' {
            current_line += 1;
            line_start = idx + 1;
        }
    }
    if current_line == line {
        return line_start + (col.saturating_sub(1) as usize);
    }
    source.len().saturating_sub(1)
}

/// Map `foo.glyph.md` → `foo.ir.json` next to the source file.
fn ir_json_output_path(input: &std::path::Path) -> PathBuf {
    let parent = input.parent().unwrap_or_else(|| std::path::Path::new("."));
    let file_name = input.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let stem = file_name
        .strip_suffix(".glyph.md")
        .unwrap_or_else(|| file_name.strip_suffix(".md").unwrap_or(file_name));
    parent.join(format!("{}.ir.json", stem))
}

/// Map `foo.glyph.md` → `foo.md` next to the source file.
fn compiled_output_path(input: &std::path::Path) -> PathBuf {
    let parent = input.parent().unwrap_or_else(|| std::path::Path::new("."));
    let file_name = input.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let stem = file_name
        .strip_suffix(".glyph.md")
        .unwrap_or_else(|| file_name.strip_suffix(".md").unwrap_or(file_name));
    parent.join(format!("{}.md", stem))
}
