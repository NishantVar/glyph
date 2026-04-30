//! Slice 11 integration tests — imports (single-file resolution).
//!
//! Covers the five acceptance criteria:
//!   1. fix_bug.glyph.md resolves names imported from prefs.glyph.md and repo_tools.glyph.md
//!   2. Circular-import path is included in the diagnostic message
//!   3. Importing a private (non-exported) name fails with import-private
//!   4. Importing a skill (not a block/text) fails with import-skill
//!   5. Duplicate / unused imports are repairable diagnostics → exit 2

use std::path::PathBuf;
use std::process::{Command, Output};

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join(relative)
}

fn run_check(file: &str, format: &str) -> Output {
    Command::new(glyph_bin())
        .arg("check")
        .arg(fixture(file))
        .arg("--format")
        .arg(format)
        .output()
        .expect("failed to spawn glyph binary")
}

fn assert_contains_diagnostic_id(stdout: &str, id: &str) {
    let mut found = false;
    for line in stdout.lines() {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => panic!("non-JSON line on stdout: {:?}", line),
        };
        if v.get("id").and_then(|x| x.as_str()) == Some(id) {
            found = true;
        }
    }
    assert!(found, "expected diagnostic id `{}` in JSON output:\n{}", id, stdout);
}

/// AC1: fix_bug.glyph.md resolves names imported from prefs.glyph.md and repo_tools.glyph.md.
#[test]
fn ac1_cross_file_resolution() {
    let output = run_check("valid/imports/fix_bug.glyph.md", "json");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // No undefined-name or undefined-call errors should appear.
    for line in stdout.lines() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
        assert_ne!(id, "G::analyze::undefined-name", "imported name should resolve");
        assert_ne!(id, "G::analyze::undefined-call", "imported block should resolve");
    }
    // Exit code should not be 1 (hard error).
    assert_ne!(
        output.status.code(),
        Some(1),
        "cross-file resolution should not produce hard errors, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// AC2: Circular-import path is included in the diagnostic message.
#[test]
fn ac2_circular_import_path() {
    let output = run_check("invalid/imports/circular_a.glyph.md", "json");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_contains_diagnostic_id(&stdout, "G::analyze::circular-import");
    // The message should include the cycle path with ->.
    for line in stdout.lines() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        if v.get("id").and_then(|x| x.as_str()) == Some("G::analyze::circular-import") {
            let msg = v.get("message").and_then(|x| x.as_str()).unwrap_or("");
            assert!(msg.contains("->"), "cycle path should use -> separator: {}", msg);
        }
    }
    assert_eq!(output.status.code(), Some(1), "circular import should be exit 1");
}

/// AC3: Importing a private (non-exported) name fails with import-private.
#[test]
fn ac3_import_private() {
    let output = run_check("invalid/imports/import_private.glyph.md", "json");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_contains_diagnostic_id(&stdout, "G::analyze::import-private");
    assert_eq!(output.status.code(), Some(1), "import-private should be exit 1");
}

/// AC4: Importing a skill (not a block/text) fails with import-skill.
#[test]
fn ac4_import_skill() {
    let output = run_check("invalid/imports/import_skill.glyph.md", "json");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_contains_diagnostic_id(&stdout, "G::analyze::import-skill");
    assert_eq!(output.status.code(), Some(1), "import-skill should be exit 1");
}

/// AC5: Duplicate imports are repairable diagnostics → exit 2.
#[test]
fn ac5_duplicate_import_exit_2() {
    let output = run_check("repairable/imports/duplicate_import.glyph.md", "json");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_contains_diagnostic_id(&stdout, "G::analyze::duplicate-import");
    assert_eq!(
        output.status.code(),
        Some(2),
        "duplicate import should be exit 2 (repairable), stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// AC5: Unused imports are repairable diagnostics → exit 2.
#[test]
fn ac5_unused_import_exit_2() {
    let output = run_check("repairable/imports/unused_import.glyph.md", "json");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_contains_diagnostic_id(&stdout, "G::analyze::unused-import");
    assert_eq!(
        output.status.code(),
        Some(2),
        "unused import should be exit 2 (repairable), stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Missing import file produces G::analyze::missing-file.
#[test]
fn missing_import_file_exit_1() {
    let output = run_check("invalid/imports/missing_import.glyph.md", "json");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_contains_diagnostic_id(&stdout, "G::analyze::missing-file");
    assert_eq!(output.status.code(), Some(1), "missing file should be exit 1");
}
