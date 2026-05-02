//! Cross-phase integration tests for issue #82 — drop `-> None` and enforce
//! export-block return-type rules.
//!
//! AC7 requires end-to-end coverage exercised through the spawned `glyph`
//! binary (parse → analyze → repair). These tests synthesize source via
//! tempfiles so they do not depend on shared corpus fixtures, and assert:
//!
//!   * AC7-1 — `skill foo() -> None` is rejected at parse with the Repairable
//!     diagnostic `G::parse::none-as-return-type`; `glyph fmt` strips the
//!     annotation; a re-run of `glyph check` on the rewritten source exits 0.
//!   * AC7-2 — `export block foo()` with a meaningful return but no
//!     `-> DomainType` fires `G::analyze::export-missing-return-type` as
//!     Repairable (exit 2) end-to-end through the binary.
//!
//! AC7-3 (multi-decl `-> None` round-trip → parse+analyze clean) is folded
//! into `fmt_strips_legacy_none_return_type` in `tests/fmt.rs` per planner
//! guidance — avoids duplicating the multi-decl corpus fixture setup.

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

fn run_fmt(file: &Path) -> Output {
    Command::new(glyph_bin())
        .arg("fmt")
        .arg(file)
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

/// AC7-1: `skill foo() -> None` is rejected at parse with the Repairable
/// diagnostic `G::parse::none-as-return-type` (exit 2). `glyph fmt` strips the
/// annotation. A second `glyph check` on the rewritten source exits 0 and
/// emits no `G::parse::none-as-return-type` diagnostic.
#[test]
fn ac7_none_return_parse_then_fmt_then_reparse_clean() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("foo.glyph.md");
    std::fs::write(
        &path,
        "skill foo() -> None\n    description: \"d\"\n    flow:\n        \"x\"\n",
    )
    .unwrap();

    // 1. First check: Repairable (exit 2) with G::parse::none-as-return-type.
    let first = run_check(&path, "json");
    assert_eq!(
        first.status.code(),
        Some(2),
        "first check should exit 2 (Repairable); stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr),
    );
    let first_stdout = String::from_utf8_lossy(&first.stdout).to_string();
    assert!(
        ndjson_contains_id(&first_stdout, "G::parse::none-as-return-type"),
        "first check stdout must contain G::parse::none-as-return-type, got:\n{}",
        first_stdout,
    );
    assert_eq!(
        classification_of(&first_stdout, "G::parse::none-as-return-type").as_deref(),
        Some("repairable"),
        "G::parse::none-as-return-type must be classified repairable, got:\n{}",
        first_stdout,
    );

    // 2. fmt rewrites the file in place, exit 0.
    let fmt_out = run_fmt(&path);
    assert!(
        fmt_out.status.success(),
        "glyph fmt should exit 0; stderr={:?}",
        String::from_utf8_lossy(&fmt_out.stderr),
    );
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(
        !after.to_ascii_lowercase().contains("-> none"),
        "fmt must strip `-> None`, got:\n{}",
        after,
    );
    assert!(
        after.contains("skill foo()\n"),
        "fmt should reduce header to `skill foo()`, got:\n{}",
        after,
    );

    // 3. Re-check: clean exit 0 with no `none-as-return-type` diagnostic.
    let second = run_check(&path, "json");
    assert_eq!(
        second.status.code(),
        Some(0),
        "post-fmt re-check should exit 0; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr),
    );
    let second_stdout = String::from_utf8_lossy(&second.stdout).to_string();
    assert!(
        !ndjson_contains_id(&second_stdout, "G::parse::none-as-return-type"),
        "post-fmt re-check must not contain G::parse::none-as-return-type, got:\n{}",
        second_stdout,
    );
}

/// AC7-2: an `export block foo()` with `return <meaningful-expr>` but no
/// `-> DomainType` annotation fires `G::analyze::export-missing-return-type`
/// as Repairable (exit 2) when run through `glyph check`.
#[test]
fn ac7_export_block_missing_return_type_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing_arrow.glyph.md");
    std::fs::write(
        &path,
        "\
export block compute_value()
    description: \"Compute a value.\"

    flow:
        \"Compute it.\"
        return \"result\"
",
    )
    .unwrap();

    let result = run_check(&path, "json");
    assert_eq!(
        result.status.code(),
        Some(2),
        "expected exit 2 (Repairable); stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    assert!(
        ndjson_contains_id(&stdout, "G::analyze::export-missing-return-type"),
        "check stdout must contain G::analyze::export-missing-return-type, got:\n{}",
        stdout,
    );
    assert_eq!(
        classification_of(&stdout, "G::analyze::export-missing-return-type").as_deref(),
        Some("repairable"),
        "G::analyze::export-missing-return-type must be classified repairable, got:\n{}",
        stdout,
    );
}
