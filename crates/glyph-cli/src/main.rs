//! Glyph CLI binary.
//!
//! Usage:
//!   glyph compile <path-to.glyph> [--format pretty|json]
//!   glyph check   <path-or-dir>      [--format pretty|json]
//!
//! Exit codes (per `docs/adr/` §A6):
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
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(name = "glyph", about = "Glyph compiler", version)]
struct Cli {
    /// Enable the `effects:` sub-section in `skill`, `block`, and `export block`
    /// declarations. When omitted (default), any `effects:` usage produces a
    /// `G::parse::gated-section` error.
    #[arg(long, global = true)]
    enable_effects: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Compile `.glyph` source file(s) into Markdown next to the source.
    ///
    /// Accepts a single file or a directory. When given a directory, all
    /// `.glyph` files are compiled in topological order with partial failure
    /// (skip-dependents, leave stale `.md`, exit 1 if any file fails).
    Compile {
        /// Path to the source file or directory.
        path: PathBuf,
        /// Diagnostic output format. `pretty` (default) renders to stderr with
        /// codespan-reporting; `json` emits one NDJSON diagnostic per line on stdout.
        #[arg(long, value_enum, default_value_t = OutputFormat::Pretty)]
        format: OutputFormat,
        /// Emit the post-Step-1 resolved IR as a sidecar JSON file (`foo.ir.json`)
        /// next to the compiled `.md`. See `docs/reference/ir-json.md`.
        #[arg(long)]
        emit_ir: bool,
        /// Treat `repairable` diagnostics as hard errors: exit code 1 instead of
        /// 2. No `.md` output is written when repairable diagnostics are present.
        #[arg(long)]
        strict: bool,
        /// Mirror input layout under <dir>. Auto-created if missing.
        #[arg(long = "out-dir", short = 'o', conflicts_with = "output")]
        out_dir: Option<PathBuf>,
        /// Write the entry file's compiled `.md` to exactly this path.
        /// Single-file input only. Parent directory must exist.
        #[arg(long = "output", conflicts_with = "out_dir")]
        output: Option<PathBuf>,
    },
    /// Run Phases 1 (Parse) and 2 (Analyze) only — fast lint mode.
    ///
    /// Reports all diagnostics (errors / repairable / warnings) without continuing
    /// to Lower/Validate/Expand/Emit. **Writes no output files.** Accepts either a
    /// single `.glyph` file or a directory (recursively walked for `*.glyph`).
    /// See `design/cli.md` §`glyph check`.
    Check {
        /// Path to the source file or directory.
        path: PathBuf,
        /// Diagnostic output format. `pretty` (default) renders to stderr with
        /// codespan-reporting; `json` emits one NDJSON diagnostic per line on stdout.
        #[arg(long, value_enum, default_value_t = OutputFormat::Pretty)]
        format: OutputFormat,
        /// Treat `repairable` diagnostics as hard errors: exit code 1 instead of 2.
        #[arg(long)]
        strict: bool,
    },
    /// Validate that a compiled `.md` structurally matches its `.ir.json`.
    ///
    /// Runs the 26 deterministic Phase 6b structural checks against the
    /// agent-rewritten Markdown and the resolved IR JSON. See `expand.md` §4.
    ValidateOutput {
        /// Path to the `.ir.json` file.
        ir_json_path: PathBuf,
        /// Path to the compiled `.md` file.
        md_path: PathBuf,
        /// Diagnostic output format.
        #[arg(long, value_enum, default_value_t = OutputFormat::Pretty)]
        format: OutputFormat,
    },
    /// Run Phase 3a deterministic source rewrites. Rewrites `.glyph` files
    /// in place. Analogous to `rustfmt` / `gofmt`.
    Fmt {
        /// Path to the source file or directory.
        path: PathBuf,
        /// Don't write changes; exit 1 if any file would be reformatted.
        #[arg(long)]
        check: bool,
    },
    /// Run the Glyph Language Server over stdio.
    ///
    /// Speaks JSON-RPC framed per the LSP spec. Intended to be launched by an
    /// editor (e.g., via `cmd = { "glyph", "lsp" }` in nvim-lspconfig). See
    /// `crates/glyph-lsp/README.md` for editor setup instructions.
    Lsp {
        /// Accepted for compatibility with `vscode-languageclient` (which
        /// appends `--stdio` when `TransportKind.stdio` is set). The Glyph
        /// LSP only supports stdio, so the flag is a no-op.
        #[arg(long, hide = true)]
        stdio: bool,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum OutputFormat {
    Pretty,
    Json,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Build the section catalogue once at the CLI top and flip the
    // `[effects]` entry if the user passed `--enable-effects`. Phase 5
    // populated the catalogue with `effects.enabled = false` as the default;
    // `set_enabled("effects", true)` flips that bit when the CLI flag is
    // present. The derived `enable_effects: bool` continues to flow through
    // the bare-bool wrappers (`check_file_with_effects`,
    // `compile_directory_with_options`, etc.) so the catalogue is the single
    // source of truth at the CLI boundary while internal call sites stay on
    // their pre-catalogue signatures.
    let mut catalogue = glyph_core::sections::SectionCatalogue::load();
    if cli.enable_effects {
        catalogue.set_enabled("effects", true);
    }
    let enable_effects = catalogue.effects_enabled();

    match cli.command {
        Command::Compile {
            path,
            format,
            emit_ir,
            strict,
            output,
            out_dir,
        } => run_compile(
            path,
            format,
            emit_ir,
            strict,
            enable_effects,
            output,
            out_dir,
        ),
        Command::Check {
            path,
            format,
            strict,
        } => run_check(path, format, strict, enable_effects),
        Command::ValidateOutput {
            ir_json_path,
            md_path,
            format,
        } => run_validate_output(ir_json_path, md_path, format),
        Command::Fmt { path, check } => run_fmt(path, check, enable_effects),
        Command::Lsp { .. } => run_lsp(),
    }
}

/// Launch the Glyph LSP server over stdio.
///
/// Delegates entirely to `glyph_lsp::run_stdio` — this CLI shim only exists so
/// editors can call `glyph lsp` instead of a separate `glyph-lsp` binary.
fn run_lsp() -> ExitCode {
    match glyph_lsp::run_stdio() {
        Ok(()) => ExitCode::from(0),
        Err(e) => {
            eprintln!("glyph: lsp server error: {}", e);
            ExitCode::from(1)
        }
    }
}

fn run_validate_output(ir_json_path: PathBuf, md_path: PathBuf, format: OutputFormat) -> ExitCode {
    let ir_json = match std::fs::read_to_string(&ir_json_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("glyph: cannot read `{}`: {}", ir_json_path.display(), e);
            return ExitCode::from(3);
        }
    };
    let md = match std::fs::read_to_string(&md_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("glyph: cannot read `{}`: {}", md_path.display(), e);
            return ExitCode::from(3);
        }
    };

    let violations = glyph_core::validate_output::validate_output(&ir_json, &md);

    if violations.is_empty() {
        ExitCode::from(0)
    } else {
        match format {
            OutputFormat::Json => {
                let json = glyph_core::validate_output::violations_to_json(&violations);
                println!("{}", json);
            }
            OutputFormat::Pretty => {
                let pretty = glyph_core::validate_output::violations_to_pretty(&violations);
                eprintln!("{}", pretty);
            }
        }
        ExitCode::from(1)
    }
}

fn run_fmt(path: PathBuf, check: bool, enable_effects: bool) -> ExitCode {
    let files = match collect_glyph_sources(&path) {
        Ok(v) => v,
        Err(code) => return code,
    };

    if files.is_empty() {
        return ExitCode::from(0);
    }

    let mut any_changed = false;
    for file in files {
        let source = match std::fs::read_to_string(&file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("glyph: cannot read `{}`: {}", file.display(), e);
                return ExitCode::from(3);
            }
        };

        let result = glyph_core::fmt::fmt_source(&source, enable_effects);

        if result.changed {
            any_changed = true;
            if !check {
                if let Err(e) = std::fs::write(&file, &result.output) {
                    eprintln!("glyph: cannot write `{}`: {}", file.display(), e);
                    return ExitCode::from(3);
                }
            }
        }
    }

    if check && any_changed {
        ExitCode::from(1)
    } else {
        ExitCode::from(0)
    }
}

/// Run `glyph check <path>` over `path`. If `path` is a directory, walks it
/// recursively for `*.glyph` files (sorted by path for byte-stable output)
/// and processes each one. The aggregate exit code follows the same
/// `1`-wins-over-`2` rule as a single-file check (per `design/cli.md`
/// §Multi-File Behavior).
///
/// Never writes output files, regardless of outcome. Diagnostics are rendered
/// per the requested `--format`.
fn run_check(path: PathBuf, format: OutputFormat, strict: bool, enable_effects: bool) -> ExitCode {
    let files = match collect_glyph_sources(&path) {
        Ok(v) => v,
        Err(code) => return code,
    };

    if files.is_empty() {
        // Directory with no `.glyph` files inside — nothing to check, exit
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
        let bag = glyph_core::check_file_with_effects(&file, enable_effects);
        emit_diagnostics(&bag, &label, &source, format);
        let code = bag.exit_code();
        worst = combine_exit_codes(worst, code);
    }

    if strict && worst == 2 {
        return ExitCode::from(1);
    }
    ExitCode::from(worst)
}

/// Combine two exit codes per the `1`-wins-over-`2` rule
/// (`docs/adr/` §A6).
fn combine_exit_codes(a: u8, b: u8) -> u8 {
    match (a, b) {
        (1, _) | (_, 1) => 1,
        (2, _) | (_, 2) => 2,
        _ => 0,
    }
}

/// Collect every `.glyph` source file under `path`.
///
/// - If `path` is a single file, returns `vec![path]` (regardless of extension —
///   surfacing extension issues is the parser's job, not the CLI's).
/// - If `path` is a directory, walks it recursively and returns every entry
///   ending in `.glyph`, sorted by path for deterministic output.
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
                        eprintln!(
                            "glyph: cannot read directory entry under `{}`: {}",
                            dir.display(),
                            e
                        );
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
                    if p.to_string_lossy().ends_with(".glyph") {
                        out.push(p);
                    }
                }
            }
        }
        out.sort();
        return Ok(out);
    }

    eprintln!(
        "glyph: `{}` is neither a regular file nor a directory",
        path.display()
    );
    Err(ExitCode::from(3))
}

fn run_compile(
    path: PathBuf,
    format: OutputFormat,
    emit_ir: bool,
    strict: bool,
    enable_effects: bool,
    output: Option<PathBuf>,
    out_dir: Option<PathBuf>,
) -> ExitCode {
    let metadata = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("glyph: cannot stat `{}`: {}", path.display(), e);
            return ExitCode::from(3);
        }
    };

    // --output validation
    if let Some(ref out) = output {
        if metadata.is_dir() {
            eprintln!(
                "glyph: --output requires a single-file input; `{}` is a directory. Use --out-dir for directory input.",
                path.display()
            );
            return ExitCode::from(3);
        }
        if out.file_name().is_none() {
            eprintln!(
                "glyph: --output `{}` has no file name component",
                out.display()
            );
            return ExitCode::from(3);
        }
        let parent_raw = out.parent().unwrap_or_else(|| std::path::Path::new("."));
        let parent = if parent_raw.as_os_str().is_empty() {
            std::path::Path::new(".")
        } else {
            parent_raw
        };
        match std::fs::metadata(parent) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => {
                eprintln!(
                    "glyph: --output parent `{}` is not a directory",
                    parent.display()
                );
                return ExitCode::from(3);
            }
            Err(_) => {
                eprintln!(
                    "glyph: --output parent directory `{}` does not exist",
                    parent.display()
                );
                return ExitCode::from(3);
            }
        }
        if let Ok(m) = std::fs::metadata(out) {
            if m.is_dir() {
                eprintln!(
                    "glyph: --output target `{}` exists and is a directory",
                    out.display()
                );
                return ExitCode::from(3);
            }
        }
    }

    // --out-dir validation + auto-create
    if let Some(ref dir) = out_dir {
        match std::fs::metadata(dir) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => {
                eprintln!(
                    "glyph: --out-dir `{}` exists and is not a directory",
                    dir.display()
                );
                return ExitCode::from(3);
            }
            Err(_) => {
                if let Err(e) = std::fs::create_dir_all(dir) {
                    eprintln!("glyph: cannot create --out-dir `{}`: {}", dir.display(), e);
                    return ExitCode::from(3);
                }
            }
        }
    }

    let layout = build_layout(
        &path,
        metadata.is_dir(),
        output.as_deref(),
        out_dir.as_deref(),
    );

    if metadata.is_dir() {
        return run_compile_directory(path, format, emit_ir, strict, enable_effects, layout);
    }

    // Single-file compile: walk the import closure, then dispatch through the
    // shared pipeline runner so the analyzer sees imported names.
    let files = glyph_core::compute_import_closure(&path, enable_effects);
    run_pipeline_on_files(&files, format, emit_ir, strict, enable_effects, layout)
}

fn build_layout(
    input: &std::path::Path,
    input_is_dir: bool,
    output: Option<&std::path::Path>,
    out_dir: Option<&std::path::Path>,
) -> glyph_core::CompileOutputLayout {
    if let Some(out) = output {
        let parent_raw = out.parent().unwrap_or_else(|| std::path::Path::new("."));
        let parent = if parent_raw.as_os_str().is_empty() {
            std::path::Path::new(".")
        } else {
            parent_raw
        };
        let canon_parent = parent
            .canonicalize()
            .unwrap_or_else(|_| parent.to_path_buf());
        let file_name = out
            .file_name()
            .expect("invariant: --output file_name validated in run_compile");
        let canon_output = canon_parent.join(file_name);
        let canon_entry = input.canonicalize().unwrap_or_else(|_| input.to_path_buf());
        glyph_core::CompileOutputLayout::EntryFile {
            entry: canon_entry,
            output: canon_output,
        }
    } else if let Some(dir) = out_dir {
        let canon_root = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        let input_root = if input_is_dir {
            input.to_path_buf()
        } else {
            input
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .to_path_buf()
        };
        let canon_input_root = input_root
            .canonicalize()
            .unwrap_or_else(|_| input_root.clone());
        glyph_core::CompileOutputLayout::OutDir {
            root: canon_root,
            input_root: canon_input_root,
        }
    } else {
        glyph_core::CompileOutputLayout::SameDir
    }
}

/// Directory-mode compile: collect all `.glyph` files, build DAG, compile
/// in topological order with partial failure.
fn run_compile_directory(
    path: PathBuf,
    format: OutputFormat,
    emit_ir: bool,
    strict: bool,
    enable_effects: bool,
    layout: glyph_core::CompileOutputLayout,
) -> ExitCode {
    let direct_files = match collect_glyph_sources(&path) {
        Ok(v) => v,
        Err(code) => return code,
    };

    // Expand each collected file's import closure so that transitive
    // dependencies outside the directory (e.g. shared libraries) are included
    // in the pipeline. Without this, cross-file symbol resolution fails for
    // imports that point outside the input root.
    let mut seen = std::collections::HashSet::new();
    let mut files: Vec<PathBuf> = Vec::new();
    for f in &direct_files {
        for dep in glyph_core::compute_import_closure(f, enable_effects) {
            if seen.insert(dep.clone()) {
                files.push(dep);
            }
        }
    }
    files.sort();

    run_pipeline_on_files(&files, format, emit_ir, strict, enable_effects, layout)
}

/// Compile a pre-collected list of `.glyph` files through the directory
/// pipeline and translate the build result into a CLI exit code. Shared by
/// single-file and directory modes.
fn run_pipeline_on_files(
    files: &[PathBuf],
    format: OutputFormat,
    emit_ir: bool,
    strict: bool,
    enable_effects: bool,
    layout: glyph_core::CompileOutputLayout,
) -> ExitCode {
    if files.is_empty() {
        return ExitCode::from(0);
    }

    let result = glyph_core::compile_directory_with_layout(files, emit_ir, enable_effects, &layout);

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
                let file_name = file_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                eprintln!(
                    "warning[G::build::skipped-due-to-failed-import]: `{}` skipped because `{}` failed",
                    file_path.display(),
                    failed_dep.display(),
                );
                let stale_path = glyph_core::resolve_output_path(
                    file_path,
                    glyph_core::OutputKind::Compiled,
                    &layout,
                );
                if stale_path.exists() {
                    eprintln!(
                        "note: `{}` was not regenerated; the on-disk version reflects the previous successful build of `{}` and may be out of sync.",
                        stale_path.display(),
                        file_name,
                    );
                }
            }
        }
    }

    let code = result.exit_code;
    if strict && code == 2 {
        return ExitCode::from(1);
    }
    ExitCode::from(code)
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
    // Translate a 1-indexed (line, col) byte position into a byte offset
    // into `source`. Both line and col are interpreted byte-wise (matching
    // `LineIndex::line_col` in `glyph_core::span`, which is what the
    // upstream diagnostic emitter uses). Column is clamped to the located
    // line's length so an out-of-line column still maps inside that line —
    // crucial for codespan-reporting, which would otherwise misattribute
    // the carry-over byte to the next line.
    if line == 0 {
        return source.len().saturating_sub(1);
    }
    let bytes = source.as_bytes();
    let mut line_start: usize = 0;
    if line > 1 {
        let mut current_line: u32 = 1;
        let mut found = false;
        for (idx, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                current_line += 1;
                if current_line == line {
                    line_start = idx + 1;
                    found = true;
                    break;
                }
            }
        }
        if !found {
            return source.len().saturating_sub(1);
        }
    }
    let line_end = bytes[line_start..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|p| line_start + p)
        .unwrap_or(bytes.len());
    let col_offset = (col.saturating_sub(1) as usize).min(line_end - line_start);
    line_start + col_offset
}
