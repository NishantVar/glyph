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
fn fmt_preserves_generated_const_and_skill_when_mixed() {
    // Regression: chunk 4's `decl_starts` recognizer didn't include the
    // `generated ` prefix, so a `generated const` line at top level was
    // absorbed into the previous decl's range. With the bug, fmt's section
    // reorder (which moves `flow:` after `description:`) inserts the
    // generated-const line BETWEEN description and flow, corrupting both
    // the skill body layout and the generated-const placement. The pin:
    // the generated const must end up AFTER the skill's flow body.
    let src = fmt_corpus_path("generated_const_after_skill.glyph.md");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    std::fs::copy(&src, &tmp_path).unwrap();

    let output = run_fmt(&tmp_path);
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let result = std::fs::read_to_string(&tmp_path).unwrap();
    // Skill body must remain intact and reorder flow after description.
    assert!(result.contains("skill demo()"), "skill header preserved");
    let desc_pos = result.find("    description: \"Demo skill.\"").expect("skill description preserved");
    let flow_pos = result.find("    flow:").expect("skill flow section preserved");
    let step_pos = result.find("        \"step one\"").expect("skill flow body preserved");
    let generated_pos = result
        .find("generated const helper_text = \"Generated helper string.\"")
        .expect("generated const declaration should pass through unchanged");
    // Canonical order inside skill: description before flow.
    assert!(desc_pos < flow_pos, "description should come before flow in skill body");
    // The bug: generated const lands between description and flow.
    // Fix asserts: generated const must be AFTER the skill body's last line.
    assert!(
        generated_pos > step_pos,
        "generated const must follow the entire skill body, not be embedded inside it; got:\n{}",
        result,
    );

    // Idempotency: running fmt again must not change anything.
    let first = result.clone();
    let output = run_fmt(&tmp_path);
    assert!(output.status.success());
    let second = std::fs::read_to_string(&tmp_path).unwrap();
    assert_eq!(first, second, "fmt should be idempotent on mixed generated-const + skill");
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

#[test]
fn fmt_strips_legacy_none_return_type() {
    // Issue #82 AC5: the deterministic repair pass rewrites legacy
    // `-> None` (case-insensitive) headers by omitting `->` entirely.
    // Multi-decl fixture exercises:
    // - skill header `-> None` stripped
    // - private block header `-> None` stripped
    // - valid `-> Path` header preserved
    // - body `return none` (value-position keyword) untouched
    let src = fmt_corpus_path("legacy_none_return.glyph.md");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    std::fs::copy(&src, &tmp_path).unwrap();

    let output = run_fmt(&tmp_path);
    assert!(
        output.status.success(),
        "glyph fmt should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let result = std::fs::read_to_string(&tmp_path).unwrap();

    // Legacy headers must be stripped of `-> None`.
    assert!(
        !result.contains("-> None"),
        "no `-> None` should remain after fmt, got:\n{}",
        result
    );
    assert!(
        !result.contains("-> none"),
        "no `-> none` should remain after fmt, got:\n{}",
        result
    );
    assert!(
        result.contains("skill cleanup()\n"),
        "skill header should be stripped to `skill cleanup()`, got:\n{}",
        result
    );
    assert!(
        result.contains("block helper()\n"),
        "block header should be stripped to `block helper()`, got:\n{}",
        result
    );

    // Valid `-> Path` header must survive.
    assert!(
        result.contains("export block compute(scope = \".\") -> Path"),
        "valid `-> Path` header must be preserved, got:\n{}",
        result
    );

    // Body `return none` value-keyword must be untouched.
    assert!(
        result.contains("return none"),
        "body `return none` should remain untouched, got:\n{}",
        result
    );

    // Idempotence: a second fmt produces byte-identical output.
    let output2 = run_fmt(&tmp_path);
    assert!(
        output2.status.success(),
        "second fmt should exit 0; stderr: {}",
        String::from_utf8_lossy(&output2.stderr)
    );
    let result2 = std::fs::read_to_string(&tmp_path).unwrap();
    assert_eq!(
        result, result2,
        "fmt should be idempotent on legacy `-> None` source"
    );

    // --check after the strip should exit 0 (no further changes needed).
    let check_output = run_fmt_check(&tmp_path);
    assert_eq!(
        check_output.status.code(),
        Some(0),
        "--check after fmt should exit 0; stderr: {}",
        String::from_utf8_lossy(&check_output.stderr)
    );

    // AC7-3: the rewritten multi-decl source must parse + analyze cleanly via
    // `glyph check` (exit 0) — confirms the repair pass produces source that
    // flows through Phase 1 + Phase 2 without surfacing the original
    // `G::parse::none-as-return-type` diagnostic.
    let glyph_check_output = Command::new(glyph_bin())
        .arg("check")
        .arg(&tmp_path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("failed to spawn glyph binary for post-fmt check");
    assert_eq!(
        glyph_check_output.status.code(),
        Some(0),
        "post-fmt `glyph check` must exit 0; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&glyph_check_output.stdout),
        String::from_utf8_lossy(&glyph_check_output.stderr),
    );
    let post_fmt_stdout = String::from_utf8_lossy(&glyph_check_output.stdout);
    assert!(
        !post_fmt_stdout.contains("G::parse::none-as-return-type"),
        "post-fmt `glyph check` stdout must not contain G::parse::none-as-return-type, got:\n{}",
        post_fmt_stdout,
    );
}
