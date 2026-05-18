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
