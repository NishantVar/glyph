//! B03: export-block flow is fully semantic-checked.
//!
//! Each repro below exercises a single diagnostic that previously did not fire
//! for an `export block` (only for `block` / `skill`). The negative control
//! ensures a well-formed export block still exits 0 with no diagnostics.
//!
//! Bug audit ID: B03 (2026-05-20).

use std::path::PathBuf;
use std::process::{Command, Output};

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

/// Write `src` to a fresh tempdir as `repro.glyph` and run `glyph check
/// <path> --format json` on it. NDJSON on stdout is the contract used by the
/// existing `check_subcommand` tests.
fn run_check_on_source(src: &str) -> (Output, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().join("repro.glyph");
    std::fs::write(&path, src).expect("write fixture");
    let output = Command::new(glyph_bin())
        .arg("check")
        .arg(&path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("failed to spawn glyph binary");
    (output, dir)
}

/// Assert that `output.stdout` contains a JSON diagnostic with `id == diag_id`
/// and that `glyph check` exited with `expected_code`.
fn assert_diagnostic(output: &Output, expected_code: i32, diag_id: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(expected_code),
        "expected exit {expected_code} for diagnostic {diag_id};\nstdout={stdout}\nstderr={stderr}",
    );
    let mut found = false;
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each NDJSON line must parse as JSON");
        if v.get("id").and_then(|x| x.as_str()) == Some(diag_id) {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "expected diagnostic id {diag_id} in stdout;\nstdout={stdout}\nstderr={stderr}",
    );
}

// -- Repro 1 --------------------------------------------------------------
// `return missing()` calls an undeclared block. Should surface
// `G::analyze::undefined-call` (repairable -> exit 2).
#[test]
fn b03_repro1_return_undefined_call() {
    let src = "\
export block caller() -> Report
    flow:
        return missing()
";
    let (output, _dir) = run_check_on_source(src);
    assert_diagnostic(&output, 2, "G::analyze::undefined-call");
}

// -- Repro 2 --------------------------------------------------------------
// `return foo()` against a callee that requires arg `x`. Should surface
// `G::analyze::missing-required-arg` (error -> exit 1).
#[test]
fn b03_repro2_return_call_missing_required_arg() {
    let src = "\
export block foo(x: Input) -> Report
    flow:
        return \"ok\"

export block caller() -> Report
    flow:
        return foo()
";
    let (output, _dir) = run_check_on_source(src);
    assert_diagnostic(&output, 1, "G::analyze::missing-required-arg");
}

// -- Repro 3 --------------------------------------------------------------
// Caller declared `-> Report` but `return foo()` where `foo() -> Plan`.
// Should surface `G::analyze::nominal-mismatch` (error -> exit 1).
#[test]
fn b03_repro3_return_call_nominal_mismatch() {
    let src = "\
export block foo() -> Plan
    flow:
        return \"plan\"

export block caller() -> Report
    flow:
        return foo()
";
    let (output, _dir) = run_check_on_source(src);
    assert_diagnostic(&output, 1, "G::analyze::nominal-mismatch");
}

// -- Repro 4 --------------------------------------------------------------
// `require missing_text` where `missing_text` is not a declared `const`.
// Should surface `G::analyze::undefined-name` (error -> exit 1).
#[test]
fn b03_repro4_require_undefined_name() {
    let src = "\
export block caller() -> Report
    require missing_text
    flow:
        return \"ok\"
";
    let (output, _dir) = run_check_on_source(src);
    assert_diagnostic(&output, 1, "G::analyze::undefined-name");
}

// -- Repro 5 --------------------------------------------------------------
// Two `return` statements at the flow root. Should surface
// `G::parse::multiple-returns` (error -> exit 1).
#[test]
fn b03_repro5_multiple_returns() {
    let src = "\
export block caller() -> Report
    flow:
        return \"a\"
        return \"b\"
";
    let (output, _dir) = run_check_on_source(src);
    assert_diagnostic(&output, 1, "G::parse::multiple-returns");
}

// -- Repro 6 --------------------------------------------------------------
// `return` followed by another root-level statement. Should surface
// `G::parse::return-not-terminal` (error -> exit 1).
#[test]
fn b03_repro6_return_not_terminal() {
    let src = "\
export block caller() -> Report
    flow:
        return \"a\"
        \"after\"
";
    let (output, _dir) = run_check_on_source(src);
    assert_diagnostic(&output, 1, "G::parse::return-not-terminal");
}

// -- Repro 7 --------------------------------------------------------------
// `return` nested inside an `if` branch body. Should surface
// `G::parse::return-in-branch` (error -> exit 1).
#[test]
fn b03_repro7_return_in_branch() {
    let src = "\
export block caller() -> Report
    flow:
        if \"x\":
            return \"a\"
        return \"b\"
";
    let (output, _dir) = run_check_on_source(src);
    assert_diagnostic(&output, 1, "G::parse::return-in-branch");
}

// -- Negative control ----------------------------------------------------
// A well-formed export block that calls another well-formed export block at
// `return` should produce no diagnostics and exit 0. This guards against the
// fix being over-broad.
#[test]
fn b03_negative_well_formed_export_block_is_clean() {
    let src = "\
export block helper() -> Report
    flow:
        return \"ok\"

export block caller() -> Report
    flow:
        return helper()
";
    let (output, _dir) = run_check_on_source(src);
    assert_eq!(
        output.status.code(),
        Some(0),
        "well-formed export block must exit 0;\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().is_empty(),
        "well-formed export block must produce no diagnostics, got: {stdout}",
    );
}
