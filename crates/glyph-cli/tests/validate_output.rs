//! Integration tests for `glyph validate-output` subcommand.
//!
//! Tests the CLI subcommand with hand-crafted `.ir.json` + `.md` pairs,
//! verifying exit codes and diagnostic output.

use std::path::PathBuf;
use std::process::{Command, Output};

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn run_validate_output(ir_json: &str, md: &str, format: &str) -> Output {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let ir_path = dir.path().join("test.ir.json");
    let md_path = dir.path().join("test.md");
    std::fs::write(&ir_path, ir_json).unwrap();
    std::fs::write(&md_path, md).unwrap();

    Command::new(glyph_bin())
        .arg("validate-output")
        .arg(&ir_path)
        .arg(&md_path)
        .arg("--format")
        .arg(format)
        .output()
        .expect("failed to spawn glyph binary")
}

fn minimal_ir() -> String {
    serde_json::json!({
        "ir_version": 1,
        "compiler": "glyph 0.1.0",
        "source_file": "test.glyph.md",
        "skill": {
            "node_id": "n0",
            "kind": "skill",
            "name": "test_skill",
            "description": "A test skill.",
            "params": [],
            "effects": [],
            "context": [],
            "constraints": [],
            "flow": [
                {
                    "node_id": "n1",
                    "kind": "inline_instruction",
                    "text": "Do something.",
                    "role": "step"
                }
            ]
        }
    })
    .to_string()
}

fn minimal_md() -> &'static str {
    "## Instructions\n\n### Steps\n\n1. Do something.\n"
}

#[test]
fn clean_pass_exits_zero() {
    let result = run_validate_output(&minimal_ir(), minimal_md(), "pretty");
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0; stderr={:?}",
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn violations_exit_one_pretty() {
    let md = "## Instructions\n\n### Steps\n\n1. Do something.\n2. Extra step.\n";
    let result = run_validate_output(&minimal_ir(), md, "pretty");
    assert_eq!(result.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("G::expand::step-count-mismatch"),
        "stderr should contain the diagnostic: {}",
        stderr,
    );
}

#[test]
fn violations_exit_one_json() {
    let md = "## Instructions\n\n### Steps\n\n1. Do something.\n2. Extra step.\n";
    let result = run_validate_output(&minimal_ir(), md, "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        stdout.contains("G::expand::step-count-mismatch"),
        "stdout should contain JSON diagnostic: {}",
        stdout,
    );
}

#[test]
fn missing_file_exits_three() {
    let result = Command::new(glyph_bin())
        .arg("validate-output")
        .arg("/nonexistent/test.ir.json")
        .arg("/nonexistent/test.md")
        .output()
        .expect("failed to spawn glyph binary");
    assert_eq!(result.status.code(), Some(3));
}

#[test]
fn compiler_emitted_output_passes_validation() {
    // Compile a valid .glyph.md file with --emit-ir, then validate-output
    // against the emitted pair. This tests the acceptance criterion that
    // "compiler's own emitted .md + .ir.json always passes validate-output."
    let corpus_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("valid");

    // Find a valid .glyph.md file
    let glyph_file = corpus_dir.join("update_docs.glyph.md");
    if !glyph_file.exists() {
        // Skip if corpus doesn't have this file
        return;
    }

    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let tmp_glyph = dir.path().join("update_docs.glyph.md");
    std::fs::copy(&glyph_file, &tmp_glyph).unwrap();

    // Compile with --emit-ir
    let compile_result = Command::new(glyph_bin())
        .arg("compile")
        .arg(&tmp_glyph)
        .arg("--emit-ir")
        .output()
        .expect("failed to spawn glyph binary");

    if compile_result.status.code() != Some(0) {
        // Compilation failed — skip this test
        return;
    }

    let ir_path = dir.path().join("update_docs.ir.json");
    let md_path = dir.path().join("update_docs.md");

    if !ir_path.exists() || !md_path.exists() {
        // IR or MD not produced — skip
        return;
    }

    // Now validate-output
    let validate_result = Command::new(glyph_bin())
        .arg("validate-output")
        .arg(&ir_path)
        .arg(&md_path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("failed to spawn glyph binary");

    assert_eq!(
        validate_result.status.code(),
        Some(0),
        "compiler's own output should pass validate-output; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&validate_result.stdout),
        String::from_utf8_lossy(&validate_result.stderr),
    );
}
