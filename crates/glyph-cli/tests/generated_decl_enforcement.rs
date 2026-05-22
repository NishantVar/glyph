//! B12 regression tests — `generated const` / `generated block` placement
//! and `generated block` body shape are enforced at parse time.
//!
//! Pinned design contract:
//! - `design/language-surface.md` §3.6 last bullet: `generated const` decls
//!   must appear after all non-generated top-level decls.
//! - `design/language-surface.md` §3.7 last two bullets: same placement
//!   rule for `generated block`; body must be a single inline-or-block
//!   string (multi-statement `flow:` bodies are not allowed).
//!
//! Both violations are Hard parse-level diagnostics (exit code 1).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn fixture(subdir: &str, name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join(subdir)
        .join(name)
}

fn run_check(file: &Path) -> Output {
    Command::new(glyph_bin())
        .arg("check")
        .arg(file)
        .arg("--format")
        .arg("json")
        .output()
        .expect("failed to spawn glyph binary")
}

fn diagnostic_ids(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
        .collect()
}

#[test]
fn generated_decl_followed_by_non_generated_decl_fires_out_of_order() {
    let src = fixture("invalid", "generated_decl_after_non_generated.glyph");
    let result = run_check(&src);
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit 1 (Hard); stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    let ids = diagnostic_ids(&stdout);
    assert!(
        ids.iter()
            .any(|x| x == "G::parse::generated-decl-out-of-order"),
        "expected `G::parse::generated-decl-out-of-order`; got {ids:?}\nraw stdout:\n{stdout}"
    );
}

#[test]
fn generated_block_with_multi_statement_flow_body_fires_body_shape() {
    let src = fixture("invalid", "generated_block_multi_statement_body.glyph");
    let result = run_check(&src);
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit 1 (Hard); stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    let ids = diagnostic_ids(&stdout);
    assert!(
        ids.iter()
            .any(|x| x == "G::parse::generated-block-body-shape"),
        "expected `G::parse::generated-block-body-shape`; got {ids:?}\nraw stdout:\n{stdout}"
    );
}

/// Covers the single-statement-`flow:` gap: a `generated block` whose body
/// is an explicit `flow:` sub-section with exactly one inline string must
/// still fire `G::parse::generated-block-body-shape` (no `flow:` keyword is
/// allowed in a generated block body).
#[test]
fn generated_block_with_single_statement_flow_body_fires_body_shape() {
    let src = fixture(
        "invalid",
        "generated_block_single_statement_flow_body.glyph",
    );
    let result = run_check(&src);
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit 1 (Hard); stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    let ids = diagnostic_ids(&stdout);
    assert!(
        ids.iter()
            .any(|x| x == "G::parse::generated-block-body-shape"),
        "expected `G::parse::generated-block-body-shape`; got {ids:?}\nraw stdout:\n{stdout}"
    );
}

#[test]
fn well_formed_generated_block_stays_clean() {
    let src = fixture("valid", "generated_block_single_string_ok.glyph");
    let result = run_check(&src);
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    let ids = diagnostic_ids(&stdout);
    assert!(
        !ids.iter().any(|x| x == "G::parse::generated-decl-out-of-order"
            || x == "G::parse::generated-block-body-shape"),
        "well-formed generated block must not fire B12 diagnostics; got {ids:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0 on well-formed fixture; stdout={stdout}\nstderr={stderr}"
    );
}
