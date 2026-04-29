//! Slice 9 integration tests — if/elif/else branching.
//!
//! AC1: valid branching corpus file compiles with lettered sub-steps.
//! AC7: applies-* diagnostics fire on corpus files.

use std::path::PathBuf;
use std::process::{Command, Output};

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn corpus_path(kind: &str, name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join(kind)
        .join(name)
}

fn run_compile(file: PathBuf) -> Output {
    Command::new(glyph_bin())
        .arg("compile")
        .arg(&file)
        .output()
        .expect("failed to spawn glyph binary")
}

fn run_check_json(file: PathBuf) -> Output {
    Command::new(glyph_bin())
        .arg("check")
        .arg(&file)
        .arg("--format")
        .arg("json")
        .output()
        .expect("failed to spawn glyph binary")
}

fn stdout_diagnostic_ids(output: &Output) -> Vec<String> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).ok()?;
            v.get("id").and_then(|x| x.as_str()).map(|s| s.to_string())
        })
        .collect()
}

// AC1: valid branching file compiles successfully with lettered sub-steps.
#[test]
fn branching_corpus_compiles_with_lettered_substeps() {
    let src = corpus_path("valid", "branching.glyph.md");
    let out = corpus_path("valid", "branching.md");
    let _ = std::fs::remove_file(&out);

    let result = run_compile(src);
    assert!(
        result.status.success(),
        "branching.glyph.md should compile; stderr={:?}",
        String::from_utf8_lossy(&result.stderr),
    );

    let emitted = std::fs::read_to_string(&out).expect("branching.md should exist");
    assert!(emitted.contains("a."), "expected lettered sub-steps in output:\n{}", emitted);
    assert!(emitted.contains("If mode =="), "expected If branch header:\n{}", emitted);
    assert!(emitted.contains("Otherwise:"), "expected else arm:\n{}", emitted);
}

// AC7: applies-no-parens corpus file fires the right diagnostic.
#[test]
fn applies_no_parens_corpus_fires_diagnostic() {
    let src = corpus_path("invalid", "applies_no_parens.glyph.md");
    let result = run_check_json(src);
    assert_eq!(result.status.code(), Some(1), "applies-no-parens is an error");
    let ids = stdout_diagnostic_ids(&result);
    assert!(
        ids.contains(&"G::parse::applies-no-parens".to_string()),
        "expected G::parse::applies-no-parens, got: {:?}",
        ids
    );
}

// AC7: applies-with-args corpus file fires the right diagnostic.
#[test]
fn applies_with_args_corpus_fires_diagnostic() {
    let src = corpus_path("invalid", "applies_with_args.glyph.md");
    let result = run_check_json(src);
    assert_eq!(result.status.code(), Some(1), "applies-with-args is an error");
    let ids = stdout_diagnostic_ids(&result);
    assert!(
        ids.contains(&"G::parse::applies-with-args".to_string()),
        "expected G::parse::applies-with-args, got: {:?}",
        ids
    );
}

// AC7: applies-on-non-block corpus file fires the right diagnostic.
#[test]
fn applies_on_non_block_corpus_fires_diagnostic() {
    let src = corpus_path("invalid", "applies_on_non_block.glyph.md");
    let result = run_check_json(src);
    assert_eq!(result.status.code(), Some(1), "applies-on-non-block is an error");
    let ids = stdout_diagnostic_ids(&result);
    assert!(
        ids.contains(&"G::analyze::applies-on-non-block".to_string()),
        "expected G::analyze::applies-on-non-block, got: {:?}",
        ids
    );
}
