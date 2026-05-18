//! Slice 18 integration tests — `--strict` mode.
//!
//! Covers acceptance criteria:
//!   1. `--strict` passes (exit 0) on every file in `tests/corpus/valid/`
//!   2. `--strict` fails (exit 1) on every file in `tests/corpus/repairable/`
//!   3. Without `--strict`, repairable files exit 2
//!   4. `--strict` works on `glyph check` as well
//!   5. `--strict` does NOT write `.md` output when repairable diagnostics exist

use std::path::PathBuf;
use std::process::{Command, Output};

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn repairable(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("repairable")
        .join(name)
}

fn valid(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("valid")
        .join(name)
}

fn run_compile(file: &std::path::Path, extra_args: &[&str]) -> Output {
    let mut cmd = Command::new(glyph_bin());
    cmd.arg("compile").arg(file);
    for arg in extra_args {
        cmd.arg(arg);
    }
    cmd.output().expect("failed to spawn glyph binary")
}

fn run_check(file: &std::path::Path, extra_args: &[&str]) -> Output {
    let mut cmd = Command::new(glyph_bin());
    cmd.arg("check").arg(file);
    for arg in extra_args {
        cmd.arg(arg);
    }
    cmd.output().expect("failed to spawn glyph binary")
}

// --- Acceptance criterion 2: --strict fails (exit 1) on repairable files ---

#[test]
fn strict_compile_repairable_exits_one() {
    let result = run_compile(&repairable("missing_description.glyph"), &["--strict"]);
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit 1 with --strict on repairable file, got {:?}\nstderr: {}",
        result.status.code(),
        String::from_utf8_lossy(&result.stderr),
    );
}

// --- Acceptance criterion 2 extended: --strict on all repairable files ---

#[test]
fn strict_compile_all_repairable_files_exit_one() {
    let repairable_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("repairable");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&repairable_dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            if p.is_file() && p.to_string_lossy().ends_with(".glyph") {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no repairable corpus files found");

    for file in &files {
        let result = run_compile(file, &["--strict"]);
        assert_eq!(
            result.status.code(),
            Some(1),
            "expected exit 1 with --strict on {}, got {:?}\nstderr: {}",
            file.display(),
            result.status.code(),
            String::from_utf8_lossy(&result.stderr),
        );
    }
}

// --- Acceptance criterion 3: without --strict, repairable exits 2 ---

#[test]
fn no_strict_compile_repairable_exits_two() {
    let result = run_compile(&repairable("missing_description.glyph"), &[]);
    assert_eq!(
        result.status.code(),
        Some(2),
        "expected exit 2 without --strict on repairable file, got {:?}\nstderr: {}",
        result.status.code(),
        String::from_utf8_lossy(&result.stderr),
    );
}

// --- Acceptance criterion 1: --strict passes (exit 0) on valid files ---

#[test]
fn strict_compile_valid_exits_zero() {
    // Copy to tempdir to avoid polluting corpus with .md output.
    let dir = tempfile::tempdir().unwrap();
    let src = valid("update_docs.glyph");
    let tmp_src = dir.path().join("update_docs.glyph");
    std::fs::copy(&src, &tmp_src).unwrap();

    let result = run_compile(&tmp_src, &["--strict"]);
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0 with --strict on valid file, got {:?}\nstderr: {}",
        result.status.code(),
        String::from_utf8_lossy(&result.stderr),
    );
}

// --- Acceptance criterion 1 extended: --strict on each valid file ---

#[test]
fn strict_compile_all_valid_files_exit_zero() {
    let valid_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("valid");
    // Collect all .glyph files (top-level only — imports/ needs multi-file
    // context which is directory-compile territory, not per-file).
    let mut files: Vec<PathBuf> = std::fs::read_dir(&valid_dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            if p.is_file() && p.to_string_lossy().ends_with(".glyph") {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no valid corpus files found");

    for file in &files {
        let dir = tempfile::tempdir().unwrap();
        let tmp_src = dir.path().join(file.file_name().unwrap());
        std::fs::copy(file, &tmp_src).unwrap();

        let result = run_compile(&tmp_src, &["--strict"]);
        assert_eq!(
            result.status.code(),
            Some(0),
            "expected exit 0 with --strict on {}, got {:?}\nstderr: {}",
            file.display(),
            result.status.code(),
            String::from_utf8_lossy(&result.stderr),
        );
    }
}

// --- --strict on glyph check ---

#[test]
fn strict_check_repairable_exits_one() {
    let result = run_check(&repairable("missing_description.glyph"), &["--strict"]);
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit 1 with `glyph check --strict` on repairable file, got {:?}\nstderr: {}",
        result.status.code(),
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn strict_check_valid_exits_zero() {
    let result = run_check(&valid("update_docs.glyph"), &["--strict"]);
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0 with `glyph check --strict` on valid file, got {:?}\nstderr: {}",
        result.status.code(),
        String::from_utf8_lossy(&result.stderr),
    );
}

// --- --strict suppresses .md output on repairable ---

#[test]
fn strict_compile_repairable_does_not_write_md() {
    let dir = tempfile::tempdir().unwrap();
    let src = repairable("missing_description.glyph");
    let tmp_src = dir.path().join("missing_description.glyph");
    std::fs::copy(&src, &tmp_src).unwrap();

    let out_path = dir.path().join("missing_description.md");

    let result = run_compile(&tmp_src, &["--strict"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(
        !out_path.exists(),
        "expected no .md output with --strict on repairable file, but {} exists",
        out_path.display(),
    );
}

// ---- issue #160: per-fixture coverage for the skill repair pair ----

#[test]
fn strict_compile_skill_meaningful_return_no_type_exits_one() {
    let result = run_compile(
        &repairable("skill_meaningful_return_no_type.glyph"),
        &["--strict"],
    );
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit 1 with --strict on skill_meaningful_return_no_type.glyph, got {:?}\nstderr: {}",
        result.status.code(),
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn strict_compile_skill_meaningful_return_no_type_does_not_write_md() {
    let dir = tempfile::tempdir().unwrap();
    let src = repairable("skill_meaningful_return_no_type.glyph");
    let tmp_src = dir.path().join("skill_meaningful_return_no_type.glyph");
    std::fs::copy(&src, &tmp_src).unwrap();

    let out_path = dir.path().join("skill_meaningful_return_no_type.md");

    let result = run_compile(&tmp_src, &["--strict"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(
        !out_path.exists(),
        "expected no .md output with --strict on repairable file, but {} exists",
        out_path.display(),
    );
}

#[test]
fn strict_compile_skill_meaningful_return_no_type_fixed_exits_zero() {
    // Copy to tempdir to avoid polluting corpus with .md output.
    let dir = tempfile::tempdir().unwrap();
    let src = valid("skill_meaningful_return_no_type_fixed.glyph");
    let tmp_src = dir
        .path()
        .join("skill_meaningful_return_no_type_fixed.glyph");
    std::fs::copy(&src, &tmp_src).unwrap();

    let result = run_compile(&tmp_src, &["--strict"]);
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0 with --strict on valid file, got {:?}\nstderr: {}",
        result.status.code(),
        String::from_utf8_lossy(&result.stderr),
    );
}

// ---- issue #161: per-fixture coverage for the block repair pair ----

#[test]
fn strict_compile_block_meaningful_return_no_type_exits_one() {
    let result = run_compile(
        &repairable("block_meaningful_return_no_type.glyph"),
        &["--strict"],
    );
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit 1 with --strict on block_meaningful_return_no_type.glyph, got {:?}\nstderr: {}",
        result.status.code(),
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn strict_compile_block_meaningful_return_no_type_does_not_write_md() {
    let dir = tempfile::tempdir().unwrap();
    let src = repairable("block_meaningful_return_no_type.glyph");
    let tmp_src = dir.path().join("block_meaningful_return_no_type.glyph");
    std::fs::copy(&src, &tmp_src).unwrap();

    let out_path = dir.path().join("block_meaningful_return_no_type.md");

    let result = run_compile(&tmp_src, &["--strict"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(
        !out_path.exists(),
        "expected no .md output with --strict on repairable file, but {} exists",
        out_path.display(),
    );
}

#[test]
fn strict_compile_block_meaningful_return_no_type_fixed_exits_zero() {
    // Copy to tempdir to avoid polluting corpus with .md output.
    let dir = tempfile::tempdir().unwrap();
    let src = valid("block_meaningful_return_no_type_fixed.glyph");
    let tmp_src = dir
        .path()
        .join("block_meaningful_return_no_type_fixed.glyph");
    std::fs::copy(&src, &tmp_src).unwrap();

    let result = run_compile(&tmp_src, &["--strict"]);
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0 with --strict on valid file, got {:?}\nstderr: {}",
        result.status.code(),
        String::from_utf8_lossy(&result.stderr),
    );
}
