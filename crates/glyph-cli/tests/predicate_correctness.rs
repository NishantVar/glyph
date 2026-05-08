//! Layer 3 corpus regression for the codex-flagged predicate correctness bugs.
//! Each fixture round-trips source → IR → emit and diffs against a checked-in
//! Markdown golden. Fixtures cover Findings 2, 3, 5 (Finding 4 is doc-only,
//! covered by Tasks 10 and 11).
//!
//! Finding 1 (block-with-string-predicate) is NOT covered here: at HEAD an
//! unrelated emit-side gap re-renders block-procedure subsections from raw
//! `flow_statements` strings rather than through the branch emit pipeline,
//! so an `if predicate_const` inside a block flow surfaces as a literal
//! `"if big_change"` step rather than a substituted If/Otherwise arm. The
//! classification authority fix (Task 6) reaches the IR correctly; the
//! downstream procedure-rendering gap is out of scope for this plan.
//!
//! Driver runs in directory-compile mode (`glyph compile <tmpdir>`), which is
//! the path that exercises `analyze_with_imports` — required for the
//! imported-string-predicate fixture (Finding 2).

use std::path::PathBuf;
use std::process::{Command, Output};

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("predicate-correctness")
}

fn run_compile(target: &std::path::Path) -> Output {
    Command::new(glyph_bin())
        .arg("compile")
        .arg(target)
        .output()
        .expect("failed to spawn glyph binary")
}

/// Copy `<name>.glyph` (and any extra sibling files) into a tempdir, compile
/// the directory, then diff the produced `<name>.md` against
/// `<name>.expected.md`.
fn run_fixture(name: &str, extra_siblings: &[&str]) {
    let dir = fixtures_dir();
    let src_path = dir.join(format!("{name}.glyph"));
    let expected_path = dir.join(format!("{name}.expected.md"));

    let expected = std::fs::read_to_string(&expected_path)
        .unwrap_or_else(|e| panic!("missing golden {expected_path:?}: {e}"));

    let tmp = tempfile::tempdir().unwrap();
    let tmp_src = tmp.path().join(format!("{name}.glyph"));
    std::fs::copy(&src_path, &tmp_src).unwrap_or_else(|e| panic!("copy fixture {name}: {e}"));

    for sibling in extra_siblings {
        let from = dir.join(sibling);
        let to = tmp.path().join(sibling);
        std::fs::copy(&from, &to).unwrap_or_else(|e| panic!("copy sibling {sibling}: {e}"));
    }

    // Directory mode so imports resolve via `analyze_with_imports`.
    let result = run_compile(tmp.path());
    assert!(
        result.status.success(),
        "compile failed for {name}:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );

    let produced = tmp.path().join(format!("{name}.md"));
    let actual = std::fs::read_to_string(&produced)
        .unwrap_or_else(|e| panic!("produced .md missing for {name}: {e}"));

    assert_eq!(actual, expected, "fixture {name} diverged from golden");
}

#[test]
fn imported_string_predicate() {
    run_fixture(
        "imported-string-predicate",
        &["imported-string-predicate-imported.glyph"],
    );
}

#[test]
fn equals_with_string_rhs() {
    run_fixture("equals-with-string-rhs", &[]);
}

#[test]
fn paren_grouped_predicates() {
    run_fixture("paren-grouped-predicates", &[]);
}
