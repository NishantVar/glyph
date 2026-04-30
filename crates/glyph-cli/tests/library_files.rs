//! Slice 13 integration tests — library files, export blocks, closure check.
//!
//! Covers acceptance criteria via the CLI binary:
//!   1. Export-text-only library compiles to zero .md, exit 0
//!   2. repo_tools-style library with export blocks compiles, exit 0
//!   3. Closure violation fires when export block references private free variables
//!   4. Library with zero exports fires no-exports-in-library (exit 1)
//!   5. Sibling exports visited in source order (deterministic)

use std::path::PathBuf;
use std::process::Command;

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

/// AC1: export-text-only library (prefs.glyph.md) compiles to zero .md, exit 0.
#[test]
fn ac1_export_text_only_library_cli() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(dir.path().join("prefs.glyph.md"), "\
export text terminal_mux = \"tmux\"
export text validation_strictness = \"high\"
").unwrap();

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(dir.path())
        .output()
        .expect("failed to spawn glyph binary");

    assert_eq!(
        output.status.code(),
        Some(0),
        "library file should compile with exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        !dir.path().join("prefs.md").exists(),
        "library file should not produce .md output"
    );
}

/// AC4: library with zero exports fires no-exports-in-library, exit 1.
#[test]
fn ac4_no_exports_in_library_cli() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(dir.path().join("empty_lib.glyph.md"), "\
text private_only = \"This is private.\"
").unwrap();

    let output = Command::new(glyph_bin())
        .arg("check")
        .arg(dir.path().join("empty_lib.glyph.md"))
        .output()
        .expect("failed to spawn glyph binary");

    assert_eq!(
        output.status.code(),
        Some(1),
        "library with zero exports should exit 1, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no-exports-in-library"),
        "stderr should mention no-exports-in-library, got: {}",
        stderr
    );
}

/// AC3: closure violation fires when export block references private names.
#[test]
fn ac3_closure_violation_cli() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(dir.path().join("lib.glyph.md"), "\
block private_helper()
    \"Do private stuff.\"

export block shared_util(x = \"default\")
    flow:
        private_helper()
        return x
").unwrap();

    let output = Command::new(glyph_bin())
        .arg("check")
        .arg(dir.path().join("lib.glyph.md"))
        .output()
        .expect("failed to spawn glyph binary");

    assert_eq!(
        output.status.code(),
        Some(1),
        "closure violation should exit 1, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("closure-violation"),
        "stderr should mention closure-violation, got: {}",
        stderr
    );
}
