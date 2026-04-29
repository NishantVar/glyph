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
        // AC7: applies-on-non-block fires when receiver is a text declaration.
        let src = "\
text my_text = \"Some text.\"

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
text project_info = \"This is a monorepo project.\"

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
text no_breaking_changes = \"Do not break backwards compatibility.\"

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
    fn check_source_flags_tab_indent_as_repairable() {
        // Tab-indented source surfaces a `repairable` diagnostic, not an error.
        let src = "skill foo()\n\tflow:\n\t\t\"bar\"\n";
        let bag = check_source(src, 0, "tab.glyph.md");
        assert_eq!(bag.exit_code(), 2, "expected exit 2 for tab indent");
        let ids: Vec<&str> = bag.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"G::parse::tab-indent"), "ids: {:?}", ids);
    }
}
