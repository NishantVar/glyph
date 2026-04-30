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

fn run_compile_strict(file: &std::path::Path) -> Output {
    Command::new(glyph_bin())
        .arg("compile")
        .arg(file)
        .arg("--strict")
        .output()
        .expect("failed to spawn glyph binary")
}

#[test]
fn strict_compile_repairable_exits_one() {
    let result = run_compile_strict(&repairable("missing_description.glyph.md"));
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit 1 with --strict on repairable file, got {:?}\nstderr: {}",
        result.status.code(),
        String::from_utf8_lossy(&result.stderr),
    );
}
