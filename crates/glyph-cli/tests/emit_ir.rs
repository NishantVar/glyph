//! Integration tests for `glyph compile --emit-ir` (Slice 17).
//!
//! Verifies the `--emit-ir` flag produces a `.ir.json` sidecar file that
//! conforms to `design/ir-json-schema.md`.

use std::path::PathBuf;
use std::process::Command;

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn run_compile_emit_ir(source: &std::path::Path) -> std::process::Output {
    Command::new(glyph_bin())
        .arg("compile")
        .arg(source)
        .arg("--emit-ir")
        .output()
        .expect("failed to spawn glyph binary")
}

/// Copy a corpus source to a tempdir to avoid parallel-test races.
fn setup_tempdir(filename: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("valid")
        .join(filename);
    let tmp_src = dir.path().join(filename);
    std::fs::copy(&src, &tmp_src).unwrap();
    (dir, tmp_src)
}

fn ir_json_path(source: &std::path::Path) -> PathBuf {
    let parent = source.parent().unwrap();
    let stem = source
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_suffix(".glyph.md")
        .unwrap();
    parent.join(format!("{}.ir.json", stem))
}

#[test]
fn emit_ir_produces_ir_json_file() {
    let (_dir, src) = setup_tempdir("update_docs.glyph.md");
    let result = run_compile_emit_ir(&src);
    assert!(
        result.status.success(),
        "glyph compile --emit-ir exited non-zero. stderr={:?}",
        String::from_utf8_lossy(&result.stderr),
    );

    let ir_path = ir_json_path(&src);
    assert!(ir_path.exists(), "expected {} to exist", ir_path.display());

    // Parse it as valid JSON.
    let content = std::fs::read_to_string(&ir_path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content)
        .expect("ir.json should be valid JSON");

    // Check top-level envelope fields.
    assert_eq!(v["ir_version"], 1);
    assert!(v["compiler"].as_str().unwrap().starts_with("glyph "));
    assert_eq!(v["source_file"].as_str().unwrap(), "update_docs.glyph.md");
    assert_eq!(v["skill"]["kind"], "skill");
    assert_eq!(v["skill"]["name"], "update_docs");
}

#[test]
fn emit_ir_is_byte_identical_across_runs() {
    let (_dir, src) = setup_tempdir("update_docs.glyph.md");
    let ir_path = ir_json_path(&src);

    let r1 = run_compile_emit_ir(&src);
    assert!(r1.status.success());
    let bytes1 = std::fs::read(&ir_path).unwrap();

    let r2 = run_compile_emit_ir(&src);
    assert!(r2.status.success());
    let bytes2 = std::fs::read(&ir_path).unwrap();

    assert_eq!(bytes1, bytes2, "IR JSON should be byte-identical across runs");
}
