//! Cross-phase integration test for issue #83 — warn on generic type names
//! in author-facing source.
//!
//! AC7 requires end-to-end coverage exercised through the spawned `glyph`
//! binary on the **default (`pretty`) output path**, where diagnostics render
//! to **stderr** via codespan-reporting. The structured (`--format json`,
//! stdout NDJSON) channel is exercised at the lib level by chunks 1+2; this
//! test pins the user-visible UX:
//!
//!   * AC7 — a fixture using banned generic type names `String` (skill header)
//!     and `List` (private block header) compiles successfully (exit 0) and
//!     surfaces both `G::analyze::generic-type-name` warnings on **stderr**.
//!
//! Mirrors `integration_issue_82.rs` for spawn convention; uses a sibling
//! `run_check_pretty` helper because the stderr/pretty path requires no
//! `--format` flag (and tests the default UX explicitly).

use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Output};

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

/// Run `glyph check <file>` with no `--format` flag — exercises the default
/// pretty path that renders diagnostics to stderr via codespan-reporting.
fn run_check_pretty(file: &Path) -> Output {
    Command::new(glyph_bin())
        .arg("check")
        .arg(file)
        .output()
        .expect("failed to spawn glyph binary")
}

/// AC7: a fixture using `String` (skill header) and `List` (private block
/// header) compiles successfully (exit 0) and surfaces both warnings on
/// stderr in the default pretty channel. Each warning carries the
/// diagnostic id and the offender name verbatim in its rendered output.
#[test]
fn ac7_pretty_stderr_emits_generic_type_name_warnings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ac7_generic.glyph");
    std::fs::write(
        &path,
        "\
skill summarize() -> String
    description: \"Summarize.\"
    flow:
        enumerate()

block enumerate() -> List
    description: \"Enumerate.\"
    flow:
        \"do it\"
",
    )
    .unwrap();

    let result = run_check_pretty(&path);

    // 1. Compilation succeeds — generic-type-name is a non-blocking warning
    //    (AC4 + AC7).
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0 (warnings non-blocking); stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );

    let stderr = String::from_utf8_lossy(&result.stderr).to_string();

    // 2. Diagnostic id appears in the rendered output (codespan emits it as
    //    the warning code in the header, e.g. `warning[G::analyze::...]:`).
    assert!(
        stderr.contains("G::analyze::generic-type-name"),
        "stderr must contain the diagnostic id `G::analyze::generic-type-name`, got:\n{}",
        stderr,
    );

    // 3. Both offender names appear verbatim in stderr (in the rendered
    //    message and/or the source-snippet line codespan prints).
    assert!(
        stderr.contains("String"),
        "stderr must contain offender `String`, got:\n{}",
        stderr,
    );
    assert!(
        stderr.contains("List"),
        "stderr must contain offender `List`, got:\n{}",
        stderr,
    );

    // 4. No-dedup at the user-visible layer — each banned occurrence yields
    //    its own warning. Codespan renders the diagnostic id once per
    //    diagnostic in the header line; expect at least 2 occurrences for
    //    the 2 banned return types in the fixture.
    let id_occurrences = stderr.matches("G::analyze::generic-type-name").count();
    assert!(
        id_occurrences >= 2,
        "expected >= 2 occurrences of `G::analyze::generic-type-name` (one per banned return type, no dedup), got {} in:\n{}",
        id_occurrences,
        stderr,
    );

    // 5. Confirm we exercised the pretty path: pretty diagnostics go to
    //    stderr, so stdout should not contain the diagnostic id (would
    //    indicate accidental NDJSON emission on stdout).
    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    assert!(
        !stdout.contains("G::analyze::generic-type-name"),
        "stdout must not contain the diagnostic id in pretty mode (pretty renders to stderr), got stdout:\n{}",
        stdout,
    );
}
