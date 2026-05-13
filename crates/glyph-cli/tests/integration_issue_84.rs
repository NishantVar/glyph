//! Multi-file integration tests for issue #84 — domain-type registry with
//! implicit first-use declaration. Exercises AC8 ("file A declares a domain
//! type via `-> T`; file B imports the block and consumes T via nominal
//! match") end-to-end through the spawned `glyph` binary.
//!
//! The chunk-4 sibling test in `glyph-core/src/lib.rs::tests` covers the same
//! cross-file pipeline through the in-process `check_file` Rust API. These
//! binary-level tests pin the user-visible `glyph check` exit-code and NDJSON
//! diagnostic surface so that CLI-only regressions cannot slip past the
//! library-only suite.
//!
//! Test set (3 cases — symmetric to the chunk brief):
//!
//!   1. **Success — exact match**: `lib.glyph` exports
//!      `block compute() -> Report`; `main.glyph`'s skill declares
//!      `-> Report` and consumes via `return compute()`. Same canonicalized
//!      name → `glyph check` exits 0 with no `G::analyze::nominal-mismatch`.
//!      Also acts as an integration-level regression pin for the chunk-7a
//!      analyze fix: before that fix, `return compute()` did not register the
//!      imported name as used and `unused-import` (Repairable, exit 2) fired
//!      spuriously, blocking the exit-0 success contract.
//!
//!   2. **Success — canonicalization match**: lib exports `-> Repo_Context`;
//!      main declares `-> repocontext`. Both spellings canonicalize to the
//!      same key per `domain_registry::canonicalize_identifier` (D6:
//!      ASCII-lowercase + strip `_`), so nominal equality holds and exit is
//!      still 0.
//!
//!   3. **Failure — mismatch fires**: lib exports `-> Plan`; main declares
//!      `-> Report`. Different canonicalized names → `nominal-mismatch`
//!      (severity `error`, exit 1). Diagnostic message names both type names
//!      and the call target, matching the chunk-4 sibling test's contract.

use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Output};

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn run_check(file: &Path, format: &str) -> Output {
    Command::new(glyph_bin())
        .arg("check")
        .arg(file)
        .arg("--format")
        .arg(format)
        .output()
        .expect("failed to spawn glyph binary")
}

/// True iff the NDJSON `stdout` contains a diagnostic with the given id.
fn ndjson_contains_id(stdout: &str, id: &str) -> bool {
    stdout
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .any(|v| v.get("id").and_then(|x| x.as_str()) == Some(id))
}

/// Classification ("error" / "repairable" / "warning") of the first diagnostic
/// on `stdout` matching `id`, if any.
fn classification_of(stdout: &str, id: &str) -> Option<String> {
    stdout
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v.get("id").and_then(|x| x.as_str()) == Some(id))
        .and_then(|v| {
            v.get("classification")
                .and_then(|x| x.as_str())
                .map(String::from)
        })
}

/// Message text of the first diagnostic on `stdout` matching `id`, if any.
fn message_of(stdout: &str, id: &str) -> Option<String> {
    stdout
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v.get("id").and_then(|x| x.as_str()) == Some(id))
        .and_then(|v| v.get("message").and_then(|x| x.as_str()).map(String::from))
}

/// AC8 success path 1 — exact-name nominal match across files.
///
/// Library exports `compute() -> Report`; consumer skill declares `-> Report`
/// and `return compute()`. Same canonicalized name → no `nominal-mismatch`
/// fires; `glyph check` exits 0.
///
/// Also pins the chunk-7a fix at integration level: before the
/// `track_flow_usage` `Return(Call)` arm landed, `return compute()` did not
/// register `compute` as a used import, so `G::analyze::unused-import`
/// (Repairable, exit 2) fired spuriously and the exit-0 success contract
/// could not be observed end-to-end.
#[test]
fn ac8_cross_file_nominal_match_succeeds_via_binary() {
    let dir = tempfile::tempdir().unwrap();

    let lib_path = dir.path().join("lib.glyph");
    std::fs::write(
        &lib_path,
        "\
export block compute() -> Report
    description: \"Build the report.\"
    flow:
        return \"a report\"
",
    )
    .unwrap();

    let main_path = dir.path().join("main.glyph");
    std::fs::write(
        &main_path,
        "\
import \"./lib.glyph\" { compute }

skill main() -> Report
    description: \"Main.\"
    flow:
        return compute()
",
    )
    .unwrap();

    let result = run_check(&main_path, "json");
    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let stderr = String::from_utf8_lossy(&result.stderr).to_string();
    assert_eq!(
        result.status.code(),
        Some(0),
        "exact-name cross-file match should exit 0 (clean); stdout={:?} stderr={:?}",
        stdout,
        stderr,
    );
    assert!(
        !ndjson_contains_id(&stdout, "G::analyze::nominal-mismatch"),
        "must NOT fire G::analyze::nominal-mismatch when types match, got:\n{}",
        stdout,
    );
    // Regression pin for chunk-7a: a `return imported_block()` is a use site
    // and must not surface `unused-import`. Without 7a, this assertion failed
    // (and the exit-0 check above did, too — Repairable diagnostics yield 2).
    assert!(
        !ndjson_contains_id(&stdout, "G::analyze::unused-import"),
        "must NOT fire G::analyze::unused-import — `return compute()` is a use of the import, got:\n{}",
        stdout,
    );
}

/// AC8 success path 2 — case-divergent (but still PascalCase) spellings
/// still match via canonicalization.
///
/// Library exports `compute() -> RepoContext`; consumer skill declares
/// `-> Repocontext`. Both spellings canonicalize to `"repocontext"` per
/// `domain_registry::canonicalize_identifier` (D6: ASCII-lowercase + strip
/// `_`), so cross-file nominal equality holds and `glyph check` exits 0.
/// This pins that the matcher uses canonicalized equality, not byte-equal
/// equality on the raw spelling — a regression here would silently break
/// AC8 for any author who imports across casing styles.
///
/// Task 8 rewrite: under the type-namespace case rule (`type-case-violation`)
/// both spellings must be PascalCase; the historical underscore-bearing form
/// `Repo_Context` is no longer legal, so the fixture uses `RepoContext` and
/// `Repocontext` instead.
#[test]
fn ac8_cross_file_nominal_match_canonicalization_succeeds() {
    let dir = tempfile::tempdir().unwrap();

    let lib_path = dir.path().join("lib.glyph");
    std::fs::write(
        &lib_path,
        "\
export block compute() -> RepoContext
    description: \"Build the repo context.\"
    flow:
        return \"ctx\"
",
    )
    .unwrap();

    let main_path = dir.path().join("main.glyph");
    std::fs::write(
        &main_path,
        "\
import \"./lib.glyph\" { compute }

skill main() -> Repocontext
    description: \"Main.\"
    flow:
        return compute()
",
    )
    .unwrap();

    let result = run_check(&main_path, "json");
    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let stderr = String::from_utf8_lossy(&result.stderr).to_string();
    assert_eq!(
        result.status.code(),
        Some(0),
        "case-divergent spellings should match via canonicalization and exit 0; stdout={:?} stderr={:?}",
        stdout,
        stderr,
    );
    assert!(
        !ndjson_contains_id(&stdout, "G::analyze::nominal-mismatch"),
        "must NOT fire G::analyze::nominal-mismatch when names canonicalize equal, got:\n{}",
        stdout,
    );
}

/// Issue #84 codex pass 3 — F2 [P2] integration pin. An import consumed
/// only inside a private `block` body (called from a skill) must not fire
/// `G::analyze::unused-import`.
///
/// Pre-fix: `analyze_with_imports` called `track_flow_usage` only from the
/// `Decl::Skill` arm. With the `Decl::Block` arm now also threading through
/// `track_flow_usage` (mirroring the skill arm), the multi-file fixture
/// below — `main.glyph` consumes `imported_foo` exclusively from inside
/// `block helper() { return imported_foo() }`, with `helper()` called from
/// `skill main()` — exits 0 cleanly. Pre-fix, exit code was 2 with a
/// Repairable `unused-import` diagnostic.
///
/// This is the binary-level pin for the analyze-side unit test
/// `analyze::tests::t18_block_flow_use_of_imported_block_marks_used_via_imports_path`,
/// extending the AC8 multi-file CLI surface coverage to the import-tracking
/// dimension that pass 3 closes.
#[test]
fn ac_codex_pass3_block_flow_import_used_via_binary() {
    let dir = tempfile::tempdir().unwrap();

    let lib_path = dir.path().join("lib.glyph");
    std::fs::write(
        &lib_path,
        "\
export block imported_foo() -> Report
    description: \"Build the report.\"
    flow:
        return \"a report\"
",
    )
    .unwrap();

    let main_path = dir.path().join("main.glyph");
    std::fs::write(
        &main_path,
        "\
import \"./lib.glyph\" { imported_foo }

skill main() -> Report
    description: \"Main.\"
    flow:
        return helper()

block helper() -> Report
    description: \"Helper.\"
    flow:
        return imported_foo()
",
    )
    .unwrap();

    let result = run_check(&main_path, "json");
    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let stderr = String::from_utf8_lossy(&result.stderr).to_string();
    assert_eq!(
        result.status.code(),
        Some(0),
        "block-flow consumption of imported block must exit 0 (clean); stdout={:?} stderr={:?}",
        stdout,
        stderr,
    );
    assert!(
        !ndjson_contains_id(&stdout, "G::analyze::unused-import"),
        "must NOT fire G::analyze::unused-import — `block helper {{ return imported_foo() }}` is a use of the import, got:\n{}",
        stdout,
    );
}

/// Issue #84 codex pass 4 integration pin. Same surface as the analyze-side
/// unit test `t19_return_call_to_undefined_name_fires_undefined_call`, but
/// exercised through the spawned `glyph` binary so the CLI exit code and
/// NDJSON diagnostic surface stay in sync with the library.
///
/// Pre-fix: a `return some_undefined()` in skill flow surfaced no diagnostic
/// at all and `glyph check` exited 0 — the carry-forward observation
/// documented in the analyze-side `t13` test. Post-fix: undefined-call
/// (Repairable) fires and `glyph check` exits 2.
#[test]
fn ac_codex_pass4_return_undefined_via_binary() {
    let dir = tempfile::tempdir().unwrap();

    let main_path = dir.path().join("main.glyph");
    std::fs::write(
        &main_path,
        "\
skill main() -> Plan
    description: \"Main.\"
    flow:
        return some_undefined()
",
    )
    .unwrap();

    let result = run_check(&main_path, "json");
    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let stderr = String::from_utf8_lossy(&result.stderr).to_string();
    assert_eq!(
        result.status.code(),
        Some(2),
        "undefined-call (Repairable) in return position should exit 2; stdout={:?} stderr={:?}",
        stdout,
        stderr,
    );
    assert!(
        ndjson_contains_id(&stdout, "G::analyze::undefined-call"),
        "must fire G::analyze::undefined-call for `return some_undefined()`, got:\n{}",
        stdout,
    );
    assert_eq!(
        classification_of(&stdout, "G::analyze::undefined-call").as_deref(),
        Some("repairable"),
        "G::analyze::undefined-call must be classified repairable, got:\n{}",
        stdout,
    );
    let msg = message_of(&stdout, "G::analyze::undefined-call")
        .expect("undefined-call diagnostic must have a message field");
    assert!(
        msg.contains("some_undefined"),
        "message must name the undefined callee, got: {}",
        msg,
    );
}

/// AC8 failure path — divergent declared types fire `nominal-mismatch`.
///
/// Library exports `compute() -> Plan`; consumer skill declares `-> Report`
/// and `return compute()`. Different canonicalized names → the cross-file
/// `G::analyze::nominal-mismatch` diagnostic fires (severity `error` per
/// `docs/reference/diagnostics.md`), `glyph check` exits 1, and the message names
/// both type names plus the call target so the author can locate the
/// mismatch.
///
/// This is the binary-level twin of
/// `lib.rs::tests::cross_file_nominal_mismatch_fires_via_check_file` — it
/// ensures CLI users see the same diagnostic surface that the in-process
/// `check_file` API exposes.
#[test]
fn ac8_cross_file_nominal_mismatch_fires_via_binary() {
    let dir = tempfile::tempdir().unwrap();

    let lib_path = dir.path().join("lib.glyph");
    std::fs::write(
        &lib_path,
        "\
export block compute() -> Plan
    description: \"Make a plan.\"
    flow:
        return \"a plan\"
",
    )
    .unwrap();

    let main_path = dir.path().join("main.glyph");
    std::fs::write(
        &main_path,
        "\
import \"./lib.glyph\" { compute }

skill main() -> Report
    description: \"Main.\"
    flow:
        return compute()
",
    )
    .unwrap();

    let result = run_check(&main_path, "json");
    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let stderr = String::from_utf8_lossy(&result.stderr).to_string();
    assert_eq!(
        result.status.code(),
        Some(1),
        "type mismatch should exit 1 (hard error); stdout={:?} stderr={:?}",
        stdout,
        stderr,
    );
    assert!(
        ndjson_contains_id(&stdout, "G::analyze::nominal-mismatch"),
        "must fire G::analyze::nominal-mismatch on divergent types, got:\n{}",
        stdout,
    );
    assert_eq!(
        classification_of(&stdout, "G::analyze::nominal-mismatch").as_deref(),
        Some("error"),
        "G::analyze::nominal-mismatch must be classified error, got:\n{}",
        stdout,
    );
    let msg = message_of(&stdout, "G::analyze::nominal-mismatch")
        .expect("nominal-mismatch diagnostic must have a message field");
    assert!(
        msg.contains("Report") && msg.contains("Plan"),
        "message must name both caller's `Report` and callee's `Plan`, got: {}",
        msg,
    );
    assert!(
        msg.contains("compute"),
        "message must name the call target `compute`, got: {}",
        msg,
    );
}
