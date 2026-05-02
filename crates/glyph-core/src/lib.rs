//! glyph-core: deterministic compiler phases for the Glyph language.
//!
//! Walking-skeleton scope (slice 1): minimum viable Phase 1 / 2 / 4 / 5 / 6-Step1 / 7
//! that produces a byte-identical golden snapshot for `update_docs.glyph.md` per
//! `design/mvp-acceptance.md` §1.

pub mod analyze;
pub mod ast;
pub mod diagnostic;
pub mod emit;
pub mod emit_ir;
pub mod expand;
pub mod fmt;
pub mod ir;
pub mod kind_infer;
pub mod lower;
pub mod parse;
pub mod slot;
pub mod span;
pub mod tokenize;
pub mod validate;
pub mod validate_output;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ast::{Decl, ImportKind};
use crate::diagnostic::{Classification, DiagBag, Diagnostic, SourceSpan};
use crate::ir::IrNode;
use crate::span::{LineIndex, Span};

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
    Compiled { markdown: String, diagnostics: DiagBag, arena: ir::IrArena },
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
    Ok(CompileOutcome::Compiled { markdown, diagnostics: bag, arena })
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
    if let CompileOutcome::Compiled { ref markdown, ref arena, .. } = outcome {
        let out_path = compiled_output_path(path);
        let _ = arena; // arena available for --emit-ir; unused in compile_file
        atomic_write(&out_path, markdown).map_err(|e| CompileError::Write {
            path: out_path.display().to_string(),
            source: e,
        })?;
    }
    Ok(outcome)
}

/// Resolve an import path relative to the importing file's directory.
///
/// If the path doesn't end with `.glyph.md` and no file exists at the literal
/// path, appends `.glyph.md` and retries. Returns the canonical path.
fn resolve_import_path(importer: &Path, import_path: &str) -> Option<PathBuf> {
    let base_dir = importer.parent().unwrap_or_else(|| Path::new("."));
    let candidate = base_dir.join(import_path);
    if candidate.is_file() {
        return candidate.canonicalize().ok();
    }
    // Auto-resolution: try appending `.glyph.md`.
    if !import_path.ends_with(".glyph.md") {
        let with_ext = base_dir.join(format!("{}.glyph.md", import_path));
        if with_ext.is_file() {
            return with_ext.canonicalize().ok();
        }
    }
    None
}

/// Describes what a file exports — used for cross-file name resolution.
#[derive(Clone, Debug)]
pub struct ExportedNames {
    /// Names of exported `text` declarations.
    pub texts: HashSet<String>,
    /// Names of exported `block` declarations.
    pub blocks: HashSet<String>,
    /// Names of `skill` declarations (not importable selectively).
    pub skills: HashSet<String>,
    /// Names of private (non-exported) declarations.
    pub privates: HashSet<String>,
}

/// Extract the exported names from a parsed source file.
fn extract_exports(file: &ast::SourceFile) -> ExportedNames {
    let mut exports = ExportedNames {
        texts: HashSet::new(),
        blocks: HashSet::new(),
        skills: HashSet::new(),
        privates: HashSet::new(),
    };
    for decl in &file.decls {
        match decl {
            Decl::Const(c) => {
                // Exported consts share the `texts` namespace (post-issue-#81
                // `const` is the sole value-binding form); non-exported
                // (including `generated const`) are private.
                if c.node.exported {
                    exports.texts.insert(c.node.name.clone());
                } else {
                    exports.privates.insert(c.node.name.clone());
                }
            }
            Decl::ExportBlock(b) => {
                exports.blocks.insert(b.node.name.clone());
            }
            Decl::Block(b) => {
                exports.privates.insert(b.node.name.clone());
            }
            Decl::Skill(s) => {
                exports.skills.insert(s.node.name.clone());
            }
            Decl::Import(_) => {}
        }
    }
    exports
}

/// Run Phase 1 (Parse) and Phase 2 (Analyze) on a file at `path`, resolving
/// imports from dependency files. This is the import-aware version of
/// `check_source`.
///
/// Handles:
/// - Path resolution (relative to importer)
/// - Missing file detection (`G::analyze::missing-file`)
/// - Circular import detection (`G::analyze::circular-import`)
/// - Private/skill import validation (`G::analyze::import-private`, `G::analyze::import-skill`)
/// - Duplicate/unused import detection
/// - Cross-file name resolution
pub fn check_file(path: &Path) -> DiagBag {
    let mut bag = DiagBag::new();
    let canon = match path.canonicalize() {
        Ok(c) => c,
        Err(_) => {
            let span = Span::new(0, 0, 0);
            let li = LineIndex::new("");
            bag.push(
                Diagnostic::error(
                    "G::analyze::missing-file",
                    format!("cannot read `{}`", path.display()),
                    SourceSpan::from_byte_span(path.to_string_lossy().as_ref(), span, &li),
                ),
                span,
            );
            return bag;
        }
    };

    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut stack: Vec<PathBuf> = Vec::new();
    check_file_recursive(&canon, &mut bag, &mut visited, &mut stack);
    bag
}

fn check_file_recursive(
    path: &Path,
    bag: &mut DiagBag,
    visited: &mut HashSet<PathBuf>,
    stack: &mut Vec<PathBuf>,
) -> Option<ExportedNames> {
    // Cycle detection.
    if let Some(pos) = stack.iter().position(|p| p == path) {
        let cycle: Vec<String> = stack[pos..]
            .iter()
            .chain(std::iter::once(&path.to_path_buf()))
            .map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned())
            .collect();
        let cycle_str = cycle.join(" -> ");
        let span = Span::new(0, 0, 0);
        let li = LineIndex::new("");
        bag.push(
            Diagnostic::error(
                "G::analyze::circular-import",
                format!("circular import: {}", cycle_str),
                SourceSpan::from_byte_span(
                    path.file_name().unwrap_or_default().to_string_lossy().as_ref(),
                    span,
                    &li,
                ),
            ),
            span,
        );
        return None;
    }

    // Already processed (no cycle, just shared dependency).
    if visited.contains(path) {
        // Re-parse to extract exports (could cache, but keep it simple).
        let source = std::fs::read_to_string(path).ok()?;
        let line_index = LineIndex::new(&source);
        let mut tmp_bag = DiagBag::new();
        let file_label = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let parsed = parse::parse_with_diagnostics(&source, 0, &file_label, &line_index, &mut tmp_bag)?;
        return Some(extract_exports(&parsed));
    }

    visited.insert(path.to_path_buf());
    stack.push(path.to_path_buf());

    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => {
            let span = Span::new(0, 0, 0);
            let li = LineIndex::new("");
            bag.push(
                Diagnostic::error(
                    "G::analyze::missing-file",
                    format!("cannot read `{}`", path.display()),
                    SourceSpan::from_byte_span(
                        path.file_name().unwrap_or_default().to_string_lossy().as_ref(),
                        span,
                        &li,
                    ),
                ),
                span,
            );
            stack.pop();
            return None;
        }
    };

    let file_label = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let line_index = LineIndex::new(&source);

    let parsed = parse::parse_with_diagnostics(&source, 0, &file_label, &line_index, bag);
    let file = match parsed {
        Some(f) => f,
        None => {
            stack.pop();
            return None;
        }
    };

    // Collect imported names for cross-file resolution.
    let mut imported_texts: HashSet<String> = HashSet::new();
    let mut imported_blocks: HashSet<String> = HashSet::new();
    let mut seen_import_paths: HashMap<PathBuf, Span> = HashMap::new();
    let mut used_import_names: HashSet<String> = HashSet::new();
    let mut all_import_names: Vec<(String, Span)> = Vec::new();

    for decl in &file.decls {
        if let Decl::Import(import_spanned) = decl {
            let import = &import_spanned.node;
            let import_span = import_spanned.span;

            // Handle `@glyph/` stdlib imports (compiler-embedded, not filesystem).
            if import.path.starts_with("@glyph/") {
                if import.path == "@glyph/std" {
                    // Stdlib module: exports `subagent` and `send` as blocks.
                    // `load` is compiler-internal and NOT importable.
                    match &import.kind {
                        ImportKind::Selective(names) => {
                            for imp_name in names {
                                let local = imp_name.alias.as_deref().unwrap_or(&imp_name.name);
                                all_import_names.push((local.to_string(), import_span));
                                if imp_name.name == "subagent" || imp_name.name == "send" {
                                    imported_blocks.insert(local.to_string());
                                } else {
                                    bag.push(
                                        Diagnostic::error(
                                            "G::analyze::import-private",
                                            format!("`{}` is not exported from `{}`", imp_name.name, import.path),
                                            SourceSpan::from_byte_span(&file_label, import_span, &line_index),
                                        ),
                                        import_span,
                                    );
                                }
                            }
                        }
                        ImportKind::WholeModule { alias } => {
                            all_import_names.push((alias.clone(), import_span));
                            imported_blocks.insert(format!("{}.subagent", alias));
                            imported_blocks.insert(format!("{}.send", alias));
                        }
                    }
                } else {
                    bag.push(
                        Diagnostic::error(
                            "G::imports::unknown-stdlib-module",
                            format!("unknown stdlib module `{}`", import.path),
                            SourceSpan::from_byte_span(&file_label, import_span, &line_index),
                        ),
                        import_span,
                    );
                }
                continue;
            }

            // Resolve the import path.
            let resolved = resolve_import_path(path, &import.path);
            let resolved = match resolved {
                Some(r) => r,
                None => {
                    bag.push(
                        Diagnostic::error(
                            "G::analyze::missing-file",
                            format!("imported file `{}` not found", import.path),
                            SourceSpan::from_byte_span(&file_label, import_span, &line_index),
                        ),
                        import_span,
                    );
                    continue;
                }
            };

            // Duplicate import detection.
            if let Some(prev_span) = seen_import_paths.get(&resolved) {
                bag.push(
                    Diagnostic {
                        id: "G::analyze::duplicate-import".into(),
                        classification: Classification::Repairable,
                        message: format!("duplicate import of `{}`", import.path),
                        span: SourceSpan::from_byte_span(&file_label, import_span, &line_index),
                        related: vec![SourceSpan::from_byte_span(&file_label, *prev_span, &line_index)],
                        hints: vec!["merge the import lists or remove the duplicate".into()],
                    },
                    import_span,
                );
                continue;
            }
            seen_import_paths.insert(resolved.clone(), import_span);

            // Recursively check/parse the dependency.
            let dep_exports = check_file_recursive(&resolved, bag, visited, stack);
            let dep_exports = match dep_exports {
                Some(e) => e,
                None => continue,
            };

            // Validate each imported name.
            match &import.kind {
                ImportKind::Selective(names) => {
                    for imp_name in names {
                        let local = imp_name.alias.as_deref().unwrap_or(&imp_name.name);
                        all_import_names.push((local.to_string(), import_span));

                        if dep_exports.skills.contains(&imp_name.name) {
                            bag.push(
                                Diagnostic::error(
                                    "G::analyze::import-skill",
                                    format!("`{}` is a `skill` and cannot be selectively imported", imp_name.name),
                                    SourceSpan::from_byte_span(&file_label, import_span, &line_index),
                                ),
                                import_span,
                            );
                        } else if dep_exports.privates.contains(&imp_name.name) {
                            bag.push(
                                Diagnostic::error(
                                    "G::analyze::import-private",
                                    format!("`{}` is not exported from `{}`", imp_name.name, import.path),
                                    SourceSpan::from_byte_span(&file_label, import_span, &line_index),
                                ),
                                import_span,
                            );
                        } else if dep_exports.texts.contains(&imp_name.name) {
                            imported_texts.insert(local.to_string());
                        } else if dep_exports.blocks.contains(&imp_name.name) {
                            imported_blocks.insert(local.to_string());
                        } else {
                            bag.push(
                                Diagnostic::error(
                                    "G::analyze::import-private",
                                    format!("`{}` is not exported from `{}`", imp_name.name, import.path),
                                    SourceSpan::from_byte_span(&file_label, import_span, &line_index),
                                ),
                                import_span,
                            );
                        }
                    }
                }
                ImportKind::WholeModule { alias } => {
                    // Whole-module import: all exported names available as `alias.name`.
                    // For now, just record that the alias is used. Whole-module imports
                    // don't selectively import names so they skip per-name validation.
                    all_import_names.push((alias.clone(), import_span));
                    // Make all exported names available prefixed.
                    for t in &dep_exports.texts {
                        imported_texts.insert(format!("{}.{}", alias, t));
                    }
                    for b in &dep_exports.blocks {
                        imported_blocks.insert(format!("{}.{}", alias, b));
                    }
                }
            }
        }
    }

    // Run Phase 2 with import-augmented name sets.
    let _ = analyze::analyze_with_imports(
        &file,
        0,
        &file_label,
        &line_index,
        bag,
        &imported_texts,
        &imported_blocks,
        &mut used_import_names,
        &HashMap::new(),
    );

    // Unused import detection.
    for (name, span) in &all_import_names {
        if !used_import_names.contains(name) {
            bag.push(
                Diagnostic {
                    id: "G::analyze::unused-import".into(),
                    classification: Classification::Repairable,
                    message: format!("imported name `{}` is never used", name),
                    span: SourceSpan::from_byte_span(&file_label, *span, &line_index),
                    related: Vec::new(),
                    hints: vec!["remove the unused import".into()],
                },
                *span,
            );
        }
    }

    stack.pop();
    Some(extract_exports(&file))
}

/// Result of compiling a directory of `.glyph.md` files.
#[derive(Debug)]
pub struct BuildResult {
    /// Per-file outcomes, keyed by the source path.
    pub outcomes: Vec<(PathBuf, FileOutcome)>,
    /// Overall exit code for the build (0 = all ok, 1 = any failure/skip).
    pub exit_code: u8,
}

/// Per-file outcome in a multi-file build.
#[derive(Debug)]
pub enum FileOutcome {
    /// File compiled successfully; `.md` was written.
    Compiled { diagnostics: DiagBag },
    /// File failed during compilation (Phases 1-7 produced errors).
    Failed { diagnostics: DiagBag },
    /// File was skipped because a transitive dependency failed.
    Skipped { failed_dep: PathBuf },
}

/// Compile all `.glyph.md` files in `sources` (already collected and sorted).
///
/// Builds the import DAG, topological-sorts, compiles each file in order.
/// Implements partial failure: skip-dependents, leave-stale-`.md`, exit 1 if
/// any file fails.
pub fn compile_directory(sources: &[PathBuf]) -> BuildResult {
    compile_directory_with_options(sources, false)
}

pub fn compile_directory_with_options(sources: &[PathBuf], emit_ir: bool) -> BuildResult {
    if sources.is_empty() {
        return BuildResult { outcomes: Vec::new(), exit_code: 0 };
    }

    // Phase 1 (partial): parse each file to extract import paths and build the DAG.
    let mut file_imports: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    let mut canonical_paths: Vec<PathBuf> = Vec::new();

    for src_path in sources {
        let canon = match src_path.canonicalize() {
            Ok(c) => c,
            Err(_) => src_path.to_path_buf(),
        };
        canonical_paths.push(canon.clone());

        let source = match std::fs::read_to_string(&canon) {
            Ok(s) => s,
            Err(_) => {
                file_imports.insert(canon, Vec::new());
                continue;
            }
        };

        let label = canon.display().to_string();
        let line_index = LineIndex::new(&source);
        let mut tmp_bag = DiagBag::new();
        let parsed = parse::parse_with_diagnostics(&source, 0, &label, &line_index, &mut tmp_bag);

        let mut deps = Vec::new();
        if let Some(file) = parsed {
            for decl in &file.decls {
                if let Decl::Import(import_spanned) = decl {
                    // Skip @glyph/ stdlib imports — they are compiler-embedded, not filesystem.
                    if import_spanned.node.path.starts_with("@glyph/") {
                        continue;
                    }
                    if let Some(resolved) = resolve_import_path(&canon, &import_spanned.node.path) {
                        deps.push(resolved);
                    }
                }
            }
        }
        file_imports.insert(canon, deps);
    }

    // Topological sort (Kahn's algorithm).
    let all_files: Vec<PathBuf> = canonical_paths;
    let file_set: HashSet<PathBuf> = all_files.iter().cloned().collect();

    // Build in-degree map and adjacency (dep -> dependents).
    let mut in_degree: HashMap<PathBuf, usize> = HashMap::new();
    let mut dependents: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for f in &all_files {
        in_degree.entry(f.clone()).or_insert(0);
    }
    for (file, deps) in &file_imports {
        for dep in deps {
            if file_set.contains(dep) {
                *in_degree.entry(file.clone()).or_insert(0) += 1;
                dependents.entry(dep.clone()).or_default().push(file.clone());
            }
        }
    }

    // Kahn's: start with zero in-degree nodes, sorted for determinism.
    let mut queue: Vec<PathBuf> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(f, _)| f.clone())
        .collect();
    queue.sort();

    let mut topo_order: Vec<PathBuf> = Vec::new();
    while let Some(node) = queue.pop() {
        // pop from end; since we sorted ascending, reverse to get smallest first
        // Actually, let's use a proper approach: sort and drain from front.
        topo_order.push(node.clone());
        if let Some(deps) = dependents.get(&node) {
            let mut newly_ready = Vec::new();
            for dep in deps {
                if let Some(deg) = in_degree.get_mut(dep) {
                    *deg -= 1;
                    if *deg == 0 {
                        newly_ready.push(dep.clone());
                    }
                }
            }
            newly_ready.sort();
            for nr in newly_ready {
                queue.push(nr);
            }
            queue.sort();
        }
    }

    // Any files not in topo_order are in cycles — treat as failed.
    for f in &all_files {
        if !topo_order.contains(f) {
            topo_order.push(f.clone());
        }
    }

    // Compile each file in topological order with partial failure.
    // Track procedure file paths emitted by library files for Tier 3 references.
    // Key: (library canonical path, block name) → relative procedure path.
    let mut procedure_paths: HashMap<(PathBuf, String), String> = HashMap::new();
    // Track exported names, text values, and block bodies per file for cross-file resolution.
    let mut file_exports: HashMap<PathBuf, ExportedNames> = HashMap::new();
    let mut file_text_values: HashMap<(PathBuf, String), String> = HashMap::new();
    let mut file_block_bodies: HashMap<(PathBuf, String), String> = HashMap::new();
    let mut file_block_descriptions: HashMap<(PathBuf, String), String> = HashMap::new();
    let mut failed_files: HashSet<PathBuf> = HashSet::new();
    let mut outcomes: Vec<(PathBuf, FileOutcome)> = Vec::new();
    let mut any_failure = false;

    for file in &topo_order {
        // Check if any dependency failed.
        let deps = file_imports.get(file).cloned().unwrap_or_default();
        let failed_dep = deps.iter().find(|d| file_set.contains(*d) && failed_files.contains(*d));

        if let Some(fd) = failed_dep {
            // Skip this file — a dependency failed.
            failed_files.insert(file.clone());
            any_failure = true;
            outcomes.push((file.clone(), FileOutcome::Skipped { failed_dep: fd.clone() }));
            continue;
        }

        // Compile the file.
        // Build the imported-block-to-procedure-path mapping for this file.
        let imported_procedure_paths = build_imported_procedure_paths(
            file, &file_imports, &procedure_paths,
        );

        // Build full resolved import data from dependency exports.
        let resolved_imports = build_resolved_imports(
            file, &file_exports, &file_text_values, &file_block_bodies, &file_block_descriptions,
        );

        match compile_file_with_resolved_imports(file, &imported_procedure_paths, &resolved_imports) {
            Ok(CompileOutcome::Compiled { diagnostics, arena, .. }) => {
                extract_and_store_exports(file, &mut file_exports, &mut file_text_values, &mut file_block_bodies, &mut file_block_descriptions);
                if emit_ir {
                    let source_file = file.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    if let Some(ir_json) = emit_ir::serialize_ir_json(&arena, source_file) {
                        let ir_path = ir_json_output_path(file);
                        atomic_write(&ir_path, &ir_json).ok();
                    }
                }
                outcomes.push((file.clone(), FileOutcome::Compiled { diagnostics }));
            }
            Ok(CompileOutcome::Diagnostics(bag)) => {
                failed_files.insert(file.clone());
                any_failure = true;
                outcomes.push((file.clone(), FileOutcome::Failed { diagnostics: bag }));
            }
            Err(CompileError::Lower(lower::LowerError::NoSkill)) => {
                // Library file (no skill declaration) — not a failure.
                extract_and_store_exports(file, &mut file_exports, &mut file_text_values, &mut file_block_bodies, &mut file_block_descriptions);
                // Emit procedure files for qualifying export blocks (Tier 3).
                let emitted = emit_library_procedures(file);
                for (block_name, rel_path) in emitted {
                    procedure_paths.insert((file.clone(), block_name), rel_path);
                }
                outcomes.push((file.clone(), FileOutcome::Compiled { diagnostics: DiagBag::new() }));
            }
            Err(_e) => {
                failed_files.insert(file.clone());
                any_failure = true;
                let bag = DiagBag::new();
                outcomes.push((file.clone(), FileOutcome::Failed { diagnostics: bag }));
            }
        }
    }

    let exit_code = if any_failure { 1 } else { 0 };
    BuildResult { outcomes, exit_code }
}

/// Write `content` to `path` atomically: write to `path.tmp`, then rename.
/// On failure, delete the `.tmp` and leave any prior `path` untouched.
pub fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    let tmp_path = tmp_path_for(path);
    // Clean stale .tmp from a previous interrupted run.
    let _ = std::fs::remove_file(&tmp_path);
    std::fs::write(&tmp_path, content)?;
    if let Err(e) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }
    Ok(())
}

/// Return the `.tmp` sibling path for a given output path.
fn tmp_path_for(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".tmp");
    PathBuf::from(s)
}

/// Map `foo.glyph.md` → `foo.ir.json` next to the source file.
fn ir_json_output_path(input: &Path) -> PathBuf {
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let file_name = input.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let stem = file_name
        .strip_suffix(".glyph.md")
        .unwrap_or_else(|| file_name.strip_suffix(".md").unwrap_or(file_name));
    parent.join(format!("{}.ir.json", stem))
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

/// Extract the library stem from a source path: `repo_tools.glyph.md` → `repo_tools`.
fn library_stem(input: &Path) -> String {
    let file_name = input.file_name().and_then(|s| s.to_str()).unwrap_or("");
    file_name
        .strip_suffix(".glyph.md")
        .unwrap_or(file_name.strip_suffix(".md").unwrap_or(file_name))
        .to_string()
}

/// Emit standalone procedure `.md` files for qualifying export blocks in a
/// library file. An export block qualifies when its body_word_count >= 150.
///
/// Output path: `<parent>/<lib_stem>/<block-name-kebab>.md`
/// Returns: Vec of (block_name, relative_procedure_path) for Tier 3 tracking.
fn emit_library_procedures(path: &Path) -> Vec<(String, String)> {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let parsed = match parse::parse(&source, 0) {
        Ok((file, _)) => file,
        Err(_) => return Vec::new(),
    };

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = library_stem(path);
    let mut emitted = Vec::new();

    for decl in &parsed.decls {
        if let Decl::ExportBlock(eb) = decl {
            if eb.node.body_word_count < 150 {
                continue;
            }
            let kebab_name = eb.node.name.replace('_', "-");
            let subdir = parent.join(&stem);
            std::fs::create_dir_all(&subdir).ok();

            let params: Vec<(String, Option<String>)> = eb
                .node
                .params
                .iter()
                .map(|p| (p.name.clone(), p.default.clone()))
                .collect();

            let desc = eb.node.description.as_deref().unwrap_or("");
            let markdown = emit::emit_procedure(
                &eb.node.name,
                desc,
                &eb.node.effects,
                &params,
                &eb.node.flow_strings,
            );

            let out_path = subdir.join(format!("{}.md", kebab_name));
            atomic_write(&out_path, &markdown).ok();

            let rel_path = format!("{}/{}.md", stem, kebab_name);
            emitted.push((eb.node.name.clone(), rel_path));
        }
    }
    emitted
}

/// Build a mapping from imported block names to their procedure file paths
/// for a given consumer file.
fn build_imported_procedure_paths(
    consumer: &Path,
    _file_imports: &HashMap<PathBuf, Vec<PathBuf>>,
    procedure_paths: &HashMap<(PathBuf, String), String>,
) -> HashMap<String, String> {
    let mut result = HashMap::new();

    // Read the consumer file to find which names are imported from each dependency.
    let source = match std::fs::read_to_string(consumer) {
        Ok(s) => s,
        Err(_) => return result,
    };
    let parsed = match parse::parse(&source, 0) {
        Ok((file, _)) => file,
        Err(_) => return result,
    };

    for decl in &parsed.decls {
        if let Decl::Import(import_spanned) = decl {
            let import = &import_spanned.node;
            // Skip @glyph/ stdlib imports — compiler-embedded, no procedure paths.
            if import.path.starts_with("@glyph/") {
                continue;
            }
            let resolved = resolve_import_path(consumer, &import.path);
            let resolved = match resolved {
                Some(r) => r,
                None => continue,
            };

            match &import.kind {
                ImportKind::Selective(names) => {
                    for imp_name in names {
                        let local = imp_name.alias.as_deref().unwrap_or(&imp_name.name);
                        let key = (resolved.clone(), imp_name.name.clone());
                        if let Some(proc_path) = procedure_paths.get(&key) {
                            result.insert(local.to_string(), proc_path.clone());
                        }
                    }
                }
                ImportKind::WholeModule { alias } => {
                    for ((lib_path, block_name), proc_path) in procedure_paths {
                        if *lib_path == resolved {
                            let qualified = format!("{}.{}", alias, block_name);
                            result.insert(qualified, proc_path.clone());
                        }
                    }
                }
            }
        }
    }
    result
}

/// Resolved import data for a consumer file: text names, block names,
/// text values (for Lower), and block body texts (for Validate).
struct ResolvedImports {
    text_names: HashSet<String>,
    block_names: HashSet<String>,
    text_values: std::collections::BTreeMap<String, String>,
    block_bodies: HashMap<String, String>,
    block_descriptions: HashMap<String, String>,
}

/// Build the full resolved import data for a consumer file.
fn build_resolved_imports(
    consumer: &Path,
    file_exports: &HashMap<PathBuf, ExportedNames>,
    file_text_values: &HashMap<(PathBuf, String), String>,
    file_block_bodies: &HashMap<(PathBuf, String), String>,
    file_block_descriptions: &HashMap<(PathBuf, String), String>,
) -> ResolvedImports {
    let mut result = ResolvedImports {
        text_names: HashSet::new(),
        block_names: HashSet::new(),
        text_values: std::collections::BTreeMap::new(),
        block_bodies: HashMap::new(),
        block_descriptions: HashMap::new(),
    };

    let source = match std::fs::read_to_string(consumer) {
        Ok(s) => s,
        Err(_) => return result,
    };
    let parsed = match parse::parse(&source, 0) {
        Ok((file, _)) => file,
        Err(_) => return result,
    };

    for decl in &parsed.decls {
        if let Decl::Import(import_spanned) = decl {
            let import = &import_spanned.node;
            if import.path.starts_with("@glyph/") {
                continue;
            }
            let resolved = match resolve_import_path(consumer, &import.path) {
                Some(r) => r,
                None => continue,
            };
            let exports = match file_exports.get(&resolved) {
                Some(e) => e,
                None => continue,
            };

            match &import.kind {
                ImportKind::Selective(names) => {
                    for imp_name in names {
                        let local = imp_name.alias.as_deref().unwrap_or(&imp_name.name);
                        if exports.texts.contains(&imp_name.name) {
                            result.text_names.insert(local.to_string());
                            if let Some(val) = file_text_values.get(&(resolved.clone(), imp_name.name.clone())) {
                                result.text_values.insert(local.to_string(), val.clone());
                            }
                        }
                        if exports.blocks.contains(&imp_name.name) {
                            result.block_names.insert(local.to_string());
                            if let Some(body) = file_block_bodies.get(&(resolved.clone(), imp_name.name.clone())) {
                                result.block_bodies.insert(local.to_string(), body.clone());
                            }
                            if let Some(desc) = file_block_descriptions.get(&(resolved.clone(), imp_name.name.clone())) {
                                result.block_descriptions.insert(local.to_string(), desc.clone());
                            }
                        }
                    }
                }
                ImportKind::WholeModule { alias } => {
                    for name in &exports.texts {
                        let qualified = format!("{}.{}", alias, name);
                        result.text_names.insert(qualified.clone());
                        if let Some(val) = file_text_values.get(&(resolved.clone(), name.clone())) {
                            result.text_values.insert(qualified, val.clone());
                        }
                    }
                    for name in &exports.blocks {
                        let qualified = format!("{}.{}", alias, name);
                        result.block_names.insert(qualified.clone());
                        if let Some(body) = file_block_bodies.get(&(resolved.clone(), name.clone())) {
                            result.block_bodies.insert(qualified.clone(), body.clone());
                        }
                        if let Some(desc) = file_block_descriptions.get(&(resolved.clone(), name.clone())) {
                            result.block_descriptions.insert(qualified, desc.clone());
                        }
                    }
                }
            }
        }
    }
    result
}

/// Extract and store exports (names, text values, block bodies) from a successfully compiled file.
fn extract_and_store_exports(
    file: &Path,
    file_exports: &mut HashMap<PathBuf, ExportedNames>,
    file_text_values: &mut HashMap<(PathBuf, String), String>,
    file_block_bodies: &mut HashMap<(PathBuf, String), String>,
    file_block_descriptions: &mut HashMap<(PathBuf, String), String>,
) {
    let source = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => return,
    };
    let parsed = match parse::parse(&source, 0) {
        Ok((file_ast, _)) => file_ast,
        Err(_) => return,
    };
    let exports = extract_exports(&parsed);
    // Store exported `const` values for cross-file inline resolution. Values
    // are rendered as source-text (kind-agnostic at inline sites per #81
    // chunk 2 / Option C — Text-equivalent observable output). Bool values
    // are normalized to lowercase here per `design/values-and-names.md`
    // §Booleans, mirroring the local `lower::collect_consts` boundary.
    for decl in &parsed.decls {
        if let Decl::Const(c) = decl {
            if c.node.exported {
                let rendered = match &c.node.value {
                    ast::ConstValue::Bool(s) => s.to_ascii_lowercase(),
                    other => other.rendered().to_string(),
                };
                file_text_values.insert(
                    (file.to_path_buf(), c.node.name.clone()),
                    rendered,
                );
            }
        }
    }
    // Store block body texts (resolved from flow strings).
    for decl in &parsed.decls {
        if let Decl::ExportBlock(eb) = decl {
            let body_text = eb.node.flow_strings.join(" ");
            if !body_text.is_empty() {
                file_block_bodies.insert((file.to_path_buf(), eb.node.name.clone()), body_text);
            }
            if let Some(ref desc) = eb.node.description {
                file_block_descriptions.insert((file.to_path_buf(), eb.node.name.clone()), desc.clone());
            }
        }
    }
    file_exports.insert(file.to_path_buf(), exports);
}

/// Compile a file with resolved import data (names, values, procedure paths).
fn compile_file_with_resolved_imports(
    path: &Path,
    imported_procedure_paths: &HashMap<String, String>,
    resolved_imports: &ResolvedImports,
) -> Result<CompileOutcome, CompileError> {
    if imported_procedure_paths.is_empty() && resolved_imports.text_names.is_empty() && resolved_imports.block_names.is_empty() {
        return compile_file(path);
    }

    let source = std::fs::read_to_string(path).map_err(|e| CompileError::Read {
        path: path.display().to_string(),
        source: e,
    })?;
    let label = path.display().to_string();

    let outcome = compile_source_with_resolved_imports(&source, 0, &label, imported_procedure_paths, resolved_imports)?;
    if let CompileOutcome::Compiled { ref markdown, ref arena, .. } = outcome {
        let out_path = compiled_output_path(path);
        let _ = arena;
        atomic_write(&out_path, markdown).map_err(|e| CompileError::Write {
            path: out_path.display().to_string(),
            source: e,
        })?;
    }
    Ok(outcome)
}

/// Compile source with full import context: text values for Lower, block bodies for Validate.
fn compile_source_with_resolved_imports(
    source: &str,
    file_id: u32,
    file_label: &str,
    imported_procedure_paths: &HashMap<String, String>,
    resolved_imports: &ResolvedImports,
) -> Result<CompileOutcome, CompileError> {
    let mut bag = DiagBag::new();
    let line_index = LineIndex::new(source);

    let parsed = parse::parse_with_diagnostics(source, file_id, file_label, &line_index, &mut bag);
    if !bag.is_empty() && (bag.has_error() || bag.has_repairable()) {
        return Ok(CompileOutcome::Diagnostics(bag));
    }

    let file = match parsed {
        Some(file) => file,
        None => {
            return Err(CompileError::Parse(parse::ParseError::Eof {
                message: "parser returned no AST and no diagnostics".into(),
            }));
        }
    };

    // Merge procedure-path blocks with the resolved imported blocks.
    let mut all_imported_blocks: HashSet<String> = resolved_imports.block_names.clone();
    for name in imported_procedure_paths.keys() {
        all_imported_blocks.insert(name.clone());
    }

    let mut used_import_names: HashSet<String> = HashSet::new();

    let file = analyze::analyze_with_imports(
        &file, file_id, file_label, &line_index, &mut bag,
        &resolved_imports.text_names, &all_imported_blocks, &mut used_import_names,
        &resolved_imports.block_descriptions,
    );
    if bag.has_error() || bag.has_repairable() {
        return Ok(CompileOutcome::Diagnostics(bag));
    }

    // Lower with imported text values available for constraint/context resolution.
    let mut arena = lower::lower_with_imports(&file, &resolved_imports.text_values).map_err(CompileError::Lower)?;

    // Tag imported block calls with resolved body text or Tier 3 procedure paths.
    for node in arena.nodes_mut() {
        if let IrNode::Call(c) = node {
            if c.resolved_body.is_none() {
                if let Some(proc_path) = imported_procedure_paths.get(&c.target) {
                    c.projection_tier = Some(3);
                    c.procedure_path = Some(proc_path.clone());
                } else if let Some(body) = resolved_imports.block_bodies.get(&c.target) {
                    c.resolved_body = Some(body.clone());
                }
            }
        }
    }

    validate::validate(&arena).map_err(CompileError::Validate)?;
    let arena = expand::expand_step1_with_imported_descriptions(arena, &resolved_imports.block_descriptions);
    let markdown = emit::emit(&arena);
    Ok(CompileOutcome::Compiled { markdown, diagnostics: bag, arena })
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
    fn block_with_description_parses() {
        let src = "\
block greet()
    description: \"Say hello to the user.\"
    flow:
        \"Hello, world!\"

skill main()
    description: \"Main skill.\"
    flow:
        greet()
";
        let (file, _) = parse::parse(src, 0).expect("should parse");
        // Find the block declaration.
        let block = file.decls.iter().find_map(|d| match d {
            ast::Decl::Block(b) => Some(&b.node),
            _ => None,
        });
        let block = block.expect("block should be present");
        assert_eq!(block.name, "greet");
        assert_eq!(
            block.description.as_deref(),
            Some("Say hello to the user.")
        );
        assert_eq!(block.flow.len(), 1);
    }

    #[test]
    fn block_without_description_parses() {
        let src = "\
block greet()
    flow:
        \"Hello, world!\"

skill main()
    description: \"Main skill.\"
    flow:
        greet()
";
        let (file, _) = parse::parse(src, 0).expect("should parse");
        let block = file.decls.iter().find_map(|d| match d {
            ast::Decl::Block(b) => Some(&b.node),
            _ => None,
        });
        let block = block.expect("block should be present");
        assert_eq!(block.name, "greet");
        assert!(block.description.is_none());
    }

    #[test]
    fn block_single_string_shorthand_parses() {
        let src = "\
block greet()
    \"Say hello to the user.\"

skill main()
    description: \"Main skill.\"
    flow:
        greet()
";
        let (file, _) = parse::parse(src, 0).expect("should parse");
        let block = file.decls.iter().find_map(|d| match d {
            ast::Decl::Block(b) => Some(&b.node),
            _ => None,
        });
        let block = block.expect("block should be present");
        assert_eq!(block.flow.len(), 1);
        match &block.flow[0] {
            ast::FlowStmt::InlineString(s) => {
                assert_eq!(s, "Say hello to the user.");
            }
            _ => panic!("expected InlineString"),
        }
    }

    #[test]
    fn call_to_same_file_block_expands_inline() {
        let src = "\
block greet()
    \"Say hello to the user.\"

skill main()
    description: \"Main skill.\"
    flow:
        greet()
";
        let outcome = compile_source(src, 0, "test.glyph.md").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                assert!(
                    markdown.contains("1. Say hello to the user."),
                    "expected inlined block body in Steps:\n{}",
                    markdown
                );
            }
            CompileOutcome::Diagnostics(bag) => {
                let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
                panic!("expected compiled output, got diagnostics: {:?}", ids);
            }
        }
    }

    #[test]
    fn word_count_computed_per_block() {
        let src = "\
block greet()
    \"Say hello to the user.\"

skill main()
    description: \"Main skill.\"
    flow:
        greet()
";
        // Compile and check that expansion happened (word count < 150 = Tier 1 inline).
        let outcome = compile_source(src, 0, "test.glyph.md").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                // The block body has 5 words, well under 150, so it should inline.
                assert!(
                    markdown.contains("Say hello to the user."),
                    "expected inlined block body:\n{}",
                    markdown
                );
            }
            CompileOutcome::Diagnostics(bag) => {
                let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
                panic!("expected compiled output, got diagnostics: {:?}", ids);
            }
        }
    }

    #[test]
    fn block_with_description_accessible_on_ir() {
        // Verify the description is reachable on the IR node by checking the
        // full compile pipeline (description doesn't affect Tier 1 output,
        // but it should be preserved in the IR for later consumers).
        let src = "\
block greet()
    description: \"Greet the user warmly.\"
    flow:
        \"Say hello to the user.\"

skill main()
    description: \"Main skill.\"
    flow:
        greet()
";
        let (file, _) = parse::parse(src, 0).expect("should parse");
        let file = analyze::analyze(file);
        let arena = lower::lower(&file).expect("should lower");
        // Find the Block IR node and check its description.
        let block_node = arena.nodes().iter().find(|n| matches!(n, ir::IrNode::Block(_)));
        let block_node = block_node.expect("IrBlock should exist");
        if let ir::IrNode::Block(b) = block_node {
            assert_eq!(b.description.as_deref(), Some("Greet the user warmly."));
        } else {
            panic!("expected IrBlock");
        }
    }

    #[test]
    fn block_multi_step_inlines_concatenated() {
        let src = "\
block setup()
    flow:
        \"Check the environment.\"
        \"Install dependencies.\"

skill main()
    description: \"Main skill.\"
    flow:
        setup()
";
        let outcome = compile_source(src, 0, "test.glyph.md").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                // Multi-step block body should be concatenated with spaces for Tier 1.
                assert!(
                    markdown.contains("Check the environment. Install dependencies."),
                    "expected concatenated body in Steps:\n{}",
                    markdown
                );
            }
            CompileOutcome::Diagnostics(bag) => {
                let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
                panic!("expected compiled output, got diagnostics: {:?}", ids);
            }
        }
    }

    #[test]
    fn undefined_call_fires_diagnostic() {
        let src = "\
skill main()
    description: \"Main skill.\"
    flow:
        unknown_block()
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::undefined-call"),
            "expected undefined-call diagnostic, got: {:?}",
            ids
        );
        // undefined-call is repairable (Phase 3 Repair generates a block).
        let diag = bag.iter().find(|d| d.id == "G::analyze::undefined-call").unwrap();
        assert_eq!(
            diag.classification,
            diagnostic::Classification::Repairable,
            "undefined-call should be repairable"
        );
    }

    #[test]
    fn effects_none_with_other_effects_rejected() {
        // `effects: none, reads_files` must produce G::parse::none-with-effects (error).
        let src = "\
skill main()
    description: \"Main skill.\"
    effects: none, reads_files
    flow:
        \"Do something.\"
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::parse::none-with-effects"),
            "expected G::parse::none-with-effects, got: {:?}",
            ids
        );
        assert_eq!(bag.exit_code(), 1, "none-with-effects should be a hard error");
    }

    #[test]
    fn effects_under_declared_produces_error() {
        // Skill declares `effects: reads_files` but calls a block that has
        // `effects: writes_files`. The inferred set is {reads_files, writes_files}
        // which is a superset of declared {reads_files} → under-declared error.
        let src = "\
block writer()
    effects: writes_files
    \"Write some files.\"

skill main()
    description: \"Main skill.\"
    effects: reads_files
    flow:
        writer()
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::effects-under-declared"),
            "expected effects-under-declared, got: {:?}",
            ids
        );
        assert_eq!(bag.exit_code(), 1, "under-declared should be a hard error");
    }

    #[test]
    fn effects_over_declared_produces_warning_exit_zero() {
        // Skill declares `effects: reads_files, writes_files` but its call graph
        // only infers `reads_files`. The extra `writes_files` is over-declared → warning.
        let src = "\
block reader()
    effects: reads_files
    \"Read some files.\"

skill main()
    description: \"Main skill.\"
    effects: reads_files, writes_files
    flow:
        reader()
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::effects-over-declared"),
            "expected effects-over-declared warning, got: {:?}",
            ids
        );
        // Warning only → exit code 0.
        assert_eq!(bag.exit_code(), 0, "over-declared should exit 0 (warning only)");
        // Classification should be Warning.
        let diag = bag.iter().find(|d| d.id == "G::analyze::effects-over-declared").unwrap();
        assert_eq!(diag.classification, diagnostic::Classification::Warning);
    }

    #[test]
    fn effects_missing_declaration_is_repairable() {
        // Skill omits `effects:` entirely but calls a block with effects.
        // This should fire G::analyze::missing-effects (repairable).
        let src = "\
block writer()
    effects: writes_files
    \"Write some files.\"

skill main()
    description: \"Main skill.\"
    flow:
        writer()
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::missing-effects"),
            "expected missing-effects diagnostic, got: {:?}",
            ids
        );
        assert_eq!(bag.exit_code(), 2, "missing-effects should be repairable (exit 2)");
        let diag = bag.iter().find(|d| d.id == "G::analyze::missing-effects").unwrap();
        assert_eq!(diag.classification, diagnostic::Classification::Repairable);
    }

    #[test]
    fn frontmatter_effects_in_canonical_order() {
        // Declared effects should appear in canonical (alphabetical) order in
        // the compiled frontmatter, regardless of source declaration order.
        let src = "\
skill main()
    description: \"Main skill.\"
    effects: writes_files, reads_files
    flow:
        \"Do something.\"
";
        let outcome = compile_source(src, 0, "test.glyph.md").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                assert!(
                    markdown.contains("effects: [reads_files, writes_files]"),
                    "effects should be alphabetically sorted in frontmatter:\n{}",
                    markdown
                );
            }
            CompileOutcome::Diagnostics(bag) => {
                let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
                panic!("expected compiled output, got diagnostics: {:?}", ids);
            }
        }
    }

    #[test]
    fn frontmatter_omits_effects_when_empty() {
        // When the inferred/declared effects set is empty, the frontmatter should
        // not include an `effects:` field at all.
        let src = "\
skill main()
    description: \"Main skill.\"
    flow:
        \"Do something.\"
";
        let outcome = compile_source(src, 0, "test.glyph.md").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                assert!(
                    !markdown.contains("effects:"),
                    "effects field should be omitted when empty:\n{}",
                    markdown
                );
            }
            CompileOutcome::Diagnostics(bag) => {
                let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
                panic!("expected compiled output, got diagnostics: {:?}", ids);
            }
        }
    }

    #[test]
    fn effects_inferred_from_call_graph_appear_in_frontmatter() {
        // Skill declares effects matching what the call graph infers.
        // The frontmatter should show the effects.
        let src = "\
block reader()
    effects: reads_files
    \"Read files.\"

block writer()
    effects: writes_files
    \"Write files.\"

skill main()
    description: \"Main skill.\"
    effects: reads_files, writes_files
    flow:
        reader()
        writer()
";
        let outcome = compile_source(src, 0, "test.glyph.md").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                assert!(
                    markdown.contains("effects: [reads_files, writes_files]"),
                    "expected inferred effects in frontmatter:\n{}",
                    markdown
                );
            }
            CompileOutcome::Diagnostics(bag) => {
                let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
                panic!("expected compiled output, got diagnostics: {:?}", ids);
            }
        }
    }

    #[test]
    fn effects_transitive_inference_through_call_chain() {
        // Block A calls Block B. Block B has effects: writes_files.
        // Block A has effects: reads_files.
        // Skill calls A, so inferred = {reads_files, writes_files}.
        let src = "\
block inner()
    effects: writes_files
    \"Write files.\"

block outer()
    effects: reads_files
    flow:
        inner()

skill main()
    description: \"Main skill.\"
    effects: reads_files, writes_files
    flow:
        outer()
";
        let bag = check_source(src, 0, "test.glyph.md");
        // No errors or repairables — declared matches inferred exactly.
        assert!(
            !bag.has_error(),
            "should not have errors: {:?}",
            bag.iter().map(|d| &d.id).collect::<Vec<_>>()
        );
        assert!(
            !bag.has_repairable(),
            "should not have repairables: {:?}",
            bag.iter().map(|d| &d.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn effects_none_assertion_with_inferred_effects_is_error() {
        // Skill declares `effects: none` but calls a block with effects.
        // This should be a contradiction — under-declared error.
        let src = "\
block writer()
    effects: writes_files
    \"Write some files.\"

skill main()
    description: \"Main skill.\"
    effects: none
    flow:
        writer()
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::effects-under-declared"),
            "expected effects-under-declared for none-vs-inferred, got: {:?}",
            ids
        );
        assert_eq!(bag.exit_code(), 1);
    }

    #[test]
    fn effects_none_alone_is_valid_when_no_effects_inferred() {
        // Skill declares `effects: none` and calls no blocks with effects.
        // This is valid — no error.
        let src = "\
skill main()
    description: \"Main skill.\"
    effects: none
    flow:
        \"Do something.\"
";
        let bag = check_source(src, 0, "test.glyph.md");
        assert!(
            !bag.has_error(),
            "effects: none with empty inferred set should be valid, got: {:?}",
            bag.iter().map(|d| &d.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn effects_none_omitted_from_frontmatter() {
        // `effects: none` means no effects. The frontmatter should omit
        // the effects field entirely.
        let src = "\
skill main()
    description: \"Main skill.\"
    effects: none
    flow:
        \"Do something.\"
";
        let outcome = compile_source(src, 0, "test.glyph.md").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                assert!(
                    !markdown.contains("effects:"),
                    "effects: none should not appear in frontmatter:\n{}",
                    markdown
                );
            }
            CompileOutcome::Diagnostics(bag) => {
                let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
                panic!("expected compiled output, got diagnostics: {:?}", ids);
            }
        }
    }

    #[test]
    fn return_call_folds_into_final_step() {
        // AC1: `return summarize_changes()` becomes the last sentence of the
        // final numbered step.
        let src = "\
block summarize_changes()
    \"Summarize what was changed and why.\"

skill update_docs()
    description: \"Update documentation.\"
    flow:
        \"Read the repository changes.\"
        return summarize_changes()
";
        let outcome = compile_source(src, 0, "test.glyph.md").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                // The final step should contain the return folding text.
                assert!(
                    markdown.contains("Return the result of summarize_changes()."),
                    "expected return folding in final step:\n{}",
                    markdown
                );
            }
            CompileOutcome::Diagnostics(bag) => {
                let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
                panic!("expected compiled output, got diagnostics: {:?}", ids);
            }
        }
    }

    #[test]
    fn private_block_may_omit_return() {
        // AC2: Private blocks may omit `return`; no diagnostic fires.
        let src = "\
block helper()
    \"Do something helpful.\"

skill main()
    description: \"Main skill.\"
    flow:
        helper()
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::missing-return"),
            "private block should not require return, got: {:?}",
            ids
        );
    }

    #[test]
    fn export_block_requires_return() {
        // AC2: export blocks require explicit `return`.
        // AC3: G::analyze::missing-return fires when export block has no return.
        let src = "\
export block shared_util(x = \"default\")
    flow:
        \"Do something.\"

skill main()
    description: \"Main skill.\"
    flow:
        \"Do something.\"
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::missing-return"),
            "expected G::analyze::missing-return for export block without return, got: {:?}",
            ids
        );
        // Should be repairable.
        let diag = bag.iter().find(|d| d.id == "G::analyze::missing-return").unwrap();
        assert_eq!(diag.classification, diagnostic::Classification::Repairable);
    }

    #[test]
    fn export_block_with_return_no_diagnostic() {
        // Export block with explicit return should not fire missing-return.
        let src = "\
export block shared_util(x = \"default\")
    flow:
        \"Do something.\"
        return x

skill main()
    description: \"Main skill.\"
    flow:
        \"Do something.\"
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::missing-return"),
            "export block with return should not fire missing-return, got: {:?}",
            ids
        );
    }

    // --- Issue #82 chunk 2: G::analyze::export-missing-return-type ---

    #[test]
    fn export_block_meaningful_return_without_arrow_fires() {
        // AC2(a): An export block whose body has `return <expr>` (where <expr>
        // is not the `none` value-keyword) and whose header lacks
        // `-> DomainType` must fire `G::analyze::export-missing-return-type`
        // as repairable.
        let src = "\
export block compute(x = \"default\")
    flow:
        \"Compute something.\"
        return x
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::export-missing-return-type"),
            "expected G::analyze::export-missing-return-type for meaningful return without `->`, got: {:?}",
            ids
        );
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::export-missing-return-type")
            .unwrap();
        assert_eq!(
            diag.classification,
            diagnostic::Classification::Repairable,
            "export-missing-return-type must be Repairable"
        );
    }

    #[test]
    fn export_block_return_none_without_arrow_no_export_missing_return_type() {
        // AC2(b): `return none` at the end of an export block body is the
        // value-position `none` keyword (no meaningful return). With no
        // `-> DomainType` on the header, the new diagnostic must NOT fire.
        // The legacy `missing-return` also must not fire because there *is*
        // an explicit `return`.
        let src = "\
export block notify(msg = \"hello\")
    flow:
        \"Send a notification.\"
        return none
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::export-missing-return-type"),
            "must NOT fire export-missing-return-type for `return none`, got: {:?}",
            ids
        );
        assert!(
            !ids.contains(&"G::analyze::missing-return"),
            "must NOT fire missing-return when `return none` is present, got: {:?}",
            ids
        );
    }

    #[test]
    fn export_block_meaningful_return_with_arrow_passes_clean() {
        // AC2(c): An export block with `-> DomainType` on the header and a
        // meaningful return must NOT fire `export-missing-return-type`.
        let src = "\
export block compute(x = \"default\") -> Path
    flow:
        \"Compute something.\"
        return x
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::export-missing-return-type"),
            "must NOT fire export-missing-return-type when `-> Path` is present, got: {:?}",
            ids
        );
        // And neither the parser's `none-as-return-type` nor analyze's
        // `missing-return` should fire.
        assert!(
            !ids.contains(&"G::analyze::missing-return"),
            "must NOT fire missing-return when return is present, got: {:?}",
            ids
        );
        assert!(
            !ids.contains(&"G::parse::none-as-return-type"),
            "must NOT fire none-as-return-type for `-> Path`, got: {:?}",
            ids
        );
    }

    #[test]
    fn export_block_no_return_still_fires_missing_return_only() {
        // AC2(d): When the export block body has no `return` at all,
        // `missing-return` (legacy) fires, but the new
        // `export-missing-return-type` does NOT — there is no meaningful
        // return to require an annotation.
        let src = "\
export block compute(x = \"default\")
    flow:
        \"Compute something.\"
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::missing-return"),
            "expected G::analyze::missing-return when body has no return, got: {:?}",
            ids
        );
        assert!(
            !ids.contains(&"G::analyze::export-missing-return-type"),
            "must NOT fire export-missing-return-type when there is no return at all, got: {:?}",
            ids
        );
    }

    #[test]
    fn return_not_terminal_fires_diagnostic() {
        // AC3: G::parse::return-not-terminal — return before the last statement.
        let src = "\
skill main()
    description: \"Main skill.\"
    flow:
        return none
        \"Do something after return.\"
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::parse::return-not-terminal"),
            "expected G::parse::return-not-terminal, got: {:?}",
            ids
        );
        assert_eq!(bag.exit_code(), 1);
    }

    #[test]
    fn multiple_returns_fires_diagnostic() {
        // AC3: G::parse::multiple-returns — more than one return.
        let src = "\
skill main()
    description: \"Main skill.\"
    flow:
        return none
        return none
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::parse::multiple-returns"),
            "expected G::parse::multiple-returns, got: {:?}",
            ids
        );
        assert_eq!(bag.exit_code(), 1);
    }

    #[test]
    fn return_in_branch_fires_diagnostic() {
        // AC3: G::parse::return-in-branch — `return` inside a branch context
        // should emit this diagnostic. Since Glyph doesn't have if/elif/else
        // syntax yet, we call check_return_rules directly with in_branch=true.
        use parse::check_return_rules;
        use ast::{FlowStmt, ReturnExpr};
        use span::Span;

        let source = "return none\n";
        let line_index = LineIndex::new(source);
        let sp = Span::new(0, 0, source.len() as u32);
        let flow = vec![FlowStmt::Return(ReturnExpr::None)];
        let mut bag = DiagBag::new();

        check_return_rules(&flow, sp, "test.glyph.md", &line_index, &mut bag, true);

        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::parse::return-in-branch"),
            "expected G::parse::return-in-branch, got: {:?}",
            ids
        );
        assert_eq!(bag.exit_code(), 1);
    }

    #[test]
    fn return_none_implicit_no_folding() {
        // `return none` should not append anything to the final step.
        let src = "\
skill main()
    description: \"Main skill.\"
    flow:
        \"Do something.\"
        return none
";
        let outcome = compile_source(src, 0, "test.glyph.md").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                assert!(
                    !markdown.contains("Return the result of"),
                    "return none should not fold into step:\n{}",
                    markdown
                );
                assert!(
                    markdown.contains("1. Do something."),
                    "step should be preserved:\n{}",
                    markdown
                );
            }
            CompileOutcome::Diagnostics(bag) => {
                let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
                panic!("expected compiled output, got diagnostics: {:?}", ids);
            }
        }
    }

    #[test]
    fn return_bare_name_folds_into_final_step() {
        // `return result` with a bare name should fold.
        let src = "\
skill main()
    description: \"Main skill.\"
    flow:
        \"Compute the result.\"
        return result
";
        let outcome = compile_source(src, 0, "test.glyph.md").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                assert!(
                    markdown.contains("Return the result of result."),
                    "expected return folding for bare name:\n{}",
                    markdown
                );
            }
            CompileOutcome::Diagnostics(bag) => {
                let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
                panic!("expected compiled output, got diagnostics: {:?}", ids);
            }
        }
    }

    #[test]
    fn skill_without_return_compiles_normally() {
        // Skills without return should compile as before (no regression).
        let src = "\
skill main()
    description: \"Main skill.\"
    flow:
        \"Do something.\"
";
        let outcome = compile_source(src, 0, "test.glyph.md").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                assert!(
                    markdown.contains("1. Do something."),
                    "step should be preserved:\n{}",
                    markdown
                );
                assert!(
                    !markdown.contains("Return"),
                    "no return text should appear:\n{}",
                    markdown
                );
            }
            CompileOutcome::Diagnostics(bag) => {
                let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
                panic!("expected compiled output, got diagnostics: {:?}", ids);
            }
        }
    }

    #[test]
    fn branch_parses_if_elif_else() {
        let src = "\
skill main()
    description: \"Main skill.\"
    flow:
        if mode == \"fast\"
            \"Do the fast thing.\"
        elif mode == \"slow\"
            \"Do the slow thing.\"
        else
            \"Do the default thing.\"
";
        let (file, _) = parse::parse(src, 0).expect("should parse");
        let skill = file.decls.iter().find_map(|d| match d {
            ast::Decl::Skill(s) => Some(&s.node),
            _ => None,
        }).unwrap();
        assert_eq!(skill.flow.len(), 1);
        match &skill.flow[0] {
            ast::FlowStmt::Branch { condition, then_body, elif_branches, else_body } => {
                assert_eq!(condition, "mode == \"fast\"");
                assert_eq!(then_body.len(), 1);
                assert_eq!(elif_branches.len(), 1);
                assert_eq!(elif_branches[0].condition, "mode == \"slow\"");
                assert_eq!(elif_branches[0].body.len(), 1);
                assert!(else_body.is_some());
                assert_eq!(else_body.as_ref().unwrap().len(), 1);
            }
            other => panic!("expected Branch, got: {:?}", other),
        }
    }

    #[test]
    fn branch_compiles_with_lettered_substeps() {
        // AC1: branching compiles; output uses lettered sub-steps per arm.
        let src = "\
skill main()
    description: \"Main skill.\"
    flow:
        \"Prepare the environment.\"
        if mode == \"fast\"
            \"Do the fast thing.\"
            \"Log performance metrics.\"
        elif mode == \"slow\"
            \"Do the slow thing.\"
        else
            \"Do the default thing.\"
";
        let outcome = compile_source(src, 0, "test.glyph.md").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                // Step 1 should be the prepare step.
                assert!(markdown.contains("1. Prepare the environment."), "markdown:\n{}", markdown);
                // Step 2 should be the branch with lettered sub-steps.
                assert!(markdown.contains("2. If mode == \"fast\":"), "markdown:\n{}", markdown);
                assert!(markdown.contains("   a. Do the fast thing."), "markdown:\n{}", markdown);
                assert!(markdown.contains("   b. Log performance metrics."), "markdown:\n{}", markdown);
                // elif arm
                assert!(markdown.contains("   If mode == \"slow\":"), "markdown:\n{}", markdown);
                assert!(markdown.contains("   a. Do the slow thing."), "markdown:\n{}", markdown);
                // else arm
                assert!(markdown.contains("   Otherwise:"), "markdown:\n{}", markdown);
                assert!(markdown.contains("   a. Do the default thing."), "markdown:\n{}", markdown);
            }
            CompileOutcome::Diagnostics(bag) => {
                let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
                panic!("expected compiled output, got diagnostics: {:?}", ids);
            }
        }
    }

    #[test]
    fn nested_branch_fires_diagnostic() {
        // AC3: `nested-branch` fires when a branch is nested inside a branch.
        let src = "\
skill main()
    description: \"Main skill.\"
    flow:
        if mode == \"fast\"
            if level == \"high\"
                \"Do the high-fast thing.\"
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::nested-branch"),
            "expected G::analyze::nested-branch, got: {:?}",
            ids
        );
    }

    #[test]
    fn applies_parses_in_branch_condition() {
        // AC5: BLOCKNAME.applies() parses inside if/elif.
        let src = "\
block fast_mode()
    description: \"When the user wants fast processing.\"
    flow:
        \"Do fast processing.\"

skill main()
    description: \"Main skill.\"
    flow:
        if fast_mode.applies()
            fast_mode()
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        // Should NOT have errors — applies() is valid in branch condition.
        assert!(
            !bag.has_error(),
            "applies() in branch condition should be valid, got: {:?}",
            ids
        );
    }

    #[test]
    fn applies_on_non_block_fires_error() {
        // AC7: applies-on-non-block fires when receiver is a const declaration.
        let src = "\
const my_text = \"Some text.\"

skill main()
    description: \"Main skill.\"
    flow:
        if my_text.applies()
            \"Do something.\"
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::applies-on-non-block"),
            "expected applies-on-non-block, got: {:?}",
            ids
        );
    }

    #[test]
    fn applies_on_undescribed_block_fires_repairable() {
        // AC6/AC7: applies-on-undescribed-block fires for same-file block without description.
        let src = "\
block my_block()
    flow:
        \"Do something.\"

skill main()
    description: \"Main skill.\"
    flow:
        if my_block.applies()
            my_block()
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::applies-on-undescribed-block"),
            "expected applies-on-undescribed-block, got: {:?}",
            ids
        );
        // Should be repairable for same-file blocks.
        let diag = bag.iter().find(|d| d.id == "G::analyze::applies-on-undescribed-block").unwrap();
        assert_eq!(diag.classification, diagnostic::Classification::Repairable);
    }

    #[test]
    fn applies_on_unknown_name_fires_non_block_error() {
        // AC7: applies on unknown name.
        let src = "\
skill main()
    description: \"Main skill.\"
    flow:
        if unknown_thing.applies()
            \"Do something.\"
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::applies-on-non-block"),
            "expected applies-on-non-block for unknown receiver, got: {:?}",
            ids
        );
    }

    #[test]
    fn context_in_branch_stays_inline() {
        // AC9: context marker inside a branch body stays inline, does not surface in ### Context.
        let src = "\
const project_info = \"This is a monorepo project.\"

skill main()
    description: \"Main skill.\"
    flow:
        if mode == \"debug\"
            context project_info
            \"Run debug checks.\"
        else
            \"Run normal checks.\"
";
        let outcome = compile_source(src, 0, "test.glyph.md").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                // context should NOT appear as a top-level ### Context section.
                // The branch-scoped context inlines into the sub-step prose.
                assert!(
                    !markdown.contains("### Context"),
                    "branch-scoped context should not surface in ### Context:\n{}",
                    markdown
                );
                // The context text should appear inline in the branch sub-steps.
                assert!(
                    markdown.contains("Note: This is a monorepo project."),
                    "branch-scoped context should be inline:\n{}",
                    markdown
                );
            }
            CompileOutcome::Diagnostics(bag) => {
                let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
                panic!("expected compiled output, got diagnostics: {:?}", ids);
            }
        }
    }

    #[test]
    fn constraint_in_branch_stays_inline() {
        // AC9-parallel: constraint marker inside a branch body stays inline.
        let src = "\
const no_breaking_changes = \"Do not break backwards compatibility.\"

skill main()
    description: \"Main skill.\"
    flow:
        if scope == \"public\"
            require no_breaking_changes
            \"Update the public API docs.\"
        else
            \"Update internal docs.\"
";
        let outcome = compile_source(src, 0, "test.glyph.md").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                // Constraint should NOT appear in ### Constraints.
                assert!(
                    !markdown.contains("### Constraints"),
                    "branch-scoped constraint should not surface in ### Constraints:\n{}",
                    markdown
                );
                // The constraint text should appear inline in the branch sub-steps.
                assert!(
                    markdown.contains("Do not break backwards compatibility."),
                    "branch-scoped constraint should be inline:\n{}",
                    markdown
                );
            }
            CompileOutcome::Diagnostics(bag) => {
                let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
                panic!("expected compiled output, got diagnostics: {:?}", ids);
            }
        }
    }

    #[test]
    fn applies_descriptions_populated_in_expand() {
        // AC6: applies_descriptions side-map is populated post-Step-1.
        let src = "\
block fast_mode()
    description: \"When the user wants fast processing.\"
    flow:
        \"Do fast processing.\"

block slow_mode()
    description: \"When the user wants thorough processing.\"
    flow:
        \"Do slow processing.\"

skill main()
    description: \"Main skill.\"
    flow:
        if fast_mode.applies()
            fast_mode()
        elif slow_mode.applies()
            slow_mode()
        else
            \"Do default processing.\"
";
        let (file, _) = parse::parse(src, 0).expect("should parse");
        let file = analyze::analyze(file);
        let arena = lower::lower(&file).expect("should lower");
        let arena = expand::expand_step1(arena);
        // Find the Branch node.
        let branch = arena.nodes().iter().find_map(|n| match n {
            ir::IrNode::Branch(br) => Some(br),
            _ => None,
        });
        let branch = branch.expect("should have a Branch node");
        let descs = branch.applies_descriptions.as_ref().expect("applies_descriptions should be populated");
        assert_eq!(descs.get("fast_mode").map(|s| s.as_str()), Some("When the user wants fast processing."));
        assert_eq!(descs.get("slow_mode").map(|s| s.as_str()), Some("When the user wants thorough processing."));
    }

    #[test]
    fn pure_applies_branch_renders_decide_form() {
        // AC8: Pure-applies branch arms render via description-keyed projection.
        let src = "\
block fast_mode()
    description: \"When the user wants fast processing.\"
    flow:
        \"Do fast processing.\"

block slow_mode()
    description: \"When the user wants thorough processing.\"
    flow:
        \"Do slow processing.\"

skill main()
    description: \"Main skill.\"
    flow:
        if fast_mode.applies()
            fast_mode()
        elif slow_mode.applies()
            slow_mode()
        else
            \"Do default processing.\"
";
        let outcome = compile_source(src, 0, "test.glyph.md").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                assert!(
                    markdown.contains("Decide which of the following applies"),
                    "expected description-driven projection:\n{}",
                    markdown
                );
                assert!(
                    markdown.contains("When the user wants fast processing."),
                    "expected fast_mode description in output:\n{}",
                    markdown
                );
                assert!(
                    markdown.contains("When the user wants thorough processing."),
                    "expected slow_mode description in output:\n{}",
                    markdown
                );
            }
            CompileOutcome::Diagnostics(bag) => {
                let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
                panic!("expected compiled output, got diagnostics: {:?}", ids);
            }
        }
    }

    #[test]
    fn applies_no_parens_fires_diagnostic() {
        // AC7: applies-no-parens — .applies without ().
        let src = "\
block my_block()
    description: \"Test.\"
    flow:
        \"Do something.\"

skill main()
    description: \"Main skill.\"
    flow:
        if my_block.applies
            \"Do something.\"
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::parse::applies-no-parens"),
            "expected G::parse::applies-no-parens, got: {:?}",
            ids
        );
    }

    #[test]
    fn applies_with_args_fires_diagnostic() {
        // AC7: applies-with-args — .applies(arg) with arguments.
        let src = "\
block my_block()
    description: \"Test.\"
    flow:
        \"Do something.\"

skill main()
    description: \"Main skill.\"
    flow:
        if my_block.applies(x)
            \"Do something.\"
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::parse::applies-with-args"),
            "expected G::parse::applies-with-args, got: {:?}",
            ids
        );
    }

    #[test]
    fn branch_condition_equals_does_not_trigger_operator_in_expression() {
        // AC2: `==` in `if` condition does NOT trigger `operator-in-expression`.
        let src = "\
skill main()
    description: \"Main skill.\"
    flow:
        if mode == \"fast\"
            \"Do the fast thing.\"
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::parse::operator-in-expression"),
            "== in branch condition should not trigger operator-in-expression, got: {:?}",
            ids
        );
    }

    #[test]
    fn applies_outside_branch_condition_is_parse_error() {
        // AC5: applies() is rejected outside branch-condition position.
        // Writing `my_block.applies()` as a flow statement should produce
        // the specific `G::parse::applies-outside-condition` diagnostic.
        let src = "\
block my_block()
    description: \"Test.\"
    flow:
        \"Do something.\"

skill main()
    description: \"Main skill.\"
    flow:
        my_block.applies()
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::parse::applies-outside-condition"),
            "expected G::parse::applies-outside-condition, got: {:?}",
            ids
        );
        // It should NOT compile successfully.
        let outcome = compile_source(src, 0, "test.glyph.md");
        match outcome {
            Ok(CompileOutcome::Compiled { .. }) => {
                panic!("applies() outside branch condition should not compile successfully");
            }
            _ => {
                // Expected — diagnostics block compilation.
            }
        }
    }

    #[test]
    fn with_on_bare_name_fires_diagnostic() {
        // AC4: `G::parse::with-on-bare-name` fires when `with` follows a bare
        // name (no parens), e.g., `some_name with "focus"`.
        let src = "\
skill main()
    description: \"Main skill.\"
    flow:
        some_name with \"focus\"
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::parse::with-on-bare-name"),
            "expected G::parse::with-on-bare-name, got: {:?}",
            ids
        );
        assert_eq!(bag.exit_code(), 1, "with-on-bare-name should be a hard error");
    }

    #[test]
    fn multiple_with_fires_diagnostic() {
        // AC3: `G::parse::multiple-with` fires on chained `with` clauses.
        let src = "\
block foo()
    \"Do something.\"

skill main()
    description: \"Main skill.\"
    flow:
        foo() with \"a\" with \"b\"
";
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::parse::multiple-with"),
            "expected G::parse::multiple-with, got: {:?}",
            ids
        );
        assert_eq!(bag.exit_code(), 1, "multiple-with should be a hard error");
    }

    #[test]
    fn with_modifier_not_applied_in_compiled_output() {
        // AC2: Compiled `.md` from Step 1 does NOT apply the modifier — the
        // modifier is for the agent's Step 2, not the mechanical output.
        let src = "\
block inspect_repo(scope)
    \"Inspect the repo for issues.\"

skill main()
    description: \"Main skill.\"
    flow:
        inspect_repo(scope) with \"focus on auth\"
";
        let outcome = compile_source(src, 0, "test.glyph.md").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                // The modifier text should NOT appear in the compiled output.
                assert!(
                    !markdown.contains("focus on auth"),
                    "modifier text should not appear in compiled .md:\n{}",
                    markdown
                );
                // The call's resolved body should still inline normally.
                assert!(
                    markdown.contains("Inspect the repo for issues."),
                    "block body should still inline:\n{}",
                    markdown
                );
            }
            CompileOutcome::Diagnostics(bag) => {
                let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
                panic!("expected compiled output, got diagnostics: {:?}", ids);
            }
        }
    }

    #[test]
    fn with_modifier_preserved_on_ir_call() {
        // AC2 supplement: verify `site_modifier` is preserved on the IrCall node
        // in the IR arena (for `--emit-ir` consumers).
        let src = "\
block inspect_repo(scope)
    \"Inspect the repo.\"

skill main()
    description: \"Main skill.\"
    flow:
        inspect_repo(scope) with \"focus on auth\"
";
        let (file, _) = parse::parse(src, 0).expect("should parse");
        let file = analyze::analyze(file);
        let arena = lower::lower(&file).expect("should lower");
        let call = arena.nodes().iter().find_map(|n| match n {
            ir::IrNode::Call(c) => Some(c),
            _ => None,
        });
        let call = call.expect("IrCall should exist");
        assert_eq!(call.target, "inspect_repo");
        assert_eq!(call.site_modifier.as_deref(), Some("focus on auth"));
    }

    #[test]
    fn with_modifier_parses_on_call() {
        // AC1: `inspect_repo(scope) with "focus on auth"` parses and stores the
        // modifier on the Call node in the AST.
        let src = "\
block inspect_repo(scope)
    \"Inspect the repo.\"

skill main()
    description: \"Main skill.\"
    flow:
        inspect_repo(scope) with \"focus on auth\"
";
        let (file, _) = parse::parse(src, 0).expect("should parse");
        let skill = file.decls.iter().find_map(|d| match d {
            ast::Decl::Skill(s) => Some(&s.node),
            _ => None,
        }).unwrap();
        assert_eq!(skill.flow.len(), 1);
        match &skill.flow[0] {
            ast::FlowStmt::Call { target, args, site_modifier } => {
                assert_eq!(target, "inspect_repo");
                assert_eq!(args, &["scope".to_string()]);
                assert_eq!(site_modifier.as_deref(), Some("focus on auth"));
            }
            other => panic!("expected Call, got: {:?}", other),
        }
    }

    #[test]
    fn import_selective_parses() {
        let src = r#"import "./prefs.glyph.md" { preserve_existing_patterns }

skill fix_bug()
    description: "Fix a bug."
    flow:
        "Do something."
"#;
        let (file, _) = parse::parse(src, 0).expect("should parse");
        let import = file.decls.iter().find_map(|d| match d {
            ast::Decl::Import(i) => Some(&i.node),
            _ => None,
        });
        let import = import.expect("import should be present");
        assert_eq!(import.path, "./prefs.glyph.md");
        match &import.kind {
            ast::ImportKind::Selective(names) => {
                assert_eq!(names.len(), 1);
                assert_eq!(names[0].name, "preserve_existing_patterns");
                assert!(names[0].alias.is_none());
            }
            _ => panic!("expected selective import"),
        }
    }

    #[test]
    fn import_whole_module_parses() {
        let src = r#"import "./prefs.glyph.md" as prefs

skill fix_bug()
    description: "Fix a bug."
    flow:
        "Do something."
"#;
        let (file, _) = parse::parse(src, 0).expect("should parse");
        let import = file.decls.iter().find_map(|d| match d {
            ast::Decl::Import(i) => Some(&i.node),
            _ => None,
        });
        let import = import.expect("import should be present");
        assert_eq!(import.path, "./prefs.glyph.md");
        match &import.kind {
            ast::ImportKind::WholeModule { alias } => {
                assert_eq!(alias, "prefs");
            }
            _ => panic!("expected whole-module import"),
        }
    }

    #[test]
    fn import_cross_file_name_resolution() {
        // AC1: fix_bug.glyph.md resolves names imported from prefs.glyph.md
        // and repo_tools.glyph.md.
        let dir = tempfile::tempdir().unwrap();

        // prefs.glyph.md — export const
        let prefs_path = dir.path().join("prefs.glyph.md");
        std::fs::write(&prefs_path, r#"export const preserve_existing_patterns = "Prefer existing patterns."
"#).unwrap();

        // repo_tools.glyph.md — export block
        let tools_path = dir.path().join("repo_tools.glyph.md");
        std::fs::write(&tools_path, r#"export block inspect_repo(scope = ".")
    description: "Inspect the repo."
    flow:
        "Examine the repository at {scope}."
        return context
"#).unwrap();

        // fix_bug.glyph.md — imports from both
        let fix_path = dir.path().join("fix_bug.glyph.md");
        std::fs::write(&fix_path, r#"import "./prefs.glyph.md" { preserve_existing_patterns }
import "./repo_tools.glyph.md" { inspect_repo }

skill fix_bug(scope = ".")
    description: "Fix a bug."
    require preserve_existing_patterns
    effects: reads_files
    flow:
        inspect_repo(scope)
"#).unwrap();

        let bag = check_file(&fix_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        // Should NOT have undefined-name or undefined-call errors.
        assert!(
            !ids.contains(&"G::analyze::undefined-name"),
            "imported text should resolve, got: {:?}", ids
        );
        assert!(
            !ids.contains(&"G::analyze::undefined-call"),
            "imported block should resolve, got: {:?}", ids
        );
    }

    #[test]
    fn circular_import_detected_with_path() {
        // AC2: Circular-import path is included in the diagnostic message.
        let dir = tempfile::tempdir().unwrap();

        let a_path = dir.path().join("a.glyph.md");
        let b_path = dir.path().join("b.glyph.md");

        std::fs::write(&a_path, r#"import "./b.glyph.md" { something }

skill main()
    description: "A."
    flow:
        "Do something."
"#).unwrap();

        std::fs::write(&b_path, r#"import "./a.glyph.md" { something }

export const something = "Hello."
"#).unwrap();

        let bag = check_file(&a_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::circular-import"),
            "expected circular-import diagnostic, got: {:?}", ids
        );
        // Check that the cycle path is in the message.
        let diag = bag.iter().find(|d| d.id == "G::analyze::circular-import").unwrap();
        assert!(
            diag.message.contains("a.glyph.md") && diag.message.contains("b.glyph.md"),
            "cycle path should include both files, got: {}", diag.message
        );
        assert!(
            diag.message.contains("->"),
            "cycle path should use -> separator, got: {}", diag.message
        );
    }

    #[test]
    fn import_private_name_fails() {
        // AC3: Importing a private (non-exported) name fails with import-private.
        let dir = tempfile::tempdir().unwrap();

        let lib_path = dir.path().join("lib.glyph.md");
        std::fs::write(&lib_path, r#"const private_text = "This is private."
export const public_text = "This is public."
"#).unwrap();

        let main_path = dir.path().join("main.glyph.md");
        std::fs::write(&main_path, r#"import "./lib.glyph.md" { private_text }

skill main()
    description: "Main."
    require private_text
    flow:
        "Do something."
"#).unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::import-private"),
            "expected import-private diagnostic, got: {:?}", ids
        );
    }

    #[test]
    fn import_skill_fails() {
        // AC4: Importing a skill (not a block/text) fails with import-skill.
        let dir = tempfile::tempdir().unwrap();

        let lib_path = dir.path().join("lib.glyph.md");
        std::fs::write(&lib_path, r#"skill some_skill()
    description: "A skill."
    flow:
        "Do something."
"#).unwrap();

        let main_path = dir.path().join("main.glyph.md");
        std::fs::write(&main_path, r#"import "./lib.glyph.md" { some_skill }

skill main()
    description: "Main."
    flow:
        "Do something."
"#).unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::import-skill"),
            "expected import-skill diagnostic, got: {:?}", ids
        );
    }

    #[test]
    fn duplicate_import_is_repairable() {
        // AC5: Duplicate imports are repairable diagnostics (exit 2).
        let dir = tempfile::tempdir().unwrap();

        let lib_path = dir.path().join("lib.glyph.md");
        std::fs::write(&lib_path, r#"export const greeting = "Hello."
"#).unwrap();

        let main_path = dir.path().join("main.glyph.md");
        std::fs::write(&main_path, r#"import "./lib.glyph.md" { greeting }
import "./lib.glyph.md" { greeting }

skill main()
    description: "Main."
    require greeting
    flow:
        "Do something."
"#).unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::duplicate-import"),
            "expected duplicate-import diagnostic, got: {:?}", ids
        );
        let diag = bag.iter().find(|d| d.id == "G::analyze::duplicate-import").unwrap();
        assert_eq!(diag.classification, diagnostic::Classification::Repairable);
    }

    #[test]
    fn unused_import_is_repairable() {
        // AC5: Unused imports are repairable diagnostics (exit 2).
        let dir = tempfile::tempdir().unwrap();

        let lib_path = dir.path().join("lib.glyph.md");
        std::fs::write(&lib_path, r#"export const greeting = "Hello."
export const farewell = "Goodbye."
"#).unwrap();

        let main_path = dir.path().join("main.glyph.md");
        std::fs::write(&main_path, r#"import "./lib.glyph.md" { greeting, farewell }

skill main()
    description: "Main."
    require greeting
    flow:
        "Do something."
"#).unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::unused-import"),
            "expected unused-import diagnostic, got: {:?}", ids
        );
        let diag = bag.iter().find(|d| d.id == "G::analyze::unused-import").unwrap();
        assert_eq!(diag.classification, diagnostic::Classification::Repairable);
        assert!(
            diag.message.contains("farewell"),
            "should mention the unused name, got: {}", diag.message
        );
    }

    #[test]
    fn missing_import_file_detected() {
        // Missing file produces G::analyze::missing-file.
        let dir = tempfile::tempdir().unwrap();

        let main_path = dir.path().join("main.glyph.md");
        std::fs::write(&main_path, r#"import "./nonexistent.glyph.md" { something }

skill main()
    description: "Main."
    flow:
        "Do something."
"#).unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::missing-file"),
            "expected missing-file diagnostic, got: {:?}", ids
        );
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

    // --- Slice 12: Multi-file build orchestration tests ---

    #[test]
    fn ac1_directory_compile_processes_every_file() {
        // AC1: `glyph compile dir/` processes every `.glyph.md` even if not
        // transitively reached by imports.
        let dir = tempfile::tempdir().unwrap();

        // Three independent files — none imports the others.
        std::fs::write(dir.path().join("a.glyph.md"), "\
skill alpha()
    description: \"Alpha skill.\"
    flow:
        \"Do alpha.\"
").unwrap();

        std::fs::write(dir.path().join("b.glyph.md"), "\
skill beta()
    description: \"Beta skill.\"
    flow:
        \"Do beta.\"
").unwrap();

        std::fs::write(dir.path().join("c.glyph.md"), "\
skill gamma()
    description: \"Gamma skill.\"
    flow:
        \"Do gamma.\"
").unwrap();

        let sources: Vec<PathBuf> = vec![
            dir.path().join("a.glyph.md"),
            dir.path().join("b.glyph.md"),
            dir.path().join("c.glyph.md"),
        ];
        let result = compile_directory(&sources);

        // All three files should produce outcomes.
        assert_eq!(result.outcomes.len(), 3, "all files should be processed");
        assert_eq!(result.exit_code, 0, "all files should succeed");

        // All three .md output files should exist.
        assert!(dir.path().join("a.md").exists(), "a.md should exist");
        assert!(dir.path().join("b.md").exists(), "b.md should exist");
        assert!(dir.path().join("c.md").exists(), "c.md should exist");
    }

    #[test]
    fn ac2_topological_order_libraries_before_consumers() {
        // AC2: Files compile in topological order (libraries before consumers).
        // We verify that the topological ordering places the imported file
        // before the importing file in the outcome list, even when the input
        // list is reversed.
        let dir = tempfile::tempdir().unwrap();

        // lib.glyph.md — standalone (no skill, just an export const — library)
        std::fs::write(dir.path().join("lib.glyph.md"), "\
export const greeting = \"Hello from lib.\"
").unwrap();

        // consumer.glyph.md — imports from lib but is self-contained for compile
        std::fs::write(dir.path().join("consumer.glyph.md"), "\
import \"./lib.glyph.md\" { greeting }

skill main()
    description: \"Main skill.\"
    flow:
        \"Use the greeting.\"
").unwrap();

        // Pass files in reverse alphabetical order to prove topo sort reorders.
        let sources: Vec<PathBuf> = vec![
            dir.path().join("consumer.glyph.md"),
            dir.path().join("lib.glyph.md"),
        ];
        let result = compile_directory(&sources);

        assert_eq!(result.outcomes.len(), 2);

        // lib should come before consumer in the outcomes (topological order).
        let first_file = &result.outcomes[0].0;
        assert!(
            first_file.to_string_lossy().contains("lib.glyph.md"),
            "lib should compile before consumer, got: {}",
            first_file.display()
        );
    }

    #[test]
    fn ac3_failure_skips_dependent_with_warning() {
        // AC3: Failure in b.glyph.md skips c.glyph.md (which imports it) with
        // the G::build::skipped-due-to-failed-import warning.
        let dir = tempfile::tempdir().unwrap();

        // a.glyph.md — valid, standalone
        std::fs::write(dir.path().join("a.glyph.md"), "\
skill alpha()
    description: \"Alpha skill.\"
    flow:
        \"Do alpha.\"
").unwrap();

        // b.glyph.md — intentionally broken (will fail Phase 1)
        std::fs::write(dir.path().join("b.glyph.md"), "\
this is not valid glyph syntax at all!!!
").unwrap();

        // c.glyph.md — imports b, should be skipped
        std::fs::write(dir.path().join("c.glyph.md"), "\
import \"./b.glyph.md\" { something }

skill gamma()
    description: \"Gamma skill.\"
    flow:
        \"Do gamma.\"
").unwrap();

        let sources: Vec<PathBuf> = vec![
            dir.path().join("a.glyph.md"),
            dir.path().join("b.glyph.md"),
            dir.path().join("c.glyph.md"),
        ];
        let result = compile_directory(&sources);

        assert_eq!(result.exit_code, 1, "build should fail");

        // a.md should exist (a succeeded).
        assert!(dir.path().join("a.md").exists(), "a.md should exist");

        // c should be skipped.
        let c_outcome = result.outcomes.iter().find(|(p, _)| {
            p.to_string_lossy().contains("c.glyph.md")
        });
        assert!(c_outcome.is_some(), "c.glyph.md should be in outcomes");
        match &c_outcome.unwrap().1 {
            FileOutcome::Skipped { failed_dep } => {
                assert!(
                    failed_dep.to_string_lossy().contains("b.glyph.md"),
                    "failed_dep should reference b.glyph.md, got: {}",
                    failed_dep.display()
                );
            }
            other => panic!("expected Skipped for c.glyph.md, got: {:?}", other),
        }
    }

    #[test]
    fn ac4_stale_md_left_untouched_on_skip() {
        // AC4: Stale c.md left untouched on disk after c.glyph.md skip.
        let dir = tempfile::tempdir().unwrap();

        // Pre-existing stale c.md from a previous build.
        let stale_content = "# Previous build output\nThis is stale.";
        std::fs::write(dir.path().join("c.md"), stale_content).unwrap();

        // b.glyph.md — broken
        std::fs::write(dir.path().join("b.glyph.md"), "\
this is broken!!!
").unwrap();

        // c.glyph.md — imports b, will be skipped
        std::fs::write(dir.path().join("c.glyph.md"), "\
import \"./b.glyph.md\" { something }

skill gamma()
    description: \"Gamma skill.\"
    flow:
        \"Do gamma.\"
").unwrap();

        let sources: Vec<PathBuf> = vec![
            dir.path().join("b.glyph.md"),
            dir.path().join("c.glyph.md"),
        ];
        let result = compile_directory(&sources);

        assert_eq!(result.exit_code, 1);

        // c.md should still contain the stale content, untouched.
        let c_md = std::fs::read_to_string(dir.path().join("c.md")).unwrap();
        assert_eq!(
            c_md, stale_content,
            "stale c.md should be left untouched"
        );
    }

    #[test]
    fn ac5_exit_1_if_any_failed_partial_output_present() {
        // AC5: Build exits 1 if any file failed; partial output present for
        // successful files.
        let dir = tempfile::tempdir().unwrap();

        // good.glyph.md — valid
        std::fs::write(dir.path().join("good.glyph.md"), "\
skill good()
    description: \"Good skill.\"
    flow:
        \"Do good.\"
").unwrap();

        // bad.glyph.md — broken
        std::fs::write(dir.path().join("bad.glyph.md"), "\
this is broken!!!
").unwrap();

        let sources: Vec<PathBuf> = vec![
            dir.path().join("good.glyph.md"),
            dir.path().join("bad.glyph.md"),
        ];
        let result = compile_directory(&sources);

        // Exit 1 because bad.glyph.md failed.
        assert_eq!(result.exit_code, 1, "should exit 1 when any file fails");

        // good.md should exist (partial output).
        assert!(dir.path().join("good.md").exists(), "good.md should exist as partial output");

        // bad.md should NOT exist.
        assert!(!dir.path().join("bad.md").exists(), "bad.md should not exist");
    }

    // --- Slice 13: Library files (export blocks/text + closure check) ---

    #[test]
    fn ac4_library_with_zero_exports_fires_no_exports_in_library() {
        // A file with zero skills AND zero exports is an error.
        let src = "\
const private_text = \"This is private.\"
block helper()
    \"Do something.\"
";
        let bag = check_source(src, 0, "empty_lib.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::no-exports-in-library"),
            "expected no-exports-in-library for library with zero exports, got: {:?}",
            ids
        );
        assert_eq!(bag.exit_code(), 1, "no-exports-in-library should be a hard error");
    }

    #[test]
    fn ac1_export_text_only_library_compiles_exit_zero() {
        // A library file with only export const declarations should compile
        // successfully (exit 0) and produce zero .md output.
        let dir = tempfile::tempdir().unwrap();

        let prefs_path = dir.path().join("prefs.glyph.md");
        std::fs::write(&prefs_path, "\
export const terminal_mux = \"tmux\"
export const validation_strictness = \"high\"
").unwrap();

        let sources: Vec<PathBuf> = vec![prefs_path];
        let result = compile_directory(&sources);

        assert_eq!(result.exit_code, 0, "library file should compile with exit 0");
        // No .md output should be produced for a library file.
        assert!(
            !dir.path().join("prefs.md").exists(),
            "library file should not produce .md output"
        );
    }

    #[test]
    fn ac1_export_text_only_library_check_source_clean() {
        // check_source on a library file with exports should produce zero
        // errors (no no-exports-in-library, no other issues).
        let src = "\
export const terminal_mux = \"tmux\"
export const validation_strictness = \"high\"
";
        let bag = check_source(src, 0, "prefs.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::no-exports-in-library"),
            "library with exports should not fire no-exports-in-library, got: {:?}",
            ids
        );
        assert_eq!(bag.exit_code(), 0, "clean library should exit 0");
    }

    #[test]
    fn ac3_closure_violation_on_private_free_variable() {
        // An export block that references a private (non-exported) block
        // should fire G::analyze::closure-violation.
        let src = "\
block private_helper()
    \"Do private stuff.\"

export block shared_util(x = \"default\")
    flow:
        private_helper()
        return x
";
        let bag = check_source(src, 0, "lib.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::closure-violation"),
            "expected closure-violation for export block referencing private name, got: {:?}",
            ids
        );
        assert_eq!(bag.exit_code(), 1, "closure-violation should be a hard error");
    }

    #[test]
    fn ac3_no_closure_violation_for_params_and_exported_names() {
        // Export block referencing its own params and exported text should
        // NOT fire closure-violation.
        let src = "\
export const greeting = \"Hello.\"

export block shared_util(x = \"default\")
    flow:
        \"Use {x}.\"
        return x
";
        let bag = check_source(src, 0, "lib.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::closure-violation"),
            "should not fire closure-violation for params/exported names, got: {:?}",
            ids
        );
    }

    #[test]
    fn name_collision_fires_for_duplicate_export_names() {
        // Two exports sharing the same name should fire G::analyze::name-collision.
        let src = "\
export const greeting = \"Hello.\"
export const greeting = \"Hi.\"
";
        let bag = check_source(src, 0, "lib.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::name-collision"),
            "expected name-collision for duplicate export names, got: {:?}",
            ids
        );
        assert_eq!(bag.exit_code(), 1, "name-collision should be a hard error");
    }

    #[test]
    fn ac5_exports_visited_in_source_order() {
        // Exports should be extracted in source order for deterministic output.
        let src = "\
export const zebra = \"Z.\"
export const alpha = \"A.\"
export const middle = \"M.\"
";
        let (file, _) = parse::parse(src, 0).expect("should parse");
        let exports = extract_exports(&file);
        // The texts HashSet doesn't preserve order, but we can verify all are present.
        assert!(exports.texts.contains("zebra"));
        assert!(exports.texts.contains("alpha"));
        assert!(exports.texts.contains("middle"));
        assert_eq!(exports.texts.len(), 3);

        // Verify source-order by walking decls directly.
        let names: Vec<&str> = file.decls.iter().filter_map(|d| match d {
            ast::Decl::Const(c) if c.node.exported => Some(c.node.name.as_str()),
            _ => None,
        }).collect();
        assert_eq!(names, vec!["zebra", "alpha", "middle"],
            "exports should be in source order");
    }

    #[test]
    fn ac2_repo_tools_library_compiles_with_large_export_block() {
        // A library file with export blocks should compile successfully.
        // Large export blocks (>= 150 words) should have their word count tracked
        // for procedure-file emission (Slice 15).
        let dir = tempfile::tempdir().unwrap();

        // Build a large body with 160+ words.
        let mut long_body = String::new();
        for i in 0..20 {
            long_body.push_str(&format!(
                "        \"Step {} of the procedure: perform the operation carefully and thoroughly.\"\n",
                i + 1
            ));
        }

        let repo_tools_src = format!("\
export block inspect_repo(scope = \".\") -> Path
    description: \"Inspect the repository for issues.\"
    flow:
{}        return scope
", long_body);

        let tools_path = dir.path().join("repo_tools.glyph.md");
        std::fs::write(&tools_path, &repo_tools_src).unwrap();

        let sources: Vec<PathBuf> = vec![tools_path.clone()];
        let result = compile_directory(&sources);

        assert_eq!(result.exit_code, 0, "repo_tools library should compile with exit 0");
        // No .md output for a library file.
        assert!(
            !dir.path().join("repo_tools.md").exists(),
            "library file should not produce .md output"
        );

        // Verify word count is tracked on the parsed AST.
        let source = std::fs::read_to_string(&tools_path).unwrap();
        let (file, _) = parse::parse(&source, 0).expect("should parse");
        let export_block = file.decls.iter().find_map(|d| match d {
            ast::Decl::ExportBlock(b) => Some(&b.node),
            _ => None,
        }).expect("export block should be present");
        assert!(
            export_block.body_word_count >= 150,
            "large export block should have >= 150 words, got {}",
            export_block.body_word_count
        );
    }

    #[test]
    fn tier3_library_emits_procedure_files() {
        // AC1: repo_tools.glyph.md with two large export blocks should emit
        //      repo_tools/inspect-repo.md and repo_tools/run-tests.md
        // AC2: Procedure files carry `kind: procedure` in frontmatter
        let dir = tempfile::tempdir().unwrap();

        // Build two export blocks with >= 150 words each.
        let mut long_body_1 = String::new();
        for i in 0..20 {
            long_body_1.push_str(&format!(
                "        \"Step {} of the inspection: carefully examine the repository structure and contents.\"\n",
                i + 1
            ));
        }
        let mut long_body_2 = String::new();
        for i in 0..20 {
            long_body_2.push_str(&format!(
                "        \"Step {} of the test run: execute the test suite and verify all assertions pass.\"\n",
                i + 1
            ));
        }

        let repo_tools_src = format!("\
export block inspect_repo(scope = \".\") -> Path
    description: \"Inspect the repository for issues.\"
    effects: reads_files
    flow:
{}        return scope

export block run_tests(target = \"all\") -> TestResult
    description: \"Run the project test suite.\"
    effects: reads_files
    flow:
{}        return target
", long_body_1, long_body_2);

        let tools_path = dir.path().join("repo_tools.glyph.md");
        std::fs::write(&tools_path, &repo_tools_src).unwrap();

        let sources: Vec<PathBuf> = vec![tools_path.clone()];
        let result = compile_directory(&sources);
        assert_eq!(result.exit_code, 0, "repo_tools library should compile with exit 0");

        // AC1: procedure files emitted
        let inspect_path = dir.path().join("repo_tools/inspect-repo.md");
        let run_tests_path = dir.path().join("repo_tools/run-tests.md");
        assert!(inspect_path.exists(), "repo_tools/inspect-repo.md should exist");
        assert!(run_tests_path.exists(), "repo_tools/run-tests.md should exist");

        // AC2: kind: procedure in frontmatter
        let inspect_content = std::fs::read_to_string(&inspect_path).unwrap();
        assert!(inspect_content.contains("kind: procedure"), "inspect-repo.md should have kind: procedure");
        assert!(inspect_content.contains("name: inspect-repo"), "inspect-repo.md should have name: inspect-repo");

        let run_tests_content = std::fs::read_to_string(&run_tests_path).unwrap();
        assert!(run_tests_content.contains("kind: procedure"), "run-tests.md should have kind: procedure");
        assert!(run_tests_content.contains("name: run-tests"), "run-tests.md should have name: run-tests");
    }

    #[test]
    fn tier3_consumer_references_procedure_file() {
        // AC3: Consumer's compiled .md references the procedure files at the conventional path
        let dir = tempfile::tempdir().unwrap();

        // Library with a large export block
        let mut long_body = String::new();
        for i in 0..20 {
            long_body.push_str(&format!(
                "        \"Step {} of the inspection: carefully examine the repository structure and contents.\"\n",
                i + 1
            ));
        }

        let lib_src = format!("\
export block inspect_repo(scope = \".\") -> Path
    description: \"Inspect the repository for issues.\"
    effects: reads_files
    flow:
{}        return scope
", long_body);

        let lib_path = dir.path().join("repo_tools.glyph.md");
        std::fs::write(&lib_path, &lib_src).unwrap();

        // Consumer skill that imports and calls inspect_repo
        let consumer_src = "\
import \"repo_tools\" { inspect_repo }

skill audit_code()
    description: \"Audit the codebase.\"
    effects: reads_files

    flow:
        inspect_repo()
";
        let consumer_path = dir.path().join("audit_code.glyph.md");
        std::fs::write(&consumer_path, consumer_src).unwrap();

        let sources: Vec<PathBuf> = vec![lib_path.clone(), consumer_path.clone()];
        let result = compile_directory(&sources);
        assert_eq!(result.exit_code, 0, "should compile with exit 0");

        // Consumer's compiled output should reference the procedure file
        let consumer_output = dir.path().join("audit_code.md");
        assert!(consumer_output.exists(), "audit_code.md should exist");
        let content = std::fs::read_to_string(&consumer_output).unwrap();
        assert!(
            content.contains("repo_tools/inspect-repo.md"),
            "consumer should reference repo_tools/inspect-repo.md, got:\n{}",
            content
        );
    }

    #[test]
    fn tier3_idempotent_output() {
        // AC4: Re-running produces byte-identical procedure files
        let dir = tempfile::tempdir().unwrap();

        let mut long_body = String::new();
        for i in 0..20 {
            long_body.push_str(&format!(
                "        \"Step {} of the inspection: carefully examine the repository structure and contents.\"\n",
                i + 1
            ));
        }

        let repo_tools_src = format!("\
export block inspect_repo(scope = \".\") -> Path
    description: \"Inspect the repository for issues.\"
    effects: reads_files
    flow:
{}        return scope
", long_body);

        let tools_path = dir.path().join("repo_tools.glyph.md");
        std::fs::write(&tools_path, &repo_tools_src).unwrap();

        let sources: Vec<PathBuf> = vec![tools_path.clone()];

        // First run
        compile_directory(&sources);
        let inspect_path = dir.path().join("repo_tools/inspect-repo.md");
        let first_content = std::fs::read_to_string(&inspect_path).unwrap();

        // Second run
        compile_directory(&sources);
        let second_content = std::fs::read_to_string(&inspect_path).unwrap();

        assert_eq!(first_content, second_content, "procedure file should be byte-identical across runs");
    }

    // --- Stdlib (slice 21) tests ---

    #[test]
    fn stdlib_subagent_resolvable_via_import() {
        // AC1: `subagent` is resolvable when imported from `@glyph/std`.
        let dir = tempfile::tempdir().unwrap();
        let main_path = dir.path().join("main.glyph.md");
        std::fs::write(&main_path, r#"import "@glyph/std" { subagent }

skill delegate(task = "do something")
    description: "Delegate work."
    effects: spawns_agent
    flow:
        subagent(task)
"#).unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::undefined-call"),
            "subagent should resolve via stdlib import, got: {:?}", ids
        );
        assert!(
            !ids.contains(&"G::analyze::missing-file"),
            "stdlib import should not trigger missing-file, got: {:?}", ids
        );
    }

    #[test]
    fn stdlib_load_not_importable() {
        // AC2: `load` is compiler-internal and NOT resolvable from author source.
        let dir = tempfile::tempdir().unwrap();
        let main_path = dir.path().join("main.glyph.md");
        std::fs::write(&main_path, r#"import "@glyph/std" { load }

skill runner()
    description: "Run something."
    flow:
        load("file.md")
"#).unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::import-private"),
            "load should not be importable, got: {:?}", ids
        );
    }

    #[test]
    fn stdlib_send_resolvable_via_import() {
        // AC1: `send` is resolvable when imported from `@glyph/std`.
        let dir = tempfile::tempdir().unwrap();
        let main_path = dir.path().join("main.glyph.md");
        std::fs::write(&main_path, r#"import "@glyph/std" { send }

skill notify(msg = "hello")
    description: "Send a message."
    effects: spawns_agent
    flow:
        send(msg)
"#).unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::undefined-call"),
            "send should resolve via stdlib import, got: {:?}", ids
        );
        assert!(
            !ids.contains(&"G::analyze::missing-file"),
            "stdlib import should not trigger missing-file, got: {:?}", ids
        );
    }

    #[test]
    fn stdlib_missing_import_fires_for_subagent() {
        // AC3: `stdlib-missing-import` repairable fires when `subagent` used without import.
        let src = r#"skill delegate(task = "do something")
    description: "Delegate work."
    effects: spawns_agent
    flow:
        subagent(task)
"#;
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::stdlib-missing-import"),
            "should fire stdlib-missing-import for subagent without import, got: {:?}", ids
        );
        let diag = bag.iter().find(|d| d.id == "G::analyze::stdlib-missing-import").unwrap();
        assert_eq!(
            diag.classification, Classification::Repairable,
            "stdlib-missing-import should be repairable"
        );
    }

    #[test]
    fn stdlib_missing_import_fires_for_send() {
        // AC3: `stdlib-missing-import` repairable fires when `send` used without import.
        let src = r#"skill notify(msg = "hello")
    description: "Send a message."
    effects: spawns_agent
    flow:
        send(msg)
"#;
        let bag = check_source(src, 0, "test.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::stdlib-missing-import"),
            "should fire stdlib-missing-import for send without import, got: {:?}", ids
        );
    }

    #[test]
    fn stdlib_unknown_module_fires() {
        // AC4: `unknown-stdlib-module` error fires on import of nonexistent @glyph/ path.
        let dir = tempfile::tempdir().unwrap();
        let main_path = dir.path().join("main.glyph.md");
        std::fs::write(&main_path, r#"import "@glyph/foo" { bar }

skill main()
    description: "Main."
    flow:
        "Do something."
"#).unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::imports::unknown-stdlib-module"),
            "should fire unknown-stdlib-module for @glyph/foo, got: {:?}", ids
        );
    }

    #[test]
    fn stdlib_subagent_effect_propagates() {
        // AC5: stdlib entry's effect signature (`spawns_agent`) propagates —
        // if a skill calls subagent() and declares effects but omits spawns_agent,
        // it should fire effects-under-declared.
        let dir = tempfile::tempdir().unwrap();
        let main_path = dir.path().join("main.glyph.md");
        std::fs::write(&main_path, r#"import "@glyph/std" { subagent }

skill delegate(task = "do something")
    description: "Delegate work."
    effects: reads_files
    flow:
        subagent(task)
"#).unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::effects-under-declared"),
            "subagent's spawns_agent effect should propagate, got: {:?}", ids
        );
    }

    // --- Slice 23: Diagnostic coverage backfill ---

    #[test]
    fn parse_nested_flow_diagnostic() {
        // `flow:` inside `flow:` is illegal.
        let src = "\
skill foo()
    description: \"Foo.\"
    flow:
        \"Do step one.\"
        flow:
            \"Nested flow not allowed.\"
";
        let bag = check_source(src, 0, "nested_flow.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"G::parse::nested-flow"), "ids: {:?}", ids);
    }

    #[test]
    fn analyze_empty_skill_body_diagnostic() {
        // A skill with no description, no flow, no constraints, no effects.
        let src = "\
skill empty()
";
        let bag = check_source(src, 0, "empty_skill.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"G::analyze::empty-skill-body"), "ids: {:?}", ids);
    }

    #[test]
    fn parse_multiple_skills_diagnostic() {
        // More than one `skill` in a file triggers multiple-skills.
        let src = "\
skill foo()
    description: \"Foo.\"
    flow:
        \"Do foo.\"

skill bar()
    description: \"Bar.\"
    flow:
        \"Do bar.\"
";
        let bag = check_source(src, 0, "two_skills.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"G::parse::multiple-skills"), "ids: {:?}", ids);
    }

    #[test]
    fn parse_duplicate_subsection_diagnostic() {
        // Two `description:` in same skill triggers duplicate-subsection.
        let src = "\
skill foo()
    description: \"First.\"
    description: \"Second.\"
    flow:
        \"Do something.\"
";
        let bag = check_source(src, 0, "dup.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"G::parse::duplicate-subsection"), "ids: {:?}", ids);
    }

    #[test]
    fn parse_operator_in_expression_diagnostic() {
        // Operator chars in expression position trigger operator-in-expression.
        let src = "\
skill foo()
    description: \"Foo.\"
    flow:
        \"prefix\" + \"suffix\"
";
        let bag = check_source(src, 0, "op.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"G::parse::operator-in-expression"), "ids: {:?}", ids);
        assert_eq!(bag.exit_code(), 2, "operator-in-expression is repairable (exit 2)");
    }

    #[test]
    fn parse_mixed_indent_diagnostic() {
        // Source with spaces then tab on the same line triggers mixed-indent.
        let src = "skill foo()\n \tflow:\n";
        let bag = check_source(src, 0, "mixed.glyph.md");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"G::parse::mixed-indent"), "ids: {:?}", ids);
        assert_eq!(bag.exit_code(), 2, "mixed-indent is repairable (exit 2)");
    }
}
