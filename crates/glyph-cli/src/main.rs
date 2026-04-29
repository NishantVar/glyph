//! Glyph CLI binary.
//!
//! Usage:
//!   glyph compile <path-to.glyph.md> [--format pretty|json]
//!
//! Exit codes (per `design/build-foundation.md` §A6):
//!   0 — success (Markdown emitted)
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
    /// Compile a `.glyph.md` source file into Markdown next to the source.
    Compile {
        /// Path to the source file.
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
        Command::Compile { path, format } => run_compile(path, format),
    }
}

fn run_compile(path: PathBuf, format: OutputFormat) -> ExitCode {
    // Read the source up front so we can hand it to codespan-reporting for
    // pretty rendering — `compile_file` swallows the source string.
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
        CompileOutcome::Compiled { markdown, diagnostics } => {
            // Write the .md output next to the source (same as before slice 2).
            let out_path = compiled_output_path(&path);
            if let Err(e) = std::fs::write(&out_path, &markdown) {
                eprintln!("glyph: cannot write `{}`: {}", out_path.display(), e);
                return ExitCode::from(3);
            }
            // Emit any non-blocking diagnostics (e.g., warnings).
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

/// Map `foo.glyph.md` → `foo.md` next to the source file.
fn compiled_output_path(input: &std::path::Path) -> PathBuf {
    let parent = input.parent().unwrap_or_else(|| std::path::Path::new("."));
    let file_name = input.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let stem = file_name
        .strip_suffix(".glyph.md")
        .unwrap_or_else(|| file_name.strip_suffix(".md").unwrap_or(file_name));
    parent.join(format!("{}.md", stem))
}
