//! Slice 22: Multi-file acceptance project integration tests.
//!
//! Exercises the 5-skill project in `tests/corpus/multi-file/` per
//! `docs/reference/mvp-acceptance.md` §3. All files are fully valid (no repair
//! needed) and compile end-to-end with exit code 0.
//!
//! Acceptance criteria (Bar 3):
//!   - prefs.glyph compiles (exit 0, zero .md emission)
//!   - repo_tools.glyph compiles (exit 0, library)
//!   - fix_bug.glyph compiles (exit 0, imports from prefs + repo_tools resolve)
//!   - review_pr.glyph compiles (exit 0, imports from repo_tools, branching works)
//!   - update_docs.glyph compiles (exit 0, standalone)
//!   - DAG order respected: libraries compile before consumers
//!   - Cross-file name resolution works
//!
//! Bar 2 (determinism): byte-identical .md output across runs.

use std::path::PathBuf;
use std::process::Command;

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("multi-file")
}

/// Copy the entire multi-file corpus to a tempdir so tests don't race.
fn setup_tempdir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let src = corpus_dir();
    for entry in std::fs::read_dir(&src).unwrap() {
        let entry = entry.unwrap();
        let dest = dir.path().join(entry.file_name());
        std::fs::copy(entry.path(), &dest).unwrap();
    }
    let p = dir.path().to_path_buf();
    (dir, p)
}

fn compile_directory(dir: &std::path::Path) -> std::process::Output {
    Command::new(glyph_bin())
        .arg("compile")
        .arg(dir)
        .output()
        .expect("failed to spawn glyph binary")
}

fn compile_directory_with_ir(dir: &std::path::Path) -> std::process::Output {
    Command::new(glyph_bin())
        .arg("compile")
        .arg(dir)
        .arg("--emit-ir")
        .output()
        .expect("failed to spawn glyph binary")
}

// ── AC: Directory compile exits 0 ──────────────────────────────────

#[test]
fn multi_file_project_compiles_successfully() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(
        output.status.success(),
        "glyph compile multi-file/ should exit 0.\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

// ── AC: Skill files emit .md, library files do not ─────────────────

#[test]
fn skill_files_produce_md_output() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    // Skill files should produce .md output.
    assert!(path.join("fix_bug.md").exists(), "fix_bug.md missing");
    assert!(path.join("review_pr.md").exists(), "review_pr.md missing");
    assert!(
        path.join("update_docs.md").exists(),
        "update_docs.md missing"
    );
}

#[test]
fn library_files_produce_no_md_output() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    // Library files (text-only or block-only, no skill) should NOT emit .md.
    assert!(!path.join("prefs.md").exists(), "prefs.md should not exist");
    assert!(
        !path.join("repo_tools.md").exists(),
        "repo_tools.md should not exist"
    );
}

// ── AC: Snapshot tests for emitted output ──────────────────────────

#[test]
fn fix_bug_snapshot() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("fix_bug.md")).unwrap();
    insta::assert_snapshot!("multi_file__fix_bug", md);
}

#[test]
fn review_pr_snapshot() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("review_pr.md")).unwrap();
    insta::assert_snapshot!("multi_file__review_pr", md);
}

#[test]
fn update_docs_snapshot() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("update_docs.md")).unwrap();
    insta::assert_snapshot!("multi_file__update_docs", md);
}

// ── AC: Cross-file name resolution ─────────────────────────────────

#[test]
fn fix_bug_resolves_imported_constraint() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("fix_bug.md")).unwrap();

    // The imported text `preserve_existing_patterns` from prefs.glyph
    // should appear in the Constraints section.
    assert!(
        md.contains("## Constraints"),
        "fix_bug.md should have a Constraints section"
    );
    assert!(
        md.contains("existing patterns"),
        "imported constraint text from prefs should be rendered"
    );
}

#[test]
fn fix_bug_resolves_imported_block_call() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("fix_bug.md")).unwrap();

    // The imported block `inspect_repo` from repo_tools.glyph has 150+
    // words, so it gets Tier 3 projection — a procedure file reference.
    assert!(
        md.contains("repo_tools/inspect-repo.md"),
        "imported inspect_repo should render as Tier 3 procedure reference in fix_bug.md"
    );
}

#[test]
fn review_pr_resolves_imported_blocks() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("review_pr.md")).unwrap();

    // inspect_repo called at top of flow — now Tier 3 procedure reference.
    assert!(
        md.contains("repo_tools/inspect-repo.md"),
        "imported inspect_repo should render as Tier 3 procedure reference in review_pr.md"
    );

    // run_tests called inside the high-risk branch — still Tier 1 inlined.
    assert!(
        md.contains("test framework"),
        "imported run_tests should be expanded in review_pr.md"
    );
}

// ── AC: Procedure files for repo_tools exports ────────────────────

#[test]
fn repo_tools_procedure_files_emitted() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    // inspect_repo has 150+ words — should get a Tier 3 procedure file.
    assert!(
        path.join("repo_tools").join("inspect-repo.md").exists(),
        "repo_tools/inspect-repo.md procedure file should exist"
    );

    // Verify the procedure file has expected frontmatter.
    let proc_md = std::fs::read_to_string(path.join("repo_tools").join("inspect-repo.md")).unwrap();
    assert!(
        proc_md.contains("kind: procedure"),
        "procedure file should have kind: procedure"
    );
    assert!(
        proc_md.contains("name: inspect-repo"),
        "procedure file should have correct name"
    );
    // Tier 3 procedure files render `## Parameters` with the same bullet
    // shape as the skill emitter (compiled-output.md §`## Parameters`),
    // not the legacy `(default: …)` / `(required)` form.
    assert!(
        proc_md.contains("- **scope**: directory to inspect. Default: \".\"."),
        "procedure file should render scope with shared bullet shape; got:\n{proc_md}"
    );
    assert!(
        !proc_md.contains("(default:"),
        "procedure file must not use the legacy `(default: …)` bullet:\n{proc_md}"
    );
}

// ── AC: Context sub-section (top-level) ────────────────────────────

#[test]
fn fix_bug_has_context_section() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("fix_bug.md")).unwrap();

    // fix_bug.glyph has `context:` sub-section at skill level,
    // which should render as `## Context` in the output.
    assert!(
        md.contains("## Context"),
        "fix_bug.md should have a ## Context section"
    );
    assert!(
        md.contains("standard project conventions"),
        "context item 'codebase_assumptions' text should appear"
    );
    assert!(
        md.contains("reproducible locally"),
        "context inline string should appear"
    );
}

// ── AC: Branch-scoped context marker ───────────────────────────────

#[test]
fn review_pr_has_branch_scoped_context() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("review_pr.md")).unwrap();

    // review_pr.glyph has `context security_note` inside the
    // `if risk == "high"` arm. This should render inline as a Note,
    // not as a top-level ## Context section.
    assert!(
        !md.contains("## Context"),
        "review_pr.md should NOT have a top-level ## Context section"
    );
    assert!(
        md.contains("high-risk change"),
        "branch-scoped security_note context text should appear inline"
    );
}

// ── AC: .applies() conditional ─────────────────────────────────────

#[test]
fn fix_bug_has_applies_conditional_same_file() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("fix_bug.md")).unwrap();

    // fix_bug.glyph uses `if deep_investigation.applies()` — same-file block.
    assert!(
        md.contains("bug spans multiple subsystems"),
        "deep_investigation.applies() should render with the block's description"
    );
    // AC 5 fix: no "When When" duplicate — description starts with "The"
    assert!(
        !md.contains("When When"),
        "should not have duplicate 'When When' prefix"
    );
}

#[test]
fn fix_bug_has_applies_conditional_imported() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("fix_bug.md")).unwrap();

    // fix_bug.glyph uses `if has_test_suite.applies()` — imported block
    // from repo_tools.glyph whose description lives in the library file.
    assert!(
        md.contains("established test suite"),
        "imported has_test_suite.applies() should render with the library block's description"
    );
}

// ── AC: Branching in review_pr ─────────────────────────────────────

#[test]
fn review_pr_has_branching() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("review_pr.md")).unwrap();

    // review_pr has `if risk == "high"` / `else` branching.
    assert!(
        md.contains("risk") && md.contains("high"),
        "review_pr.md should contain the risk == high branch condition"
    );
    assert!(
        md.contains("Otherwise"),
        "review_pr.md should contain the else arm"
    );
}

// ── AC: Standalone file compiles alongside import-heavy siblings ───

#[test]
fn update_docs_is_standalone() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("update_docs.md")).unwrap();

    // update_docs has no imports — same as the walking skeleton.
    assert!(md.contains("name: update_docs"), "frontmatter name");
    assert!(
        md.contains("Scan the repository"),
        "first flow step should appear"
    );
}

// ── Bar 2: Deterministic output (byte-identical re-run) ────────────

#[test]
fn multi_file_compile_is_idempotent() {
    let (_dir, path) = setup_tempdir();

    // First run (with --emit-ir to also produce .ir.json files).
    let r1 = compile_directory_with_ir(&path);
    assert!(r1.status.success(), "first compile failed");
    let fix1 = std::fs::read(path.join("fix_bug.md")).unwrap();
    let rev1 = std::fs::read(path.join("review_pr.md")).unwrap();
    let upd1 = std::fs::read(path.join("update_docs.md")).unwrap();
    // .ir.json files
    let fix_ir1 = std::fs::read(path.join("fix_bug.ir.json")).unwrap();
    let rev_ir1 = std::fs::read(path.join("review_pr.ir.json")).unwrap();
    let upd_ir1 = std::fs::read(path.join("update_docs.ir.json")).unwrap();
    // Procedure files
    let proc1 = std::fs::read(path.join("repo_tools").join("inspect-repo.md")).unwrap();

    // Second run.
    let r2 = compile_directory_with_ir(&path);
    assert!(r2.status.success(), "second compile failed");
    let fix2 = std::fs::read(path.join("fix_bug.md")).unwrap();
    let rev2 = std::fs::read(path.join("review_pr.md")).unwrap();
    let upd2 = std::fs::read(path.join("update_docs.md")).unwrap();
    let fix_ir2 = std::fs::read(path.join("fix_bug.ir.json")).unwrap();
    let rev_ir2 = std::fs::read(path.join("review_pr.ir.json")).unwrap();
    let upd_ir2 = std::fs::read(path.join("update_docs.ir.json")).unwrap();
    let proc2 = std::fs::read(path.join("repo_tools").join("inspect-repo.md")).unwrap();

    // .md files byte-identical
    assert_eq!(fix1, fix2, "fix_bug.md not byte-identical across runs");
    assert_eq!(rev1, rev2, "review_pr.md not byte-identical across runs");
    assert_eq!(upd1, upd2, "update_docs.md not byte-identical across runs");
    // .ir.json files byte-identical
    assert_eq!(
        fix_ir1, fix_ir2,
        "fix_bug.ir.json not byte-identical across runs"
    );
    assert_eq!(
        rev_ir1, rev_ir2,
        "review_pr.ir.json not byte-identical across runs"
    );
    assert_eq!(
        upd_ir1, upd_ir2,
        "update_docs.ir.json not byte-identical across runs"
    );
    // Procedure files byte-identical
    assert_eq!(
        proc1, proc2,
        "repo_tools/inspect-repo.md procedure not byte-identical across runs"
    );
}

// ── AC: Frontmatter correctness ────────────────────────────────────

#[test]
fn fix_bug_frontmatter() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("fix_bug.md")).unwrap();
    assert!(md.contains("name: fix_bug"), "frontmatter name");
    assert!(
        md.contains("description: 'Debug and fix a bug"),
        "frontmatter description"
    );
}

#[test]
fn review_pr_frontmatter() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("review_pr.md")).unwrap();
    assert!(md.contains("name: review_pr"), "frontmatter name");
    assert!(
        md.contains("description: 'Review a pull request"),
        "frontmatter description"
    );
}

// ── AC: Parameters rendered ────────────────────────────────────────

#[test]
fn fix_bug_parameters() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("fix_bug.md")).unwrap();
    assert!(
        md.contains("## Parameters"),
        "fix_bug.md should have Parameters section"
    );
    assert!(md.contains("scope"), "scope parameter should appear");
}

#[test]
fn review_pr_parameters() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("review_pr.md")).unwrap();
    assert!(
        md.contains("## Parameters"),
        "review_pr.md should have Parameters section"
    );
    assert!(md.contains("scope"), "scope parameter");
    assert!(md.contains("risk"), "risk parameter");
}

// ── AC: Return folding ─────────────────────────────────────────────

#[test]
#[ignore = "PRD #159: this surface is now unreachable end-to-end — the driver fixture fix_bug.glyph is typed (-> ChangeSummary) to satisfy #160, which gates off the row-3 fold; if it were untyped, the analyzer would short-circuit compile first. Relift as expand-pass-level test against IrArena directly. See todo/expand-todos.md."]
fn fix_bug_return_folded() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("fix_bug.md")).unwrap();

    // return summarize_changes() should fold the block's body into the
    // last step with "Return the result of" prefix or inline the body.
    assert!(
        md.contains("summarize_changes") || md.contains("List what was changed"),
        "return folding should include summarize_changes block content"
    );
}

#[test]
fn review_pr_return_folded() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("review_pr.md")).unwrap();

    // return "Produce a structured review..." should appear inline.
    assert!(
        md.contains("structured review"),
        "inline return text should appear in review_pr.md"
    );
}

// ── AC: Constraint rendering ───────────────────────────────────────

#[test]
fn fix_bug_constraints() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("fix_bug.md")).unwrap();
    assert!(
        md.contains("## Constraints"),
        "fix_bug.md should have Constraints"
    );
    // `require preserve_existing_patterns` (imported from prefs)
    assert!(
        md.contains("existing patterns"),
        "imported require constraint should render"
    );
    // `avoid unrelated_edits` (local text)
    assert!(
        md.contains("unrelated"),
        "local avoid constraint should render"
    );
}

#[test]
fn review_pr_constraints() {
    let (_dir, path) = setup_tempdir();
    let output = compile_directory(&path);
    assert!(output.status.success(), "compile failed");

    let md = std::fs::read_to_string(path.join("review_pr.md")).unwrap();
    assert!(
        md.contains("## Constraints"),
        "review_pr.md should have Constraints"
    );
    assert!(
        md.contains("every changed file"),
        "thorough_review constraint text"
    );
    assert!(md.contains("tests exist"), "check_tests constraint text");
}
