//! Issue #81 chunk 3 — End-to-end fixtures for each `const` primitive kind.
//!
//! Verifies that consts of all four primitive kinds (String, Int, Float, Bool)
//! parse, infer (`crate::kind_infer`), and inline through the lower → emit
//! pipeline, with the rendered value appearing in compiled output. Each
//! fixture references its const via `require NAME` (text-equivalent semantics
//! per chunk 2) so the value materializes in `### Constraints`.

use std::path::PathBuf;
use std::process::Command;

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn corpus_source(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("valid")
        .join(name)
}

/// Copy fixture to a tempdir, compile via the CLI, and return the emitted .md.
fn compile_fixture(name: &str) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let src = corpus_source(name);
    let tmp_src = dir.path().join(name);
    std::fs::copy(&src, &tmp_src).unwrap();
    let out = dir.path().join(name.replace(".glyph", ".md"));

    let result = Command::new(glyph_bin())
        .arg("compile")
        .arg(&tmp_src)
        .output()
        .expect("failed to spawn glyph binary");
    assert_eq!(
        result.status.code(),
        Some(0),
        "glyph compile failed for {}. stderr: {}",
        name,
        String::from_utf8_lossy(&result.stderr),
    );
    let md = std::fs::read_to_string(&out).expect("compiled .md missing");
    (dir, md)
}

#[test]
fn const_string_inlines_into_compiled_md() {
    let (_d, md) = compile_fixture("const_string.glyph");
    assert!(
        md.contains("- Hello, world."),
        "expected string const value in ### Constraints, got:\n{}",
        md
    );
}

#[test]
fn const_int_inlines_into_compiled_md() {
    // Inferer's no-`.` → Int rule (kind_infer.rs).
    let (_d, md) = compile_fixture("const_int.glyph");
    assert!(
        md.contains("- 3"),
        "expected int const value `3` in ### Constraints, got:\n{}",
        md
    );
}

#[test]
fn const_float_inlines_into_compiled_md() {
    // Inferer's `.`-present → Float rule (kind_infer.rs).
    let (_d, md) = compile_fixture("const_float.glyph");
    assert!(
        md.contains("- 0.001"),
        "expected float const value `0.001` in ### Constraints, got:\n{}",
        md
    );
}

#[test]
fn const_bool_inlines_into_compiled_md() {
    let (_d, md) = compile_fixture("const_bool.glyph");
    assert!(
        md.contains("- True."),
        "expected bool const value `True.` in ### Constraints, got:\n{}",
        md
    );
}

#[test]
fn const_bool_uppercase_normalizes_to_lowercase() {
    // Per `design/values-and-names.md` §Booleans, mixed/upper-case bool
    // literals (`True`, `TRUE`) normalize to lowercase `true` in IR. Chunk 4
    // applies the normalization at the lowering boundary
    // (`lower::collect_consts`), so the rendered value reaching emit is
    // always lowercase regardless of source-text casing.
    // The four-form renderer (Soft/Require) capitalizes and appends a period.
    let (_d, md) = compile_fixture("const_bool_uppercase.glyph");
    assert!(
        md.contains("- True."),
        "expected bool const value normalized to `True.` in ### Constraints, got:\n{}",
        md
    );
}
