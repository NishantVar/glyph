//! Snapshot test for the type/value namespace split.
//!
//! `legal_cross_kind.glyph` uses `LinkMode` (a type) and `link_mode`
//! (a value) in the same scope. Before the namespace split, the
//! case-insensitive collision sweep treated these as duplicates and
//! refused the program. After the split, the two identifiers live in
//! disjoint namespaces and compilation succeeds.
//!
//! This test locks that contract: the fixture must compile (exit 0)
//! and the compiled `.md` must contain the value identifier
//! `link_mode`. The type identifier is canonicalized to `linkmode` in
//! the IR (Glyph erases user-facing type names in compiled output),
//! so this test asserts on the value side and on successful exit.

use std::path::PathBuf;
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

fn run_compile(path: PathBuf) -> Output {
    Command::new(glyph_bin())
        .arg("compile")
        .arg(path)
        .arg("--emit-ir")
        .arg("--format")
        .arg("json")
        .output()
        .expect("failed to spawn glyph binary")
}

#[test]
fn compiled_output_distinguishes_type_and_value_with_same_canonical() {
    let src = fixture("valid", "legal_cross_kind.glyph");
    let md_path = src.with_file_name("legal_cross_kind.md");
    let ir_path = src.with_file_name("legal_cross_kind.ir.json");
    let _ = std::fs::remove_file(&md_path);
    let _ = std::fs::remove_file(&ir_path);

    let result = run_compile(src);
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0 (namespaces should be disjoint); stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );

    let md = std::fs::read_to_string(&md_path).expect(".md file missing after compile");
    assert!(
        md.contains("link_mode"),
        "expected value identifier `link_mode` in compiled .md:\n{md}"
    );

    // The IR canonicalizes user-facing type names (lowercased), so
    // assert on the canonical form `linkmode` to lock the type-side
    // contract.
    let ir = std::fs::read_to_string(&ir_path).expect(".ir.json missing after compile");
    assert!(
        ir.contains("linkmode"),
        "expected canonicalized type `linkmode` in IR:\n{ir}"
    );
    assert!(
        ir.contains("link_mode"),
        "expected value identifier `link_mode` in IR:\n{ir}"
    );
}
