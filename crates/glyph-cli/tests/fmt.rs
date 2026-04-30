//! Integration tests for `glyph fmt`.

use std::path::PathBuf;
use std::process::{Command, Output};

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn fmt_corpus_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("fmt")
        .join(name)
}

fn run_fmt(file: &PathBuf) -> Output {
    Command::new(glyph_bin())
        .arg("fmt")
        .arg(file)
        .output()
        .expect("failed to spawn glyph binary")
}

fn run_fmt_check(file: &PathBuf) -> Output {
    Command::new(glyph_bin())
        .arg("fmt")
        .arg("--check")
        .arg(file)
        .output()
        .expect("failed to spawn glyph binary")
}

#[test]
fn fmt_rewrites_tabs_to_four_spaces() {
    let src = fmt_corpus_path("tabs.glyph.md");
    // Copy to a temp file so we don't mutate the corpus.
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    std::fs::copy(&src, &tmp_path).unwrap();

    let output = run_fmt(&tmp_path);
    assert!(output.status.success(), "glyph fmt should exit 0; stderr: {}", String::from_utf8_lossy(&output.stderr));

    let result = std::fs::read_to_string(&tmp_path).unwrap();
    assert!(!result.contains('\t'), "tabs should be replaced with spaces");
    assert!(result.contains("    description:"), "tabs should become 4 spaces");
    assert!(result.contains("        \"Find the bug.\""), "nested tabs should become 8 spaces");
}

#[test]
fn fmt_hoists_body_constraints_into_constraints_section() {
    let src = fmt_corpus_path("body_constraints.glyph.md");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    std::fs::copy(&src, &tmp_path).unwrap();

    let output = run_fmt(&tmp_path);
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let result = std::fs::read_to_string(&tmp_path).unwrap();
    // Body-level markers should be gone.
    // They should now be inside a constraints: section.
    assert!(result.contains("    constraints:\n"), "should have a constraints: section");
    assert!(result.contains("        require accuracy"), "require should be inside constraints:");
    assert!(result.contains("        avoid stale_references"), "avoid should be inside constraints:");
    // The markers should NOT appear at body level (indent 1).
    let lines: Vec<&str> = result.lines().collect();
    for line in &lines {
        let trimmed = line.trim();
        if trimmed == "require accuracy" || trimmed == "avoid stale_references" {
            let indent = line.len() - line.trim_start().len();
            assert_eq!(indent, 8, "constraint markers should be at indent 2 (8 spaces), inside constraints:");
        }
    }
}

#[test]
fn fmt_hoists_body_and_flow_context_into_context_section() {
    let src = fmt_corpus_path("body_context.glyph.md");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    std::fs::copy(&src, &tmp_path).unwrap();

    let output = run_fmt(&tmp_path);
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let result = std::fs::read_to_string(&tmp_path).unwrap();
    // Should have a context: section with all three entries.
    assert!(result.contains("    context:\n"), "should have a context: section");
    assert!(result.contains("        project_conventions"), "body-level context ref should be hoisted");
    assert!(result.contains("        \"Always check for security vulnerabilities.\""), "body-level inline context should be hoisted");
    assert!(result.contains("        repo_layout"), "flow-top-level context should be hoisted");
    // Flow should NOT contain `context repo_layout` anymore.
    let flow_start = result.find("    flow:\n").expect("should have flow:");
    let flow_section = &result[flow_start..];
    assert!(!flow_section.contains("context repo_layout"), "flow-top-level context should be removed from flow");
}

#[test]
fn fmt_preserves_branch_scoped_markers() {
    let src = fmt_corpus_path("branch_scoped.glyph.md");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    std::fs::copy(&src, &tmp_path).unwrap();

    let output = run_fmt(&tmp_path);
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let result = std::fs::read_to_string(&tmp_path).unwrap();
    // Branch-scoped markers should remain inside the branch.
    let flow_start = result.find("    flow:\n").expect("should have flow:");
    let flow_section = &result[flow_start..];
    assert!(flow_section.contains("require safety_checks"), "branch-scoped constraint should stay in branch");
    assert!(flow_section.contains("context production_config"), "branch-scoped context should stay in branch");
    // Should NOT have a top-level constraints: or context: section created from branch markers.
    assert!(!result.contains("    constraints:\n"), "branch-scoped constraints should not create a constraints: section");
    assert!(!result.contains("    context:\n"), "branch-scoped context should not create a context: section");
}

#[test]
fn fmt_reorders_sections_to_canonical_layout() {
    let src = fmt_corpus_path("noncanonical_order.glyph.md");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    std::fs::copy(&src, &tmp_path).unwrap();

    let output = run_fmt(&tmp_path);
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let result = std::fs::read_to_string(&tmp_path).unwrap();
    // Canonical order: description, effects, context, constraints, flow.
    let desc_pos = result.find("    description:").expect("should have description:");
    let effects_pos = result.find("    effects:").expect("should have effects:");
    let context_pos = result.find("    context:").expect("should have context:");
    let constraints_pos = result.find("    constraints:").expect("should have constraints:");
    let flow_pos = result.find("    flow:").expect("should have flow:");
    assert!(desc_pos < effects_pos, "description should come before effects");
    assert!(effects_pos < context_pos, "effects should come before context");
    assert!(context_pos < constraints_pos, "context should come before constraints");
    assert!(constraints_pos < flow_pos, "constraints should come before flow");
}

#[test]
fn fmt_check_exits_1_when_changes_needed() {
    let src = fmt_corpus_path("body_constraints.glyph.md");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    std::fs::copy(&src, &tmp_path).unwrap();

    let before = std::fs::read_to_string(&tmp_path).unwrap();
    let output = run_fmt_check(&tmp_path);
    let after = std::fs::read_to_string(&tmp_path).unwrap();

    assert_eq!(output.status.code(), Some(1), "--check should exit 1 when changes needed");
    assert_eq!(before, after, "--check should not modify the file");
}

#[test]
fn fmt_check_exits_0_when_already_formatted() {
    // First format the file, then run --check on the result.
    let src = fmt_corpus_path("body_constraints.glyph.md");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    std::fs::copy(&src, &tmp_path).unwrap();

    // Format first.
    let output = run_fmt(&tmp_path);
    assert!(output.status.success());

    // Now --check should exit 0.
    let output = run_fmt_check(&tmp_path);
    assert_eq!(output.status.code(), Some(0), "--check should exit 0 when already formatted");
}

#[test]
fn fmt_is_idempotent() {
    // Use the most complex fixture (non-canonical order).
    let src = fmt_corpus_path("noncanonical_order.glyph.md");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    std::fs::copy(&src, &tmp_path).unwrap();

    // First format.
    let output = run_fmt(&tmp_path);
    assert!(output.status.success());
    let first_result = std::fs::read_to_string(&tmp_path).unwrap();

    // Second format.
    let output = run_fmt(&tmp_path);
    assert!(output.status.success());
    let second_result = std::fs::read_to_string(&tmp_path).unwrap();

    assert_eq!(first_result, second_result, "formatting should be idempotent");
    // --check should confirm no changes needed.
    let output = run_fmt_check(&tmp_path);
    assert_eq!(output.status.code(), Some(0), "--check after two formats should exit 0");
}
