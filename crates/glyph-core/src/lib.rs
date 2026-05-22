//! glyph-core: deterministic compiler phases for the Glyph language.
//!
//! Walking-skeleton scope (slice 1): minimum viable Phase 1 / 2 / 4 / 5 / 6-Step1 / 7
//! that produces a byte-identical golden snapshot for `update_docs.glyph` per
//! `docs/reference/mvp-acceptance.md` §1.

pub mod analyze;
pub mod ast;
pub mod condition;
pub mod diagnostic;
pub mod domain_registry;
pub mod emit;
pub mod emit_ir;
pub mod expand;
pub mod fmt;
pub mod ir;
pub mod kind_infer;
pub mod lower;
pub mod name_kind;
pub mod output_target;
pub mod parse;
pub mod sections;
pub mod semantic_tokens;
pub mod slot;
pub mod span;
pub mod tokenize;
pub mod type_position;
pub mod validate;
pub mod validate_output;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ast::{Decl, ImportKind, ReturnExpr};
use crate::diagnostic::{Classification, DiagBag, Diagnostic, SourceSpan};
use crate::ir::IrNode;
use crate::ir::OutputTargetForm;
use crate::output_target::OutputTargetExpr;
use crate::span::{LineIndex, Span, Spanned};

// ---------------------------------------------------------------------------
// Stdlib call signatures (flow-position-assignments §4 lib.rs touch row,
// Codex Round 2 Med 5).
//
// Analyze's no-value check (§6.2.b) needs to know whether `subagent` /
// `send` declare a return type. Today the AST-side path for `@glyph/std`
// imports (around L880-918 below) only registers names in `imported_blocks`
// — it does not capture per-name return-type metadata.
//
// We expose a small inline registry keyed by the bare stdlib name:
// - `subagent → Some("Agent"), is_agent: true`
// - `send     → None,           is_agent: false`
//
// `is_agent` mirrors `TypeTag::Agent`; we keep it as an explicit flag
// because the agent-shape rule (§9.1 Codex Round 3 Med 5) lives at the
// flow-local-type layer, which doesn't have to round-trip the kind_infer
// enum for stdlib-only callees.
// ---------------------------------------------------------------------------

/// Return-type signature for a `@glyph/std` block. Used by analyze (and
/// later phases) to resolve flow-position assignment RHS types when the
/// callee is a stdlib import.
#[derive(Debug, Clone, Copy)]
pub struct StdlibCallSig {
    /// Declared `-> Type` text, or `None` if the callee returns no value.
    pub return_type: Option<&'static str>,
    /// Whether the callee's return is agent-shape (matches `TypeTag::Agent`).
    pub is_agent: bool,
}

const STDLIB_SIGS: &[(&str, StdlibCallSig)] = &[
    (
        "subagent",
        StdlibCallSig {
            return_type: Some("Agent"),
            is_agent: true,
        },
    ),
    (
        "send",
        StdlibCallSig {
            return_type: None,
            is_agent: false,
        },
    ),
];

/// Look up the return-type signature for a stdlib block by bare name.
///
/// Returns `None` if `name` is not a registered stdlib block (or is
/// `load`, which is compiler-internal and not author-facing).
pub(crate) fn stdlib_sig(name: &str) -> Option<&'static StdlibCallSig> {
    STDLIB_SIGS.iter().find(|(n, _)| *n == name).map(|(_, s)| s)
}

#[derive(Debug)]
pub enum CompileError {
    Read {
        path: String,
        source: std::io::Error,
    },
    Parse(parse::ParseError),
    Lower(lower::LowerError),
    Validate(validate::ValidateError),
    Write {
        path: String,
        source: std::io::Error,
    },
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
/// rule in `docs/adr/` §A6.
#[derive(Debug)]
pub enum CompileOutcome {
    Compiled {
        markdown: String,
        diagnostics: DiagBag,
        arena: ir::IrArena,
    },
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
    compile_source_with_effects(source, file_id, file_label, false)
}

/// Compile with explicit effects gate.
///
/// The CLI derives `enable_effects` from
/// [`sections::SectionCatalogue::effects_enabled`] at the boundary and
/// threads the bare bool through this entry point.
pub fn compile_source_with_effects(
    source: &str,
    file_id: u32,
    file_label: &str,
    enable_effects: bool,
) -> Result<CompileOutcome, CompileError> {
    let mut bag = DiagBag::new();

    // Build a line index up front for diagnostic span conversion. The parser
    // builds its own when there is no diagnostic; on the diagnostic path we
    // recompute here to avoid plumbing an extra return value out of `parse`.
    let line_index = LineIndex::new(source);

    let parsed = parse::parse_with_diagnostics_opts(
        source,
        file_id,
        file_label,
        &line_index,
        &mut bag,
        enable_effects,
    );
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

    let mut registry = domain_registry::Registry::new();
    let file = analyze::analyze_with_diagnostics(
        file,
        file_id,
        file_label,
        &line_index,
        &mut bag,
        &mut registry,
    );
    if bag.has_error() || bag.has_repairable() {
        return Ok(CompileOutcome::Diagnostics(bag));
    }
    let arena = lower::lower(&file).map_err(CompileError::Lower)?;
    validate::validate(&arena).map_err(CompileError::Validate)?;
    let arena = expand::expand_step1(arena);
    let markdown = match emit::emit(&arena, enable_effects) {
        Ok(md) => md,
        Err(errors) => {
            let mut diag_bag = llm_required_diagnostics_from_errors(errors, file_label);
            diag_bag.merge(bag);
            return Ok(CompileOutcome::Diagnostics(diag_bag));
        }
    };
    Ok(CompileOutcome::Compiled {
        markdown,
        diagnostics: bag,
        arena,
    })
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
    check_source_with_effects(source, file_id, file_label, false)
}

/// Check with explicit effects gate.
pub fn check_source_with_effects(
    source: &str,
    file_id: u32,
    file_label: &str,
    enable_effects: bool,
) -> DiagBag {
    let mut bag = DiagBag::new();
    let line_index = LineIndex::new(source);

    let parsed = parse::parse_with_diagnostics_opts(
        source,
        file_id,
        file_label,
        &line_index,
        &mut bag,
        enable_effects,
    );

    // Phase 2 (Analyze) — slice 4 adds the parameter-related diagnostics
    // (`G::analyze::unknown-param-slot`, `G::analyze::missing-required-arg`).
    if let Some(file) = parsed {
        let mut registry = domain_registry::Registry::new();
        let _ = analyze::analyze_with_diagnostics(
            file,
            file_id,
            file_label,
            &line_index,
            &mut bag,
            &mut registry,
        );
    } else if !bag.has_error() && !bag.has_repairable() {
        // B01 belt-and-suspenders: parsing returned `None` but no
        // diagnostic was pushed. Surface a hard `G::parse::unexpected`
        // so `glyph check` can never silently exit 0 on a file the
        // pipeline considers unparseable. Every parser bail path
        // should push its own structured diagnostic; if a future
        // refactor forgets one this fallback keeps the contract.
        let span = Span::new(file_id, 0, 0);
        bag.push(
            Diagnostic::error(
                "G::parse::unexpected",
                "source could not be parsed and no specific diagnostic was reported",
                SourceSpan::from_byte_span(file_label, span, &line_index),
            ),
            span,
        );
    }

    bag
}

/// Bundle returned by [`check_source_with_resolutions`].
///
/// All four pieces are needed by the LSP: the diagnostics drive
/// `publishDiagnostics`, the `ast` and `line_index` are stored on the
/// per-buffer `ParsedView` (per `design/glyph-lsp.md` §5), and the
/// `resolutions` table powers `textDocument/definition`.
#[derive(Debug)]
pub struct CheckedView {
    pub bag: DiagBag,
    pub ast: ast::SourceFile,
    pub line_index: LineIndex,
    pub resolutions: Vec<analyze::Resolution>,
}

/// Run Phase 1 + Phase 2 like [`check_source_with_effects`], but additionally
/// return the parsed AST, line index, and the same-file resolution table
/// driving go-to-definition (M2).
///
/// Returns `None` if parsing failed catastrophically (no AST recovered).
/// In that case the caller should fall back to `check_source_with_effects`
/// for diagnostics.
///
/// This same-file variant is preserved for callers that don't have a
/// real path on disk (e.g., in-memory tests, scratch buffers). The LSP
/// uses [`check_source_with_resolutions_at_path`] so it can also follow
/// cross-file imports.
pub fn check_source_with_resolutions(
    source: &str,
    file_id: u32,
    file_label: &str,
    enable_effects: bool,
) -> Option<CheckedView> {
    let mut bag = DiagBag::new();
    let line_index = LineIndex::new(source);

    let parsed = parse::parse_with_diagnostics_opts(
        source,
        file_id,
        file_label,
        &line_index,
        &mut bag,
        enable_effects,
    )?;

    let file_path = PathBuf::from(file_label);
    let (file, resolutions) = analyze::analyze_with_resolutions(
        parsed,
        file_id,
        file_label,
        &file_path,
        &line_index,
        &mut bag,
        enable_effects,
    );

    Some(CheckedView {
        bag,
        ast: file,
        line_index,
        resolutions,
    })
}

/// Like [`check_source_with_resolutions`] but also resolves cross-file
/// references (M2 cross-file go-to-def, design §4.4 + §7.cross-file).
///
/// `current_path` is the on-disk path of the buffer being analyzed — needed
/// so `import "./<rel>"` paths can be resolved against the importer's
/// directory. Each imported declaration is parsed once; matching use-sites
/// in the buffer get [`analyze::Resolution`]s whose `def_file` points at
/// the imported file.
///
/// The same-file resolutions returned by `analyze_with_resolutions` are
/// concatenated with the cross-file ones, in that order. Resolutions remain
/// span-disjoint (no use-site is both same-file and cross-file).
pub fn check_source_with_resolutions_at_path(
    source: &str,
    file_id: u32,
    current_path: &Path,
    enable_effects: bool,
) -> Option<CheckedView> {
    let file_label = current_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| current_path.to_string_lossy().into_owned());

    let mut bag = DiagBag::new();
    let line_index = LineIndex::new(source);

    let parsed = parse::parse_with_diagnostics_opts(
        source,
        file_id,
        &file_label,
        &line_index,
        &mut bag,
        enable_effects,
    )?;

    let current_path_buf = current_path.to_path_buf();
    let (file, mut resolutions) = analyze::analyze_with_resolutions(
        parsed,
        file_id,
        &file_label,
        &current_path_buf,
        &line_index,
        &mut bag,
        enable_effects,
    );

    // Walk imports — for every imported name we can resolve to a declaration
    // in the dependency file, build a target descriptor.
    let cross_targets = collect_cross_file_targets(&file, current_path, enable_effects);
    let cross_resolutions = analyze::collect_cross_file_resolutions(&file, &cross_targets);
    resolutions.extend(cross_resolutions);

    Some(CheckedView {
        bag,
        ast: file,
        line_index,
        resolutions,
    })
}

/// For each `import "<rel>" { name [as alias] }` clause in `file`, parse
/// the dependency file and locate the declaration matching each imported
/// name. Returns a `local_name → ImportTarget` map keyed on the importer's
/// view (alias-resolved).
///
/// Stdlib (`@glyph/...`) and unresolvable imports are silently skipped — the
/// LSP returns `null` for those rather than surfacing a fake jump.
fn collect_cross_file_targets(
    file: &ast::SourceFile,
    current_path: &Path,
    enable_effects: bool,
) -> HashMap<String, analyze::ImportTarget> {
    use crate::ast::ImportKind;

    let mut out: HashMap<String, analyze::ImportTarget> = HashMap::new();

    for decl in &file.decls {
        let imp = match decl {
            Decl::Import(i) => i,
            _ => continue,
        };
        if imp.node.path.starts_with("@glyph/") {
            continue;
        }
        let names = match &imp.node.kind {
            ImportKind::Selective(n) => n,
            ImportKind::WholeModule { .. } => continue, // not LSP-jumpable in MVP
        };

        let resolved = match resolve_import_path(current_path, &imp.node.path) {
            Some(p) => p,
            None => continue,
        };
        let dep_source = match std::fs::read_to_string(&resolved) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let dep_li = LineIndex::new(&dep_source);
        let dep_label = resolved
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| resolved.to_string_lossy().into_owned());
        let mut tmp_bag = DiagBag::new();
        let dep_file = match parse::parse_with_diagnostics_opts(
            &dep_source,
            0,
            &dep_label,
            &dep_li,
            &mut tmp_bag,
            enable_effects,
        ) {
            Some(f) => f,
            None => continue,
        };

        for imp_name in names {
            let local = imp_name
                .alias
                .as_ref()
                .map(|a| a.node.clone())
                .unwrap_or_else(|| imp_name.name.node.clone());
            // Find the declaration in the dependency file by exported name.
            for dep_decl in &dep_file.decls {
                match dep_decl {
                    Decl::Const(t) if t.node.exported && t.node.name == imp_name.name.node => {
                        out.insert(
                            local.clone(),
                            analyze::ImportTarget {
                                local_name: local.clone(),
                                def_file: resolved.clone(),
                                def_span: t.span,
                                kind: analyze::ResolutionKind::Text,
                            },
                        );
                        break;
                    }
                    Decl::ExportBlock(b) if b.node.name == imp_name.name.node => {
                        out.insert(
                            local.clone(),
                            analyze::ImportTarget {
                                local_name: local.clone(),
                                def_file: resolved.clone(),
                                def_span: b.span,
                                kind: analyze::ResolutionKind::ExportBlock,
                            },
                        );
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    out
}

/// End-to-end file-driven compile.
///
/// Reads `<name>.glyph`, runs the pipeline, and (on the success path) writes
/// `<name>.md` next to the source file. The returned `CompileOutcome` carries
/// either the compiled output or a `DiagBag`; the CLI is responsible for
/// rendering and exit-code mapping.
pub fn compile_file(path: &Path) -> Result<CompileOutcome, CompileError> {
    compile_file_with_effects(path, false)
}

pub fn compile_file_with_effects(
    path: &Path,
    enable_effects: bool,
) -> Result<CompileOutcome, CompileError> {
    compile_file_with_layout(path, enable_effects, &CompileOutputLayout::SameDir)
}

pub fn compile_file_with_layout(
    path: &Path,
    enable_effects: bool,
    layout: &CompileOutputLayout,
) -> Result<CompileOutcome, CompileError> {
    let source = std::fs::read_to_string(path).map_err(|e| CompileError::Read {
        path: path.display().to_string(),
        source: e,
    })?;
    let label = path.display().to_string();
    let outcome = compile_source_with_effects(&source, 0, &label, enable_effects)?;
    if let CompileOutcome::Compiled {
        ref markdown,
        ref arena,
        ..
    } = outcome
    {
        let out_path = resolve_output_path(path, OutputKind::Compiled, layout);
        let _ = arena; // arena available for --emit-ir; unused in compile_file
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| CompileError::Write {
                path: out_path.display().to_string(),
                source: e,
            })?;
        }
        atomic_write(&out_path, markdown).map_err(|e| CompileError::Write {
            path: out_path.display().to_string(),
            source: e,
        })?;
    }
    Ok(outcome)
}

/// Resolve an import path relative to the importing file's directory.
///
/// If the path doesn't end with `.glyph` and no file exists at the literal
/// path, appends `.glyph` and retries. Returns the canonical path.
fn resolve_import_path(importer: &Path, import_path: &str) -> Option<PathBuf> {
    let base_dir = importer.parent().unwrap_or_else(|| Path::new("."));
    let candidate = base_dir.join(import_path);
    if candidate.is_file() {
        return candidate.canonicalize().ok();
    }
    // Auto-resolution: try appending `.glyph`.
    if !import_path.ends_with(".glyph") {
        let with_ext = base_dir.join(format!("{}.glyph", import_path));
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
    /// Issue #84 Chunk 4 (D15 / Option-Y): per-exported-block declared
    /// `-> Type` annotation, keyed by the block's name. `Spanned<String>`
    /// preserves the producer-file span (chunk-4 captures-but-does-not-render
    /// per D15 — the consumer only renders the *caller's* `-> Type` span).
    /// Absent when an exported block omits the annotation.
    pub block_return_types: HashMap<String, crate::span::Spanned<String>>,
    /// PRD #103 / Slice 2 (#105): per-exported-block parameter list, keyed by
    /// the block's name. The consumer's call-site validator
    /// (`validate_call_args`) consults this map to fire
    /// `G::analyze::missing-required-arg` when a positional argument for a
    /// required parameter is omitted in a cross-file call.
    pub block_params: HashMap<String, Vec<ast::Param>>,
    /// Issue #85: per-exported-block lowered `OutputTargetForm`, keyed by the
    /// block's name. Populated from `ExportBlockDecl::terminal_return` so the
    /// consumer can hoist the form onto the cross-file `IrCall` during the
    /// import fix-up step in `compile_source_with_resolved_imports`. This is
    /// the cross-file counterpart of `lower::block_callee_output_form` /
    /// `export_block_callee_output_form` for same-file callees.
    pub block_output_contracts: HashMap<String, OutputTargetForm>,
    /// Phase B.7: per-exported `type` decl description text, keyed by the type's
    /// name. Consumer files fold these into their `TypeRegistry` for cross-file
    /// type-level description lookup at emit time. Same-file decls take precedence.
    pub types: HashMap<String, String>,
    /// Codex review Finding 3: rendered source-text of every exported `const`
    /// keyed by the producer name. Bool values are normalized to lowercase to
    /// mirror the multi-file compile path (`extract_and_store_exports`). The
    /// import-aware check pipeline re-keys these under the consumer-local
    /// spelling and passes them to `analyze_with_imports` so an imported
    /// numeric/string const used bare in condition position is classified by
    /// the same `TypeTag` that compile would see.
    pub text_values: HashMap<String, String>,
    /// Companion to `text_values` carrying the inferred `TypeTag` for each
    /// exported const. Without this, the classifier in `collect_consts_for_file`
    /// falls back to `TypeTag::String` for every imported name and silently
    /// skips `condition-non-boolean-non-predicate` for imported numerics.
    pub text_value_types: HashMap<String, kind_infer::TypeTag>,
    /// B03 GAP 6: per-exported-block `description:` sub-section text, keyed by
    /// the block's name. Re-keyed under the consumer-local spelling in
    /// `check_one_file` and passed into `analyze_with_imports` so
    /// `check_applies_in_condition` recognises imported described blocks as
    /// valid `.applies()` receivers (otherwise
    /// `G::analyze::applies-on-undescribed-block` fires on every imported
    /// block reference in an export-block `if`/`elif` condition).
    pub block_descriptions: HashMap<String, String>,
}

/// Extract the exported names from a parsed source file.
fn extract_exports(file: &ast::SourceFile) -> ExportedNames {
    let mut exports = ExportedNames {
        texts: HashSet::new(),
        blocks: HashSet::new(),
        skills: HashSet::new(),
        privates: HashSet::new(),
        block_return_types: HashMap::new(),
        block_params: HashMap::new(),
        block_output_contracts: HashMap::new(),
        types: HashMap::new(),
        text_values: HashMap::new(),
        text_value_types: HashMap::new(),
        block_descriptions: HashMap::new(),
    };
    for decl in &file.decls {
        match decl {
            Decl::Const(c) => {
                // Exported consts share the `texts` namespace (post-issue-#81
                // `const` is the sole value-binding form); non-exported
                // (including `generated const`) are private.
                if c.node.exported {
                    exports.texts.insert(c.node.name.clone());
                    let rendered = match &c.node.value {
                        ast::ConstValue::Bool(s) => s.to_ascii_lowercase(),
                        other => other.rendered().to_string(),
                    };
                    let lit = match &c.node.value {
                        ast::ConstValue::String(s) => kind_infer::Literal::String(s.clone()),
                        ast::ConstValue::Int(s) | ast::ConstValue::Float(s) => {
                            kind_infer::Literal::Number(s.clone())
                        }
                        ast::ConstValue::Bool(s) => kind_infer::Literal::Bool(s.clone()),
                    };
                    let tag = kind_infer::infer_primitive(&lit);
                    exports.text_values.insert(c.node.name.clone(), rendered);
                    exports.text_value_types.insert(c.node.name.clone(), tag);
                } else {
                    exports.privates.insert(c.node.name.clone());
                }
            }
            Decl::ExportBlock(b) => {
                exports.blocks.insert(b.node.name.clone());
                if let Some(rt) = b.node.return_type.as_ref() {
                    exports
                        .block_return_types
                        .insert(b.node.name.clone(), rt.clone());
                }
                exports
                    .block_params
                    .insert(b.node.name.clone(), b.node.params.clone());
                // B03 GAP 6: capture the producer-side `description:` text so the
                // consumer can plumb it into `check_applies_in_condition` and avoid a
                // spurious `G::analyze::applies-on-undescribed-block` on a described
                // imported block used in an export-block condition.
                if let Some(desc) = b.node.description.as_ref() {
                    exports
                        .block_descriptions
                        .insert(b.node.name.clone(), desc.clone());
                }
                if let Some(form) = b.node.terminal_return.as_ref().and_then(|expr| match expr {
                    ReturnExpr::OutputTarget(OutputTargetExpr::Identifier(id)) => {
                        Some(OutputTargetForm::Identifier(id.name.clone()))
                    }
                    ReturnExpr::OutputTarget(OutputTargetExpr::Description(d)) => {
                        Some(OutputTargetForm::Description(d.content.clone()))
                    }
                    _ => None,
                }) {
                    exports
                        .block_output_contracts
                        .insert(b.node.name.clone(), form);
                }
            }
            Decl::Block(b) => {
                exports.privates.insert(b.node.name.clone());
            }
            Decl::Skill(s) => {
                exports.skills.insert(s.node.name.clone());
            }
            Decl::Import(_) => {}
            Decl::TypeDecl(t) => {
                if t.node.exported {
                    exports
                        .types
                        .insert(t.node.name.clone(), t.node.description.node.clone());
                } else {
                    exports.privates.insert(t.node.name.clone());
                }
            }
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
    check_file_with_effects(path, false)
}

pub fn check_file_with_effects(path: &Path, enable_effects: bool) -> DiagBag {
    let bags = check_file_partition(path, enable_effects);
    let mut merged = DiagBag::new();
    for (_p, b) in bags {
        merged.merge(b);
    }
    merged
}

/// Like [`check_file_with_effects`] but returns a per-file map of diagnostic
/// bags. Used by the LSP (M3) so it can publish each file's diagnostics under
/// the correct URI; back-compat callers keep using the merged-bag entry point
/// above.
///
/// The map key is the file's *canonical* path (as returned by
/// `Path::canonicalize`). On the failure path where the entry file itself
/// can't be canonicalized, the original (non-canonical) path is used as the
/// key for the synthetic `missing-file` diagnostic — there is no canonical
/// path to use.
pub fn check_file_partition(path: &Path, enable_effects: bool) -> HashMap<PathBuf, DiagBag> {
    let mut bags: HashMap<PathBuf, DiagBag> = HashMap::new();
    let canon = match path.canonicalize() {
        Ok(c) => c,
        Err(_) => {
            let span = Span::new(0, 0, 0);
            let li = LineIndex::new("");
            bags.entry(path.to_path_buf()).or_default().push(
                Diagnostic::error(
                    "G::analyze::missing-file",
                    format!("cannot read `{}`", path.display()),
                    SourceSpan::from_byte_span(path.to_string_lossy().as_ref(), span, &li),
                ),
                span,
            );
            return bags;
        }
    };

    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut stack: Vec<PathBuf> = Vec::new();
    check_file_recursive(&canon, &mut bags, &mut visited, &mut stack, enable_effects);
    // Guarantee the entry file always has a (possibly empty) bag in the map
    // so the LSP can clear stale diagnostics on a clean save.
    bags.entry(canon).or_default();
    bags
}

/// Like [`check_file_partition`] but takes an in-memory `source` for the
/// entry file at `current_path`. Dependencies are still read from disk.
///
/// This is the import-aware companion to [`check_source_with_resolutions_at_path`]:
/// the LSP holds the unsaved buffer in memory and wants cross-file diagnostics
/// from the dep DAG without writing the buffer back to disk first.
///
/// Returns a `path → DiagBag` map keyed on canonical paths (the entry file's
/// path is canonicalized via `current_path.canonicalize()` if possible; if the
/// file does not yet exist on disk, the un-canonicalized path is used as the
/// key — this lets the LSP publish diagnostics for new buffers).
pub fn check_source_with_imports(
    source: &str,
    file_id: u32,
    current_path: &Path,
    enable_effects: bool,
) -> HashMap<PathBuf, DiagBag> {
    let mut bags: HashMap<PathBuf, DiagBag> = HashMap::new();
    // Canonicalize when possible so the LSP-side URI key matches the canonical
    // form used by recursive imports (avoids the same file appearing under two
    // different keys).
    let canon = current_path
        .canonicalize()
        .unwrap_or_else(|_| current_path.to_path_buf());
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut stack: Vec<PathBuf> = Vec::new();
    check_one_file(
        &canon,
        source,
        file_id,
        &mut bags,
        &mut visited,
        &mut stack,
        enable_effects,
    );
    bags.entry(canon).or_default();
    bags
}

fn check_file_recursive(
    path: &Path,
    bags: &mut HashMap<PathBuf, DiagBag>,
    visited: &mut HashSet<PathBuf>,
    stack: &mut Vec<PathBuf>,
    enable_effects: bool,
) -> Option<ExportedNames> {
    // Cycle detection.
    if let Some(pos) = stack.iter().position(|p| p == path) {
        let cycle: Vec<String> = stack[pos..]
            .iter()
            .chain(std::iter::once(&path.to_path_buf()))
            .map(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        let cycle_str = cycle.join(" -> ");
        let span = Span::new(0, 0, 0);
        let li = LineIndex::new("");
        bags.entry(path.to_path_buf()).or_default().push(
            Diagnostic::error(
                "G::analyze::circular-import",
                format!("circular import: {}", cycle_str),
                SourceSpan::from_byte_span(
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .as_ref(),
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
        let file_label = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let parsed = parse::parse_with_diagnostics_opts(
            &source,
            0,
            &file_label,
            &line_index,
            &mut tmp_bag,
            enable_effects,
        )?;
        return Some(extract_exports(&parsed));
    }

    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => {
            let span = Span::new(0, 0, 0);
            let li = LineIndex::new("");
            bags.entry(path.to_path_buf()).or_default().push(
                Diagnostic::error(
                    "G::analyze::missing-file",
                    format!("cannot read `{}`", path.display()),
                    SourceSpan::from_byte_span(
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .as_ref(),
                        span,
                        &li,
                    ),
                ),
                span,
            );
            return None;
        }
    };

    check_one_file(path, &source, 0, bags, visited, stack, enable_effects)
}

/// Shared body for [`check_file_recursive`] (on-disk source) and
/// [`check_source_with_imports`] (in-memory source). Performs the per-file
/// parse + analyze step, walks `import` decls, and recurses into deps.
///
/// All diagnostics produced for `path` are written under
/// `bags.entry(path.to_path_buf())`. Recursive calls into deps write under
/// the dep's path. This is the per-file partitioning M3 needs.
fn check_one_file(
    path: &Path,
    source: &str,
    file_id: u32,
    bags: &mut HashMap<PathBuf, DiagBag>,
    visited: &mut HashSet<PathBuf>,
    stack: &mut Vec<PathBuf>,
    enable_effects: bool,
) -> Option<ExportedNames> {
    let key = path.to_path_buf();
    visited.insert(key.clone());
    stack.push(key.clone());

    let file_label = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let line_index = LineIndex::new(source);

    let parsed = parse::parse_with_diagnostics_opts(
        source,
        file_id,
        &file_label,
        &line_index,
        bags.entry(key.clone()).or_default(),
        enable_effects,
    );
    let file = match parsed {
        Some(f) => f,
        None => {
            // B01 belt-and-suspenders: parsing returned `None` but the
            // file's diagnostic bag is still empty. Surface a hard
            // `G::parse::unexpected` so directory- and check-mode never
            // silently report a clean exit on an unparseable file.
            {
                let entry_bag = bags.entry(key.clone()).or_default();
                if !entry_bag.has_error() && !entry_bag.has_repairable() {
                    let span = Span::new(file_id, 0, 0);
                    entry_bag.push(
                        Diagnostic::error(
                            "G::parse::unexpected",
                            "source could not be parsed and no specific diagnostic was reported",
                            SourceSpan::from_byte_span(&file_label, span, &line_index),
                        ),
                        span,
                    );
                }
            }
            stack.pop();
            return None;
        }
    };

    // Collect imported names for cross-file resolution.
    let mut imported_texts: HashSet<String> = HashSet::new();
    // Codex review Finding 3: parallel maps that carry the rendered body and
    // inferred TypeTag for every imported const, re-keyed under the
    // consumer-local spelling. Threaded into `analyze_with_imports` so the
    // classifier in `collect_consts_for_file` lands the correct
    // `ConditionTokenKind` for imported numeric/string consts in condition
    // position — matching what `compile_source_with_resolved_imports` does.
    let mut imported_text_values: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let mut imported_const_types: std::collections::BTreeMap<String, kind_infer::TypeTag> =
        std::collections::BTreeMap::new();
    let mut imported_blocks: HashSet<String> = HashSet::new();
    // Phase B.7: imported `export type` names (consumer-local spelling).
    // Used to mark `param.type_annotation` references as "used" so the
    // `G::analyze::unused-import` check passes. Not threaded into
    // `analyze_with_imports` — the analyzer doesn't yet validate type
    // annotations; that comes in a later phase.
    let mut imported_types: HashSet<String> = HashSet::new();
    // B.5 / spec §"Unified implicit-type-registration helper": consumer-local
    // type-import alias spans for every successful selective type-import.
    // Threaded into `analyze_with_imports` so the importing-side spelling
    // anchors the per-file domain-type registry — same-file param /
    // return / explicit-decl uses that drift from this spelling fire
    // `G::analyze::inconsistent-type-spelling`. Whole-module imports do
    // NOT contribute (qualified `alias.Type` refs unsupported in MVP).
    let mut imported_type_spans: HashMap<String, Span> = HashMap::new();
    let mut seen_import_paths: HashMap<PathBuf, Span> = HashMap::new();
    let mut used_import_names: HashSet<String> = HashSet::new();
    let mut all_import_names: Vec<(String, Span)> = Vec::new();
    // Issue #84 Chunk 4 (D15 / Option-Y): per-import-statement aliased
    // return-type map (consumer-local name → producer `Spanned<-> Type>`).
    let mut imported_block_return_types: HashMap<String, crate::span::Spanned<String>> =
        HashMap::new();
    // PRD #103 / Slice 2 (#105): per-import-statement aliased parameter list
    // (consumer-local name → producer `Vec<Param>`). Mirrors the structure
    // used for return types above; consumed by `analyze_with_imports` to
    // validate cross-file call-arg counts.
    let mut imported_block_params: HashMap<String, Vec<ast::Param>> = HashMap::new();
    // B03 GAP 6: consumer-local re-keyed map of producer-side `description:`
    // text per imported block. Threaded into `analyze_with_imports` so
    // `check_applies_in_condition` recognises a described imported block as a
    // valid `.applies()` receiver — without this, the export-block condition
    // validator (added in GAP 5) would fire
    // `G::analyze::applies-on-undescribed-block` on every imported block.
    let mut imported_block_descriptions: HashMap<String, String> = HashMap::new();
    // Task 9: per-import-alias resolved namespace kind, keyed by the
    // consumer-local name. Drives both the alias-case rule (PascalCase iff
    // Type, snake_case iff Value) and the kind-aware lookups in
    // `sweep_value_name_collisions` / `sweep_type_decl_name_collisions`.
    let mut import_alias_kinds: HashMap<String, (crate::name_kind::ResolvedImportKind, Span)> =
        HashMap::new();

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
                                let local = imp_name
                                    .alias
                                    .as_ref()
                                    .map(|a| a.node.as_str())
                                    .unwrap_or(imp_name.name.node.as_str());
                                let alias_span = imp_name
                                    .alias
                                    .as_ref()
                                    .map(|a| a.span)
                                    .unwrap_or(imp_name.name.span);
                                all_import_names.push((local.to_string(), import_span));
                                if imp_name.name.node == "subagent" || imp_name.name.node == "send"
                                {
                                    imported_blocks.insert(local.to_string());
                                    // Task 9: stdlib selective imports are
                                    // always Value-kinded (stdlib has no
                                    // exported types in MVP).
                                    import_alias_kinds.insert(
                                        local.to_string(),
                                        (crate::name_kind::ResolvedImportKind::Value, alias_span),
                                    );
                                } else {
                                    bags.entry(key.clone()).or_default().push(
                                        Diagnostic::error(
                                            "G::analyze::import-private",
                                            format!(
                                                "`{}` is not exported from `{}`",
                                                imp_name.name.node, import.path
                                            ),
                                            SourceSpan::from_byte_span(
                                                &file_label,
                                                import_span,
                                                &line_index,
                                            ),
                                        ),
                                        import_span,
                                    );
                                }
                            }
                        }
                        ImportKind::WholeModule { alias } => {
                            all_import_names.push((alias.node.clone(), import_span));
                            imported_blocks.insert(format!("{}.subagent", alias.node));
                            imported_blocks.insert(format!("{}.send", alias.node));
                            // Task 9: whole-module aliases bind to the value
                            // namespace (qualified `alias.Type` refs are
                            // MVP-unsupported, see B.7).
                            import_alias_kinds.insert(
                                alias.node.clone(),
                                (crate::name_kind::ResolvedImportKind::Value, alias.span),
                            );
                        }
                    }
                } else {
                    bags.entry(key.clone()).or_default().push(
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
                    bags.entry(key.clone()).or_default().push(
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
                bags.entry(key.clone()).or_default().push(
                    Diagnostic {
                        id: "G::analyze::duplicate-import".into(),
                        classification: Classification::Repairable,
                        message: format!("duplicate import of `{}`", import.path),
                        span: SourceSpan::from_byte_span(&file_label, import_span, &line_index),
                        related: vec![SourceSpan::from_byte_span(
                            &file_label,
                            *prev_span,
                            &line_index,
                        )],
                        hints: vec!["merge the import lists or remove the duplicate".into()],
                    },
                    import_span,
                );
                continue;
            }
            seen_import_paths.insert(resolved.clone(), import_span);

            // Recursively check/parse the dependency.
            let dep_exports = check_file_recursive(&resolved, bags, visited, stack, enable_effects);
            let dep_exports = match dep_exports {
                Some(e) => e,
                None => continue,
            };

            // Validate each imported name.
            match &import.kind {
                ImportKind::Selective(names) => {
                    for imp_name in names {
                        let local = imp_name
                            .alias
                            .as_ref()
                            .map(|a| a.node.as_str())
                            .unwrap_or(imp_name.name.node.as_str());
                        all_import_names.push((local.to_string(), import_span));

                        if dep_exports.skills.contains(&imp_name.name.node) {
                            bags.entry(key.clone()).or_default().push(
                                Diagnostic::error(
                                    "G::analyze::import-skill",
                                    format!(
                                        "`{}` is a `skill` and cannot be selectively imported",
                                        imp_name.name.node
                                    ),
                                    SourceSpan::from_byte_span(
                                        &file_label,
                                        import_span,
                                        &line_index,
                                    ),
                                ),
                                import_span,
                            );
                        } else if dep_exports.privates.contains(&imp_name.name.node) {
                            bags.entry(key.clone()).or_default().push(
                                Diagnostic::error(
                                    "G::analyze::import-private",
                                    format!(
                                        "`{}` is not exported from `{}`",
                                        imp_name.name.node, import.path
                                    ),
                                    SourceSpan::from_byte_span(
                                        &file_label,
                                        import_span,
                                        &line_index,
                                    ),
                                ),
                                import_span,
                            );
                        } else if dep_exports.texts.contains(&imp_name.name.node) {
                            imported_texts.insert(local.to_string());
                            // Re-key the producer-side const value/type under
                            // the consumer-local name (alias if present).
                            if let Some(v) = dep_exports.text_values.get(&imp_name.name.node) {
                                imported_text_values.insert(local.to_string(), v.clone());
                            }
                            if let Some(t) = dep_exports.text_value_types.get(&imp_name.name.node) {
                                imported_const_types.insert(local.to_string(), t.clone());
                            }
                            // Task 9: const re-export → Value alias.
                            let alias_span = imp_name
                                .alias
                                .as_ref()
                                .map(|a| a.span)
                                .unwrap_or(imp_name.name.span);
                            import_alias_kinds.insert(
                                local.to_string(),
                                (crate::name_kind::ResolvedImportKind::Value, alias_span),
                            );
                        } else if dep_exports.blocks.contains(&imp_name.name.node) {
                            imported_blocks.insert(local.to_string());
                            // Issue #84 Chunk 4: re-key the producer-side
                            // block return type under the consumer-local name.
                            if let Some(rt) =
                                dep_exports.block_return_types.get(&imp_name.name.node)
                            {
                                imported_block_return_types.insert(local.to_string(), rt.clone());
                            }
                            // PRD #103 / Slice 2 (#105): same re-keying for
                            // the producer-side parameter list.
                            if let Some(params) = dep_exports.block_params.get(&imp_name.name.node)
                            {
                                imported_block_params.insert(local.to_string(), params.clone());
                            }
                            // B03 GAP 6: mirror the params re-keying for descriptions.
                            if let Some(desc) =
                                dep_exports.block_descriptions.get(&imp_name.name.node)
                            {
                                imported_block_descriptions.insert(local.to_string(), desc.clone());
                            }
                            // Task 9: block re-export → Value alias.
                            let alias_span = imp_name
                                .alias
                                .as_ref()
                                .map(|a| a.span)
                                .unwrap_or(imp_name.name.span);
                            import_alias_kinds.insert(
                                local.to_string(),
                                (crate::name_kind::ResolvedImportKind::Value, alias_span),
                            );
                        } else if dep_exports.types.contains_key(&imp_name.name.node) {
                            // Phase B.7: a selectively imported `export type`
                            // name. Recorded so the `unused-import` post-pass
                            // can recognise param `type_annotation` references
                            // as a use.
                            imported_types.insert(local.to_string());
                            // B.5 / spec §"Unified implicit-type-registration
                            // helper": anchor the consumer-local type spelling
                            // in the per-file registry. Use the alias span
                            // when `as Foo` is present, otherwise the
                            // imported name's span.
                            let alias_span = imp_name
                                .alias
                                .as_ref()
                                .map(|a| a.span)
                                .unwrap_or(imp_name.name.span);
                            imported_type_spans.insert(local.to_string(), alias_span);
                            // Task 9: type re-export → Type alias.
                            import_alias_kinds.insert(
                                local.to_string(),
                                (crate::name_kind::ResolvedImportKind::Type, alias_span),
                            );
                        } else {
                            bags.entry(key.clone()).or_default().push(
                                Diagnostic::error(
                                    "G::analyze::import-private",
                                    format!(
                                        "`{}` is not exported from `{}`",
                                        imp_name.name.node, import.path
                                    ),
                                    SourceSpan::from_byte_span(
                                        &file_label,
                                        import_span,
                                        &line_index,
                                    ),
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
                    all_import_names.push((alias.node.clone(), import_span));
                    // Task 9: whole-module aliases bind to the value namespace
                    // (qualified `alias.Type` refs are MVP-unsupported, see
                    // B.7), so the alias-case rule treats them as Value.
                    import_alias_kinds.insert(
                        alias.node.clone(),
                        (crate::name_kind::ResolvedImportKind::Value, alias.span),
                    );
                    // Make all exported names available prefixed.
                    for t in &dep_exports.texts {
                        let qualified = format!("{}.{}", alias.node, t);
                        imported_texts.insert(qualified.clone());
                        // Mirror selective import: prefix the alias on
                        // const value/type so the classifier can resolve
                        // `alias.name` references in condition position.
                        if let Some(v) = dep_exports.text_values.get(t) {
                            imported_text_values.insert(qualified.clone(), v.clone());
                        }
                        if let Some(tag) = dep_exports.text_value_types.get(t) {
                            imported_const_types.insert(qualified, tag.clone());
                        }
                    }
                    for b in &dep_exports.blocks {
                        let qualified = format!("{}.{}", alias.node, b);
                        imported_blocks.insert(qualified.clone());
                        // Issue #84 Chunk 4: prefix imported block return
                        // types under `alias.name` to match the consumer's
                        // call-site spelling.
                        if let Some(rt) = dep_exports.block_return_types.get(b) {
                            imported_block_return_types.insert(qualified.clone(), rt.clone());
                        }
                        // PRD #103 / Slice 2 (#105): mirror the alias prefix
                        // on parameter lists.
                        if let Some(params) = dep_exports.block_params.get(b) {
                            imported_block_params.insert(qualified.clone(), params.clone());
                        }
                        // B03 GAP 6: prefix imported block descriptions under `alias.name` to
                        // match the consumer's call-site spelling, so an imported described
                        // block used in an export-block condition is recognised as a valid
                        // `.applies()` receiver.
                        if let Some(desc) = dep_exports.block_descriptions.get(b) {
                            imported_block_descriptions.insert(qualified, desc.clone());
                        }
                    }
                    // Phase B.7: prefix imported `export type` names under
                    // `alias.name` so the unused-import post-pass recognises
                    // whole-module-style param `type_annotation` references.
                    for t in dep_exports.types.keys() {
                        imported_types.insert(format!("{}.{}", alias.node, t));
                    }
                }
            }
        }
    }

    // A param `type_annotation` or header `-> ReturnType` referencing an
    // imported type counts as a "use" of that import so
    // `G::analyze::unused-import` does not fire. Selective only: whole-module
    // type imports (`import "..." as M; p: M.T`) inherit the same
    // alias-vs-qualified-name parity as whole-module text/block imports and
    // are parking-lot-deferred per the typed-params spec — when that's
    // resolved here, also fix it for texts and blocks in
    // `track_skill_usage` / `track_flow_usage`.
    for decl in &file.decls {
        let (params, return_type): (&[ast::Param], Option<&Spanned<String>>) = match decl {
            Decl::Skill(s) => (&s.node.params, s.node.return_type.as_ref()),
            Decl::Block(b) => (&b.node.params, b.node.return_type.as_ref()),
            Decl::ExportBlock(b) => (&b.node.params, b.node.return_type.as_ref()),
            _ => continue,
        };
        for p in params {
            if let Some(ta) = &p.type_annotation {
                if imported_types.contains(&ta.node) {
                    used_import_names.insert(ta.node.clone());
                }
            }
            // A `name_ref` parameter default that resolves to an imported
            // const counts as a use of that import — symmetric with the
            // type_annotation branch above. The lower-side resolver
            // (`resolve_param_default`) already accepts this shape; without
            // this branch, `unused-import` fires on imports that are only
            // referenced by a default value.
            if p.default_is_name_ref {
                if let Some(raw) = p.default.as_deref() {
                    if imported_texts.contains(raw) {
                        used_import_names.insert(raw.to_string());
                    }
                }
            }
        }
        if let Some(rt) = return_type {
            if imported_types.contains(&rt.node) {
                used_import_names.insert(rt.node.clone());
            }
        }
    }

    // Run Phase 2 with import-augmented name sets.
    let mut registry = domain_registry::Registry::new();
    // Codex review Finding 3: pass the imported const value/type maps built
    // above so the check-only path classifies imported numeric/string
    // consts identically to `compile_source_with_resolved_imports`. Without
    // these, every imported name fell back to `TypeTag::String` and a bare
    // imported numeric in condition position silently skipped
    // `condition-non-boolean-non-predicate`.
    let _ = analyze::analyze_with_imports(
        &file,
        0,
        &file_label,
        &line_index,
        bags.entry(key.clone()).or_default(),
        &imported_texts,
        &imported_blocks,
        &HashSet::new(),
        &HashSet::new(),
        &mut used_import_names,
        // B03 GAP 6: was `&HashMap::new()` — pass the consumer-local re-keyed
        // imported-block descriptions so `check_applies_in_condition` (now
        // invoked on export-block conditions per GAP 5) does not flag a
        // described imported block as undescribed.
        &imported_block_descriptions,
        &mut registry,
        &imported_block_return_types,
        &imported_block_params,
        &imported_text_values,
        &imported_const_types,
        &imported_type_spans,
        &import_alias_kinds,
        enable_effects,
    );

    // Unused import detection.
    for (name, span) in &all_import_names {
        if !used_import_names.contains(name) {
            bags.entry(key.clone()).or_default().push(
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

/// Result of compiling a directory of `.glyph` files.
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

/// DFS-walk the import graph from a single entry file and return the canonical
/// paths of every reachable `.glyph` file (entry inclusive), sorted for
/// determinism.
///
/// Used by the CLI to seed the directory pipeline from a single-file entry.
/// Skips `@glyph/...` stdlib imports (compiler-embedded, not on disk).
/// Tolerates read/parse failures on transitive files — the pipeline will
/// re-encounter and diagnose them. Terminates on import cycles via a visited
/// set.
pub fn compute_import_closure(entry: &Path, enable_effects: bool) -> Vec<PathBuf> {
    let canonical_entry = match entry.canonicalize() {
        Ok(p) => p,
        Err(_) => return vec![entry.to_path_buf()],
    };

    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut out: Vec<PathBuf> = Vec::new();
    let mut stack: Vec<PathBuf> = vec![canonical_entry];

    while let Some(current) = stack.pop() {
        if !visited.insert(current.clone()) {
            continue;
        }
        out.push(current.clone());

        let source = match std::fs::read_to_string(&current) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let label = current.display().to_string();
        let line_index = LineIndex::new(&source);
        let mut throwaway = DiagBag::new();
        let parsed = parse::parse_with_diagnostics_opts(
            &source,
            0,
            &label,
            &line_index,
            &mut throwaway,
            enable_effects,
        );

        if let Some(file) = parsed {
            for decl in &file.decls {
                if let Decl::Import(import_spanned) = decl {
                    if import_spanned.node.path.starts_with("@glyph/") {
                        continue;
                    }
                    if let Some(resolved) = resolve_import_path(&current, &import_spanned.node.path)
                    {
                        stack.push(resolved);
                    }
                }
            }
        }
    }

    out.sort();
    out
}

/// Compile all `.glyph` files in `sources` (already collected and sorted).
///
/// Builds the import DAG, topological-sorts, compiles each file in order.
/// Implements partial failure: skip-dependents, leave-stale-`.md`, exit 1 if
/// any file fails.
pub fn compile_directory(sources: &[PathBuf]) -> BuildResult {
    compile_directory_with_options(sources, false, false)
}

pub fn compile_directory_with_options(
    sources: &[PathBuf],
    emit_ir: bool,
    enable_effects: bool,
) -> BuildResult {
    compile_directory_with_layout(
        sources,
        emit_ir,
        enable_effects,
        &CompileOutputLayout::SameDir,
    )
}

pub fn compile_directory_with_layout(
    sources: &[PathBuf],
    emit_ir: bool,
    enable_effects: bool,
    layout: &CompileOutputLayout,
) -> BuildResult {
    if sources.is_empty() {
        return BuildResult {
            outcomes: Vec::new(),
            exit_code: 0,
        };
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
                dependents
                    .entry(dep.clone())
                    .or_default()
                    .push(file.clone());
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
    let mut procedure_paths: HashMap<(PathBuf, String), PathBuf> = HashMap::new();
    // Track exported names, text values, and block bodies per file for cross-file resolution.
    let mut file_exports: HashMap<PathBuf, ExportedNames> = HashMap::new();
    let mut file_text_values: HashMap<(PathBuf, String), String> = HashMap::new();
    let mut file_text_value_types: HashMap<(PathBuf, String), kind_infer::TypeTag> = HashMap::new();
    let mut file_block_bodies: HashMap<(PathBuf, String), String> = HashMap::new();
    let mut file_block_descriptions: HashMap<(PathBuf, String), String> = HashMap::new();
    let mut failed_files: HashSet<PathBuf> = HashSet::new();
    let mut outcomes: Vec<(PathBuf, FileOutcome)> = Vec::new();
    let mut any_failure = false;
    let mut warned_outside_root: HashSet<PathBuf> = HashSet::new();

    for file in &topo_order {
        // Check if any dependency failed.
        let deps = file_imports.get(file).cloned().unwrap_or_default();
        let failed_dep = deps
            .iter()
            .find(|d| file_set.contains(*d) && failed_files.contains(*d));

        if let Some(fd) = failed_dep {
            // Skip this file — a dependency failed.
            failed_files.insert(file.clone());
            any_failure = true;
            outcomes.push((
                file.clone(),
                FileOutcome::Skipped {
                    failed_dep: fd.clone(),
                },
            ));
            continue;
        }

        // Emit G::build::import-outside-out-dir warning if this file falls
        // outside the --out-dir input root. The file still compiles in-place.
        let mut outside_root_warn: Option<DiagBag> = None;
        if let CompileOutputLayout::OutDir { input_root, .. } = layout {
            if file.strip_prefix(input_root).is_err()
                && warned_outside_root.insert(file.to_path_buf())
            {
                // Synthetic span at file start — no source read needed.
                let li = LineIndex::new("");
                let label = file.display().to_string();
                let span = Span::new(0, 0, 0);
                let mut warn_bag = DiagBag::new();
                warn_bag.push(
                    Diagnostic {
                        id: diagnostic::IMPORT_OUTSIDE_OUT_DIR_DIAG_ID.into(),
                        classification: Classification::Warning,
                        message: format!(
                            "`{}` is imported from outside the `--out-dir` input root; writing in-place",
                            file.display()
                        ),
                        span: SourceSpan::from_byte_span(&label, span, &li),
                        related: Vec::new(),
                        hints: Vec::new(),
                    },
                    span,
                );
                outside_root_warn = Some(warn_bag);
            }
        }

        // Compile the file.
        // Build the imported-block-to-procedure-path mapping for this file.
        let imported_procedure_paths =
            build_imported_procedure_paths(file, &file_imports, &procedure_paths);

        // Build full resolved import data from dependency exports.
        let resolved_imports = build_resolved_imports(
            file,
            &file_exports,
            &file_text_values,
            &file_text_value_types,
            &file_block_bodies,
            &file_block_descriptions,
        );

        match compile_file_with_resolved_imports(
            file,
            &imported_procedure_paths,
            &resolved_imports,
            enable_effects,
            layout,
        ) {
            Ok(CompileOutcome::Compiled {
                mut diagnostics,
                arena,
                ..
            }) => {
                extract_and_store_exports(
                    file,
                    &mut file_exports,
                    &mut file_text_values,
                    &mut file_text_value_types,
                    &mut file_block_bodies,
                    &mut file_block_descriptions,
                );
                if emit_ir {
                    let source_file = file.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    if let Some(ir_json) =
                        emit_ir::serialize_ir_json(&arena, source_file, enable_effects)
                    {
                        let ir_path = resolve_output_path(file, OutputKind::IrJson, layout);
                        if let Some(parent) = ir_path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        atomic_write(&ir_path, &ir_json).ok();
                    }
                }
                if let Some(warn) = outside_root_warn {
                    diagnostics.merge(warn);
                }
                outcomes.push((file.clone(), FileOutcome::Compiled { diagnostics }));
            }
            Ok(CompileOutcome::Diagnostics(mut bag)) => {
                if let Some(w) = outside_root_warn.take() {
                    bag.merge(w);
                }
                failed_files.insert(file.clone());
                if bag.has_error() {
                    any_failure = true;
                }
                outcomes.push((file.clone(), FileOutcome::Failed { diagnostics: bag }));
            }
            Err(CompileError::Lower(lower::LowerError::NoSkill)) => {
                // Library file (no skill declaration) — not a failure unless
                // procedure emission produces error diagnostics (e.g. a Tier-3
                // `export block` whose params lack descriptions hard-fails
                // under stub fill). When emission errors, mark the library
                // failed so dependents are skipped by the cascade above —
                // otherwise consumers would resolve imports to the exported
                // block body and emit dangling `Follow the X procedure below.`
                // anchors against procedure files that were never written.
                extract_and_store_exports(
                    file,
                    &mut file_exports,
                    &mut file_text_values,
                    &mut file_text_value_types,
                    &mut file_block_bodies,
                    &mut file_block_descriptions,
                );
                let (emitted, proc_diags) = emit_library_procedures(file, enable_effects, layout);
                for (block_name, rel_path) in emitted {
                    procedure_paths.insert((file.clone(), block_name), rel_path);
                }
                let mut lib_diags = outside_root_warn.unwrap_or_default();
                lib_diags.merge(proc_diags);
                if lib_diags.has_error() {
                    failed_files.insert(file.clone());
                    any_failure = true;
                    outcomes.push((
                        file.clone(),
                        FileOutcome::Failed {
                            diagnostics: lib_diags,
                        },
                    ));
                } else {
                    outcomes.push((
                        file.clone(),
                        FileOutcome::Compiled {
                            diagnostics: lib_diags,
                        },
                    ));
                }
            }
            Err(e) => {
                failed_files.insert(file.clone());
                any_failure = true;
                // Synthesise a diagnostic so the CLI surfaces *something*
                // instead of a silent exit-1 for Read/Write/Parse/Lower/Validate
                // errors that aren't already wired to structured IDs.
                let mut bag = DiagBag::new();
                if let Some(w) = outside_root_warn.take() {
                    bag.merge(w);
                }
                let li = LineIndex::new("");
                let label = file.display().to_string();
                let span = Span::new(0, 0, 0);
                bag.push(
                    Diagnostic::error(
                        "G::build::compile-error",
                        format!("compile pipeline failed: {:?}", e),
                        SourceSpan::from_byte_span(&label, span, &li),
                    ),
                    span,
                );
                outcomes.push((file.clone(), FileOutcome::Failed { diagnostics: bag }));
            }
        }
    }

    let diag_worst = outcomes
        .iter()
        .map(|(_, o)| match o {
            FileOutcome::Compiled { diagnostics } | FileOutcome::Failed { diagnostics } => {
                diagnostics.exit_code()
            }
            FileOutcome::Skipped { .. } => 0,
        })
        .fold(0u8, |acc, code| match (acc, code) {
            (1, _) | (_, 1) => 1,
            (2, _) | (_, 2) => 2,
            _ => 0,
        });
    let exit_code = if any_failure { 1 } else { diag_worst };
    BuildResult {
        outcomes,
        exit_code,
    }
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

// Return the `.tmp` sibling path for a given output path.
fn tmp_path_for(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".tmp");
    PathBuf::from(s)
}

fn llm_required_diagnostics_from_errors(
    mut errors: Vec<emit::StubFillError>,
    file_label: &str,
) -> DiagBag {
    errors.sort_by_key(|e| e.sort_key());
    let mut bag = DiagBag::new();
    let li = LineIndex::new("");
    let span = Span::new(0, 0, 0);
    for e in errors {
        match e {
            emit::StubFillError::CallBody {
                ir_node,
                target_name,
                has_modifier,
                has_local_refs,
            } => {
                let msg = build_call_body_message(
                    target_name.as_deref(),
                    ir_node,
                    has_modifier,
                    has_local_refs,
                );
                bag.push(
                    Diagnostic::error(
                        "G::expand::llm-required-for-call",
                        msg,
                        SourceSpan::from_byte_span(file_label, span, &li),
                    ),
                    span,
                );
            }
            emit::StubFillError::ParamDescription {
                origin: _,
                param_name,
                param_type,
                param_default,
            } => {
                let msg = build_param_description_message(
                    param_name.as_deref(),
                    param_type.as_deref(),
                    param_default.as_deref(),
                );
                bag.push(
                    Diagnostic::error(
                        "G::expand::llm-required-for-param-description",
                        msg,
                        SourceSpan::from_byte_span(file_label, span, &li),
                    ),
                    span,
                );
            }
        }
    }
    bag
}

fn build_call_body_message(
    target_name: Option<&str>,
    ir_node: crate::ir::NodeId,
    has_modifier: bool,
    has_local_refs: bool,
) -> String {
    let reason_phrase = match (has_modifier, has_local_refs) {
        (true, false) => "a with modifier",
        (false, true) => "local-ref cross-references",
        (true, true) => "a with modifier and local-ref cross-references",
        (false, false) => {
            unreachable!("CallBody is only pushed when site_modifier or local_refs is non-empty")
        }
    };
    let remediation = match (has_modifier, has_local_refs) {
        (true, false) => "the with modifier",
        (false, true) => "the local reference",
        (true, true) => "the with modifier / rewrite the local reference",
        (false, false) => unreachable!(),
    };
    let target = target_name.unwrap_or("<unknown>");
    let nid = format!("n{}", ir_node.0);
    let mut out = String::new();
    out.push_str("Call to `");
    out.push_str(target);
    out.push_str("` (IR ");
    out.push_str(&nid);
    out.push_str(") requires LLM-grade expansion because it has ");
    out.push_str(reason_phrase);
    out.push_str("; this compiler build is using the stub filler. ");
    out.push_str("Enable the LLM expand filler, or drop ");
    out.push_str(remediation);
    out.push('.');
    out
}

fn build_param_description_message(
    param_name: Option<&str>,
    param_type: Option<&str>,
    param_default: Option<&str>,
) -> String {
    let name = param_name.unwrap_or("<unknown>");
    let mut out = String::new();
    out.push_str("Parameter `");
    out.push_str(name);
    out.push('`');
    if let Some(t) = param_type {
        out.push_str(" (");
        out.push_str(t);
        out.push(')');
    }
    if let Some(d) = param_default {
        out.push_str(", default `");
        out.push_str(d);
        out.push('`');
    }
    out.push_str(", has no description; this compiler build is using the stub filler and cannot synthesize prose. ");

    if param_type.is_some() {
        out.push_str("Add an inline description `<\"...\">` on the parameter slot, ");
        out.push_str("add a `type <Type> = <\"...\">` decl so the type registry has one, ");
        out.push_str("or enable the LLM expand filler.");
    } else {
        out.push_str("Add an inline description `<\"...\">` on the parameter slot ");
        out.push_str("(optionally add a type annotation so a registry description applies), ");
        out.push_str("or enable the LLM expand filler.");
    }
    out
}

/// Map `foo.glyph` → `foo.ir.json` next to the source file.
fn ir_json_output_path(input: &Path) -> PathBuf {
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let file_name = input.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let stem = file_name
        .strip_suffix(".glyph")
        .unwrap_or_else(|| file_name.strip_suffix(".md").unwrap_or(file_name));
    parent.join(format!("{}.ir.json", stem))
}

#[derive(Debug, Clone)]
pub enum CompileOutputLayout {
    SameDir,
    EntryFile { entry: PathBuf, output: PathBuf },
    OutDir { root: PathBuf, input_root: PathBuf },
}

#[derive(Debug, Clone)]
pub enum OutputKind {
    Compiled,
    IrJson,
    Procedure {
        lib_stem: String,
        block_kebab: String,
    },
}

pub fn resolve_output_path(
    source: &Path,
    kind: OutputKind,
    layout: &CompileOutputLayout,
) -> PathBuf {
    let same_dir = || match &kind {
        OutputKind::Compiled => compiled_output_path(source),
        OutputKind::IrJson => ir_json_output_path(source),
        OutputKind::Procedure {
            lib_stem,
            block_kebab,
        } => source
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(lib_stem)
            .join(format!("{}.md", block_kebab)),
    };

    match layout {
        CompileOutputLayout::SameDir => same_dir(),
        CompileOutputLayout::EntryFile { entry, output } => {
            if source == entry.as_path() {
                match &kind {
                    OutputKind::Compiled => output.clone(),
                    OutputKind::IrJson => {
                        let parent = output.parent().unwrap_or_else(|| Path::new("."));
                        let stem = output
                            .file_name()
                            .and_then(|s| s.to_str())
                            .and_then(|s| s.strip_suffix(".md").map(|x| x.to_string()))
                            .unwrap_or_else(|| {
                                output
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("out")
                                    .to_string()
                            });
                        parent.join(format!("{}.ir.json", stem))
                    }
                    OutputKind::Procedure {
                        lib_stem: _,
                        block_kebab,
                    } => {
                        let parent = output.parent().unwrap_or_else(|| Path::new("."));
                        let stem = output
                            .file_name()
                            .and_then(|s| s.to_str())
                            .and_then(|s| s.strip_suffix(".md"))
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| {
                                output
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("out")
                                    .to_string()
                            });
                        parent.join(stem).join(format!("{}.md", block_kebab))
                    }
                }
            } else {
                same_dir()
            }
        }
        CompileOutputLayout::OutDir { root, input_root } => match source.strip_prefix(input_root) {
            Ok(rel) => {
                let parent_rel = rel.parent().unwrap_or_else(|| Path::new(""));
                let stem = rel
                    .file_name()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.strip_suffix(".glyph").or_else(|| s.strip_suffix(".md")))
                    .unwrap_or("out");
                match &kind {
                    OutputKind::Compiled => root.join(parent_rel).join(format!("{}.md", stem)),
                    OutputKind::IrJson => root.join(parent_rel).join(format!("{}.ir.json", stem)),
                    OutputKind::Procedure {
                        lib_stem,
                        block_kebab,
                    } => root
                        .join(parent_rel)
                        .join(lib_stem)
                        .join(format!("{}.md", block_kebab)),
                }
            }
            Err(_) => same_dir(),
        },
    }
}

/// Compute the string baked into the consumer's compiled output and IR JSON
/// as `procedure_path`. Returns a forward-slash relative path when both
/// arguments share a common ancestor; otherwise returns the procedure's
/// absolute path with forward-slash separators.
pub fn resolve_procedure_reference(consumer_output: &Path, procedure_output: &Path) -> String {
    use std::path::Component;

    let consumer_dir = consumer_output.parent().unwrap_or_else(|| Path::new(""));

    let mut up = 0usize;
    let mut cursor = consumer_dir;
    loop {
        // Only accept strip_prefix if cursor is not empty or if it's a rooted path.
        // Skip empty cursors to avoid false common-ancestor matches.
        if !cursor.as_os_str().is_empty() {
            if let Ok(rel) = procedure_output.strip_prefix(cursor) {
                let mut parts: Vec<String> = (0..up).map(|_| "..".to_string()).collect();
                for c in rel.components() {
                    // Skip RootDir components.
                    if !matches!(c, Component::RootDir) {
                        parts.push(c.as_os_str().to_string_lossy().into_owned());
                    }
                }
                return parts.join("/");
            }
        }
        match cursor.parent() {
            Some(p) if p != cursor => {
                cursor = p;
                up += 1;
            }
            _ => break,
        }
    }

    // No common ancestor: return absolute path with forward-slash separators.
    let mut parts = Vec::new();
    for c in procedure_output.components() {
        match c {
            Component::RootDir => {
                // Will be handled by prepending "/" if absolute.
            }
            _ => {
                parts.push(c.as_os_str().to_string_lossy().into_owned());
            }
        }
    }
    let joined = parts.join("/");
    if procedure_output.is_absolute() {
        format!("/{}", joined)
    } else {
        joined
    }
}

/// Map `foo.glyph` → `foo.md` next to the source file.
fn compiled_output_path(input: &Path) -> std::path::PathBuf {
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let file_name = input.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let stem = file_name
        .strip_suffix(".glyph")
        .unwrap_or(file_name.strip_suffix(".md").unwrap_or(file_name));
    parent.join(format!("{}.md", stem))
}

/// Extract the library stem from a source path: `repo_tools.glyph` → `repo_tools`.
fn library_stem(input: &Path) -> String {
    let file_name = input.file_name().and_then(|s| s.to_str()).unwrap_or("");
    file_name
        .strip_suffix(".glyph")
        .unwrap_or(file_name.strip_suffix(".md").unwrap_or(file_name))
        .to_string()
}

/// Emit standalone procedure `.md` files for qualifying export blocks in a
/// library file. An export block qualifies when its body_word_count >= 150.
///
/// Output path: `<parent>/<lib_stem>/<block-name-kebab>.md`
/// Returns: Vec of (block_name, absolute_procedure_path) for Tier 3 tracking.
fn emit_library_procedures(
    path: &Path,
    enable_effects: bool,
    layout: &CompileOutputLayout,
) -> (Vec<(String, PathBuf)>, DiagBag) {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return (Vec::new(), DiagBag::new()),
    };
    let parsed = match parse::parse(&source, 0) {
        Ok((file, _)) => file,
        Err(_) => return (Vec::new(), DiagBag::new()),
    };

    let stem = library_stem(path);
    let mut emitted = Vec::new();
    let mut diags = DiagBag::new();

    // Build a local TypeRegistry from same-file `type` decls so the §8.4
    // templates can resolve type-level descriptions when `-> Foo` matches a
    // declared type. Imported `export type` descriptions are folded in below
    // (Codex finding #3): a Tier 3 procedure file's `## Parameters` section
    // depends on the registry to render the type-description sentence, and
    // that sentence is just as relevant when `Foo` is `import`-ed from a
    // sibling library file as when it's declared in this file. Same-file
    // decls take precedence on name collision (mirrors the local-vs-imported
    // rule in `lower::lower_with_imports`).
    let mut local_type_registry = ir::TypeRegistry::default();
    // Codex finding #2: name_ref parameter defaults must be resolved against
    // the same const context the consumer-side lower pass uses, otherwise the
    // procedure file emits `Default: default_scope` instead of the resolved
    // `"."`. Build text-value and TypeTag maps in the same pass that already
    // walks imports for type descriptions.
    let mut imported_texts: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let mut imported_const_types: std::collections::BTreeMap<String, kind_infer::TypeTag> =
        std::collections::BTreeMap::new();
    // Imported entries first, then same-file decls overwrite them.
    for decl in &parsed.decls {
        if let Decl::Import(import_spanned) = decl {
            let import = &import_spanned.node;
            // `@glyph/std` is compiler-embedded and exports no `type` decls;
            // skip the filesystem lookup.
            if import.path.starts_with("@glyph/") {
                continue;
            }
            let resolved = match resolve_import_path(path, &import.path) {
                Some(r) => r,
                None => continue,
            };
            let dep_source = match std::fs::read_to_string(&resolved) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let dep_parsed = match parse::parse(&dep_source, 0) {
                Ok((f, _)) => f,
                Err(_) => continue,
            };
            let dep_exports = extract_exports(&dep_parsed);
            // The dep's exported `const` rendered values + TypeTag, keyed by
            // the *producer* name. Re-keyed to the consumer-local spelling
            // below.
            let dep_consts = lower::collect_consts(&dep_parsed);
            match &import.kind {
                ImportKind::Selective(names) => {
                    for imp_name in names {
                        let producer = imp_name.name.node.as_str();
                        let local = imp_name
                            .alias
                            .as_ref()
                            .map(|a| a.node.as_str())
                            .unwrap_or(producer);
                        if let Some(desc) = dep_exports.types.get(producer) {
                            // Honour `as` aliasing — the consumer's type
                            // annotation will spell the type with the local name.
                            local_type_registry.insert(local, desc.clone());
                        }
                        if dep_exports.texts.contains(producer) {
                            if let Some((rendered, tag)) = dep_consts.get(producer) {
                                imported_texts.insert(local.to_string(), rendered.clone());
                                imported_const_types.insert(local.to_string(), tag.clone());
                            }
                        }
                    }
                }
                ImportKind::WholeModule { alias } => {
                    for (type_name, desc) in &dep_exports.types {
                        local_type_registry
                            .insert(&format!("{}.{}", alias.node, type_name), desc.clone());
                    }
                    for text_name in &dep_exports.texts {
                        if let Some((rendered, tag)) = dep_consts.get(text_name) {
                            let qualified = format!("{}.{}", alias.node, text_name);
                            imported_texts.insert(qualified.clone(), rendered.clone());
                            imported_const_types.insert(qualified, tag.clone());
                        }
                    }
                }
            }
        }
    }
    for decl in &parsed.decls {
        if let Decl::TypeDecl(t) = decl {
            local_type_registry.insert(&t.node.name, t.node.description.node.clone());
        }
    }
    let same_file_consts = lower::collect_consts(&parsed);

    for decl in &parsed.decls {
        if let Decl::ExportBlock(eb) = decl {
            if eb.node.body_word_count < 150 {
                continue;
            }
            let kebab_name = eb.node.name.replace('_', "-");

            // Resolve name_ref defaults parallel to `eb.node.params` (Codex
            // finding #2). Stored separately so each ProcedureParam can
            // borrow from it for the duration of the emit call.
            let resolved_defaults: Vec<Option<String>> = eb
                .node
                .params
                .iter()
                .map(|p| {
                    lower::resolve_param_default(
                        p,
                        &same_file_consts,
                        &imported_const_types,
                        &imported_texts,
                    )
                })
                .collect();
            let params: Vec<emit::ProcedureParam<'_>> = eb
                .node
                .params
                .iter()
                .zip(resolved_defaults.iter())
                .map(|(p, default)| emit::ProcedureParam {
                    name: p.name.as_str(),
                    type_annotation: p.type_annotation.as_ref().map(|s| s.node.as_str()),
                    description: p.description.as_ref().map(|s| s.node.as_str()),
                    default: default.as_deref(),
                })
                .collect();

            let desc = eb.node.description.as_deref().unwrap_or("");
            let output_form = match &eb.node.terminal_return {
                Some(ReturnExpr::OutputTarget(OutputTargetExpr::Identifier(id))) => {
                    Some(OutputTargetForm::Identifier(id.name.clone()))
                }
                Some(ReturnExpr::OutputTarget(OutputTargetExpr::Description(d))) => {
                    Some(OutputTargetForm::Description(d.content.clone()))
                }
                _ => None,
            };
            let return_type_text = eb.node.return_type.as_ref().map(|s| s.node.clone());
            // Resolve the export block's freeform sections per design
            // §4.1.5 / D12 — heading-depth threading for Tier 3 sits at H2.
            // NameRef items dereference through `same_file_consts`; marker
            // metadata maps the source-spelling reserved word to its
            // canonical (strength, polarity) pair (mirrors `lower::marker_metadata`).
            // Heading resolution mirrors the Tier 1 / Tier 2 path via
            // `lower::resolve_freeform_heading` so a catalogue entry's
            // `heading` override is honored (e.g. `acceptance:` → `Acceptance
            // Criteria` rather than the derived `Acceptance`).
            let catalogue = crate::sections::SectionCatalogue::load();
            let freeform_sections: Vec<emit::ProcedureFreeformSection> = eb
                .node
                .freeform_sections
                .iter()
                .map(|fs| {
                    let items: Vec<emit::ProcedureFreeformItem> = fs
                        .items
                        .iter()
                        .filter_map(|item| resolve_freeform_item(item, &same_file_consts))
                        .collect();
                    emit::ProcedureFreeformSection {
                        heading: lower::resolve_freeform_heading(&catalogue, &fs.name),
                        items,
                    }
                })
                .collect();
            // #168: resolve body-level constraints + context to their flat
            // (strength, polarity, text) / (name, text) forms before passing
            // to `emit_procedure`. Resolution order mirrors the rest of this
            // function: `same_file_consts` first, then `imported_texts`. Markers
            // that don't resolve are silently skipped — analyze fires
            // `G::analyze::closure-violation` and `G::analyze::unresolved-name`
            // for those cases at the front of the pipeline, so they cannot
            // reach this point in a green build.
            let resolved_constraints: Vec<(ir::Strength, ir::Polarity, String)> = eb
                .node
                .body_constraints
                .iter()
                .filter_map(|m| {
                    let (strength, polarity) = match m.marker {
                        ast::ConstraintMarkerKind::Require => {
                            (ir::Strength::Soft, ir::Polarity::Require)
                        }
                        ast::ConstraintMarkerKind::Avoid => {
                            (ir::Strength::Soft, ir::Polarity::Avoid)
                        }
                        ast::ConstraintMarkerKind::Must => {
                            (ir::Strength::Hard, ir::Polarity::Require)
                        }
                        ast::ConstraintMarkerKind::MustAvoid => {
                            (ir::Strength::Hard, ir::Polarity::Avoid)
                        }
                    };
                    let name = m.name.node.as_str();
                    let text = same_file_consts
                        .get(name)
                        .map(|(s, _)| s.clone())
                        .or_else(|| imported_texts.get(name).cloned())?;
                    Some((strength, polarity, text))
                })
                .collect();
            let resolved_context: Vec<(Option<String>, String)> = eb
                .node
                .body_context
                .iter()
                .filter_map(|c| match c {
                    ast::ContextEntry::NameRef(spanned) => {
                        let name = spanned.node.as_str();
                        same_file_consts
                            .get(name)
                            .map(|(s, _)| s.clone())
                            .or_else(|| imported_texts.get(name).cloned())
                            .map(|text| (Some(name.to_string()), text))
                    }
                    ast::ContextEntry::InlineString(s) => Some((None, s.clone())),
                })
                .collect();
            let constraints_view: Vec<emit::ProcedureConstraint<'_>> = resolved_constraints
                .iter()
                .map(|(strength, polarity, text)| emit::ProcedureConstraint {
                    strength: *strength,
                    polarity: *polarity,
                    text: text.as_str(),
                })
                .collect();
            let context_view: Vec<emit::ProcedureContext<'_>> = resolved_context
                .iter()
                .map(|(name, text)| emit::ProcedureContext {
                    name: name.as_deref(),
                    text: text.as_str(),
                })
                .collect();
            // Synthesize structured `flow_items` from the export block's
            // parse-collected `flow_strings`. The library emit path does not
            // currently lower flow into the arena, so all items project as
            // `IrBlockFlowItem::Inline` and the arena is an empty stub.
            let synthesized_flow: Vec<crate::ir::IrBlockFlowItem> = eb
                .node
                .flow_strings
                .iter()
                .map(|s| crate::ir::IrBlockFlowItem::Inline { text: s.clone() })
                .collect();
            let stub_arena = crate::ir::IrArena::new();
            let markdown_res = emit::emit_procedure(
                &eb.node.name,
                desc,
                &eb.node.effects,
                &params,
                &synthesized_flow,
                &stub_arena,
                output_form.as_ref(),
                return_type_text.as_deref(),
                &local_type_registry,
                enable_effects,
                &freeform_sections,
                &constraints_view,
                &context_view,
            );
            let markdown = match markdown_res {
                Ok(md) => md,
                Err(errors) => {
                    let label = path.display().to_string();
                    diags.merge(llm_required_diagnostics_from_errors(errors, &label));
                    continue;
                }
            };

            let out_path = resolve_output_path(
                path,
                OutputKind::Procedure {
                    lib_stem: stem.to_string(),
                    block_kebab: kebab_name.clone(),
                },
                layout,
            );
            if let Some(parent) = out_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            atomic_write(&out_path, &markdown).ok();

            emitted.push((eb.node.name.clone(), out_path));
        }
    }
    (emitted, diags)
}

/// Resolve one AST `FreeformItem` into a pre-rendered `ProcedureFreeformItem`
/// for Tier 3 emission. Mirrors `lower::lower_freeform_item` (which builds the
/// equivalent IR node for Tier 1 / Tier 2): `NameRef` dereferences through
/// `consts`; `MarkerClause` renders into prose via the locked four-form
/// constraint template (`require`/`avoid`/`must`/`must avoid`) or keeps the
/// raw operand text for `context`.
///
/// # CROSS-PHASE INVARIANT — READ BEFORE TOUCHING
///
/// Lower-time analysis (see `lower::lower_freeform_item` returning
/// `LowerError::UndefinedContextRef` and the analyze passes that fire
/// `G::analyze::undefined-name` for bare-name refs) **MUST** surface every
/// unresolved `NameRef` BEFORE this function runs. The `NameRef` arm below
/// returns `None` and the `Option` is dropped silently downstream — if
/// validation order ever regresses such that an unresolved `NameRef` reaches
/// this point, the item will vanish from Tier 3 output with **no diagnostic,
/// no warning, and no log**: the author's authored line just disappears.
///
/// If you change validation order, reorganize the lower pipeline, add a new
/// caller that bypasses lower-time analysis, or for any reason cannot
/// guarantee that the `NameRef` arm is unreachable here, you MUST either:
///   (a) add a `debug_assert!(false, "unresolved NameRef in Tier 3 …")` in
///       the `NameRef` arm below so debug builds panic instead of silently
///       eliding the item, OR
///   (b) change the return type to `Result<Option<…>, …>` and propagate the
///       error so callers can surface a diagnostic.
///
/// Do not "fix" the silent drop by inserting placeholder text — that hides
/// the regression instead of catching it.
fn resolve_freeform_item(
    item: &ast::FreeformItem,
    consts: &std::collections::BTreeMap<String, (String, kind_infer::TypeTag)>,
) -> Option<emit::ProcedureFreeformItem> {
    use ast::FreeformItem;
    match item {
        FreeformItem::StringLiteral(s) => Some(emit::ProcedureFreeformItem {
            text: s.node.clone(),
        }),
        FreeformItem::NameRef(name) => consts
            .get(&name.node)
            .map(|(text, _)| text.clone())
            .map(|text| emit::ProcedureFreeformItem { text }),
        FreeformItem::MarkerClause { marker, text } => {
            // Marker → (strength, polarity, _word) from the canonical mapper.
            let (strength, polarity, _word) = lower::marker_metadata(*marker);
            // Resolve operand: bare-name lookups via `consts`; string-literal
            // operands pass through. Mirrors `lower::lower_freeform_item`.
            let raw = text.node.clone();
            let resolved_operand = consts
                .get(&raw)
                .map(|(t, _)| t.clone())
                .unwrap_or_else(|| raw.clone());
            let rendered = match (strength, polarity) {
                (Some(s), Some(p)) => {
                    crate::sections::hooks::dispatch_constraints_expand(s, p, &resolved_operand)
                }
                _ => resolved_operand,
            };
            Some(emit::ProcedureFreeformItem { text: rendered })
        }
    }
}

/// Build a mapping from imported block names to their procedure file paths
/// for a given consumer file.
fn build_imported_procedure_paths(
    consumer: &Path,
    _file_imports: &HashMap<PathBuf, Vec<PathBuf>>,
    procedure_paths: &HashMap<(PathBuf, String), PathBuf>,
) -> HashMap<String, PathBuf> {
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
                        let local = imp_name
                            .alias
                            .as_ref()
                            .map(|a| a.node.as_str())
                            .unwrap_or(imp_name.name.node.as_str());
                        let key = (resolved.clone(), imp_name.name.node.clone());
                        if let Some(proc_path) = procedure_paths.get(&key) {
                            result.insert(local.to_string(), proc_path.clone());
                        }
                    }
                }
                ImportKind::WholeModule { alias } => {
                    for ((lib_path, block_name), proc_path) in procedure_paths {
                        if *lib_path == resolved {
                            let qualified = format!("{}.{}", alias.node, block_name);
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
    /// Inferred `TypeTag` for each imported `const` value present in
    /// `text_values`. Lets the lower pass re-render name_ref param defaults
    /// with the correct quoting (string consts wrap in `"…"`; numeric/bool
    /// pass through verbatim).
    text_value_types: std::collections::BTreeMap<String, kind_infer::TypeTag>,
    block_bodies: HashMap<String, String>,
    block_descriptions: HashMap<String, String>,
    /// Issue #84 Chunk 4 (D15 / Option-Y): aliased imported-block return
    /// types. Keyed by the *consumer-side* local name (post-alias /
    /// post-prefix), valued by the producer-file `Spanned<-> Type>`.
    block_return_types: HashMap<String, crate::span::Spanned<String>>,
    /// PRD #103 / Slice 2 (#105): aliased imported-block parameter lists,
    /// keyed by the *consumer-side* local name. Powers
    /// `G::analyze::missing-required-arg` enforcement at cross-file call
    /// sites — see `analyze_with_imports`.
    block_params: HashMap<String, Vec<ast::Param>>,
    /// Issue #85: aliased imported-block output-contract forms, keyed by the
    /// consumer-side local name. Hoisted onto cross-file Tier-1 `IrCall`
    /// nodes so expand- and emit-time gates can read the callee's OC without
    /// an arena lookup.
    block_output_contracts: HashMap<String, OutputTargetForm>,
    /// Imported `export type` description text, re-keyed by the consumer-side
    /// local (post-alias / post-prefix) name. Folded into the consumer's
    /// `TypeRegistry` during lowering.
    type_descriptions: std::collections::BTreeMap<String, String>,
    /// B.5 / spec §"Unified implicit-type-registration helper": consumer-side
    /// type-import alias spans for every selective type-import. Keyed by the
    /// consumer-local spelling (alias if present, else exported name).
    /// Whole-module imports do NOT contribute — qualified `alias.Type` refs
    /// are MVP-unsupported. Threaded into `analyze_with_imports`.
    type_spans: HashMap<String, crate::span::Span>,
    /// Task 9: resolved namespace kind for every import alias (consumer-local
    /// name → (Type|Value, alias span)). Drives the alias-case rule and the
    /// kind-aware lookups in `sweep_value_name_collisions` /
    /// `sweep_type_decl_name_collisions`. Mirrors the same map the
    /// check-only pipeline builds inside `check_file_recursive`.
    import_alias_kinds: HashMap<String, (crate::name_kind::ResolvedImportKind, crate::span::Span)>,
    /// B06 concern 3: diagnostics raised while resolving `@glyph/std`
    /// imports — importing the compiler-internal `load`, an unknown
    /// selective name, or an unknown stdlib module. The check pipeline
    /// raises these inline in `check_one_file`; the compile/directory
    /// pipeline has no equivalent import-validation pass, so they are
    /// collected here and surfaced by `compile_file_with_resolved_imports`
    /// (which also keeps such a file off the no-import fast path).
    import_diagnostics: DiagBag,
}

/// Build the full resolved import data for a consumer file.
fn stdlib_synthetic_block(
    exported_name: &str,
) -> Option<(
    String,
    Vec<ast::Param>,
    Option<crate::span::Spanned<String>>,
)> {
    // Synthetic in-memory definitions for `@glyph/std` selective imports.
    // `load` is intentionally excluded: it is compiler-internal and not
    // author-importable (`design/stdlib.md` §The `load` Primitive).
    let zero = crate::span::Span::new(0, 0, 0);
    let mk_param = |name: &str, ty: Option<&str>| ast::Param {
        name: name.to_string(),
        default: None,
        default_is_name_ref: false,
        type_annotation: ty.map(|t| crate::span::Spanned::new(t.to_string(), zero)),
        description: None,
        span: zero,
    };
    match exported_name {
        "subagent" => Some((
            "Spawn a new subagent to perform the given task.".to_string(),
            vec![mk_param("task", None)],
            Some(crate::span::Spanned::new("Agent".to_string(), zero)),
        )),
        "send" => Some((
            "Send a follow-up message to the given agent.".to_string(),
            vec![mk_param("agent", Some("Agent")), mk_param("message", None)],
            None,
        )),
        _ => None,
    }
}

fn build_resolved_imports(
    consumer: &Path,
    file_exports: &HashMap<PathBuf, ExportedNames>,
    file_text_values: &HashMap<(PathBuf, String), String>,
    file_text_value_types: &HashMap<(PathBuf, String), kind_infer::TypeTag>,
    file_block_bodies: &HashMap<(PathBuf, String), String>,
    file_block_descriptions: &HashMap<(PathBuf, String), String>,
) -> ResolvedImports {
    let mut result = ResolvedImports {
        text_names: HashSet::new(),
        block_names: HashSet::new(),
        text_values: std::collections::BTreeMap::new(),
        text_value_types: std::collections::BTreeMap::new(),
        block_bodies: HashMap::new(),
        block_descriptions: HashMap::new(),
        block_return_types: HashMap::new(),
        block_params: HashMap::new(),
        block_output_contracts: HashMap::new(),
        type_descriptions: std::collections::BTreeMap::new(),
        type_spans: HashMap::new(),
        import_alias_kinds: HashMap::new(),
        import_diagnostics: DiagBag::new(),
    };

    let source = match std::fs::read_to_string(consumer) {
        Ok(s) => s,
        Err(_) => return result,
    };
    let parsed = match parse::parse(&source, 0) {
        Ok((file, _)) => file,
        Err(_) => return result,
    };
    // B06 concern 3: anchor stdlib-import diagnostics to the consumer file.
    let file_label = consumer.display().to_string();
    let line_index = LineIndex::new(&source);

    for decl in &parsed.decls {
        if let Decl::Import(import_spanned) = decl {
            let import = &import_spanned.node;

            if import.path.starts_with("@glyph/") {
                // B06: synthesize `@glyph/std` exported-block metadata so the
                // CLI/directory compile path resolves stdlib calls (`subagent`,
                // `send`) end-to-end, mirroring `check_one_file`. `load`, unknown
                // selective names, and unknown `@glyph/*` modules are not
                // resolved; each pushes a diagnostic onto
                // `result.import_diagnostics` (`G::analyze::import-private` /
                // `G::imports::unknown-stdlib-module`, matching the check path)
                // which `compile_file_with_resolved_imports` surfaces — B06
                // concern 3.

                if import.path == "@glyph/std" {
                    match &import.kind {
                        ImportKind::Selective(names) => {
                            for imp_name in names {
                                let local = imp_name
                                    .alias
                                    .as_ref()
                                    .map(|a| a.node.as_str())
                                    .unwrap_or(imp_name.name.node.as_str());
                                let alias_span = imp_name
                                    .alias
                                    .as_ref()
                                    .map(|a| a.span)
                                    .unwrap_or(imp_name.name.span);
                                if let Some((body, params, return_type)) =
                                    stdlib_synthetic_block(&imp_name.name.node)
                                {
                                    result.block_names.insert(local.to_string());
                                    result.block_bodies.insert(local.to_string(), body);
                                    result.block_params.insert(local.to_string(), params);
                                    if let Some(rt) = return_type {
                                        result.block_return_types.insert(local.to_string(), rt);
                                    }
                                    result.import_alias_kinds.insert(
                                        local.to_string(),
                                        (crate::name_kind::ResolvedImportKind::Value, alias_span),
                                    );
                                } else {
                                    // B06 concern 3: `load` and any unknown selective name are not
                                    // author-importable from `@glyph/std`. Mirror `check_one_file`'s
                                    // `G::analyze::import-private` so the compile/directory path
                                    // rejects the file instead of silently dropping the name.
                                    result.import_diagnostics.push(
                                        Diagnostic::error(
                                            "G::analyze::import-private",
                                            format!(
                                                "`{}` is not exported from `{}`",
                                                imp_name.name.node, import.path
                                            ),
                                            SourceSpan::from_byte_span(
                                                &file_label,
                                                import_spanned.span,
                                                &line_index,
                                            ),
                                        ),
                                        import_spanned.span,
                                    );
                                }
                            }
                        }
                        // Whole-module `import "@glyph/std" as std` is intentionally not
                        // resolved: dotted call targets (`std.subagent(...)`) are not yet
                        // accepted by the parser, so registering `std.subagent` here would
                        // be dead metadata. Out of scope for B06; the check path's
                        // whole-module arm only registers names for the unused-import sweep.
                        ImportKind::WholeModule { .. } => {}
                    }
                } else {
                    // B06 concern 3: an unknown `@glyph/*` module. Mirror the check
                    // path's `G::imports::unknown-stdlib-module` so the compile path
                    // fails the file instead of silently dropping the import.
                    result.import_diagnostics.push(
                        Diagnostic::error(
                            "G::imports::unknown-stdlib-module",
                            format!("unknown stdlib module `{}`", import.path),
                            SourceSpan::from_byte_span(
                                &file_label,
                                import_spanned.span,
                                &line_index,
                            ),
                        ),
                        import_spanned.span,
                    );
                }
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
                        let local = imp_name
                            .alias
                            .as_ref()
                            .map(|a| a.node.as_str())
                            .unwrap_or(imp_name.name.node.as_str());
                        let alias_span = imp_name
                            .alias
                            .as_ref()
                            .map(|a| a.span)
                            .unwrap_or(imp_name.name.span);
                        if exports.texts.contains(&imp_name.name.node) {
                            result.text_names.insert(local.to_string());
                            // Task 9: const re-export → Value alias.
                            result.import_alias_kinds.insert(
                                local.to_string(),
                                (crate::name_kind::ResolvedImportKind::Value, alias_span),
                            );
                            if let Some(val) = file_text_values
                                .get(&(resolved.clone(), imp_name.name.node.clone()))
                            {
                                result.text_values.insert(local.to_string(), val.clone());
                            }
                            if let Some(tag) = file_text_value_types
                                .get(&(resolved.clone(), imp_name.name.node.clone()))
                            {
                                result
                                    .text_value_types
                                    .insert(local.to_string(), tag.clone());
                            }
                        }
                        if let Some(desc) = exports.types.get(&imp_name.name.node) {
                            // Phase B.7: re-key the producer-side `export type`
                            // description under the consumer-local name so the
                            // consumer's `TypeRegistry` resolves it by the
                            // spelling the consumer actually wrote.
                            result
                                .type_descriptions
                                .insert(local.to_string(), desc.clone());
                            // B.5 / spec §"Unified implicit-type-registration
                            // helper": anchor the consumer-local type spelling
                            // span — alias span when `as Foo` present, else
                            // the imported name's span. Threaded into
                            // `analyze_with_imports`.
                            result.type_spans.insert(local.to_string(), alias_span);
                            // Task 9: type re-export → Type alias.
                            result.import_alias_kinds.insert(
                                local.to_string(),
                                (crate::name_kind::ResolvedImportKind::Type, alias_span),
                            );
                        }
                        if exports.blocks.contains(&imp_name.name.node) {
                            result.block_names.insert(local.to_string());
                            // Task 9: block re-export → Value alias.
                            result.import_alias_kinds.insert(
                                local.to_string(),
                                (crate::name_kind::ResolvedImportKind::Value, alias_span),
                            );
                            if let Some(body) = file_block_bodies
                                .get(&(resolved.clone(), imp_name.name.node.clone()))
                            {
                                result.block_bodies.insert(local.to_string(), body.clone());
                            }
                            if let Some(desc) = file_block_descriptions
                                .get(&(resolved.clone(), imp_name.name.node.clone()))
                            {
                                result
                                    .block_descriptions
                                    .insert(local.to_string(), desc.clone());
                            }
                            // Issue #84 Chunk 4 (D15 / Option-Y): re-key the
                            // exporter-side block return type under the
                            // consumer-side local (post-alias) name so the
                            // chunk-4 check resolves callees by the spelling
                            // the consumer actually wrote.
                            if let Some(rt) = exports.block_return_types.get(&imp_name.name.node) {
                                result
                                    .block_return_types
                                    .insert(local.to_string(), rt.clone());
                            }
                            // PRD #103 / Slice 2 (#105): re-key the
                            // exporter-side parameter list under the
                            // consumer-side local name so cross-file call-arg
                            // validation resolves by the spelling the
                            // consumer actually wrote.
                            if let Some(params) = exports.block_params.get(&imp_name.name.node) {
                                result
                                    .block_params
                                    .insert(local.to_string(), params.clone());
                            }
                            // Issue #85: re-key the exporter-side
                            // output-contract form under the consumer-side
                            // local name so the cross-file Tier-1 fix-up
                            // hoists it onto the IrCall.
                            if let Some(form) =
                                exports.block_output_contracts.get(&imp_name.name.node)
                            {
                                result
                                    .block_output_contracts
                                    .insert(local.to_string(), form.clone());
                            }
                        }
                    }
                }

                ImportKind::WholeModule { alias } => {
                    // Task 9: whole-module aliases bind to the value namespace
                    // (qualified `alias.Type` refs are MVP-unsupported per B.7).
                    result.import_alias_kinds.insert(
                        alias.node.clone(),
                        (crate::name_kind::ResolvedImportKind::Value, alias.span),
                    );
                    for name in &exports.texts {
                        let qualified = format!("{}.{}", alias.node, name);
                        result.text_names.insert(qualified.clone());
                        if let Some(val) = file_text_values.get(&(resolved.clone(), name.clone())) {
                            result.text_values.insert(qualified.clone(), val.clone());
                        }
                        if let Some(tag) =
                            file_text_value_types.get(&(resolved.clone(), name.clone()))
                        {
                            result.text_value_types.insert(qualified, tag.clone());
                        }
                    }
                    for name in &exports.blocks {
                        let qualified = format!("{}.{}", alias.node, name);
                        result.block_names.insert(qualified.clone());
                        if let Some(body) = file_block_bodies.get(&(resolved.clone(), name.clone()))
                        {
                            result.block_bodies.insert(qualified.clone(), body.clone());
                        }
                        if let Some(desc) =
                            file_block_descriptions.get(&(resolved.clone(), name.clone()))
                        {
                            result
                                .block_descriptions
                                .insert(qualified.clone(), desc.clone());
                        }
                        // Issue #84 Chunk 4 (D15 / Option-Y): whole-module
                        // imports prefix every block name with `alias.` —
                        // mirror the same prefix on return types.
                        if let Some(rt) = exports.block_return_types.get(name) {
                            result
                                .block_return_types
                                .insert(qualified.clone(), rt.clone());
                        }
                        // PRD #103 / Slice 2 (#105): mirror the alias prefix
                        // on parameter lists.
                        if let Some(params) = exports.block_params.get(name) {
                            result
                                .block_params
                                .insert(qualified.clone(), params.clone());
                        }
                        // Issue #85: mirror the alias prefix on output-contract
                        // forms so cross-file Tier-1 inline calls hoist the
                        // imported callee's OC onto the IrCall.
                        if let Some(form) = exports.block_output_contracts.get(name) {
                            result
                                .block_output_contracts
                                .insert(qualified, form.clone());
                        }
                    }
                    // Phase B.7: prefix imported `export type` descriptions
                    // under `alias.name` so the consumer's `TypeRegistry`
                    // resolves whole-module-style call-site spellings.
                    for (name, desc) in &exports.types {
                        let qualified = format!("{}.{}", alias.node, name);
                        result.type_descriptions.insert(qualified, desc.clone());
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
    file_text_value_types: &mut HashMap<(PathBuf, String), kind_infer::TypeTag>,
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
    //
    // The parallel `file_text_value_types` map carries the inferred
    // `TypeTag` for each exported const so the consumer-side lower can
    // re-render name_ref param defaults with the correct quoting (string
    // consts wrap in `"…"`; numeric/bool pass through verbatim).
    for decl in &parsed.decls {
        if let Decl::Const(c) = decl {
            if c.node.exported {
                let rendered = match &c.node.value {
                    ast::ConstValue::Bool(s) => s.to_ascii_lowercase(),
                    other => other.rendered().to_string(),
                };
                let lit = match &c.node.value {
                    ast::ConstValue::String(s) => kind_infer::Literal::String(s.clone()),
                    ast::ConstValue::Int(s) | ast::ConstValue::Float(s) => {
                        kind_infer::Literal::Number(s.clone())
                    }
                    ast::ConstValue::Bool(s) => kind_infer::Literal::Bool(s.clone()),
                };
                let tag = kind_infer::infer_primitive(&lit);
                file_text_values.insert((file.to_path_buf(), c.node.name.clone()), rendered);
                file_text_value_types.insert((file.to_path_buf(), c.node.name.clone()), tag);
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
                file_block_descriptions
                    .insert((file.to_path_buf(), eb.node.name.clone()), desc.clone());
            }
        }
    }
    file_exports.insert(file.to_path_buf(), exports);
}

/// Compile a file with resolved import data (names, values, procedure paths).
fn compile_file_with_resolved_imports(
    path: &Path,
    imported_procedure_paths: &HashMap<String, PathBuf>,
    resolved_imports: &ResolvedImports,
    enable_effects: bool,
    layout: &CompileOutputLayout,
) -> Result<CompileOutcome, CompileError> {
    // Fast-path single-file compile when there are no imports at all.
    // Phase B.7: also check `type_descriptions` so a types-only consumer
    // (no imported texts/blocks/procedure-paths) still routes through the
    // resolved-imports path that folds imported types into the TypeRegistry.

    // B06 concern 3: `build_resolved_imports` raises `import-private` /
    // `unknown-stdlib-module` for an invalid `@glyph/std` import (e.g.
    // `load`, an unknown selective name, or an unknown module). The
    // compile/directory path has no separate import-validation pass, so
    // surface them here — and bail before the no-import fast path, which
    // would otherwise route an invalid-import file to the non-import-aware
    // `compile_file_with_layout` and exit 0.
    if !resolved_imports.import_diagnostics.is_empty() {
        return Ok(CompileOutcome::Diagnostics(
            resolved_imports.import_diagnostics.clone(),
        ));
    }

    if imported_procedure_paths.is_empty()
        && resolved_imports.text_names.is_empty()
        && resolved_imports.block_names.is_empty()
        && resolved_imports.type_descriptions.is_empty()
    {
        return compile_file_with_layout(path, enable_effects, layout);
    }

    let source = std::fs::read_to_string(path).map_err(|e| CompileError::Read {
        path: path.display().to_string(),
        source: e,
    })?;
    let label = path.display().to_string();

    let outcome = compile_source_with_resolved_imports(
        &source,
        0,
        &label,
        imported_procedure_paths,
        resolved_imports,
        enable_effects,
        path,
        layout,
    )?;
    if let CompileOutcome::Compiled {
        ref markdown,
        ref arena,
        ..
    } = outcome
    {
        let out_path = resolve_output_path(path, OutputKind::Compiled, layout);
        let _ = arena;
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| CompileError::Write {
                path: out_path.display().to_string(),
                source: e,
            })?;
        }
        atomic_write(&out_path, markdown).map_err(|e| CompileError::Write {
            path: out_path.display().to_string(),
            source: e,
        })?;
    }
    Ok(outcome)
}

/// Compile source with full import context: text values for Lower, block bodies for Validate.
#[expect(
    clippy::too_many_arguments,
    reason = "compile-pipeline helper; long parameter list threads resolved-import context"
)]
fn compile_source_with_resolved_imports(
    source: &str,
    file_id: u32,
    file_label: &str,
    imported_procedure_paths: &HashMap<String, PathBuf>,
    resolved_imports: &ResolvedImports,
    enable_effects: bool,
    consumer_source: &Path,
    layout: &CompileOutputLayout,
) -> Result<CompileOutcome, CompileError> {
    let mut bag = DiagBag::new();
    let line_index = LineIndex::new(source);

    let parsed = parse::parse_with_diagnostics_opts(
        source,
        file_id,
        file_label,
        &line_index,
        &mut bag,
        enable_effects,
    );
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

    let mut registry = domain_registry::Registry::new();
    // Issue #84 Chunk 4 (D15 / Option-Y): the per-file
    // `ResolvedImports.block_return_types` map carries the producer-side
    // `Spanned<-> Type>` for every imported (selective + whole-module)
    // exported block, re-keyed under the consumer's local spelling.
    let file = analyze::analyze_with_imports(
        &file,
        file_id,
        file_label,
        &line_index,
        &mut bag,
        &resolved_imports.text_names,
        &all_imported_blocks,
        &HashSet::new(),
        &HashSet::new(),
        &mut used_import_names,
        &resolved_imports.block_descriptions,
        &mut registry,
        &resolved_imports.block_return_types,
        &resolved_imports.block_params,
        // Task 6: rendered import bodies + inferred types reach the
        // classifier so a bare imported name in a branch condition lands
        // in the correct ConditionTokenKind variant per its TypeTag.
        &resolved_imports.text_values,
        &resolved_imports.text_value_types,
        &resolved_imports.type_spans,
        &resolved_imports.import_alias_kinds,
        enable_effects,
    );
    if bag.has_error() || bag.has_repairable() {
        return Ok(CompileOutcome::Diagnostics(bag));
    }

    // Lower with imported text values available for constraint/context resolution.
    // Phase B.7: also pass imported `export type` descriptions so they can be
    // folded into the `TypeRegistry` for cross-file type-level description
    // lookup at emit time.
    let mut arena = lower::lower_with_imports(
        &file,
        &resolved_imports.text_values,
        &resolved_imports.text_value_types,
        &resolved_imports.type_descriptions,
    )
    .map_err(CompileError::Lower)?;

    // Tag imported block calls with resolved body text or Tier 3 procedure paths.
    for node in arena.nodes_mut() {
        if let IrNode::Call(c) = node {
            if c.resolved_body.is_none() {
                if let Some(proc_path) = imported_procedure_paths.get(&c.target) {
                    c.projection_tier = Some(3);
                    let consumer_out =
                        resolve_output_path(consumer_source, OutputKind::Compiled, layout);
                    c.procedure_path = Some(resolve_procedure_reference(&consumer_out, proc_path));
                } else if let Some(body) = resolved_imports.block_bodies.get(&c.target) {
                    c.resolved_body = Some(body.clone());
                } else if resolved_imports
                    .block_output_contracts
                    .contains_key(&c.target)
                {
                    // Return-only imported helper: the producer's body is
                    // empty (only `return <…>`), so it isn't in
                    // `block_bodies`. Materialize an empty resolved_body so
                    // the Tier-1 inline path treats it as inlinable rather
                    // than leaving `projection_tier = None`, which panics in
                    // the scaffold walk. The empty-body guard added in the
                    // emit pass produces a standalone return step.
                    c.resolved_body = Some(String::new());
                }
            }
            // Issue #85: hoist the imported callee's OC form onto the Call so
            // expand- and emit-time gates can read it without crossing the
            // import boundary again. Same-file callees are populated at lower
            // time in `lower::*_callee_output_form`; this is the cross-file
            // counterpart.
            if c.callee_output_contract.is_none() {
                if let Some(form) = resolved_imports.block_output_contracts.get(&c.target) {
                    c.callee_output_contract = Some(form.clone());
                }
            }
            // §8.4 return-prose templates need the callee's source-text
            // `-> Foo` spelling for the `(Foo)` parenthetical. Mirror the OC
            // hoist above using the consumer-side re-keyed `block_return_types`.
            if c.callee_return_type_text.is_none() {
                if let Some(rt) = resolved_imports.block_return_types.get(&c.target) {
                    c.callee_return_type_text = Some(rt.node.clone());
                }
            }
            // B06 concern 2: an aliased imported call (e.g. `subagent as
            // spawn`) is not recognized by `lower::callee_is_agent`, which
            // only consults `crate::stdlib_sig` on the *bare* target name.
            // The resolved `block_return_types` (re-keyed under the local /
            // alias name) already records the Agent return-shape, so correct
            // `is_agent` here for bound calls — mirroring scaffold emit's
            // agent-vs-result prose split.
            if !c.is_agent && c.bound_name.is_some() {
                if let Some(rt) = resolved_imports.block_return_types.get(&c.target) {
                    if rt.node.eq_ignore_ascii_case("Agent") {
                        c.is_agent = true;
                    }
                }
            }
        }
    }

    validate::validate(&arena).map_err(CompileError::Validate)?;
    let arena = expand::expand_step1_with_imported_descriptions(
        arena,
        &resolved_imports.block_descriptions,
    );
    let markdown = match emit::emit(&arena, enable_effects) {
        Ok(md) => md,
        Err(errors) => {
            let mut diag_bag = llm_required_diagnostics_from_errors(errors, file_label);
            diag_bag.merge(bag);
            return Ok(CompileOutcome::Diagnostics(diag_bag));
        }
    };
    Ok(CompileOutcome::Compiled {
        markdown,
        diagnostics: bag,
        arena,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_path_strips_glyph_md() {
        let p = compiled_output_path(Path::new("tests/corpus/valid/update_docs.glyph"));
        assert_eq!(p, Path::new("tests/corpus/valid/update_docs.md"));
    }

    #[test]
    fn llm_required_diagnostics_sort_by_ir_node_id_ascending() {
        // Errors arrive in arbitrary order; the helper must sort by
        // `ir_node.0` ascending so emitted diagnostics are deterministic
        // regardless of which call site the emitter encountered first.
        let errors = vec![
            emit::StubFillError::CallBody {
                ir_node: crate::ir::NodeId(7),
                target_name: Some("late".to_string()),
                has_modifier: true,
                has_local_refs: false,
            },
            emit::StubFillError::CallBody {
                ir_node: crate::ir::NodeId(3),
                target_name: Some("early".to_string()),
                has_modifier: true,
                has_local_refs: false,
            },
        ];
        let bag = llm_required_diagnostics_from_errors(errors, "delegate.glyph");
        let sorted = bag.sorted();
        let messages: Vec<String> = sorted.iter().map(|d| d.message.clone()).collect();
        assert_eq!(messages.len(), 2, "got {messages:?}");
        assert!(
            messages[0].contains("(IR n3)"),
            "first message should be the n3 site, got {:?}",
            messages[0]
        );
        assert!(
            messages[1].contains("(IR n7)"),
            "second message should be the n7 site, got {:?}",
            messages[1]
        );
    }

    #[test]
    fn check_source_returns_empty_bag_on_empty_file_repairs_skipped() {
        // An empty file produces `G::parse::empty-file` (error). check_source
        // surfaces it and exits without running later phases.
        let bag = check_source("", 0, "empty.glyph");
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
        assert_eq!(block.description.as_deref(), Some("Say hello to the user."));
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
        let outcome = compile_source(src, 0, "test.glyph").expect("should compile");
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
        let outcome = compile_source(src, 0, "test.glyph").expect("should compile");
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
        let block_node = arena
            .nodes()
            .iter()
            .find(|n| matches!(n, ir::IrNode::Block(_)));
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
        let outcome = compile_source(src, 0, "test.glyph").expect("should compile");
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
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::undefined-call"),
            "expected undefined-call diagnostic, got: {:?}",
            ids
        );
        // undefined-call is repairable (Phase 3 Repair generates a block).
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::undefined-call")
            .unwrap();
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
        let bag = check_source_with_effects(src, 0, "test.glyph", true);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::parse::none-with-effects"),
            "expected G::parse::none-with-effects, got: {:?}",
            ids
        );
        assert_eq!(
            bag.exit_code(),
            1,
            "none-with-effects should be a hard error"
        );
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
        let bag = check_source_with_effects(src, 0, "test.glyph", true);
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
        let bag = check_source_with_effects(src, 0, "test.glyph", true);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::effects-over-declared"),
            "expected effects-over-declared warning, got: {:?}",
            ids
        );
        // Warning only → exit code 0.
        assert_eq!(
            bag.exit_code(),
            0,
            "over-declared should exit 0 (warning only)"
        );
        // Classification should be Warning.
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::effects-over-declared")
            .unwrap();
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
        let bag = check_source_with_effects(src, 0, "test.glyph", true);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::missing-effects"),
            "expected missing-effects diagnostic, got: {:?}",
            ids
        );
        assert_eq!(
            bag.exit_code(),
            2,
            "missing-effects should be repairable (exit 2)"
        );
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::missing-effects")
            .unwrap();
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
        let outcome =
            compile_source_with_effects(src, 0, "test.glyph", true).expect("should compile");
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
        let outcome = compile_source(src, 0, "test.glyph").expect("should compile");
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
        let outcome =
            compile_source_with_effects(src, 0, "test.glyph", true).expect("should compile");
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
        let bag = check_source_with_effects(src, 0, "test.glyph", true);
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
        let bag = check_source_with_effects(src, 0, "test.glyph", true);
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
        let bag = check_source_with_effects(src, 0, "test.glyph", true);
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
        let outcome =
            compile_source_with_effects(src, 0, "test.glyph", true).expect("should compile");
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
    fn effects_disabled_produces_diagnostic() {
        // When enable_effects is false (default), `effects:` in source
        // produces a `G::parse::gated-section` error diagnostic (the
        // unified catalogue-disabled-section id; Phase 5 renamed the
        // legacy `effects-disabled` to match other gated sections).
        let src = "\
skill main()
    description: \"Main skill.\"
    effects: reads_files
    flow:
        \"Do something.\"
";
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::parse::gated-section"),
            "expected G::parse::gated-section when effects are off, got: {:?}",
            ids
        );
        assert_eq!(bag.exit_code(), 1, "gated-section should be a hard error");
    }

    #[test]
    fn effects_disabled_on_block_produces_diagnostic() {
        // `effects:` on a block declaration should also fire gated-section.
        let src = "\
block helper()
    effects: writes_files
    \"Do something.\"
";
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::parse::gated-section"),
            "expected G::parse::gated-section on block, got: {:?}",
            ids
        );
    }

    #[test]
    #[ignore = "PRD #159: this surface is now Repairable through compile; relift as expand-pass-level test against IrArena directly. See todo/expand-todos.md."]
    fn return_call_folds_into_final_step() {
        // AC1: `return summarize_changes()` becomes the last sentence of the
        // final numbered step.
        //
        // Ignored under PRD #159 — see todo/expand-todos.md. Relift target:
        // drive the expand pass directly against an IrArena to bypass the
        // analyzer (which now flags this surface as Repairable).
        let src = concat!(
            "block summarize_changes()\n",
            "    \"Summarize what was changed and why.\"\n",
            "\n",
            "skill update_docs()\n",
            "    description: \"Update documentation.\"\n",
            "    flow:\n",
            "        \"Read the repository changes.\"\n",
            "        return summarize_changes()\n",
        );
        let outcome = compile_source(src, 0, "test.glyph").expect("should compile");
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
        let bag = check_source(src, 0, "test.glyph");
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
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::missing-return"),
            "expected G::analyze::missing-return for export block without return, got: {:?}",
            ids
        );
        // Should be repairable.
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::missing-return")
            .unwrap();
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
        let bag = check_source(src, 0, "test.glyph");
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
        let bag = check_source(src, 0, "test.glyph");
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
        let bag = check_source(src, 0, "test.glyph");
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
        let bag = check_source(src, 0, "test.glyph");
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
        let bag = check_source(src, 0, "test.glyph");
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

    // --- Issue #83: G::analyze::generic-type-name ---

    // ---- issue #160: same rule broadened to `skill` decls ----

    #[test]
    fn skill_meaningful_return_without_arrow_fires() {
        // Issue #160: a `skill` whose body has `return <expr>` (where `<expr>`
        // is not the `none` value-keyword) and whose header lacks
        // `-> DomainType` must fire `G::analyze::export-missing-return-type`
        // as Repairable. Same diagnostic ID as the export-block fire-site;
        // the message text varies per kind.
        let src = concat!(
            "skill compute(x = \"default\")\n",
            "    flow:\n",
            "        \"Compute something.\"\n",
            "        return x\n",
        );
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::export-missing-return-type"),
            "expected G::analyze::export-missing-return-type for skill meaningful return without `->`, got: {:?}",
            ids
        );
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::export-missing-return-type")
            .unwrap();
        assert_eq!(
            diag.classification,
            diagnostic::Classification::Repairable,
            "export-missing-return-type on skill must be Repairable"
        );
        assert!(
            diag.message.contains("skill compute"),
            "skill flavor of the diagnostic should mention `skill compute`, got: {:?}",
            diag.message
        );
    }

    #[test]
    fn skill_return_none_without_arrow_no_export_missing_return_type() {
        // Issue #160: `return none` at the end of a skill body is the
        // value-position `none` keyword (no meaningful return). With no
        // `-> DomainType` on the header, the new diagnostic must NOT fire.
        let src = concat!(
            "skill notify(msg = \"hello\")\n",
            "    flow:\n",
            "        \"Send a notification.\"\n",
            "        return none\n",
        );
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::export-missing-return-type"),
            "must NOT fire export-missing-return-type for skill `return none`, got: {:?}",
            ids
        );
    }

    // Codex round-1 Issue 5 (coverage gap):
    // `flow_has_meaningful_return` uses `eq_ignore_ascii_case("none")`,
    // so `return None` and `return NONE` must also be treated as
    // no-meaningful-return. The export-block parser tests pin this at
    // the parse layer (parse.rs); these tests pin it end-to-end at the
    // analyze layer for the broadened skill fire site so a regression
    // that drops the case-insensitive branch lights up loudly here.

    /// `return None` (PascalCase) at end of a skill body with no `-> Type`
    /// must NOT fire `G::analyze::export-missing-return-type`.
    #[test]
    fn skill_return_none_pascal_case_without_arrow_no_export_missing_return_type() {
        let src = concat!(
            "skill notify(msg = \"hello\")\n",
            "    flow:\n",
            "        \"Send a notification.\"\n",
            "        return None\n",
        );
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::export-missing-return-type"),
            "must NOT fire export-missing-return-type for skill `return None` (PascalCase), got: {:?}",
            ids
        );
    }

    /// `return NONE` (all-caps) at end of a skill body with no `-> Type`
    /// must NOT fire `G::analyze::export-missing-return-type`.
    #[test]
    fn skill_return_none_uppercase_without_arrow_no_export_missing_return_type() {
        let src = concat!(
            "skill notify(msg = \"hello\")\n",
            "    flow:\n",
            "        \"Send a notification.\"\n",
            "        return NONE\n",
        );
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::export-missing-return-type"),
            "must NOT fire export-missing-return-type for skill `return NONE` (all-caps), got: {:?}",
            ids
        );
    }

    #[test]
    fn skill_meaningful_return_with_arrow_passes_clean() {
        // Issue #160: a `skill` with `-> DomainType` on the header and a
        // meaningful return must NOT fire `export-missing-return-type`.
        let src = concat!(
            "skill compute(x = \"default\") -> Path\n",
            "    flow:\n",
            "        \"Compute something.\"\n",
            "        return x\n",
        );
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::export-missing-return-type"),
            "must NOT fire export-missing-return-type when `-> Path` is present on skill, got: {:?}",
            ids
        );
        assert!(
            !ids.contains(&"G::parse::none-as-return-type"),
            "must NOT fire none-as-return-type for `-> Path` on skill, got: {:?}",
            ids
        );
    }

    // ---- issue #161: private `block` cluster, mirrors the skill cluster above ----

    #[test]
    fn block_meaningful_return_without_arrow_fires() {
        // Issue #161: a private `block` whose body has `return <expr>` (where
        // `<expr>` is not the `none` value-keyword) and whose header lacks
        // `-> DomainType` must fire `G::analyze::export-missing-return-type`
        // as Repairable. Same diagnostic ID as the skill / export-block
        // fire-sites; the message text identifies the decl kind.
        //
        // An export skill is included so `G::analyze::no-exports-in-library`
        // does not fire and the test isolates the block diagnostic.
        let src = concat!(
            "skill orchestrate()\n",
            "    description: \"Orchestrate the helper.\"\n",
            "    flow:\n",
            "        \"Use the helper.\"\n",
            "        helper()\n",
            "\n",
            "block helper(x = \"default\")\n",
            "    description: \"Helper that returns x.\"\n",
            "    flow:\n",
            "        return x\n",
        );
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::export-missing-return-type"),
            "expected G::analyze::export-missing-return-type for private block meaningful return without `->`, got: {:?}",
            ids
        );
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::export-missing-return-type")
            .unwrap();
        assert_eq!(
            diag.classification,
            diagnostic::Classification::Repairable,
            "export-missing-return-type on private block must be Repairable"
        );
        assert!(
            diag.message.contains("block helper"),
            "block flavor of the diagnostic should mention `block helper`, got: {:?}",
            diag.message
        );
    }

    #[test]
    fn block_return_none_without_arrow_no_export_missing_return_type() {
        // Issue #161: `return none` at the end of a private block body is the
        // value-position `none` keyword (no meaningful return). With no
        // `-> DomainType` on the header, the new diagnostic must NOT fire.
        //
        // An export skill is included so `G::analyze::no-exports-in-library`
        // does not fire.
        let src = concat!(
            "skill orchestrate()\n",
            "    description: \"Orchestrate the helper.\"\n",
            "    flow:\n",
            "        \"Use the helper.\"\n",
            "        helper()\n",
            "\n",
            "block helper(msg = \"hello\")\n",
            "    description: \"Helper that returns nothing.\"\n",
            "    flow:\n",
            "        return none\n",
        );
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::export-missing-return-type"),
            "must NOT fire export-missing-return-type for private block `return none`, got: {:?}",
            ids
        );
    }

    // Codex round-1 Issue 5 (coverage gap):
    // `flow_has_meaningful_return` uses `eq_ignore_ascii_case("none")`,
    // so `return None` and `return NONE` must also be treated as
    // no-meaningful-return. The export-block parser tests pin this at
    // the parse layer (parse.rs); these tests pin it end-to-end at the
    // analyze layer for the broadened private-block fire site so a
    // regression that drops the case-insensitive branch lights up here.

    /// `return None` (PascalCase) at end of a private block body with no
    /// `-> Type` must NOT fire `G::analyze::export-missing-return-type`.
    #[test]
    fn block_return_none_pascal_case_without_arrow_no_export_missing_return_type() {
        let src = concat!(
            "skill orchestrate()\n",
            "    description: \"Orchestrate the helper.\"\n",
            "    flow:\n",
            "        \"Use the helper.\"\n",
            "        helper()\n",
            "\n",
            "block helper(msg = \"hello\")\n",
            "    description: \"Helper that returns nothing.\"\n",
            "    flow:\n",
            "        return None\n",
        );
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::export-missing-return-type"),
            "must NOT fire export-missing-return-type for private block `return None` (PascalCase), got: {:?}",
            ids
        );
    }

    /// `return NONE` (all-caps) at end of a private block body with no
    /// `-> Type` must NOT fire `G::analyze::export-missing-return-type`.
    #[test]
    fn block_return_none_uppercase_without_arrow_no_export_missing_return_type() {
        let src = concat!(
            "skill orchestrate()\n",
            "    description: \"Orchestrate the helper.\"\n",
            "    flow:\n",
            "        \"Use the helper.\"\n",
            "        helper()\n",
            "\n",
            "block helper(msg = \"hello\")\n",
            "    description: \"Helper that returns nothing.\"\n",
            "    flow:\n",
            "        return NONE\n",
        );
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::export-missing-return-type"),
            "must NOT fire export-missing-return-type for private block `return NONE` (all-caps), got: {:?}",
            ids
        );
    }

    #[test]
    fn block_meaningful_return_with_arrow_passes_clean() {
        // Issue #161: a private `block` with `-> DomainType` on the header and
        // a meaningful return must NOT fire `export-missing-return-type`.
        //
        // An export skill is included so `G::analyze::no-exports-in-library`
        // does not fire.
        let src = concat!(
            "skill orchestrate()\n",
            "    description: \"Orchestrate the helper.\"\n",
            "    flow:\n",
            "        \"Use the helper.\"\n",
            "        helper()\n",
            "\n",
            "block helper(x = \"default\") -> Path\n",
            "    description: \"Helper that returns a path.\"\n",
            "    flow:\n",
            "        return x\n",
        );
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::export-missing-return-type"),
            "must NOT fire export-missing-return-type when `-> Path` is present on private block, got: {:?}",
            ids
        );
        assert!(
            !ids.contains(&"G::parse::none-as-return-type"),
            "must NOT fire none-as-return-type for `-> Path` on private block, got: {:?}",
            ids
        );
    }

    #[test]
    fn block_no_return_does_not_fire_export_missing_return_type() {
        // Issue #161 critical negative case: a private `block` with NO `return`
        // at all must stay silent (preserves the no-meaningful-contract idiom
        // for internal helpers — many private blocks are pure side-effect
        // helpers and must not be forced to declare a return type).
        //
        // An export skill is included so `G::analyze::no-exports-in-library`
        // does not fire.
        let src = concat!(
            "skill orchestrate()\n",
            "    description: \"Orchestrate the helper.\"\n",
            "    flow:\n",
            "        \"Use the helper.\"\n",
            "        helper()\n",
            "\n",
            "block helper(msg = \"hello\")\n",
            "    description: \"Helper that performs a side effect and returns nothing.\"\n",
        );
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::export-missing-return-type"),
            "must NOT fire export-missing-return-type for private block with NO return at all, got: {:?}",
            ids
        );
    }

    #[test]
    fn skill_no_return_does_not_fire_export_missing_return_type() {
        // Issue #160: when the skill body has no `return` at all,
        // `export-missing-return-type` must NOT fire — there is no
        // meaningful return value to require a `-> DomainType` annotation.
        // (Skill has no analog of the export-block `missing-return` rule,
        // so we only assert about the new diagnostic.)
        let src = concat!(
            "skill compute(x = \"default\")\n",
            "    flow:\n",
            "        \"Compute something.\"\n",
        );
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::export-missing-return-type"),
            "must NOT fire export-missing-return-type when skill has no return at all, got: {:?}",
            ids
        );
    }

    #[test]
    fn return_type_string_on_skill_fires_warning() {
        // AC2 + AC3 (emit half): a skill header with `-> String` (banned
        // generic type name) must fire `G::analyze::generic-type-name` at
        // warning tier.
        let src = "\
skill foo() -> String
    description: \"Foo.\"
    flow:
        \"do something\"
";
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::generic-type-name"),
            "expected G::analyze::generic-type-name for `-> String` on skill, got: {:?}",
            ids
        );
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::generic-type-name")
            .unwrap();
        assert_eq!(
            diag.classification,
            diagnostic::Classification::Warning,
            "generic-type-name must be Warning tier"
        );
        assert!(
            diag.message.contains("String"),
            "warning message should mention the offending identifier `String`, got: {:?}",
            diag.message
        );
        assert!(
            !diag.hints.is_empty(),
            "warning should carry a hint suggesting domain types"
        );
    }

    #[test]
    fn return_type_string_on_export_block_fires_warning() {
        // Banned `-> String` on an export-block header fires the warning.
        let src = "\
export block compute(x = \"d\") -> String
    flow:
        \"x\"
        return x
";
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::generic-type-name"),
            "expected G::analyze::generic-type-name for `-> String` on export block, got: {:?}",
            ids
        );
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::generic-type-name")
            .unwrap();
        assert_eq!(diag.classification, diagnostic::Classification::Warning);
    }

    #[test]
    fn return_type_string_on_block_fires_warning() {
        // Banned `-> String` on a private `block` header fires the warning.
        // Per D7 (planner-83 decision): private blocks are in scope for AC2.
        let src = "\
skill main()
    description: \"Main.\"
    flow:
        helper()

block helper() -> String
    description: \"Helper.\"
    flow:
        \"work\"
";
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::generic-type-name"),
            "expected G::analyze::generic-type-name for `-> String` on private block, got: {:?}",
            ids
        );
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::generic-type-name")
            .unwrap();
        assert_eq!(diag.classification, diagnostic::Classification::Warning);
    }

    #[test]
    fn valid_domain_return_type_emits_no_generic_warning() {
        // Negative case: `-> BranchName` is a legitimate domain type and
        // must NOT trigger generic-type-name.
        let src = "\
skill foo() -> BranchName
    description: \"Foo.\"
    flow:
        \"do something\"
";
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::generic-type-name"),
            "must NOT fire generic-type-name for valid domain type, got: {:?}",
            ids
        );
    }

    #[test]
    fn compilation_continues_after_generic_type_name_warning() {
        // AC4: warning is non-blocking — a banned `-> String` must NOT halt
        // compilation. Fixture is a valid skill in every other respect; the
        // only diagnostic should be the warning, and exit_code should be 0.
        let src = "\
skill foo() -> String
    description: \"Foo.\"
    flow:
        return \"do something\"
";
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::generic-type-name"),
            "fixture should fire generic-type-name; got: {:?}",
            ids
        );
        assert!(
            !bag.has_error(),
            "compilation must not halt on a generic-type-name warning, got errors: {:?}",
            ids
        );
        assert!(
            !bag.has_repairable(),
            "generic-type-name must not be repairable-tier (would set exit 2), got: {:?}",
            ids
        );
        assert_eq!(
            bag.exit_code(),
            0,
            "warning-only bag must produce exit code 0 (`build-foundation.md` §A6), got: {:?}",
            ids
        );
    }

    #[test]
    fn multiple_banned_return_types_in_one_file_no_dedup() {
        // No-dedup rule: every banned occurrence in source produces its own
        // warning, each with its own span. Author needs to fix each one.
        let src = "\
skill main() -> String
    description: \"Main.\"
    flow:
        helper()

block helper() -> Int
    description: \"Helper.\"
    flow:
        \"work\"

export block compute(x = \"d\") -> Bool
    flow:
        \"x\"
        return x
";
        let bag = check_source(src, 0, "test.glyph");
        let warnings: Vec<&Diagnostic> = bag
            .iter()
            .filter(|d| d.id == "G::analyze::generic-type-name")
            .collect();
        assert_eq!(
            warnings.len(),
            3,
            "expected 3 generic-type-name warnings (one per banned return type, no dedup), got {}: {:?}",
            warnings.len(),
            warnings.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
        );
        // Each warning carries a distinct span (no dedup means distinct origins).
        let starts: HashSet<(u32, u32)> = warnings
            .iter()
            .map(|d| (d.span.start.line, d.span.start.col))
            .collect();
        assert_eq!(
            starts.len(),
            3,
            "expected 3 distinct warning spans, got duplicates: {:?}",
            warnings.iter().map(|d| &d.span).collect::<Vec<_>>()
        );
    }

    #[test]
    fn offender_appears_verbatim_in_warning_message() {
        // Survives a refactor that mistakenly canonicalizes the offender
        // (e.g. emits "String" in the message when source said "string").
        let src = "\
skill foo() -> string
    description: \"Foo.\"
    flow:
        \"do something\"
";
        let bag = check_source(src, 0, "test.glyph");
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::generic-type-name")
            .expect("expected generic-type-name warning for `-> string`");
        assert!(
            diag.message.contains("string"),
            "warning message must echo the offender verbatim (lowercase `string`), got: {:?}",
            diag.message
        );
    }

    #[test]
    fn return_type_warning_span_targets_the_annotation() {
        // Pins span convention: warning span starts at the `->` arrow on
        // the header line (mirrors `parse.rs::try_parse_return_type` and
        // `G::parse::none-as-return-type`).
        let src = "\
skill foo() -> String
    description: \"Foo.\"
    flow:
        \"do something\"
";
        let bag = check_source(src, 0, "test.glyph");
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::generic-type-name")
            .expect("expected generic-type-name warning");
        // Header is line 1; `->` starts at byte 12 (0-indexed) in
        // "skill foo() -> String", which is col 13 (1-indexed).
        assert_eq!(
            diag.span.start.line, 1,
            "warning span should start on the header line, got line {}",
            diag.span.start.line
        );
        let arrow_col = src.lines().next().unwrap().find("->").unwrap() as u32 + 1;
        assert_eq!(
            diag.span.start.col, arrow_col,
            "warning span should start at the `->` arrow (col {}), got col {}",
            arrow_col, diag.span.start.col
        );
    }

    #[test]
    fn agent_return_type_does_not_warn() {
        // `Agent` is a legitimate IR-internal `TypeTag` for stdlib
        // `subagent()` — must NOT trigger generic-type-name.
        let src = "\
skill foo() -> Agent
    description: \"Foo.\"
    flow:
        \"do something\"
";
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::generic-type-name"),
            "must NOT fire generic-type-name for `-> Agent`, got: {:?}",
            ids
        );
    }

    #[test]
    fn return_type_none_fires_parse_repairable_not_analyze_generic_warning() {
        // D9: `-> None` in author-facing source is intercepted by
        // `G::parse::none-as-return-type` (#82, repairable, Phase 3a auto-fix)
        // and never reaches the analyze-tier validator. `None` stays in the
        // banned list (defense in depth for any future call site that
        // bypasses parse), but for the author-visible exit-code path the
        // parse repairable always wins. This test pins the cross-issue
        // precedence — if either diagnostic moves, it breaks loud.
        let src = "\
skill foo() -> None
    description: \"Foo.\"
    flow:
        \"do something\"
";
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::parse::none-as-return-type"),
            "expected `-> None` to fire G::parse::none-as-return-type (repairable, #82); got: {:?}",
            ids,
        );
        assert!(
            !ids.contains(&"G::analyze::generic-type-name"),
            "must NOT also fire G::analyze::generic-type-name — parse intercept takes precedence over the analyze warning; got: {:?}",
            ids,
        );
    }

    #[test]
    fn banned_return_types_warn_on_imports_path() {
        // AC2 imports-path parity: every header-bearing decl arm
        // (skill / export block / private block) must fire the warning when
        // analysis runs through `analyze_with_imports` (the path used by
        // `check_file` whenever the file has any `import` declaration).
        // Regression: the `Decl::Block` arm in `analyze_with_imports` was a
        // no-op before this fix, silently skipping private blocks reached
        // through the imports path.
        let dir = tempfile::tempdir().unwrap();

        // lib.glyph — exports a const so the importer has something to import.
        let lib_path = dir.path().join("lib.glyph");
        std::fs::write(
            &lib_path,
            r#"export const greeting = "hello"
"#,
        )
        .unwrap();

        // main.glyph — has an `import` (forces the imports path) and one
        // banned return type per header-bearing decl kind.
        let main_path = dir.path().join("main.glyph");
        std::fs::write(
            &main_path,
            r#"import "./lib.glyph" { greeting }

skill main() -> String
    description: "Main."
    require greeting
    flow:
        helper()

block helper() -> Int
    description: "Helper."
    flow:
        "work"

export block compute(x = "d") -> Bool
    flow:
        "x"
        return x
"#,
        )
        .unwrap();

        let bag = check_file(&main_path);
        let warnings: Vec<&Diagnostic> = bag
            .iter()
            .filter(|d| d.id == "G::analyze::generic-type-name")
            .collect();
        assert_eq!(
            warnings.len(),
            3,
            "imports path must fire the warning at every header-bearing decl site (skill/block/export block); got {}: {:?}",
            warnings.len(),
            warnings.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
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
        let bag = check_source(src, 0, "test.glyph");
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
        let bag = check_source(src, 0, "test.glyph");
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
        use ast::{FlowStmt, ReturnExpr};
        use parse::check_return_rules;
        use span::Span;

        let source = "return none\n";
        let line_index = LineIndex::new(source);
        let sp = Span::new(0, 0, source.len() as u32);
        let flow = vec![FlowStmt::Return(ReturnExpr::None)];
        let mut bag = DiagBag::new();

        check_return_rules(&flow, sp, "test.glyph", &line_index, &mut bag, true);

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
        let outcome = compile_source(src, 0, "test.glyph").expect("should compile");
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
    #[ignore = "PRD #159: this surface is now Repairable through compile; relift as expand-pass-level test against IrArena directly. See todo/expand-todos.md."]
    fn return_bare_name_folds_into_final_step() {
        // `return result` with a bare name should fold.
        //
        // Ignored under PRD #159 — see todo/expand-todos.md. Relift target:
        // drive the expand pass directly against an IrArena to bypass the
        // analyzer (which now flags this surface as Repairable).
        let src = concat!(
            "skill main()\n",
            "    description: \"Main skill.\"\n",
            "    flow:\n",
            "        \"Compute the result.\"\n",
            "        return result\n",
        );
        let outcome = compile_source(src, 0, "test.glyph").expect("should compile");
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
        let outcome = compile_source(src, 0, "test.glyph").expect("should compile");
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
        let skill = file
            .decls
            .iter()
            .find_map(|d| match d {
                ast::Decl::Skill(s) => Some(&s.node),
                _ => None,
            })
            .unwrap();
        assert_eq!(skill.flow.len(), 1);
        match &skill.flow[0] {
            ast::FlowStmt::Branch {
                condition,
                then_body,
                elif_branches,
                else_body,
                ..
            } => {
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
        let outcome = compile_source(src, 0, "test.glyph").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                // Step 1 should be the prepare step.
                assert!(
                    markdown.contains("1. Prepare the environment."),
                    "markdown:\n{}",
                    markdown
                );
                // Step 2 should be the branch with lettered sub-steps.
                assert!(
                    markdown.contains("2. If mode == \"fast\":"),
                    "markdown:\n{}",
                    markdown
                );
                assert!(
                    markdown.contains("   a. Do the fast thing."),
                    "markdown:\n{}",
                    markdown
                );
                assert!(
                    markdown.contains("   b. Log performance metrics."),
                    "markdown:\n{}",
                    markdown
                );
                // elif arm
                assert!(
                    markdown.contains("   If mode == \"slow\":"),
                    "markdown:\n{}",
                    markdown
                );
                assert!(
                    markdown.contains("   a. Do the slow thing."),
                    "markdown:\n{}",
                    markdown
                );
                // else arm
                assert!(
                    markdown.contains("   Otherwise:"),
                    "markdown:\n{}",
                    markdown
                );
                assert!(
                    markdown.contains("   a. Do the default thing."),
                    "markdown:\n{}",
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
        let bag = check_source(src, 0, "test.glyph");
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
        let bag = check_source(src, 0, "test.glyph");
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
        let bag = check_source(src, 0, "test.glyph");
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
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::applies-on-undescribed-block"),
            "expected applies-on-undescribed-block, got: {:?}",
            ids
        );
        // Should be repairable for same-file blocks.
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::applies-on-undescribed-block")
            .unwrap();
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
        let bag = check_source(src, 0, "test.glyph");
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
        let outcome = compile_source(src, 0, "test.glyph").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                // context should NOT appear as a top-level ## Context section.
                // The branch-scoped context inlines into the sub-step prose.
                assert!(
                    !markdown.contains("## Context"),
                    "branch-scoped context should not surface in ## Context:\n{}",
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
        let outcome = compile_source(src, 0, "test.glyph").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                // Constraint should NOT appear in ## Constraints.
                assert!(
                    !markdown.contains("## Constraints"),
                    "branch-scoped constraint should not surface in ## Constraints:\n{}",
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
    fn resolved_predicates_populated_in_expand() {
        // AC6: resolved_predicates side-map is populated post-Step-1.
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
        // Task 7: expand consumes IrBranch.classification, populated by the
        // analyze→lower path. The bare `analyze::analyze` stub is a no-op, so
        // this test must use `analyze_with_diagnostics` (which runs
        // `annotate_file_branches`) for classification to reach the IR.
        let (file, _) = parse::parse(src, 0).expect("should parse");
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let mut registry = crate::domain_registry::Registry::new();
        let file = analyze::analyze_with_diagnostics(
            file,
            0,
            "test.glyph",
            &line_index,
            &mut bag,
            &mut registry,
        );
        let arena = lower::lower(&file).expect("should lower");
        let arena = expand::expand_step1(arena);
        // Find the Branch node.
        let branch = arena.nodes().iter().find_map(|n| match n {
            ir::IrNode::Branch(br) => Some(br),
            _ => None,
        });
        let branch = branch.expect("should have a Branch node");
        let descs = branch
            .resolved_predicates
            .as_ref()
            .expect("resolved_predicates should be populated");
        assert_eq!(
            descs.get("fast_mode").map(|s| s.as_str()),
            Some("When the user wants fast processing.")
        );
        assert_eq!(
            descs.get("slow_mode").map(|s| s.as_str()),
            Some("When the user wants thorough processing.")
        );
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
        let outcome = compile_source(src, 0, "test.glyph").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                assert!(
                    markdown.contains("Decide which of the following applies"),
                    "expected description-driven projection:\n{}",
                    markdown
                );
                assert!(
                    markdown.contains("If When the user wants fast processing:"),
                    "expected fast_mode description in output:\n{}",
                    markdown
                );
                assert!(
                    markdown.contains("If When the user wants thorough processing:"),
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
        let bag = check_source(src, 0, "test.glyph");
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
        let bag = check_source(src, 0, "test.glyph");
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
        let bag = check_source(src, 0, "test.glyph");
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
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::parse::applies-outside-condition"),
            "expected G::parse::applies-outside-condition, got: {:?}",
            ids
        );
        // It should NOT compile successfully.
        let outcome = compile_source(src, 0, "test.glyph");
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
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::parse::with-on-bare-name"),
            "expected G::parse::with-on-bare-name, got: {:?}",
            ids
        );
        assert_eq!(
            bag.exit_code(),
            1,
            "with-on-bare-name should be a hard error"
        );
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
        let bag = check_source(src, 0, "test.glyph");
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
        // Post-Task-7: a top-level Tier-1 Call with a `with` modifier now
        // hard-fails under the deterministic stub filler with
        // `G::expand::llm-required-for-call` (spec §6.2). The legacy
        // "modifier is silently dropped from compiled output" contract is
        // gone — the modifier is now a load-bearing input that the agent's
        // Step 2 must consume, so Step 1 refuses to emit a `.md` at all.
        let src = "block inspect_repo(scope)\n    \"Inspect the repo for issues.\"\n\nskill main()\n    description: \"Main skill.\"\n    flow:\n        inspect_repo(scope) with \"focus on auth\"\n";
        let outcome = compile_source(src, 0, "test.glyph").expect("should compile");
        match outcome {
            CompileOutcome::Compiled { markdown, .. } => {
                panic!(
                    "expected llm-required-for-call diagnostic; got compiled markdown:\n{}",
                    markdown
                );
            }
            CompileOutcome::Diagnostics(bag) => {
                let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
                assert!(
                    ids.contains(&"G::expand::llm-required-for-call"),
                    "expected G::expand::llm-required-for-call; got {:?}",
                    ids
                );
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
        let skill = file
            .decls
            .iter()
            .find_map(|d| match d {
                ast::Decl::Skill(s) => Some(&s.node),
                _ => None,
            })
            .unwrap();
        assert_eq!(skill.flow.len(), 1);
        match &skill.flow[0] {
            ast::FlowStmt::Call {
                target,
                args,
                site_modifier,
                bound_name: _,
            } => {
                assert_eq!(target.node, "inspect_repo");
                assert_eq!(args, &["scope".to_string()]);
                assert_eq!(site_modifier.as_deref(), Some("focus on auth"));
            }
            other => panic!("expected Call, got: {:?}", other),
        }
    }

    #[test]
    fn import_selective_parses() {
        let src = r#"import "./prefs.glyph" { preserve_existing_patterns }

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
        assert_eq!(import.path, "./prefs.glyph");
        match &import.kind {
            ast::ImportKind::Selective(names) => {
                assert_eq!(names.len(), 1);
                assert_eq!(names[0].name.node, "preserve_existing_patterns");
                assert!(names[0].alias.is_none());
            }
            _ => panic!("expected selective import"),
        }
    }

    #[test]
    fn import_whole_module_parses() {
        let src = r#"import "./prefs.glyph" as prefs

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
        assert_eq!(import.path, "./prefs.glyph");
        match &import.kind {
            ast::ImportKind::WholeModule { alias } => {
                assert_eq!(alias.node, "prefs");
            }
            _ => panic!("expected whole-module import"),
        }
    }

    #[test]
    fn import_cross_file_name_resolution() {
        // AC1: fix_bug.glyph resolves names imported from prefs.glyph
        // and repo_tools.glyph.
        let dir = tempfile::tempdir().unwrap();

        // prefs.glyph — export const
        let prefs_path = dir.path().join("prefs.glyph");
        std::fs::write(
            &prefs_path,
            r#"export const preserve_existing_patterns = "Prefer existing patterns."
"#,
        )
        .unwrap();

        // repo_tools.glyph — export block
        let tools_path = dir.path().join("repo_tools.glyph");
        std::fs::write(
            &tools_path,
            r#"export block inspect_repo(scope = ".")
    description: "Inspect the repo."
    flow:
        "Examine the repository at {scope}."
        return context
"#,
        )
        .unwrap();

        // fix_bug.glyph — imports from both
        let fix_path = dir.path().join("fix_bug.glyph");
        std::fs::write(
            &fix_path,
            r#"import "./prefs.glyph" { preserve_existing_patterns }
import "./repo_tools.glyph" { inspect_repo }

skill fix_bug(scope = ".")
    description: "Fix a bug."
    require preserve_existing_patterns
    effects: reads_files
    flow:
        inspect_repo(scope)
"#,
        )
        .unwrap();

        let bag = check_file(&fix_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        // Should NOT have undefined-name or undefined-call errors.
        assert!(
            !ids.contains(&"G::analyze::undefined-name"),
            "imported text should resolve, got: {:?}",
            ids
        );
        assert!(
            !ids.contains(&"G::analyze::undefined-call"),
            "imported block should resolve, got: {:?}",
            ids
        );
    }

    #[test]
    fn circular_import_detected_with_path() {
        // AC2: Circular-import path is included in the diagnostic message.
        let dir = tempfile::tempdir().unwrap();

        let a_path = dir.path().join("a.glyph");
        let b_path = dir.path().join("b.glyph");

        std::fs::write(
            &a_path,
            r#"import "./b.glyph" { something }

skill main()
    description: "A."
    flow:
        "Do something."
"#,
        )
        .unwrap();

        std::fs::write(
            &b_path,
            r#"import "./a.glyph" { something }

export const something = "Hello."
"#,
        )
        .unwrap();

        let bag = check_file(&a_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::circular-import"),
            "expected circular-import diagnostic, got: {:?}",
            ids
        );
        // Check that the cycle path is in the message.
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::circular-import")
            .unwrap();
        assert!(
            diag.message.contains("a.glyph") && diag.message.contains("b.glyph"),
            "cycle path should include both files, got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("->"),
            "cycle path should use -> separator, got: {}",
            diag.message
        );
    }

    #[test]
    fn import_private_name_fails() {
        // AC3: Importing a private (non-exported) name fails with import-private.
        let dir = tempfile::tempdir().unwrap();

        let lib_path = dir.path().join("lib.glyph");
        std::fs::write(
            &lib_path,
            r#"const private_text = "This is private."
export const public_text = "This is public."
"#,
        )
        .unwrap();

        let main_path = dir.path().join("main.glyph");
        std::fs::write(
            &main_path,
            r#"import "./lib.glyph" { private_text }

skill main()
    description: "Main."
    require private_text
    flow:
        "Do something."
"#,
        )
        .unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::import-private"),
            "expected import-private diagnostic, got: {:?}",
            ids
        );
    }

    #[test]
    fn import_skill_fails() {
        // AC4: Importing a skill (not a block/text) fails with import-skill.
        let dir = tempfile::tempdir().unwrap();

        let lib_path = dir.path().join("lib.glyph");
        std::fs::write(
            &lib_path,
            r#"skill some_skill()
    description: "A skill."
    flow:
        "Do something."
"#,
        )
        .unwrap();

        let main_path = dir.path().join("main.glyph");
        std::fs::write(
            &main_path,
            r#"import "./lib.glyph" { some_skill }

skill main()
    description: "Main."
    flow:
        "Do something."
"#,
        )
        .unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::import-skill"),
            "expected import-skill diagnostic, got: {:?}",
            ids
        );
    }

    #[test]
    fn duplicate_import_is_repairable() {
        // AC5: Duplicate imports are repairable diagnostics (exit 2).
        let dir = tempfile::tempdir().unwrap();

        let lib_path = dir.path().join("lib.glyph");
        std::fs::write(
            &lib_path,
            r#"export const greeting = "Hello."
"#,
        )
        .unwrap();

        let main_path = dir.path().join("main.glyph");
        std::fs::write(
            &main_path,
            r#"import "./lib.glyph" { greeting }
import "./lib.glyph" { greeting }

skill main()
    description: "Main."
    require greeting
    flow:
        "Do something."
"#,
        )
        .unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::duplicate-import"),
            "expected duplicate-import diagnostic, got: {:?}",
            ids
        );
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::duplicate-import")
            .unwrap();
        assert_eq!(diag.classification, diagnostic::Classification::Repairable);
    }

    #[test]
    fn unused_import_is_repairable() {
        // AC5: Unused imports are repairable diagnostics (exit 2).
        let dir = tempfile::tempdir().unwrap();

        let lib_path = dir.path().join("lib.glyph");
        std::fs::write(
            &lib_path,
            r#"export const greeting = "Hello."
export const farewell = "Goodbye."
"#,
        )
        .unwrap();

        let main_path = dir.path().join("main.glyph");
        std::fs::write(
            &main_path,
            r#"import "./lib.glyph" { greeting, farewell }

skill main()
    description: "Main."
    require greeting
    flow:
        "Do something."
"#,
        )
        .unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::unused-import"),
            "expected unused-import diagnostic, got: {:?}",
            ids
        );
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::unused-import")
            .unwrap();
        assert_eq!(diag.classification, diagnostic::Classification::Repairable);
        assert!(
            diag.message.contains("farewell"),
            "should mention the unused name, got: {}",
            diag.message
        );
    }

    /// Issue #84 Chunk 7a: an imported block that is consumed *only* in
    /// `return imported_block()` position should be recognized as used. Pre-fix,
    /// `track_flow_usage` (analyze.rs) only walked `FlowStmt::Call`,
    /// `ConstraintMarker`, `ContextMarker`, and `Branch` — `FlowStmt::Return`
    /// fell into the catch-all `_` arm, so `return imported_block()` did not
    /// mark the import as used and `G::analyze::unused-import` fired
    /// spuriously, blocking AC8's exit-0 success contract for cross-file
    /// nominal-match consumers. Post-fix, the import is correctly marked used.
    #[test]
    fn imported_block_used_in_return_call_position_does_not_fire_unused_import() {
        let dir = tempfile::tempdir().unwrap();

        let lib_path = dir.path().join("lib.glyph");
        std::fs::write(
            &lib_path,
            r#"export block do_thing()
    description: "Do a thing."
    flow:
        "Do it."
"#,
        )
        .unwrap();

        let main_path = dir.path().join("main.glyph");
        std::fs::write(
            &main_path,
            r#"import "./lib.glyph" { do_thing }

skill main()
    description: "Main."
    flow:
        return do_thing()
"#,
        )
        .unwrap();

        let bag = check_file(&main_path);
        let unused_for_do_thing = bag
            .iter()
            .any(|d| d.id == "G::analyze::unused-import" && d.message.contains("do_thing"));
        assert!(
            !unused_for_do_thing,
            "`return do_thing()` should mark `do_thing` as used; got diagnostics: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>(),
        );
    }

    /// Issue #84 Chunk 7a: companion to the `Return(Call)` test — a
    /// `return imported_name` (no parens, bare-name reference) should also
    /// mark the import as used. The `Name` arm is symmetric with
    /// `ContextMarker(NameRef)` (analyze.rs L753-758) and checks both
    /// `imported_texts` and `imported_blocks` since a bare name could resolve
    /// to either pool.
    #[test]
    fn imported_name_used_in_return_name_position_does_not_fire_unused_import() {
        let dir = tempfile::tempdir().unwrap();

        let lib_path = dir.path().join("lib.glyph");
        std::fs::write(
            &lib_path,
            r#"export const greeting = "Hello."
"#,
        )
        .unwrap();

        let main_path = dir.path().join("main.glyph");
        std::fs::write(
            &main_path,
            r#"import "./lib.glyph" { greeting }

skill main()
    description: "Main."
    flow:
        return greeting
"#,
        )
        .unwrap();

        let bag = check_file(&main_path);
        let unused_for_greeting = bag
            .iter()
            .any(|d| d.id == "G::analyze::unused-import" && d.message.contains("greeting"));
        assert!(
            !unused_for_greeting,
            "`return greeting` should mark `greeting` as used; got diagnostics: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>(),
        );
    }

    /// An imported block consumed *only* as a branch condition
    /// (`if imported_block(...)`) should be recognized as used. Pre-fix,
    /// `track_flow_usage`'s `Branch` arm (analyze.rs) recursed into
    /// `then_body` / `elif_branches[i].body` / `else_body` but never inspected
    /// the `condition` strings, so a call that lived only in condition
    /// position left the import looking unused. Post-fix, the condition is
    /// tokenized via `condition::tokenize_condition` and matching imported
    /// names are marked used — symmetric with the `Return(Call)` arm.
    #[test]
    fn imported_block_used_in_branch_condition_does_not_fire_unused_import() {
        let dir = tempfile::tempdir().unwrap();

        let lib_path = dir.path().join("lib.glyph");
        std::fs::write(
            &lib_path,
            r#"export block ask_user(question = "") -> Bool
    description: "Ask the user a yes/no question."
    flow:
        return <"the user's response as a bool">
"#,
        )
        .unwrap();

        let main_path = dir.path().join("main.glyph");
        std::fs::write(
            &main_path,
            r#"import "./lib.glyph" { ask_user }

skill main()
    description: "Main."
    flow:
        if ask_user("yes or no"):
            "do the yes thing"
        else
            "do the no thing"
"#,
        )
        .unwrap();

        let bag = check_file(&main_path);
        let unused_for_ask_user = bag
            .iter()
            .any(|d| d.id == "G::analyze::unused-import" && d.message.contains("ask_user"));
        assert!(
            !unused_for_ask_user,
            "`if ask_user(...)` should mark `ask_user` as used; got diagnostics: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>(),
        );
    }

    /// Companion to the branch-condition `Call` test — a bare imported
    /// predicate const referenced only as `if imported_const` should also
    /// mark the import as used. Same `tokenize_condition` sweep covers both
    /// `imported_blocks` and `imported_texts`.
    #[test]
    fn imported_text_used_in_branch_condition_does_not_fire_unused_import() {
        let dir = tempfile::tempdir().unwrap();

        let lib_path = dir.path().join("lib.glyph");
        std::fs::write(
            &lib_path,
            r#"export const should_proceed = "the user has confirmed they want to proceed"
"#,
        )
        .unwrap();

        let main_path = dir.path().join("main.glyph");
        std::fs::write(
            &main_path,
            r#"import "./lib.glyph" { should_proceed }

skill main()
    description: "Main."
    flow:
        if should_proceed:
            "do it"
        else
            "skip it"
"#,
        )
        .unwrap();

        let bag = check_file(&main_path);
        let unused_for_should_proceed = bag
            .iter()
            .any(|d| d.id == "G::analyze::unused-import" && d.message.contains("should_proceed"));
        assert!(
            !unused_for_should_proceed,
            "`if should_proceed` should mark `should_proceed` as used; got diagnostics: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>(),
        );
    }

    /// An imported block consumed only as `if imported_block.applies()` (the
    /// predicate-applies form) should also be recognized as used. The
    /// branch-condition sweep tokenizes via `condition::tokenize_condition`,
    /// which keeps `name.applies()` as a single token — so the receiver
    /// recovery must strip `.applies()` before matching against
    /// `imported_blocks`. Without the strip, the receiver became
    /// `imported_block.applies` and missed.
    #[test]
    fn imported_block_used_in_branch_applies_condition_does_not_fire_unused_import() {
        let dir = tempfile::tempdir().unwrap();

        let lib_path = dir.path().join("lib.glyph");
        std::fs::write(
            &lib_path,
            r#"export block fast_mode()
    description: "the user has opted into fast mode"
    flow:
        "noop"
"#,
        )
        .unwrap();

        let main_path = dir.path().join("main.glyph");
        std::fs::write(
            &main_path,
            r#"import "./lib.glyph" { fast_mode }

skill main()
    description: "Main."
    flow:
        if fast_mode.applies():
            "go fast"
        else
            "go slow"
"#,
        )
        .unwrap();

        let bag = check_file(&main_path);
        let unused_for_fast_mode = bag
            .iter()
            .any(|d| d.id == "G::analyze::unused-import" && d.message.contains("fast_mode"));
        assert!(
            !unused_for_fast_mode,
            "`if fast_mode.applies()` should mark `fast_mode` as used; got diagnostics: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>(),
        );
    }

    /// Codex finding (typed-params follow-up #1): an imported `const` used
    /// only as a `name_ref` parameter default must count as a use of the
    /// import. Before the fix, the type-annotation/return-type sweep at
    /// lib.rs:1064-1083 ignored `default_is_name_ref` defaults, so `risk =
    /// default_risk` left the import looking unused.
    #[test]
    fn imported_const_used_only_as_name_ref_default_does_not_fire_unused_import() {
        let dir = tempfile::tempdir().unwrap();

        let prefs_path = dir.path().join("prefs.glyph");
        std::fs::write(
            &prefs_path,
            r#"export const default_risk = "low"
"#,
        )
        .unwrap();

        let main_path = dir.path().join("main.glyph");
        std::fs::write(
            &main_path,
            r#"import "./prefs.glyph" { default_risk }

skill demo(risk = default_risk)
    description: "Demo."
    flow:
        "Do work."
"#,
        )
        .unwrap();

        let bag = check_file(&main_path);
        let unused_for_default_risk = bag
            .iter()
            .any(|d| d.id == "G::analyze::unused-import" && d.message.contains("default_risk"));
        assert!(
            !unused_for_default_risk,
            "`risk = default_risk` should mark `default_risk` as used; got diagnostics: {:?}",
            bag.iter()
                .map(|d| (d.id.as_str(), d.message.as_str()))
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn missing_import_file_detected() {
        // Missing file produces G::analyze::missing-file.
        let dir = tempfile::tempdir().unwrap();

        let main_path = dir.path().join("main.glyph");
        std::fs::write(
            &main_path,
            r#"import "./nonexistent.glyph" { something }

skill main()
    description: "Main."
    flow:
        "Do something."
"#,
        )
        .unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::missing-file"),
            "expected missing-file diagnostic, got: {:?}",
            ids
        );
    }

    #[test]
    fn check_source_flags_tab_indent_as_repairable() {
        // Tab-indented source surfaces a `repairable` diagnostic, not an error.
        let src = "skill foo()\n\tflow:\n\t\t\"bar\"\n";
        let bag = check_source(src, 0, "tab.glyph");
        assert_eq!(bag.exit_code(), 2, "expected exit 2 for tab indent");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"G::parse::tab-indent"), "ids: {:?}", ids);
    }

    // --- Slice 12: Multi-file build orchestration tests ---

    #[test]
    fn ac1_directory_compile_processes_every_file() {
        // AC1: `glyph compile dir/` processes every `.glyph` even if not
        // transitively reached by imports.
        let dir = tempfile::tempdir().unwrap();

        // Three independent files — none imports the others.
        std::fs::write(
            dir.path().join("a.glyph"),
            "\
skill alpha()
    description: \"Alpha skill.\"
    flow:
        \"Do alpha.\"
",
        )
        .unwrap();

        std::fs::write(
            dir.path().join("b.glyph"),
            "\
skill beta()
    description: \"Beta skill.\"
    flow:
        \"Do beta.\"
",
        )
        .unwrap();

        std::fs::write(
            dir.path().join("c.glyph"),
            "\
skill gamma()
    description: \"Gamma skill.\"
    flow:
        \"Do gamma.\"
",
        )
        .unwrap();

        let sources: Vec<PathBuf> = vec![
            dir.path().join("a.glyph"),
            dir.path().join("b.glyph"),
            dir.path().join("c.glyph"),
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

        // lib.glyph — standalone (no skill, just an export const — library)
        std::fs::write(
            dir.path().join("lib.glyph"),
            "\
export const greeting = \"Hello from lib.\"
",
        )
        .unwrap();

        // consumer.glyph — imports from lib but is self-contained for compile
        std::fs::write(
            dir.path().join("consumer.glyph"),
            "\
import \"./lib.glyph\" { greeting }

skill main()
    description: \"Main skill.\"
    flow:
        \"Use the greeting.\"
",
        )
        .unwrap();

        // Pass files in reverse alphabetical order to prove topo sort reorders.
        let sources: Vec<PathBuf> = vec![
            dir.path().join("consumer.glyph"),
            dir.path().join("lib.glyph"),
        ];
        let result = compile_directory(&sources);

        assert_eq!(result.outcomes.len(), 2);

        // lib should come before consumer in the outcomes (topological order).
        let first_file = &result.outcomes[0].0;
        assert!(
            first_file.to_string_lossy().contains("lib.glyph"),
            "lib should compile before consumer, got: {}",
            first_file.display()
        );
    }

    #[test]
    fn ac3_failure_skips_dependent_with_warning() {
        // AC3: Failure in b.glyph skips c.glyph (which imports it) with
        // the G::build::skipped-due-to-failed-import warning.
        let dir = tempfile::tempdir().unwrap();

        // a.glyph — valid, standalone
        std::fs::write(
            dir.path().join("a.glyph"),
            "\
skill alpha()
    description: \"Alpha skill.\"
    flow:
        \"Do alpha.\"
",
        )
        .unwrap();

        // b.glyph — intentionally broken (will fail Phase 1)
        std::fs::write(
            dir.path().join("b.glyph"),
            "\
this is not valid glyph syntax at all!!!
",
        )
        .unwrap();

        // c.glyph — imports b, should be skipped
        std::fs::write(
            dir.path().join("c.glyph"),
            "\
import \"./b.glyph\" { something }

skill gamma()
    description: \"Gamma skill.\"
    flow:
        \"Do gamma.\"
",
        )
        .unwrap();

        let sources: Vec<PathBuf> = vec![
            dir.path().join("a.glyph"),
            dir.path().join("b.glyph"),
            dir.path().join("c.glyph"),
        ];
        let result = compile_directory(&sources);

        assert_eq!(result.exit_code, 1, "build should fail");

        // a.md should exist (a succeeded).
        assert!(dir.path().join("a.md").exists(), "a.md should exist");

        // c should be skipped.
        let c_outcome = result
            .outcomes
            .iter()
            .find(|(p, _)| p.to_string_lossy().contains("c.glyph"));
        assert!(c_outcome.is_some(), "c.glyph should be in outcomes");
        match &c_outcome.unwrap().1 {
            FileOutcome::Skipped { failed_dep } => {
                assert!(
                    failed_dep.to_string_lossy().contains("b.glyph"),
                    "failed_dep should reference b.glyph, got: {}",
                    failed_dep.display()
                );
            }
            other => panic!("expected Skipped for c.glyph, got: {:?}", other),
        }
    }

    #[test]
    fn ac4_stale_md_left_untouched_on_skip() {
        // AC4: Stale c.md left untouched on disk after c.glyph skip.
        let dir = tempfile::tempdir().unwrap();

        // Pre-existing stale c.md from a previous build.
        let stale_content = "# Previous build output\nThis is stale.";
        std::fs::write(dir.path().join("c.md"), stale_content).unwrap();

        // b.glyph — broken
        std::fs::write(
            dir.path().join("b.glyph"),
            "\
this is broken!!!
",
        )
        .unwrap();

        // c.glyph — imports b, will be skipped
        std::fs::write(
            dir.path().join("c.glyph"),
            "\
import \"./b.glyph\" { something }

skill gamma()
    description: \"Gamma skill.\"
    flow:
        \"Do gamma.\"
",
        )
        .unwrap();

        let sources: Vec<PathBuf> = vec![dir.path().join("b.glyph"), dir.path().join("c.glyph")];
        let result = compile_directory(&sources);

        assert_eq!(result.exit_code, 1);

        // c.md should still contain the stale content, untouched.
        let c_md = std::fs::read_to_string(dir.path().join("c.md")).unwrap();
        assert_eq!(c_md, stale_content, "stale c.md should be left untouched");
    }

    #[test]
    fn ac5_exit_1_if_any_failed_partial_output_present() {
        // AC5: Build exits 1 if any file failed; partial output present for
        // successful files.
        let dir = tempfile::tempdir().unwrap();

        // good.glyph — valid
        std::fs::write(
            dir.path().join("good.glyph"),
            "\
skill good()
    description: \"Good skill.\"
    flow:
        \"Do good.\"
",
        )
        .unwrap();

        // bad.glyph — broken
        std::fs::write(
            dir.path().join("bad.glyph"),
            "\
this is broken!!!
",
        )
        .unwrap();

        let sources: Vec<PathBuf> =
            vec![dir.path().join("good.glyph"), dir.path().join("bad.glyph")];
        let result = compile_directory(&sources);

        // Exit 1 because bad.glyph failed.
        assert_eq!(result.exit_code, 1, "should exit 1 when any file fails");

        // good.md should exist (partial output).
        assert!(
            dir.path().join("good.md").exists(),
            "good.md should exist as partial output"
        );

        // bad.md should NOT exist.
        assert!(
            !dir.path().join("bad.md").exists(),
            "bad.md should not exist"
        );
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
        let bag = check_source(src, 0, "empty_lib.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::no-exports-in-library"),
            "expected no-exports-in-library for library with zero exports, got: {:?}",
            ids
        );
        assert_eq!(
            bag.exit_code(),
            1,
            "no-exports-in-library should be a hard error"
        );
    }

    #[test]
    fn ac1_export_text_only_library_compiles_exit_zero() {
        // A library file with only export const declarations should compile
        // successfully (exit 0) and produce zero .md output.
        let dir = tempfile::tempdir().unwrap();

        let prefs_path = dir.path().join("prefs.glyph");
        std::fs::write(
            &prefs_path,
            "\
export const terminal_mux = \"tmux\"
export const validation_strictness = \"high\"
",
        )
        .unwrap();

        let sources: Vec<PathBuf> = vec![prefs_path];
        let result = compile_directory(&sources);

        assert_eq!(
            result.exit_code, 0,
            "library file should compile with exit 0"
        );
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
        let bag = check_source(src, 0, "prefs.glyph");
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
        let bag = check_source(src, 0, "lib.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::closure-violation"),
            "expected closure-violation for export block referencing private name, got: {:?}",
            ids
        );
        assert_eq!(
            bag.exit_code(),
            1,
            "closure-violation should be a hard error"
        );
    }

    #[test]
    fn ac3_no_closure_violation_for_params_and_exported_names() {
        // Export block referencing its own params and exported text should
        // NOT fire closure-violation.
        let src = "export const greeting = \"Hello.\"\n\nexport block shared_util(x = \"default\")\n    flow:\n        \"Use {x}.\"\n        return x\n";
        let bag = check_source(src, 0, "lib.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::closure-violation"),
            "should not fire closure-violation for params/exported names, got: {:?}",
            ids
        );
    }

    /// Issue #166: body-level `context X` on an `export block` must fire
    /// `G::analyze::closure-violation` when `X` resolves to a private
    /// (non-exported) name in the same file. Mirrors the existing
    /// flow-level coverage above.
    #[test]
    fn ac3_closure_violation_on_export_block_body_level_context_marker() {
        let src = "const codebase_notes = \"Private prose used as a closure capture.\"\n\nexport block shared_util(x = \"default\")\n    context codebase_notes\n    flow:\n        return x\n";
        let bag = check_source(src, 0, "lib.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::closure-violation"),
            "expected closure-violation for body-level `context <private>`, got: {:?}",
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
        let bag = check_source(src, 0, "lib.glyph");
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
        let names: Vec<&str> = file
            .decls
            .iter()
            .filter_map(|d| match d {
                ast::Decl::Const(c) if c.node.exported => Some(c.node.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            names,
            vec!["zebra", "alpha", "middle"],
            "exports should be in source order"
        );
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

        let repo_tools_src = format!(
            "\
export block inspect_repo(scope = \".\" <\"directory to inspect\">) -> Path
    description: \"Inspect the repository for issues.\"
    flow:
{}        return scope
",
            long_body
        );

        let tools_path = dir.path().join("repo_tools.glyph");
        std::fs::write(&tools_path, &repo_tools_src).unwrap();

        let sources: Vec<PathBuf> = vec![tools_path.clone()];
        let result = compile_directory(&sources);

        assert_eq!(
            result.exit_code, 0,
            "repo_tools library should compile with exit 0"
        );
        // No .md output for a library file.
        assert!(
            !dir.path().join("repo_tools.md").exists(),
            "library file should not produce .md output"
        );

        // Verify word count is tracked on the parsed AST.
        let source = std::fs::read_to_string(&tools_path).unwrap();
        let (file, _) = parse::parse(&source, 0).expect("should parse");
        let export_block = file
            .decls
            .iter()
            .find_map(|d| match d {
                ast::Decl::ExportBlock(b) => Some(&b.node),
                _ => None,
            })
            .expect("export block should be present");
        assert!(
            export_block.body_word_count >= 150,
            "large export block should have >= 150 words, got {}",
            export_block.body_word_count
        );
    }

    #[test]
    fn tier3_library_emits_procedure_files() {
        // AC1: repo_tools.glyph with two large export blocks should emit
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

        let repo_tools_src = format!(
            "\
export block inspect_repo(scope = \".\" <\"directory to inspect\">) -> Path
    description: \"Inspect the repository for issues.\"
    flow:
{}        return scope

export block run_tests(target = \"all\" <\"test target\">) -> TestResult
    description: \"Run the project test suite.\"
    flow:
{}        return target
",
            long_body_1, long_body_2
        );

        let tools_path = dir.path().join("repo_tools.glyph");
        std::fs::write(&tools_path, &repo_tools_src).unwrap();

        let sources: Vec<PathBuf> = vec![tools_path.clone()];
        let result = compile_directory(&sources);
        assert_eq!(
            result.exit_code, 0,
            "repo_tools library should compile with exit 0"
        );

        // AC1: procedure files emitted
        let inspect_path = dir.path().join("repo_tools/inspect-repo.md");
        let run_tests_path = dir.path().join("repo_tools/run-tests.md");
        assert!(
            inspect_path.exists(),
            "repo_tools/inspect-repo.md should exist"
        );
        assert!(
            run_tests_path.exists(),
            "repo_tools/run-tests.md should exist"
        );

        // AC2: kind: procedure in frontmatter
        let inspect_content = std::fs::read_to_string(&inspect_path).unwrap();
        assert!(
            inspect_content.contains("kind: procedure"),
            "inspect-repo.md should have kind: procedure"
        );
        assert!(
            inspect_content.contains("name: inspect-repo"),
            "inspect-repo.md should have name: inspect-repo"
        );

        let run_tests_content = std::fs::read_to_string(&run_tests_path).unwrap();
        assert!(
            run_tests_content.contains("kind: procedure"),
            "run-tests.md should have kind: procedure"
        );
        assert!(
            run_tests_content.contains("name: run-tests"),
            "run-tests.md should have name: run-tests"
        );
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

        let lib_src = format!(
            "\
export block inspect_repo(scope = \".\" <\"directory to inspect\">) -> Path
    description: \"Inspect the repository for issues.\"
    flow:
{}        return scope
",
            long_body
        );

        let lib_path = dir.path().join("repo_tools.glyph");
        std::fs::write(&lib_path, &lib_src).unwrap();

        // Consumer skill that imports and calls inspect_repo
        let consumer_src = "\
import \"repo_tools\" { inspect_repo }

skill audit_code()
    description: \"Audit the codebase.\"

    flow:
        inspect_repo()
";
        let consumer_path = dir.path().join("audit_code.glyph");
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

        let repo_tools_src = format!(
            "\
export block inspect_repo(scope = \".\" <\"directory to inspect\">) -> Path
    description: \"Inspect the repository for issues.\"
    flow:
{}        return scope
",
            long_body
        );

        let tools_path = dir.path().join("repo_tools.glyph");
        std::fs::write(&tools_path, &repo_tools_src).unwrap();

        let sources: Vec<PathBuf> = vec![tools_path.clone()];

        // First run
        compile_directory(&sources);
        let inspect_path = dir.path().join("repo_tools/inspect-repo.md");
        let first_content = std::fs::read_to_string(&inspect_path).unwrap();

        // Second run
        compile_directory(&sources);
        let second_content = std::fs::read_to_string(&inspect_path).unwrap();

        assert_eq!(
            first_content, second_content,
            "procedure file should be byte-identical across runs"
        );
    }

    #[test]
    fn tier3_procedure_resolves_imported_export_type_description() {
        // Codex finding #3 follow-up: a Tier 3 procedure file's `## Parameters`
        // section depends on the local TypeRegistry to fill in type-level
        // descriptions when a param has only a `: Foo` annotation (no
        // per-param `<"…">`). Prior to this fix, the registry was built only
        // from same-file `type` decls, so an imported `export type Foo = <"X">`
        // would silently render as `- **scope** (Foo). Default: …` (no
        // description). This test pins that imported type descriptions are
        // folded into the registry and surface as the bullet's description.
        let dir = tempfile::tempdir().unwrap();

        // Library 1: types-only library exporting `RepoPath`.
        let types_lib_src = "\
export type RepoPath = <\"absolute path to a repository\">
";
        let types_lib_path = dir.path().join("types_lib.glyph");
        std::fs::write(&types_lib_path, types_lib_src).unwrap();

        // Library 2: imports `RepoPath` and uses it as a param type on a
        // large export block (>= 150 words → emits a Tier 3 procedure file).
        let mut long_body = String::new();
        for i in 0..20 {
            long_body.push_str(&format!(
                "        \"Step {} of the inspection: carefully examine the repository structure and contents.\"\n",
                i + 1
            ));
        }
        let tools_lib_src = format!(
            "\
import \"types_lib\" {{ RepoPath }}

export block inspect_repo(scope: RepoPath = \".\") -> Path
    description: \"Inspect the repository for issues.\"
    flow:
{}        return scope
",
            long_body
        );
        let tools_lib_path = dir.path().join("tools_lib.glyph");
        std::fs::write(&tools_lib_path, &tools_lib_src).unwrap();

        let sources: Vec<PathBuf> = vec![types_lib_path.clone(), tools_lib_path.clone()];
        let result = compile_directory(&sources);
        assert_eq!(result.exit_code, 0, "compile should succeed");

        let proc_path = dir.path().join("tools_lib/inspect-repo.md");
        assert!(proc_path.exists(), "procedure file should exist");
        let content = std::fs::read_to_string(&proc_path).unwrap();
        // Bullet shape per `templates::render_param_bullet` (single-line with
        // description): `- **name** (Type): desc. Default: X.`
        assert!(
            content.contains("**scope** (RepoPath): absolute path to a repository. Default:"),
            "expected RepoPath description folded into procedure ## Parameters; got:\n{}",
            content
        );
    }

    /// Codex finding #2: a Tier 3 procedure file's `## Parameters` bullet
    /// must show the *resolved* default value, not the raw `name_ref`. Before
    /// the fix, `emit_library_procedures` passed `p.default.as_deref()`
    /// straight from the AST, so a same-file `const default_scope = "."` plus
    /// `inspect(scope = default_scope)` rendered as `Default: default_scope.`
    /// — diverging from the consumer-side lower path that resolves to
    /// `Default: ".".`.
    #[test]
    fn tier3_procedure_resolves_same_file_name_ref_default() {
        let dir = tempfile::tempdir().unwrap();

        let mut long_body = String::new();
        for i in 0..20 {
            long_body.push_str(&format!(
                "        \"Step {} of the inspection: carefully examine the repository structure and contents.\"\n",
                i + 1
            ));
        }
        let tools_lib_src = format!(
            "\
const default_scope = \".\"

export block inspect(scope = default_scope <\"directory to inspect\">) -> Path
    description: \"Inspect the repository for issues.\"
    flow:
{}        return scope
",
            long_body
        );
        let tools_lib_path = dir.path().join("tools_lib.glyph");
        std::fs::write(&tools_lib_path, &tools_lib_src).unwrap();

        let result = compile_directory(std::slice::from_ref(&tools_lib_path));
        assert_eq!(result.exit_code, 0, "compile should succeed");

        let proc_path = dir.path().join("tools_lib/inspect.md");
        assert!(proc_path.exists(), "procedure file should exist");
        let content = std::fs::read_to_string(&proc_path).unwrap();
        assert!(
            content.contains("Default: \".\"."),
            "expected resolved default `.` (string-quoted) in procedure ## Parameters; got:\n{}",
            content
        );
        assert!(
            !content.contains("Default: default_scope"),
            "raw name_ref default leaked into procedure file:\n{}",
            content
        );
    }

    /// Companion to `tier3_procedure_resolves_same_file_name_ref_default`:
    /// the resolver must also reach across imports. A `default_scope`
    /// imported from a sibling library and used as a name_ref default on a
    /// Tier 3 export block should render `Default: ".".`.
    #[test]
    fn tier3_procedure_resolves_imported_name_ref_default() {
        let dir = tempfile::tempdir().unwrap();

        let prefs_src = "\
export const default_scope = \".\"
";
        let prefs_path = dir.path().join("prefs.glyph");
        std::fs::write(&prefs_path, prefs_src).unwrap();

        let mut long_body = String::new();
        for i in 0..20 {
            long_body.push_str(&format!(
                "        \"Step {} of the inspection: carefully examine the repository structure and contents.\"\n",
                i + 1
            ));
        }
        let tools_lib_src = format!(
            "\
import \"./prefs.glyph\" {{ default_scope }}

export block inspect(scope = default_scope <\"directory to inspect\">) -> Path
    description: \"Inspect the repository for issues.\"
    flow:
{}        return scope
",
            long_body
        );
        let tools_lib_path = dir.path().join("tools_lib.glyph");
        std::fs::write(&tools_lib_path, &tools_lib_src).unwrap();

        let result = compile_directory(&[prefs_path.clone(), tools_lib_path.clone()]);
        assert_eq!(result.exit_code, 0, "compile should succeed");

        let proc_path = dir.path().join("tools_lib/inspect.md");
        assert!(proc_path.exists(), "procedure file should exist");
        let content = std::fs::read_to_string(&proc_path).unwrap();
        assert!(
            content.contains("Default: \".\"."),
            "expected resolved imported default `.` in procedure ## Parameters; got:\n{}",
            content
        );
    }

    /// Regression for the ParamDescription hard-fail cascade: a Tier-3
    /// library whose `export block` params lack descriptions must surface as
    /// `FileOutcome::Failed`, and any consumer importing it must be
    /// `FileOutcome::Skipped` with no `.md` written. Pins the fix for the
    /// dangling `Follow the X procedure below.` anchor that earlier let
    /// consumers compile against a procedure file that was never emitted.
    #[test]
    fn tier3_library_param_description_failure_cascades_to_consumer() {
        let dir = tempfile::tempdir().unwrap();

        let mut long_body = String::new();
        for i in 0..20 {
            long_body.push_str(&format!(
                "        \"Step {} of the inspection: carefully examine the repository structure and contents.\"\n",
                i + 1
            ));
        }
        let lib_src = format!(
            "\
export block inspect_repo(scope = \".\") -> Path
    description: \"Inspect the repository for issues.\"
    flow:
{}        return scope
",
            long_body
        );
        let lib_path = dir.path().join("repo_tools.glyph");
        std::fs::write(&lib_path, &lib_src).unwrap();

        let consumer_src = "\
import \"repo_tools\" { inspect_repo }

skill audit_code()
    description: \"Audit the codebase.\"

    flow:
        inspect_repo()
";
        let consumer_path = dir.path().join("audit_code.glyph");
        std::fs::write(&consumer_path, consumer_src).unwrap();

        let result = compile_directory(&[lib_path.clone(), consumer_path.clone()]);
        assert_eq!(result.exit_code, 1, "directory compile must fail");

        let lib_outcome = result
            .outcomes
            .iter()
            .find(|(p, _)| p.to_string_lossy().contains("repo_tools.glyph"))
            .map(|(_, o)| o)
            .expect("library outcome present");
        assert!(
            matches!(lib_outcome, FileOutcome::Failed { .. }),
            "library must surface as Failed; got {:?}",
            lib_outcome
        );

        let consumer_outcome = result
            .outcomes
            .iter()
            .find(|(p, _)| p.to_string_lossy().contains("audit_code.glyph"))
            .map(|(_, o)| o)
            .expect("consumer outcome present");
        assert!(
            matches!(consumer_outcome, FileOutcome::Skipped { .. }),
            "consumer must be Skipped when its library import fails; got {:?}",
            consumer_outcome
        );

        assert!(
            !dir.path().join("repo_tools/inspect-repo.md").exists(),
            "procedure file must not be written when emission hard-fails"
        );
        assert!(
            !dir.path().join("audit_code.md").exists(),
            "consumer .md must not be written when dependency failed"
        );
    }

    // --- Stdlib (slice 21) tests ---

    #[test]
    fn stdlib_subagent_resolvable_via_import() {
        // AC1: `subagent` is resolvable when imported from `@glyph/std`.
        let dir = tempfile::tempdir().unwrap();
        let main_path = dir.path().join("main.glyph");
        std::fs::write(
            &main_path,
            r#"import "@glyph/std" { subagent }

skill delegate(task = "do something")
    description: "Delegate work."
    flow:
        subagent(task)
"#,
        )
        .unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::undefined-call"),
            "subagent should resolve via stdlib import, got: {:?}",
            ids
        );
        assert!(
            !ids.contains(&"G::analyze::missing-file"),
            "stdlib import should not trigger missing-file, got: {:?}",
            ids
        );
    }

    #[test]
    fn stdlib_load_not_importable() {
        // AC2: `load` is compiler-internal and NOT resolvable from author source.
        let dir = tempfile::tempdir().unwrap();
        let main_path = dir.path().join("main.glyph");
        std::fs::write(
            &main_path,
            r#"import "@glyph/std" { load }

skill runner()
    description: "Run something."
    flow:
        load("file.md")
"#,
        )
        .unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::import-private"),
            "load should not be importable, got: {:?}",
            ids
        );
    }

    #[test]
    fn stdlib_send_resolvable_via_import() {
        // AC1: `send` is resolvable when imported from `@glyph/std`.
        let dir = tempfile::tempdir().unwrap();
        let main_path = dir.path().join("main.glyph");
        std::fs::write(
            &main_path,
            r#"import "@glyph/std" { send }

skill notify(msg = "hello")
    description: "Send a message."
    flow:
        send(msg)
"#,
        )
        .unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids.contains(&"G::analyze::undefined-call"),
            "send should resolve via stdlib import, got: {:?}",
            ids
        );
        assert!(
            !ids.contains(&"G::analyze::missing-file"),
            "stdlib import should not trigger missing-file, got: {:?}",
            ids
        );
    }

    #[test]
    fn stdlib_missing_import_fires_for_subagent() {
        // AC3: `stdlib-missing-import` repairable fires when `subagent` used without import.
        let src = r#"skill delegate(task = "do something")
    description: "Delegate work."
    flow:
        subagent(task)
"#;
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::stdlib-missing-import"),
            "should fire stdlib-missing-import for subagent without import, got: {:?}",
            ids
        );
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::stdlib-missing-import")
            .unwrap();
        assert_eq!(
            diag.classification,
            Classification::Repairable,
            "stdlib-missing-import should be repairable"
        );
    }

    #[test]
    fn stdlib_missing_import_fires_for_send() {
        // AC3: `stdlib-missing-import` repairable fires when `send` used without import.
        let src = r#"skill notify(msg = "hello")
    description: "Send a message."
    flow:
        send(msg)
"#;
        let bag = check_source(src, 0, "test.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::stdlib-missing-import"),
            "should fire stdlib-missing-import for send without import, got: {:?}",
            ids
        );
    }

    #[test]
    fn stdlib_unknown_module_fires() {
        // AC4: `unknown-stdlib-module` error fires on import of nonexistent @glyph/ path.
        let dir = tempfile::tempdir().unwrap();
        let main_path = dir.path().join("main.glyph");
        std::fs::write(
            &main_path,
            r#"import "@glyph/foo" { bar }

skill main()
    description: "Main."
    flow:
        "Do something."
"#,
        )
        .unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::imports::unknown-stdlib-module"),
            "should fire unknown-stdlib-module for @glyph/foo, got: {:?}",
            ids
        );
    }

    #[test]
    fn stdlib_subagent_effect_propagates() {
        // AC5: stdlib entry's effect signature (`spawns_agent`) propagates —
        // if a skill calls subagent() and declares effects but omits spawns_agent,
        // it should fire effects-under-declared.
        // Note: effects require --enable-effects; this test uses
        // check_source_with_effects to exercise the effects-on path.
        // The analyzer has hardcoded knowledge of stdlib names (`subagent` →
        // `spawns_agent`), so we define a local block to bring the name into
        // scope while still exercising effect propagation.
        let src = r#"block subagent(task)
    effects: spawns_agent
    "Spawn a sub-agent."

skill delegate(task = "do something")
    description: "Delegate work."
    effects: reads_files
    flow:
        subagent(task)
"#;
        let bag = check_source_with_effects(src, 0, "test.glyph", true);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::effects-under-declared"),
            "subagent's spawns_agent effect should propagate, got: {:?}",
            ids
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
        let bag = check_source(src, 0, "nested_flow.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"G::parse::nested-flow"), "ids: {:?}", ids);
    }

    #[test]
    fn analyze_empty_skill_body_diagnostic() {
        // A skill with no description, no flow, no constraints, no effects.
        let src = "\
skill empty()
";
        let bag = check_source(src, 0, "empty_skill.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::empty-skill-body"),
            "ids: {:?}",
            ids
        );
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
        let bag = check_source(src, 0, "two_skills.glyph");
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
        let bag = check_source(src, 0, "dup.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::parse::duplicate-subsection"),
            "ids: {:?}",
            ids
        );
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
        let bag = check_source(src, 0, "op.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::parse::operator-in-expression"),
            "ids: {:?}",
            ids
        );
        assert_eq!(
            bag.exit_code(),
            2,
            "operator-in-expression is repairable (exit 2)"
        );
    }

    #[test]
    fn parse_mixed_indent_diagnostic() {
        // Source with spaces then tab on the same line triggers mixed-indent.
        let src = "skill foo()\n \tflow:\n";
        let bag = check_source(src, 0, "mixed.glyph");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"G::parse::mixed-indent"), "ids: {:?}", ids);
        assert_eq!(bag.exit_code(), 2, "mixed-indent is repairable (exit 2)");
    }

    #[test]
    fn cross_file_nominal_mismatch_fires_via_check_file() {
        // Issue #84 Chunk 4: end-to-end Option-Y plumbing test. Two files —
        // a library exports `do_thing() -> Plan`; a consumer skill declares
        // `-> Report` and `return do_thing()`. The cross-file
        // `nominal-mismatch` diagnostic must fire through the import-aware
        // pipeline, not just from manually-constructed maps in unit tests.
        //
        // Without the Option-Y plumbing
        // (`ExportedNames.block_return_types` populated from `extract_exports`
        // and threaded through `ResolvedImports` / `check_file_recursive`),
        // the consumer's check sees an empty imported-return-types map and
        // silently skips the mismatch.
        let dir = tempfile::tempdir().unwrap();

        let lib_path = dir.path().join("lib.glyph");
        std::fs::write(&lib_path, "export block do_thing() -> Plan\n    description: \"Make a plan.\"\n    flow:\n        return \"a plan was made\"\n").unwrap();

        let main_path = dir.path().join("main.glyph");
        std::fs::write(
            &main_path,
            "import \"./lib.glyph\" { do_thing }\n\nskill main() -> Report\n    description: \"Main.\"\n    flow:\n        return do_thing()\n",
        )
        .unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::nominal-mismatch"),
            "expected G::analyze::nominal-mismatch from cross-file pipeline, got: {:?}",
            ids
        );
        let diag = bag
            .iter()
            .find(|d| d.id == "G::analyze::nominal-mismatch")
            .unwrap();
        assert!(
            diag.message.contains("Report") && diag.message.contains("Plan"),
            "message must name both caller's `Report` and callee's `Plan`, got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("do_thing"),
            "message must name the call target `do_thing`, got: {}",
            diag.message
        );
    }

    #[test]
    fn cross_file_imported_numeric_const_fires_condition_non_boolean_via_check_file() {
        // Finding 3 (review): when a library exports `const max_attempts = 3`
        // and a consumer uses it bare in `if max_attempts:`, the import-aware
        // check pipeline must classify the imported const as Numeric and emit
        // `G::analyze::condition-non-boolean-non-predicate`. Without imported
        // const TypeTag plumbing through `check_file_recursive`, the
        // imported name falls back to `TypeTag::String` and the diagnostic
        // silently skips — so check classifies the same condition
        // differently from compile.
        let dir = tempfile::tempdir().unwrap();

        let lib_path = dir.path().join("lib.glyph");
        std::fs::write(&lib_path, "export const max_attempts = 3\n").unwrap();

        let main_path = dir.path().join("main.glyph");
        std::fs::write(
            &main_path,
            "import \"./lib.glyph\" { max_attempts }\n\nskill main()\n    description: \"Main.\"\n    flow:\n        if max_attempts:\n            \"do something\"\n",
        )
        .unwrap();

        let bag = check_file(&main_path);
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids.contains(&"G::analyze::condition-non-boolean-non-predicate"),
            "expected G::analyze::condition-non-boolean-non-predicate from cross-file pipeline (imported numeric const used as bare condition), got: {:?}",
            ids
        );
    }

    // --- Effects gate: flag-off behavior ---

    #[test]
    fn effects_off_no_missing_effects_diagnostic() {
        // When enable_effects is false, the analyzer should NOT fire
        // missing-effects even if the call graph would infer effects.
        // Use a block with effects and a skill that calls it (with effects on).
        // First verify the diagnostic fires with effects on:
        let src = "\
block writer()
    effects: writes_files
    \"Write some files.\"

skill main()
    description: \"Main skill.\"
    flow:
        writer()
";
        let bag_on = check_source_with_effects(src, 0, "test.glyph", true);
        let ids_on: Vec<&str> = bag_on.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids_on.contains(&"G::analyze::missing-effects"),
            "with effects on, expected missing-effects, got: {:?}",
            ids_on
        );

        // Now with effects off — the parser rejects `effects:` on the block,
        // so we need a source WITHOUT effects syntax. Use just the skill+block
        // without effects declarations, and the analyzer should not infer.
        let src_no_effects = "\
block writer()
    \"Write some files.\"

skill main()
    description: \"Main skill.\"
    flow:
        writer()
";
        let bag_off = check_source_with_effects(src_no_effects, 0, "test.glyph", false);
        let ids_off: Vec<&str> = bag_off.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids_off
                .iter()
                .any(|id| id.starts_with("G::analyze::effects")),
            "with effects off, no effect diagnostics should fire, got: {:?}",
            ids_off
        );
    }

    #[test]
    fn effects_off_no_under_declared_diagnostic() {
        // When enable_effects is false, effects-under-declared should not fire.
        // With effects on: skill declares fewer effects than call graph infers.
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
        let bag_on = check_source_with_effects(src, 0, "test.glyph", true);
        let ids_on: Vec<&str> = bag_on.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids_on.contains(&"G::analyze::effects-under-declared"),
            "with effects on, expected effects-under-declared, got: {:?}",
            ids_on
        );

        // With effects off, same source structure but no effects syntax.
        let src_off = "\
block writer()
    \"Write some files.\"

skill main()
    description: \"Main skill.\"
    flow:
        writer()
";
        let bag_off = check_source_with_effects(src_off, 0, "test.glyph", false);
        let ids_off: Vec<&str> = bag_off.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids_off
                .iter()
                .any(|id| id.starts_with("G::analyze::effects")),
            "with effects off, no effect diagnostics should fire, got: {:?}",
            ids_off
        );
    }

    #[test]
    fn effects_off_no_over_declared_diagnostic() {
        // When enable_effects is false, effects-over-declared should not fire.
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
        let bag_on = check_source_with_effects(src, 0, "test.glyph", true);
        let ids_on: Vec<&str> = bag_on.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids_on.contains(&"G::analyze::effects-over-declared"),
            "with effects on, expected effects-over-declared, got: {:?}",
            ids_on
        );

        // With effects off — can't have effects syntax, so no over-declared scenario.
        // Still verify no effect diagnostics fire on a clean source.
        let src_off = "\
block reader()
    \"Read some files.\"

skill main()
    description: \"Main skill.\"
    flow:
        reader()
";
        let bag_off = check_source_with_effects(src_off, 0, "test.glyph", false);
        let ids_off: Vec<&str> = bag_off.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids_off
                .iter()
                .any(|id| id.starts_with("G::analyze::effects")),
            "with effects off, no effect diagnostics should fire, got: {:?}",
            ids_off
        );
    }

    #[test]
    fn effects_off_empty_skill_body_excludes_effects() {
        // When enable_effects is on, a skill with only effects: is not empty.
        // When off, effects don't count as content.
        let src_on = "\
skill main()
    effects: reads_files
";
        let bag_on = check_source_with_effects(src_on, 0, "test.glyph", true);
        let ids_on: Vec<&str> = bag_on.iter().map(|d| d.id.as_str()).collect();
        assert!(
            !ids_on.contains(&"G::analyze::empty-skill-body"),
            "with effects on, skill with only effects should NOT be empty, got: {:?}",
            ids_on
        );

        // With effects off, a completely empty skill (parser strips effects:)
        // should fire empty-skill-body.
        let src_off = "\
skill main()
";
        let bag_off = check_source_with_effects(src_off, 0, "test.glyph", false);
        let ids_off: Vec<&str> = bag_off.iter().map(|d| d.id.as_str()).collect();
        assert!(
            ids_off.contains(&"G::analyze::empty-skill-body"),
            "with effects off, empty skill should fire empty-skill-body, got: {:?}",
            ids_off
        );
    }

    // ── M3 cross-file diagnostics ──────────────────────────────────────────

    #[test]
    fn check_source_with_imports_partitions_diagnostics_per_file() {
        // Two files: a clean importer that depends on a dep with a diagnostic.
        // The dep error should publish under the dep's URI, NOT the importer's.
        let dir = tempfile::tempdir().unwrap();
        let dep_path = dir.path().join("dep.glyph");
        // `dep_text` references an undefined name → fires `undefined-name`.
        let dep_text = "\
export const alpha = \"alpha.\"

skill dep_skill()
    description: \"dep skill.\"
    require ghost
    flow:
        \"hello\"
";
        std::fs::write(&dep_path, dep_text).unwrap();

        // Importer pulls `alpha` and uses it as a constraint.
        let importer_path = dir.path().join("main.glyph");
        let importer_src = "\
import \"./dep.glyph\" { alpha }

skill main()
    description: \"main.\"
    require alpha
    flow:
        \"hello\"
";
        std::fs::write(&importer_path, importer_src).unwrap();

        let bags = check_source_with_imports(importer_src, 0, &importer_path, false);

        // Both files should have entries in the map.
        let canon_importer = importer_path.canonicalize().unwrap();
        let canon_dep = dep_path.canonicalize().unwrap();
        assert!(
            bags.contains_key(&canon_importer),
            "importer must have a bag entry. keys = {:?}",
            bags.keys().collect::<Vec<_>>()
        );
        assert!(
            bags.contains_key(&canon_dep),
            "dep must have a bag entry. keys = {:?}",
            bags.keys().collect::<Vec<_>>()
        );

        // Importer bag should be EMPTY of errors (clean side).
        let importer_bag = bags.get(&canon_importer).unwrap();
        let importer_errors: Vec<&str> = importer_bag
            .iter()
            .filter(|d| matches!(d.classification, diagnostic::Classification::Error))
            .map(|d| d.id.as_str())
            .collect();
        assert!(
            importer_errors.is_empty(),
            "importer should have no errors, got: {:?}",
            importer_errors
        );

        // Dep bag should carry the `undefined-name` for `ghost`.
        let dep_bag = bags.get(&canon_dep).unwrap();
        let dep_ids: Vec<&str> = dep_bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            dep_ids
                .iter()
                .any(|id| id.starts_with("G::analyze::undefined")),
            "dep should carry the undefined-name diagnostic. got: {:?}",
            dep_ids
        );
    }

    #[test]
    fn check_source_with_imports_attributes_import_private_to_importer() {
        // The importer asks for a name the dep doesn't export. The
        // `import-private` diagnostic must surface under the importer URI
        // (importer is the file with the buggy `import` line).
        let dir = tempfile::tempdir().unwrap();
        let dep_path = dir.path().join("dep.glyph");
        std::fs::write(&dep_path, "const private_text = \"private.\"\n").unwrap();

        let importer_path = dir.path().join("main.glyph");
        let importer_src = "\
import \"./dep.glyph\" { not_exported }

skill main()
    description: \"main.\"
    flow:
        \"hello\"
";
        std::fs::write(&importer_path, importer_src).unwrap();

        let bags = check_source_with_imports(importer_src, 0, &importer_path, false);

        let canon_importer = importer_path.canonicalize().unwrap();
        let importer_bag = bags.get(&canon_importer).unwrap();
        let importer_ids: Vec<&str> = importer_bag.iter().map(|d| d.id.as_str()).collect();
        assert!(
            importer_ids.contains(&"G::analyze::import-private"),
            "import-private should surface on the importer. got: {:?}",
            importer_ids
        );
    }

    #[test]
    fn check_file_partition_round_trip_matches_legacy_check_file() {
        // Sanity: check_file_partition + merge == check_file_with_effects.
        let dir = tempfile::tempdir().unwrap();
        let dep_path = dir.path().join("dep.glyph");
        std::fs::write(&dep_path, "export const alpha = \"alpha.\"\n").unwrap();
        let main_path = dir.path().join("main.glyph");
        std::fs::write(
            &main_path,
            "\
import \"./dep.glyph\" { alpha }

skill main()
    description: \"main.\"
    require alpha
    flow:
        \"hello\"
",
        )
        .unwrap();

        let merged = check_file_with_effects(&main_path, false);
        let bags = check_file_partition(&main_path, false);
        let mut merged_from_partition = DiagBag::new();
        for (_p, b) in bags {
            merged_from_partition.merge(b);
        }
        // Compare sorted output (partition map insertion order is non-deterministic).
        assert_eq!(
            merged
                .sorted()
                .iter()
                .map(|d| d.id.clone())
                .collect::<Vec<_>>(),
            merged_from_partition
                .sorted()
                .iter()
                .map(|d| d.id.clone())
                .collect::<Vec<_>>(),
            "check_file_partition + merge should equal check_file_with_effects"
        );
    }

    /// Finding 3 regression: the Tier 3 export-block path resolves section
    /// headings through `lower::resolve_freeform_heading`, so a catalogue
    /// entry's `heading` override beats the derived Title Case heading. The
    /// embedded catalogue has no entry whose `heading` differs from
    /// `derive_heading(name)` for any freeform-eligible name (the `flow`
    /// entry overrides "Flow" → "Steps" but flow is parsed as a built-in,
    /// not freeform). We construct a synthetic catalogue with a distinct
    /// override and call the helper that Tier 3 now delegates to, proving
    /// the wire-up honors the override.
    #[test]
    fn tier3_catalogue_heading_override_beats_derive() {
        use crate::sections::{CatalogueEntry, SectionCatalogue};
        let entry = CatalogueEntry {
            heading: Some("Acceptance Criteria".to_string()),
            ..Default::default()
        };
        let catalogue = SectionCatalogue::from_entries(vec![("acceptance".to_string(), entry)]);
        // `derive_heading("acceptance")` would produce "Acceptance"; the
        // catalogue override wins for the Tier 3 emit path.
        assert_eq!(
            lower::resolve_freeform_heading(&catalogue, "acceptance"),
            "Acceptance Criteria"
        );
    }

    #[test]
    fn resolve_same_dir_compiled() {
        let layout = CompileOutputLayout::SameDir;
        let p = resolve_output_path(
            Path::new("/abs/proj/foo.glyph"),
            OutputKind::Compiled,
            &layout,
        );
        assert_eq!(p, Path::new("/abs/proj/foo.md"));
    }

    #[test]
    fn resolve_same_dir_ir_json() {
        let layout = CompileOutputLayout::SameDir;
        let p = resolve_output_path(
            Path::new("/abs/proj/foo.glyph"),
            OutputKind::IrJson,
            &layout,
        );
        assert_eq!(p, Path::new("/abs/proj/foo.ir.json"));
    }

    #[test]
    fn resolve_same_dir_procedure() {
        let layout = CompileOutputLayout::SameDir;
        let p = resolve_output_path(
            Path::new("/abs/proj/lib.glyph"),
            OutputKind::Procedure {
                lib_stem: "lib".into(),
                block_kebab: "do-thing".into(),
            },
            &layout,
        );
        assert_eq!(p, Path::new("/abs/proj/lib/do-thing.md"));
    }

    #[test]
    fn resolve_entry_file_redirects_entry_only() {
        let entry = PathBuf::from("/abs/proj/foo.glyph");
        let output = PathBuf::from("/abs/build/bar.md");
        let layout = CompileOutputLayout::EntryFile {
            entry: entry.clone(),
            output: output.clone(),
        };
        assert_eq!(
            resolve_output_path(&entry, OutputKind::Compiled, &layout),
            Path::new("/abs/build/bar.md")
        );
        assert_eq!(
            resolve_output_path(&entry, OutputKind::IrJson, &layout),
            Path::new("/abs/build/bar.ir.json")
        );
        assert_eq!(
            resolve_output_path(
                Path::new("/abs/proj/lib.glyph"),
                OutputKind::Compiled,
                &layout,
            ),
            Path::new("/abs/proj/lib.md")
        );
    }

    #[test]
    fn resolve_out_dir_mirrors_layout() {
        let layout = CompileOutputLayout::OutDir {
            root: PathBuf::from("/abs/build"),
            input_root: PathBuf::from("/abs/src"),
        };
        assert_eq!(
            resolve_output_path(Path::new("/abs/src/a.glyph"), OutputKind::Compiled, &layout,),
            Path::new("/abs/build/a.md")
        );
        assert_eq!(
            resolve_output_path(
                Path::new("/abs/src/sub/b.glyph"),
                OutputKind::Compiled,
                &layout,
            ),
            Path::new("/abs/build/sub/b.md")
        );
        assert_eq!(
            resolve_output_path(
                Path::new("/abs/src/sub/lib.glyph"),
                OutputKind::Procedure {
                    lib_stem: "lib".into(),
                    block_kebab: "do-thing".into(),
                },
                &layout,
            ),
            Path::new("/abs/build/sub/lib/do-thing.md")
        );
    }

    #[test]
    fn resolve_out_dir_falls_back_for_outside_root() {
        let layout = CompileOutputLayout::OutDir {
            root: PathBuf::from("/abs/build"),
            input_root: PathBuf::from("/abs/src"),
        };
        assert_eq!(
            resolve_output_path(
                Path::new("/abs/elsewhere/x.glyph"),
                OutputKind::Compiled,
                &layout,
            ),
            Path::new("/abs/elsewhere/x.md")
        );
    }

    #[test]
    fn proc_ref_same_dir() {
        let consumer = Path::new("/abs/proj/main.md");
        let proc = Path::new("/abs/proj/lib/do-thing.md");
        assert_eq!(
            resolve_procedure_reference(consumer, proc),
            "lib/do-thing.md"
        );
    }

    #[test]
    fn proc_ref_nested_under_out_dir() {
        let consumer = Path::new("/abs/build/skills/main.md");
        let proc = Path::new("/abs/build/libs/lib/do-thing.md");
        assert_eq!(
            resolve_procedure_reference(consumer, proc),
            "../libs/lib/do-thing.md"
        );
    }

    #[test]
    fn proc_ref_with_renamed_entry() {
        let consumer = Path::new("/abs/build/bar.md");
        let proc = Path::new("/abs/proj/libs/lib/do-thing.md");
        assert_eq!(
            resolve_procedure_reference(consumer, proc),
            "../proj/libs/lib/do-thing.md"
        );
    }

    #[test]
    fn proc_ref_absolute_fallback_when_no_relative() {
        let consumer = Path::new("bar.md"); // parent is ""
        let proc = Path::new("/abs/libs/lib/do-thing.md");
        let s = resolve_procedure_reference(consumer, proc);
        // No common ancestor → returns absolute (forward-slash) path.
        assert_eq!(s, "/abs/libs/lib/do-thing.md");
    }
}
