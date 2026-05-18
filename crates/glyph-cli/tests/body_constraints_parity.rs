use std::path::PathBuf;
use std::process::Command;

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("valid")
}

fn compile_to_tempdir(fixture_stem: &str) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let src = corpus_dir().join(format!("{}.glyph", fixture_stem));

    let result = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src)
        .arg("--out-dir")
        .arg(dir.path())
        .output()
        .expect("failed to spawn glyph binary");

    assert!(
        result.status.success(),
        "glyph compile failed for {}. stderr={}",
        fixture_stem,
        String::from_utf8_lossy(&result.stderr),
    );
    let md_path = dir.path().join(format!("{}.md", fixture_stem));
    let md = std::fs::read_to_string(&md_path)
        .unwrap_or_else(|e| panic!("compiled .md missing at {:?}: {}", md_path, e));
    (dir, md)
}

fn compile_tier3_to_tempdir(fixture_stem: &str, proc_kebab: &str) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let src = corpus_dir().join(format!("{}.glyph", fixture_stem));

    let result = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src)
        .arg("--out-dir")
        .arg(dir.path())
        .output()
        .expect("failed to spawn glyph binary");

    assert!(
        result.status.success(),
        "glyph compile failed for {}. stderr={}",
        fixture_stem,
        String::from_utf8_lossy(&result.stderr),
    );
    let md_path = dir
        .path()
        .join(fixture_stem)
        .join(format!("{}.md", proc_kebab));
    let md = std::fs::read_to_string(&md_path)
        .unwrap_or_else(|e| panic!("Tier 3 procedure .md missing at {:?}: {}", md_path, e));
    (dir, md)
}

#[test]
fn tier2_block_with_body_constraints_emits_preamble() {
    let (_dir, md) = compile_to_tempdir("tier2_block_body_constraints");

    assert!(
        md.contains("### Procedure: careful-review"),
        "expected `### Procedure: careful-review` section (Tier 2 promotion); got:\n{md}"
    );
    assert!(
        md.contains("**Must:** preserving existing behavior."),
        "expected `**Must:** preserving existing behavior.` preamble; got:\n{md}"
    );
    assert!(
        md.contains("**Require:** logging each decision with a rationale."),
        "expected `**Require:** logging each decision with a rationale.` preamble; got:\n{md}"
    );
    assert!(
        md.contains("**Avoid:** silently swallowing errors without surfacing them."),
        "expected `**Avoid:** silently swallowing errors without surfacing them.` preamble; got:\n{md}"
    );
    // Gap 1: cover the fourth marker form (`must avoid`) on the same fixture.
    assert!(
        md.contains("**Must avoid:** unrelated refactors outside the requested scope."),
        "expected `**Must avoid:** unrelated refactors outside the requested scope.` preamble; got:\n{md}"
    );

    let head = md
        .find("### Procedure: careful-review")
        .expect("heading missing");
    let must = md
        .find("**Must:** preserving")
        .expect("Must preamble missing");
    let step1 = md.find("1. Scan the code.").expect("step 1 missing");
    assert!(head < must, "preamble must appear after the heading");
    assert!(must < step1, "preamble must appear before step 1");
}

#[test]
fn tier2_block_with_body_context_named_emits_kebab_label() {
    let (_dir, md) = compile_to_tempdir("tier2_block_body_context");

    assert!(
        md.contains("### Procedure: scoped-review"),
        "expected `### Procedure: scoped-review` section (Tier 2 promotion); got:\n{md}"
    );
    assert!(
        md.contains("**monorepo-layout:** this codebase is a Rust workspace with three crates."),
        "expected named-context preamble using kebab-cased const name; got:\n{md}"
    );
    assert!(
        md.contains("**workspace-conventions:** follow the workspace's existing conventions."),
        "expected named-context preamble using kebab-cased const name; got:\n{md}"
    );

    let head = md
        .find("### Procedure: scoped-review")
        .expect("heading missing");
    let ctx1 = md
        .find("**monorepo-layout:**")
        .expect("first context preamble missing");
    let step1 = md
        .find("1. Open the relevant crate.")
        .expect("step 1 missing");
    assert!(
        head < ctx1,
        "context preamble must appear after the heading"
    );
    assert!(ctx1 < step1, "context preamble must appear before step 1");
}

#[test]
fn tier3_export_block_with_body_constraints_emits_preamble_in_standalone() {
    let (_dir, md) = compile_tier3_to_tempdir("tier3_export_body_constraints", "thorough-review");

    assert!(
        md.contains("kind: procedure"),
        "Tier 3 standalone must carry `kind: procedure` frontmatter; got:\n{md}"
    );
    assert!(
        md.contains("**Must:** preserving existing behavior across refactors."),
        "expected `**Must:**` preamble in Tier 3 standalone; got:\n{md}"
    );
    assert!(
        md.contains("**Require:** logging each decision with a clear rationale."),
        "expected `**Require:**` preamble in Tier 3 standalone; got:\n{md}"
    );

    let must = md.find("**Must:** preserving").expect("Must missing");
    let step1 = md
        .find("1. Read the project structure")
        .expect("step 1 missing");
    assert!(must < step1, "preamble must appear before step 1");
}

#[test]
fn tier3_export_block_with_body_context_named_emits_kebab_label() {
    let (_dir, md) = compile_tier3_to_tempdir("tier3_export_body_constraints", "thorough-review");

    assert!(
        md.contains("**monorepo-layout:** this codebase is a Rust workspace with multiple crates."),
        "expected named-context preamble in Tier 3 standalone; got:\n{md}"
    );

    let ctx = md
        .find("**monorepo-layout:**")
        .expect("context preamble missing");
    let step1 = md
        .find("1. Read the project structure")
        .expect("step 1 missing");
    assert!(
        ctx < step1,
        "context preamble must appear before step 1; got:\n{md}"
    );
}

#[test]
fn skill_body_level_constraints_unchanged_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let src = corpus_dir().join("update_docs.glyph");
    let result = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src)
        .arg("--out-dir")
        .arg(dir.path())
        .output()
        .expect("spawn failed");
    assert!(result.status.success(), "update_docs compile failed");

    let md =
        std::fs::read_to_string(dir.path().join("update_docs.md")).expect("update_docs.md missing");

    let expected = "---\n\
name: update_docs\n\
description: 'Update repository documentation to match current code.'\n\
---\n\
\n\
## Constraints\n\
\n\
- **Require:** ensure all documentation accurately reflects the current code.\n\
- **Avoid:** leaving references to removed or renamed symbols.\n\
\n\
## Steps\n\
\n\
1. Scan the repository for files with documentation.\n\
2. Compare each document against the current code for accuracy.\n\
3. Update any sections that are outdated or incorrect.\n\
4. Verify all cross-references and links are still valid.\n\
\n";

    assert_eq!(
        md, expected,
        "Skill body-level constraints must remain byte-identical to pre-#168 baseline"
    );
}

#[test]
fn branch_arm_constraint_inlining_unchanged_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("fmt")
        .join("branch_scoped.glyph");
    let result = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src)
        .arg("--out-dir")
        .arg(dir.path())
        .output()
        .expect("spawn failed");
    assert!(result.status.success(), "branch_scoped compile failed");

    let md = std::fs::read_to_string(dir.path().join("branch_scoped.md"))
        .expect("branch_scoped.md missing");

    let expected = "---\n\
name: deploy\n\
description: 'Deploy the application.'\n\
---\n\
\n\
## Steps\n\
\n\
1. Prepare deployment.\n\
2. If env == \"production\":\n   \
a. run all safety checks\n   \
b. Note: Production uses strict settings.\n   \
c. Deploy to production.\n   \
Otherwise:\n   \
a. Deploy to staging.\n\
\n";

    assert_eq!(
        md, expected,
        "Branch-arm constraint inlining must remain byte-identical to pre-#168 baseline"
    );
}

#[test]
fn preamble_does_not_count_toward_validator_steps() {
    let dir = tempfile::tempdir().unwrap();
    let src = corpus_dir().join("tier2_block_body_constraints.glyph");
    let result = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src)
        .arg("--out-dir")
        .arg(dir.path())
        .arg("--emit-ir")
        .output()
        .expect("compile spawn failed");
    assert!(
        result.status.success(),
        "compile w/ --emit-ir failed: stderr={}",
        String::from_utf8_lossy(&result.stderr)
    );

    let md_path = dir.path().join("tier2_block_body_constraints.md");
    let ir_path = dir.path().join("tier2_block_body_constraints.ir.json");

    let v = Command::new(glyph_bin())
        .arg("validate-output")
        .arg(&ir_path)
        .arg(&md_path)
        .output()
        .expect("validate-output spawn failed");

    assert!(
        v.status.success(),
        "validate-output failed (preamble must not count as steps): stderr={}\nstdout={}",
        String::from_utf8_lossy(&v.stderr),
        String::from_utf8_lossy(&v.stdout)
    );
}

#[test]
fn tier2_promotion_fires_only_on_non_empty_body_markers() {
    let (_dir, md) = compile_to_tempdir("explicit_blocks");

    assert!(
        !md.contains("### Procedure: small-helper"),
        "small_helper has no body markers, < 4 stmts, no branches, no freeform — must stay Tier 1; got:\n{md}"
    );
    assert!(
        md.contains("Do a quick check."),
        "small_helper body must inline into Steps; got:\n{md}"
    );
}

/// Gap 5 (Finding 1 regression): a Tier-2 parent block calls a child block
/// at its top level. The child has body `require careful_handoff` (the only
/// Tier-2 trigger — only 1 flow statement, no branches, no freeform). The
/// child must still get its own `### Procedure: child-step` section AND a
/// preamble. This locks in the Finding 1 fix where `classifies_as_tier2`
/// consults `b.constraints` / `b.context`.
#[test]
fn nested_block_promotes_when_only_trigger_is_body_constraint() {
    let (_dir, md) = compile_to_tempdir("tier2_nested_block_promotion");

    assert!(
        md.contains("### Procedure: parent-review"),
        "parent must be Tier 2; got:\n{md}"
    );
    assert!(
        md.contains("### Procedure: child-step"),
        "child must promote to Tier 2 even with only 1 flow stmt because it carries a body constraint; got:\n{md}"
    );
    assert!(
        md.contains("**Require:** leaving a clear note for the next reviewer."),
        "child preamble must render the body constraint; got:\n{md}"
    );

    let parent_head = md
        .find("### Procedure: parent-review")
        .expect("parent heading missing");
    let child_head = md
        .find("### Procedure: child-step")
        .expect("child heading missing");
    let child_preamble = md
        .find("**Require:** leaving a clear note")
        .expect("child preamble missing");
    assert!(
        parent_head < child_head,
        "parent procedure should be emitted before child"
    );
    assert!(
        child_head < child_preamble,
        "child preamble must appear after the child heading"
    );
}

/// Gap 4 (Tier 2): subsection-body forms (`constraints:` and `context:`)
/// declared on a `block` render into the preamble identically to inline
/// marker forms. The block has only 2 flow statements, so promotion to
/// Tier 2 fires solely via the non-empty constraints / context (Finding 1
/// path).
#[test]
fn tier2_block_with_subsection_body_forms_emits_preamble() {
    let (_dir, md) = compile_to_tempdir("tier2_block_subsection_bodies");

    assert!(
        md.contains("### Procedure: subsectioned-review"),
        "expected Tier 2 promotion via subsection bodies; got:\n{md}"
    );
    assert!(
        md.contains("**Require:** logging each decision with a rationale."),
        "expected `**Require:**` from `constraints:` subsection; got:\n{md}"
    );
    assert!(
        md.contains("**Avoid:** silently swallowing errors without surfacing them."),
        "expected `**Avoid:**` from `constraints:` subsection; got:\n{md}"
    );
    assert!(
        md.contains("**Context:** subsection inline context."),
        "expected `**Context:**` from `context:` subsection; got:\n{md}"
    );

    let head = md
        .find("### Procedure: subsectioned-review")
        .expect("heading missing");
    let req = md.find("**Require:**").expect("require preamble missing");
    let avd = md.find("**Avoid:**").expect("avoid preamble missing");
    let ctx = md.find("**Context:**").expect("context preamble missing");
    let step1 = md.find("1. Open the diff.").expect("step 1 missing");
    assert!(head < req, "preamble must appear after heading");
    assert!(req < step1, "require preamble must appear before step 1");
    assert!(avd < step1, "avoid preamble must appear before step 1");
    assert!(ctx < step1, "context preamble must appear before step 1");
}

/// Gap 3 (Tier 3): inline string context `context "..."` on an
/// `export block` renders as `**Context:** <text>` in the standalone
/// procedure preamble.
#[test]
fn tier3_export_block_with_inline_string_context_emits_default_label() {
    let (_dir, md) = compile_tier3_to_tempdir("tier3_export_body_constraints", "thorough-review");

    assert!(
        md.contains("**Context:** background prose for the reviewer."),
        "expected `**Context:** background prose for the reviewer.` preamble in Tier 3 standalone; got:\n{md}"
    );

    let ctx = md
        .find("**Context:** background prose")
        .expect("inline-context preamble missing");
    let step1 = md
        .find("1. Read the project structure")
        .expect("step 1 missing");
    assert!(
        ctx < step1,
        "inline-context preamble must appear before step 1"
    );
}

/// Gap 2 (Tier 2): inline string context `context "..."` renders as
/// `**Context:** <text>` and triggers Tier 2 promotion on its own (no
/// other Tier 2 trigger in the fixture — only 2 flow statements, no
/// branches, no named constraints).
#[test]
fn tier2_block_with_inline_string_context_emits_default_label() {
    let (_dir, md) = compile_to_tempdir("tier2_block_body_context_string");

    assert!(
        md.contains("### Procedure: briefed-review"),
        "expected `### Procedure: briefed-review` section (Tier 2 promotion via body context); got:\n{md}"
    );
    assert!(
        md.contains("**Context:** background prose for the reviewer."),
        "expected `**Context:** background prose for the reviewer.` preamble (default label, period-appended); got:\n{md}"
    );

    let head = md
        .find("### Procedure: briefed-review")
        .expect("heading missing");
    let ctx = md.find("**Context:**").expect("Context preamble missing");
    let step1 = md.find("1. Open the diff.").expect("step 1 missing");
    assert!(head < ctx, "preamble must appear after the heading");
    assert!(ctx < step1, "preamble must appear before step 1");
}

/// Gap 6 (Tier 3): lock the exact blank-line shape from the preamble
/// through the first step on a Tier 3 standalone procedure.
#[test]
fn tier3_preamble_paragraphs_are_blank_line_separated() {
    let (_dir, md) = compile_tier3_to_tempdir("tier3_export_body_constraints", "thorough-review");

    let must_marker = "**Must:** preserving existing behavior across refactors.";
    let must_at = md.find(must_marker).expect("Must preamble missing");
    let step1_marker = "1. Read the project structure";
    let step1_at = md.find(step1_marker).expect("step 1 missing");
    let step1_end = step1_at + step1_marker.len();
    let region = &md[must_at..step1_end];

    let expected = "**Must:** preserving existing behavior across refactors.\n\
\n\
**Require:** logging each decision with a clear rationale.\n\
\n\
**monorepo-layout:** this codebase is a Rust workspace with multiple crates.\n\
\n\
**Context:** background prose for the reviewer.\n\
\n\
## Steps\n\
\n\
1. Read the project structure";

    assert_eq!(
        region, expected,
        "Tier 3 preamble must be blank-line separated paragraphs; got:\n{region}"
    );
}

/// Gap 6 (Tier 2): lock the exact blank-line shape between the heading,
/// each preamble paragraph, and the first numbered step. Per spec, each
/// preamble paragraph is a standalone prose paragraph separated by blank
/// lines from each other AND from the step list.
#[test]
fn tier2_preamble_paragraphs_are_blank_line_separated() {
    let (_dir, md) = compile_to_tempdir("tier2_block_body_constraints");

    let head = "### Procedure: careful-review";
    let head_at = md.find(head).expect("heading missing");
    let step1_marker = "1. Scan the code.";
    let step1_at = md.find(step1_marker).expect("step 1 missing");
    let step1_end = step1_at + step1_marker.len();
    let region = &md[head_at..step1_end];

    let expected = "### Procedure: careful-review\n\
\n\
**Must:** preserving existing behavior.\n\
\n\
**Require:** logging each decision with a rationale.\n\
\n\
**Avoid:** silently swallowing errors without surfacing them.\n\
\n\
**Must avoid:** unrelated refactors outside the requested scope.\n\
\n\
1. Scan the code.";

    assert_eq!(
        region, expected,
        "Tier 2 preamble must be blank-line separated paragraphs; got:\n{region}"
    );
}
