//! Slice 5 integration tests — constraints, context, text declarations, and
//! `### Constraints` + `### Context` sections.
//!
//! Covers the acceptance criteria from `mvp-issues.md` slice 5.

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

fn run_compile(path: PathBuf, format: &str) -> Output {
    Command::new(glyph_bin())
        .arg("compile")
        .arg(path)
        .arg("--format")
        .arg(format)
        .output()
        .expect("failed to spawn glyph binary")
}

fn run_check(path: PathBuf, format: &str) -> Output {
    Command::new(glyph_bin())
        .arg("check")
        .arg(path)
        .arg("--format")
        .arg(format)
        .output()
        .expect("failed to spawn glyph binary")
}

fn assert_has_diagnostic_id(stdout: &str, id: &str) {
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
    assert!(
        found,
        "expected diagnostic `{}` in JSON output, got:\n{}",
        id, stdout
    );
}

// --- Acceptance criterion 1: constraint_only.glyph.md compiles, emits
// ### Constraints only (no ### Steps) ---

#[test]
fn constraint_only_compiles_with_constraints_no_steps() {
    let src = fixture("valid", "constraint_only.glyph.md");
    let out = src.with_file_name("constraint_only.md");
    let _ = std::fs::remove_file(&out);

    let result = run_compile(src, "json");
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );

    let md = std::fs::read_to_string(&out).expect("compiled .md file is missing");
    assert!(
        md.contains("### Constraints"),
        "expected ### Constraints section; got:\n{}",
        md
    );
    assert!(
        !md.contains("### Steps"),
        "expected no ### Steps section for constraint-only skill; got:\n{}",
        md
    );
}

// --- Acceptance criterion 2: require accuracy + text resolution ---

#[test]
fn require_text_resolves_and_renders_constraint() {
    // update_docs.glyph.md has `require accuracy` + `text accuracy = "..."`.
    let src = fixture("valid", "update_docs.glyph.md");
    let out = src.with_file_name("update_docs.md");
    let _ = std::fs::remove_file(&out);

    let result = run_compile(src, "json");
    assert_eq!(result.status.code(), Some(0));

    let md = std::fs::read_to_string(&out).expect("compiled .md file is missing");
    assert!(
        md.contains("Ensure all documentation accurately reflects the current code."),
        "expected resolved text content in ### Constraints; got:\n{}",
        md
    );
}

// --- Acceptance criterion 3: body-level avoid X hoists into constraints ---

#[test]
fn body_level_avoid_hoists_to_constraints_section() {
    // update_docs.glyph.md has `avoid stale_references` at body level.
    let src = fixture("valid", "update_docs.glyph.md");
    let out = src.with_file_name("update_docs.md");
    let _ = std::fs::remove_file(&out);

    let result = run_compile(src, "json");
    assert_eq!(result.status.code(), Some(0));

    let md = std::fs::read_to_string(&out).expect("compiled .md file is missing");
    // Avoid polarity should render as "Do not ..." phrasing.
    assert!(
        md.contains("### Constraints"),
        "expected ### Constraints section; got:\n{}",
        md
    );
    assert!(
        md.contains("Do not leave references to removed or renamed symbols."),
        "expected avoid-polarity constraint phrasing; got:\n{}",
        md
    );
}

// --- Acceptance criterion 4: context: sub-section emits ### Context before ### Steps ---

#[test]
fn context_section_emits_before_steps() {
    let src = fixture("valid", "with_context.glyph.md");
    let out = src.with_file_name("with_context.md");
    let _ = std::fs::remove_file(&out);

    let result = run_compile(src, "json");
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );

    let md = std::fs::read_to_string(&out).expect("compiled .md file is missing");
    let context_idx = md.find("### Context").expect("expected ### Context section");
    let steps_idx = md.find("### Steps").expect("expected ### Steps section");
    assert!(
        context_idx < steps_idx,
        "### Context must appear before ### Steps; got:\n{}",
        md
    );
}

// --- Acceptance criterion 7: text name referenced from context: resolves ---

#[test]
fn text_in_context_resolves_to_string() {
    let src = fixture("valid", "with_context.glyph.md");
    let out = src.with_file_name("with_context.md");
    let _ = std::fs::remove_file(&out);

    let result = run_compile(src, "json");
    assert_eq!(result.status.code(), Some(0));

    let md = std::fs::read_to_string(&out).expect("compiled .md file is missing");
    assert!(
        md.contains("This codebase uses a monorepo layout with per-crate Cargo.toml files."),
        "expected resolved text in ### Context; got:\n{}",
        md
    );
    assert!(
        md.contains("The bug is assumed to be reproducible locally."),
        "expected inline string in ### Context; got:\n{}",
        md
    );
}
