//! Walking-skeleton integration test for slice 1.
//!
//! Verifies the contract from `design/mvp-acceptance.md` §1:
//!   1. `glyph compile tests/corpus/valid/update_docs.glyph` exits 0.
//!   2. The emitted `update_docs.md` matches the byte-stable golden snapshot.
//!   3. Re-running the compile produces byte-identical output.

use std::path::PathBuf;
use std::process::Command;

fn glyph_bin() -> PathBuf {
    // CARGO sets CARGO_BIN_EXE_glyph for integration tests of the `glyph` binary.
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn corpus_source() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("valid")
        .join("update_docs.glyph")
}

/// Copy the corpus source to a tempdir to avoid parallel-test races on
/// the shared output file (atomic_write uses a `.tmp` intermediate).
fn setup_tempdir() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let src = corpus_source();
    let tmp_src = dir.path().join("update_docs.glyph");
    std::fs::copy(&src, &tmp_src).unwrap();
    let out = dir.path().join("update_docs.md");
    (dir, tmp_src, out)
}

fn run_glyph_compile(source: &std::path::Path) -> std::process::Output {
    Command::new(glyph_bin())
        .arg("compile")
        .arg(source)
        .output()
        .expect("failed to spawn glyph binary")
}

#[test]
fn walking_skeleton_compiles_to_golden_snapshot() {
    let (_dir, src, out_path) = setup_tempdir();

    let result = run_glyph_compile(&src);
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
    let (_dir, src, out_path) = setup_tempdir();

    // First run.
    let r1 = run_glyph_compile(&src);
    assert!(r1.status.success());
    let bytes_1 = std::fs::read(&out_path).expect("first run did not emit .md");

    // Second run.
    let r2 = run_glyph_compile(&src);
    assert!(r2.status.success());
    let bytes_2 = std::fs::read(&out_path).expect("second run did not emit .md");

    assert_eq!(
        bytes_1, bytes_2,
        "byte-identical output expected across runs"
    );
}
