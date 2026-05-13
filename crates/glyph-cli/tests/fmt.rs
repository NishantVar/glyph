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

fn run_fmt_with_effects(file: &PathBuf) -> Output {
    Command::new(glyph_bin())
        .arg("--enable-effects")
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

fn run_fmt_check_with_effects(file: &PathBuf) -> Output {
    Command::new(glyph_bin())
        .arg("--enable-effects")
        .arg("fmt")
        .arg("--check")
        .arg(file)
        .output()
        .expect("failed to spawn glyph binary")
}

#[test]
fn fmt_rewrites_tabs_to_four_spaces() {
    let src = fmt_corpus_path("tabs.glyph");
    // Copy to a temp file so we don't mutate the corpus.
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
    assert!(
        !result.contains('\t'),
        "tabs should be replaced with spaces"
    );
    assert!(
        result.contains("    description:"),
        "tabs should become 4 spaces"
    );
    assert!(
        result.contains("        \"Find the bug.\""),
        "nested tabs should become 8 spaces"
    );
}

#[test]
fn fmt_preserves_body_constraint_markers_at_body_level() {
    // Per D9 / D10: fmt does not hoist body-level constraint markers into a
    // `constraints:` section. The emit-side handles synthetic-slot placement
    // (so the compiled `## Constraints` heading still lands in canonical
    // position), but fmt preserves the source verbatim.
    let src = fmt_corpus_path("body_constraints.glyph");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    std::fs::copy(&src, &tmp_path).unwrap();

    let output = run_fmt(&tmp_path);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let result = std::fs::read_to_string(&tmp_path).unwrap();
    // fmt must NOT have synthesized a `constraints:` host section.
    assert!(
        !result.contains("    constraints:\n"),
        "fmt should not synthesize a constraints: section; got:\n{}",
        result
    );
    // Markers should remain at body level (indent 1, four spaces).
    assert!(
        result.contains("    require accuracy\n"),
        "body-level `require` should be preserved at indent 1; got:\n{}",
        result
    );
    assert!(
        result.contains("    avoid stale_references\n"),
        "body-level `avoid` should be preserved at indent 1; got:\n{}",
        result
    );
}

#[test]
fn fmt_preserves_body_and_flow_context_markers() {
    // Per D9 / D10: fmt does not hoist body-level or flow-top-level context
    // markers. The compile-time emit path handles synthetic-section slot
    // placement; fmt leaves the source markers in place.
    let src = fmt_corpus_path("body_context.glyph");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    std::fs::copy(&src, &tmp_path).unwrap();

    let output = run_fmt(&tmp_path);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let result = std::fs::read_to_string(&tmp_path).unwrap();
    // fmt must NOT have synthesized a `context:` host section.
    assert!(
        !result.contains("    context:\n"),
        "fmt should not synthesize a context: section; got:\n{}",
        result
    );
    // Body-level markers preserved at indent 1.
    assert!(
        result.contains("    context project_conventions\n"),
        "body-level context ref should remain at body level; got:\n{}",
        result
    );
    assert!(
        result.contains("    context \"Always check for security vulnerabilities.\"\n"),
        "body-level inline context should remain at body level; got:\n{}",
        result
    );
    // Flow-top-level marker remains inside flow (indent 2).
    assert!(
        result.contains("        context repo_layout\n"),
        "flow-top-level context should remain inside flow; got:\n{}",
        result
    );
}

#[test]
fn fmt_preserves_branch_scoped_markers() {
    let src = fmt_corpus_path("branch_scoped.glyph");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    std::fs::copy(&src, &tmp_path).unwrap();

    let output = run_fmt(&tmp_path);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let result = std::fs::read_to_string(&tmp_path).unwrap();
    // Branch-scoped markers should remain inside the branch.
    let flow_start = result.find("    flow:\n").expect("should have flow:");
    let flow_section = &result[flow_start..];
    assert!(
        flow_section.contains("require safety_checks"),
        "branch-scoped constraint should stay in branch"
    );
    assert!(
        flow_section.contains("context production_config"),
        "branch-scoped context should stay in branch"
    );
    // Should NOT have a top-level constraints: or context: section created from branch markers.
    assert!(
        !result.contains("    constraints:\n"),
        "branch-scoped constraints should not create a constraints: section"
    );
    assert!(
        !result.contains("    context:\n"),
        "branch-scoped context should not create a context: section"
    );
}

#[test]
fn fmt_preserves_section_source_order() {
    // Per D9/D10: `glyph fmt` preserves the author's section order. The
    // canonical default order is a presentation-layer concern handled by
    // the emitter; the formatter does not reorder.
    let src = fmt_corpus_path("noncanonical_order.glyph");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    std::fs::copy(&src, &tmp_path).unwrap();

    let output = run_fmt_with_effects(&tmp_path);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let result = std::fs::read_to_string(&tmp_path).unwrap();
    // Source order in the fixture is flow, constraints, effects, context,
    // description. fmt must preserve that order.
    let flow_pos = result.find("    flow:").expect("should have flow:");
    let constraints_pos = result
        .find("    constraints:")
        .expect("should have constraints:");
    let effects_pos = result.find("    effects:").expect("should have effects:");
    let context_pos = result.find("    context:").expect("should have context:");
    let desc_pos = result
        .find("    description:")
        .expect("should have description:");
    assert!(
        flow_pos < constraints_pos,
        "source order: flow should come before constraints; got:\n{}",
        result
    );
    assert!(
        constraints_pos < effects_pos,
        "source order: constraints should come before effects; got:\n{}",
        result
    );
    assert!(
        effects_pos < context_pos,
        "source order: effects should come before context; got:\n{}",
        result
    );
    assert!(
        context_pos < desc_pos,
        "source order: context should come before description; got:\n{}",
        result
    );
}

#[test]
fn fmt_check_exits_1_when_changes_needed() {
    // `tabs.glyph` uses tab indentation; fmt rewrites tabs to four spaces, so
    // `--check` must report a needed change.
    let src = fmt_corpus_path("tabs.glyph");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    std::fs::copy(&src, &tmp_path).unwrap();

    let before = std::fs::read_to_string(&tmp_path).unwrap();
    let output = run_fmt_check(&tmp_path);
    let after = std::fs::read_to_string(&tmp_path).unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "--check should exit 1 when changes needed"
    );
    assert_eq!(before, after, "--check should not modify the file");
}

#[test]
fn fmt_check_exits_0_when_already_formatted() {
    // First format the file, then run --check on the result.
    let src = fmt_corpus_path("tabs.glyph");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    std::fs::copy(&src, &tmp_path).unwrap();

    // Format first.
    let output = run_fmt(&tmp_path);
    assert!(output.status.success());

    // Now --check should exit 0.
    let output = run_fmt_check(&tmp_path);
    assert_eq!(
        output.status.code(),
        Some(0),
        "--check should exit 0 when already formatted"
    );
}

#[test]
fn fmt_preserves_generated_const_and_skill_when_mixed() {
    // Regression: chunk 4's `decl_starts` recognizer didn't include the
    // `generated ` prefix, so a `generated const` line at top level was
    // absorbed into the previous decl's range. With the bug, the
    // generated-const line was inserted between sections of the skill body,
    // corrupting both the skill body layout and the generated-const
    // placement. The pin: the generated const must end up AFTER the skill's
    // entire body, never embedded inside it.
    let src = fmt_corpus_path("generated_const_after_skill.glyph");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    std::fs::copy(&src, &tmp_path).unwrap();

    let output = run_fmt(&tmp_path);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let result = std::fs::read_to_string(&tmp_path).unwrap();
    // Skill body must remain intact.
    assert!(result.contains("skill demo()"), "skill header preserved");
    let _desc_pos = result
        .find("    description: \"Demo skill.\"")
        .expect("skill description preserved");
    let _flow_pos = result
        .find("    flow:")
        .expect("skill flow section preserved");
    let step_pos = result
        .find("        \"step one\"")
        .expect("skill flow body preserved");
    let generated_pos = result
        .find("generated const helper_text = \"Generated helper string.\"")
        .expect("generated const declaration should pass through unchanged");
    // The bug: generated const lands inside the skill body.
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
    assert_eq!(
        first, second,
        "fmt should be idempotent on mixed generated-const + skill"
    );
}

#[test]
fn fmt_is_idempotent() {
    // Use the most complex fixture (non-canonical order).
    let src = fmt_corpus_path("noncanonical_order.glyph");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    std::fs::copy(&src, &tmp_path).unwrap();

    // First format. Fixture contains an `effects:` section, so we need
    // `--enable-effects` for the parser to accept it.
    let output = run_fmt_with_effects(&tmp_path);
    assert!(output.status.success());
    let first_result = std::fs::read_to_string(&tmp_path).unwrap();

    // Second format.
    let output = run_fmt_with_effects(&tmp_path);
    assert!(output.status.success());
    let second_result = std::fs::read_to_string(&tmp_path).unwrap();

    assert_eq!(
        first_result, second_result,
        "formatting should be idempotent"
    );
    // --check should confirm no changes needed.
    let output = run_fmt_check_with_effects(&tmp_path);
    assert_eq!(
        output.status.code(),
        Some(0),
        "--check after two formats should exit 0"
    );
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
    let src = fmt_corpus_path("legacy_none_return.glyph");
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

/// Regression: `glyph fmt --enable-effects` must preserve `effects:` sections.
///
/// Prior to the fix, `run_fmt` did not thread `--enable-effects` into
/// `fmt_source`, which in turn called `parse::parse_with_diagnostics` (the
/// legacy entry that hardcodes `enable_effects=false`). The parser rejected
/// `effects:`, returned `None`, and fmt silently fell back to the pre-parse
/// stratum — leaving the effects section intact only because no AST rewrite
/// happened. Worse, on a tabs fixture or any fixture needing AST rewrite,
/// the effects section would be dropped on the next round-trip through fmt.
///
/// This test exercises the full path: fmt with `--enable-effects` over a
/// source containing `effects:`. It must (a) succeed and (b) preserve the
/// `effects:` section in its source position.
#[test]
fn fmt_with_enable_effects_preserves_effects_section() {
    let src = fmt_corpus_path("noncanonical_order.glyph");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    std::fs::copy(&src, &tmp_path).unwrap();

    // Sanity: fixture must contain `effects:` for this regression to be
    // meaningful.
    let before = std::fs::read_to_string(&tmp_path).unwrap();
    assert!(
        before.contains("effects:"),
        "fixture must contain `effects:` for this regression test to be meaningful",
    );

    let output = run_fmt_with_effects(&tmp_path);
    assert!(
        output.status.success(),
        "glyph fmt --enable-effects should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    let after = std::fs::read_to_string(&tmp_path).unwrap();
    assert!(
        after.contains("    effects:"),
        "effects: section must be preserved after fmt --enable-effects; got:\n{}",
        after,
    );

    // Per D9/D10 fmt preserves the source order. In the fixture, the source
    // order is flow, constraints, effects, context, description; verify that
    // the effects section sits between constraints and context.
    let constraints_pos = after
        .find("    constraints:")
        .expect("should have constraints:");
    let effects_pos = after.find("    effects:").expect("should have effects:");
    let context_pos = after.find("    context:").expect("should have context:");
    assert!(
        constraints_pos < effects_pos && effects_pos < context_pos,
        "effects: must be preserved in source order between constraints: and context:; got:\n{}",
        after,
    );
}

#[test]
fn multi_autofix_converges() {
    let input = include_str!("corpus/fmt/multi_autofix_input.glyph");
    let expected = include_str!("corpus/fmt/multi_autofix_expected.glyph");

    // Step 1: fmt produces the expected canonical source.
    let result = glyph_core::fmt::fmt_source(input, true);
    assert_eq!(result.output, expected, "fmt output mismatch");

    // Step 2: the post-fmt source must analyze cleanly (no repairables).
    // Use the import-aware `check_file_with_effects` path (what `glyph check`
    // uses) so stdlib import names actually populate `block_names`. The
    // single-file `analyze_with_diagnostics` entry does not resolve imports
    // and would spuriously flag `send`/`subagent` as missing imports even
    // though they ARE imported on the post-fmt source.
    let dir = tempfile::tempdir().unwrap();
    let tmp_path = dir.path().join("multi_autofix.glyph");
    std::fs::write(&tmp_path, expected).unwrap();
    let bag = glyph_core::check_file_with_effects(&tmp_path, true);
    let repairable: Vec<_> = bag
        .iter()
        .filter(|d| d.classification == glyph_core::diagnostic::Classification::Repairable)
        .collect();
    assert!(
        repairable.is_empty(),
        "expected no repairable diagnostics on post-fmt source, got: {:?}",
        repairable
    );
}

/// Regression for the four-case hoisting rule (see
/// `design/language-surface.md` §4.2a and `GLYPH_LANGUAGE_GUIDE.md` §7.2):
/// a marker placed inside another named section (case 4) must stay scoped to
/// that section. `glyph fmt` must not move markers across section boundaries.
#[test]
fn fmt_does_not_hoist_markers_out_of_context_section() {
    let src = "skill demo()\n    description: \"Demo.\"\n    context:\n        context project_conventions\n        require accuracy\n    flow:\n        \"Do.\"\n";
    // enable_effects=false: the four-case hoisting rule is independent of
    // effects expansion; no need to gate this test on `--enable-effects`.
    let result = glyph_core::fmt::fmt_source(src, false);
    // A `require` marker placed inside `context:` is case 4 of the four-case
    // hoisting rule (stays scoped to the section). `glyph fmt` must not move
    // it to body-level (which would change its compiled output destination
    // from `context:` to `constraints:`). Byte-for-byte preservation is the
    // strongest assertion: if the input is already canonical w.r.t. the
    // four-case rule, fmt is a no-op.
    assert_eq!(
        result.output, src,
        "fmt must not rewrite a marker-in-context source"
    );
    assert!(
        !result.changed,
        "fmt should report no change for marker-in-context source"
    );
}

/// Finding 1 regression: fmt must not synthesize a `constraints:` host
/// section when the only constraint marker is at body level. The compile
/// pipeline already routes body-level markers into the synthetic `##
/// Constraints` slot (D9 canonical slot 3), which lands BEFORE `## Steps`
/// (slot 5). Synthesizing an explicit `constraints:` section at end-of-body
/// would anchor it AFTER `flow:` in source order, flipping the compiled
/// output so `## Constraints` lands AFTER `## Steps`.
#[test]
fn fmt_does_not_synthesize_constraints_section_for_body_marker() {
    let src = "skill demo()\n    description: \"Demo.\"\n    must avoid foo\n    flow:\n        \"Do.\"\n\nconst foo = \"a foot-gun.\"\n";
    let result = glyph_core::fmt::fmt_source(src, false);
    assert!(
        !result.output.contains("    constraints:\n"),
        "fmt must not synthesize a constraints: section; got:\n{}",
        result.output
    );
    assert!(
        result.output.contains("    must avoid foo\n"),
        "body-level `must avoid` marker should be preserved at body level; got:\n{}",
        result.output
    );

    // End-to-end: compile and verify `## Constraints` lands BEFORE `## Steps`
    // (D9 slot 3 vs. slot 5).
    let dir = tempfile::tempdir().unwrap();
    let src_path = dir.path().join("demo.glyph");
    std::fs::write(&src_path, &result.output).unwrap();

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src_path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("failed to spawn glyph binary");
    assert!(
        output.status.success(),
        "compile should succeed; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let md = std::fs::read_to_string(dir.path().join("demo.md"))
        .expect("compiled .md file should exist");
    let constraints_pos = md
        .find("## Constraints")
        .expect("expected ## Constraints heading in compiled output");
    let steps_pos = md
        .find("## Steps")
        .expect("expected ## Steps heading in compiled output");
    assert!(
        constraints_pos < steps_pos,
        "D9 slot 3 (## Constraints) must precede slot 5 (## Steps); got:\n{}",
        md
    );
}
