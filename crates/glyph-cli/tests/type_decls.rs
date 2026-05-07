//! Tests for `type` declarations and library-only export semantics.

use std::path::PathBuf;
use std::process::Command;

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

/// A type-only library (no `skill`, only `export type`) must satisfy the
/// library-export rule and compile with exit 0.
#[test]
fn type_only_library_compiles_cleanly() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("types_only.glyph"),
        "export type RepoContext = <\"the inspected repo state, including file tree and dependencies\">\n\
         export type RiskLevel = <\"one of: low, medium, high; severity of the change\">\n",
    )
    .unwrap();

    let output = Command::new(glyph_bin())
        .arg("check")
        .arg(dir.path().join("types_only.glyph"))
        .output()
        .expect("failed to spawn glyph binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("G::analyze::no-exports-in-library"),
        "type-only file should satisfy library-export rule; stderr: {}",
        stderr
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "expected exit 0; stderr: {}",
        stderr
    );
}
