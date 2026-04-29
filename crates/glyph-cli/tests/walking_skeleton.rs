//! Walking-skeleton integration test for slice 1.
//!
//! Verifies the contract from `design/mvp-acceptance.md` §1:
//!   1. `glyph compile tests/corpus/valid/update_docs.glyph.md` exits 0.
//!   2. The emitted `update_docs.md` matches the byte-stable golden snapshot.
//!   3. Re-running the compile produces byte-identical output.

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at the glyph-cli crate; the workspace root is two parents up.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates
    p.pop(); // workspace root
    p
}

fn glyph_bin() -> PathBuf {
    // CARGO sets CARGO_BIN_EXE_glyph for integration tests of the `glyph` binary.
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn corpus_source() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("valid")
        .join("update_docs.glyph.md")
}

fn compiled_output() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("valid")
        .join("update_docs.md")
}

fn run_glyph_compile() -> std::process::Output {
    let _ = workspace_root(); // currently only used to keep the helper around for future tests
    Command::new(glyph_bin())
        .arg("compile")
        .arg(corpus_source())
        .output()
        .expect("failed to spawn glyph binary")
}

#[test]
fn walking_skeleton_compiles_to_golden_snapshot() {
    // Clean any prior compiled artifact so the run actually creates the file.
    let out_path = compiled_output();
    let _ = std::fs::remove_file(&out_path);

    let result = run_glyph_compile();
    assert!(
        result.status.success(),
        "glyph compile exited non-zero. stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );

    let emitted = std::fs::read_to_string(&out_path).expect("emitted .md file is missing");
    insta::assert_snapshot!("update_docs", emitted);
}

#[test]
fn walking_skeleton_compile_is_idempotent() {
    let out_path = compiled_output();

    // First run.
    let r1 = run_glyph_compile();
    assert!(r1.status.success());
    let bytes_1 = std::fs::read(&out_path).expect("first run did not emit .md");

    // Second run.
    let r2 = run_glyph_compile();
    assert!(r2.status.success());
    let bytes_2 = std::fs::read(&out_path).expect("second run did not emit .md");

    assert_eq!(
        bytes_1, bytes_2,
        "byte-identical output expected across runs"
    );
}
