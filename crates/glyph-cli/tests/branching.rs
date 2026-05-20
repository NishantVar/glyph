//! Slice 9 integration tests — if/elif/else branching.
//!
//! AC1: valid branching corpus file compiles with lettered sub-steps.
//! AC7: applies-* diagnostics fire on corpus files.

use std::path::PathBuf;
use std::process::{Command, Output};

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn corpus_path(kind: &str, name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join(kind)
        .join(name)
}

fn run_compile(file: PathBuf) -> Output {
    Command::new(glyph_bin())
        .arg("compile")
        .arg(&file)
        .output()
        .expect("failed to spawn glyph binary")
}

fn run_check_json(file: PathBuf) -> Output {
    Command::new(glyph_bin())
        .arg("check")
        .arg(&file)
        .arg("--format")
        .arg("json")
        .output()
        .expect("failed to spawn glyph binary")
}

fn stdout_diagnostic_ids(output: &Output) -> Vec<String> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).ok()?;
            v.get("id").and_then(|x| x.as_str()).map(|s| s.to_string())
        })
        .collect()
}

// AC1: valid branching file compiles successfully with lettered sub-steps.
#[test]
fn branching_corpus_compiles_with_lettered_substeps() {
    let src = corpus_path("valid", "branching.glyph");
    let out = corpus_path("valid", "branching.md");
    let _ = std::fs::remove_file(&out);

    let result = run_compile(src);
    assert!(
        result.status.success(),
        "branching.glyph should compile; stderr={:?}",
        String::from_utf8_lossy(&result.stderr),
    );

    let emitted = std::fs::read_to_string(&out).expect("branching.md should exist");
    assert!(
        emitted.contains("a."),
        "expected lettered sub-steps in output:\n{}",
        emitted
    );
    assert!(
        emitted.contains("If mode =="),
        "expected If branch header:\n{}",
        emitted
    );
    assert!(
        emitted.contains("Otherwise:"),
        "expected else arm:\n{}",
        emitted
    );
}

// AC7: applies-no-parens corpus file fires the right diagnostic.
#[test]
fn applies_no_parens_corpus_fires_diagnostic() {
    let src = corpus_path("invalid", "applies_no_parens.glyph");
    let result = run_check_json(src);
    assert_eq!(
        result.status.code(),
        Some(1),
        "applies-no-parens is an error"
    );
    let ids = stdout_diagnostic_ids(&result);
    assert!(
        ids.contains(&"G::parse::applies-no-parens".to_string()),
        "expected G::parse::applies-no-parens, got: {:?}",
        ids
    );
}

// AC7: applies-with-args corpus file fires the right diagnostic.
#[test]
fn applies_with_args_corpus_fires_diagnostic() {
    let src = corpus_path("invalid", "applies_with_args.glyph");
    let result = run_check_json(src);
    assert_eq!(
        result.status.code(),
        Some(1),
        "applies-with-args is an error"
    );
    let ids = stdout_diagnostic_ids(&result);
    assert!(
        ids.contains(&"G::parse::applies-with-args".to_string()),
        "expected G::parse::applies-with-args, got: {:?}",
        ids
    );
}

// AC7: applies-on-non-block corpus file fires the right diagnostic.
#[test]
fn applies_on_non_block_corpus_fires_diagnostic() {
    let src = corpus_path("invalid", "applies_on_non_block.glyph");
    let result = run_check_json(src);
    assert_eq!(
        result.status.code(),
        Some(1),
        "applies-on-non-block is an error"
    );
    let ids = stdout_diagnostic_ids(&result);
    assert!(
        ids.contains(&"G::analyze::applies-on-non-block".to_string()),
        "expected G::analyze::applies-on-non-block, got: {:?}",
        ids
    );
}

#[test]
fn pure_predicate_const_single_arm_emits_natural_prose() {
    let src = r#"
const complex_change = "the change requires regenerating multi-line prose, beyond a localised wording or value swap"

skill foo()
    description: "test"
    flow:
        if complex_change:
            "stop and recommend full compile"
"#;
    let dir = tempfile::tempdir().unwrap();
    let src_path = dir.path().join("foo.glyph");
    std::fs::write(&src_path, src).unwrap();

    let result = run_compile(src_path);
    assert!(
        result.status.success(),
        "foo.glyph should compile; stderr={}",
        String::from_utf8_lossy(&result.stderr)
    );

    let md = std::fs::read_to_string(dir.path().join("foo.md")).expect("foo.md should be produced");
    assert!(
        md.contains("Decide whether the change requires regenerating multi-line prose, beyond a localised wording or value swap applies and, if so:"),
        "expected single-arm opener with const prose; got:\n{}",
        md
    );
}

fn fixture_path(name: &str, ext: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(format!("{}.{}", name, ext))
}

fn snapshot_test(fixture_name: &str) {
    let src_path = fixture_path(fixture_name, "glyph");
    let expected_path = fixture_path(fixture_name, "expected.md");

    let expected_raw = std::fs::read_to_string(&expected_path)
        .unwrap_or_else(|e| panic!("failed reading {}: {}", expected_path.display(), e));

    // Strip leading HTML comment lines (used for human annotations, e.g. in predicate_mixed).
    // We strip whole lines from the raw bytes so trailing newlines are preserved exactly.
    let expected: &str = {
        let mut s = expected_raw.as_str();
        while let Some(rest) = s.strip_prefix("<!--") {
            // skip to end of this comment line
            if let Some(pos) = rest.find('\n') {
                s = &rest[pos + 1..];
            } else {
                s = "";
                break;
            }
        }
        s
    };

    // Copy the fixture into a tempdir so the compile output lands there,
    // not next to the in-tree source (which would pollute `git status`).
    let dir = tempfile::tempdir().unwrap();
    let tmp_src = dir.path().join(format!("{}.glyph", fixture_name));
    std::fs::copy(&src_path, &tmp_src)
        .unwrap_or_else(|e| panic!("failed copying fixture {}: {}", fixture_name, e));

    let result = run_compile(tmp_src);
    assert!(
        result.status.success(),
        "compile failed for {}:\nstdout:\n{}\nstderr:\n{}",
        fixture_name,
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );

    let produced_path = dir.path().join(format!("{}.md", fixture_name));
    let actual_md = std::fs::read_to_string(&produced_path)
        .unwrap_or_else(|e| panic!("produced .md not found for {}: {}", fixture_name, e));

    assert_eq!(
        actual_md, expected,
        "snapshot mismatch for {}",
        fixture_name
    );
}

#[test]
fn fixture_predicate_const_single_arm() {
    snapshot_test("predicate_const_single_arm");
}

#[test]
fn fixture_predicate_const_multi_arm() {
    snapshot_test("predicate_const_multi_arm");
}

#[test]
fn fixture_predicate_mixed() {
    snapshot_test("predicate_mixed");
}

#[test]
fn fixture_predicate_inline_literal() {
    snapshot_test("predicate_inline_literal");
}

#[test]
fn fixture_predicate_literal_or_literal() {
    snapshot_test("predicate_literal_or_literal");
}

#[test]
fn fixture_predicate_literal_and_literal() {
    snapshot_test("predicate_literal_and_literal");
}

#[test]
fn fixture_predicate_param_default() {
    snapshot_test("predicate_param_default");
}

#[test]
fn fixture_predicate_branch_in_block_procedure() {
    snapshot_test("predicate_branch_in_block_procedure");
}

#[test]
fn fixture_predicate_branch_last_in_block_procedure_with_return() {
    snapshot_test("predicate_branch_last_in_block_procedure_with_return");
}

#[test]
fn fixture_predicate_branch_in_tier1_block() {
    snapshot_test("predicate_branch_in_tier1_block");
}

#[test]
fn fixture_predicate_block_param_default() {
    snapshot_test("predicate_block_param_default");
}

#[test]
fn fixture_predicate_block_param_default_per_block_scope() {
    snapshot_test("predicate_block_param_default_per_block_scope");
}

#[test]
fn mixed_predicate_const_with_not_emits_branch_condition_span() {
    let src = r#"
const big = "the change is big"

skill foo()
    description: "test"
    flow:
        if big and not "is dry run":
            "stop"
"#;
    let dir = tempfile::tempdir().unwrap();
    let src_path = dir.path().join("foo.glyph");
    std::fs::write(&src_path, src).unwrap();

    let result = run_compile(src_path);
    assert!(
        result.status.success(),
        "foo.glyph should compile; stderr={}",
        String::from_utf8_lossy(&result.stderr)
    );

    let md = std::fs::read_to_string(dir.path().join("foo.md")).expect("foo.md should be produced");
    // After substitution, the BranchCondition span should replace the `big`
    // const token with its resolved body and strip quotes from the inline literal.
    assert!(
        md.contains("the change is big"),
        "expected resolved const prose in mixed condition; compiled md = {}",
        md
    );
    assert!(
        md.contains("is dry run"),
        "expected stripped literal in mixed condition; compiled md = {}",
        md
    );
    // If substitution failed, the raw const name `big` would appear immediately
    // after the "If " opener — verify it was replaced by its resolved prose.
    assert!(
        !md.contains("If big"),
        "raw `big` token should have been substituted; got:\n{}",
        md
    );
}

fn compile_and_read_md(filename: &str, source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let src_path = dir.path().join(filename);
    std::fs::write(&src_path, source).unwrap();
    let result = run_compile(src_path.clone());
    assert!(
        result.status.success(),
        "compile failed: {:?}",
        String::from_utf8_lossy(&result.stderr)
    );
    let md_path = src_path.with_extension("md");
    std::fs::read_to_string(&md_path)
        .unwrap_or_else(|_| panic!("missing emitted markdown at {md_path:?}"))
}

#[test]
fn emit_or_composed_applies_substitutes_both_descriptions() {
    // Block descriptions intentionally omit trailing periods so the existing
    // `strip_trailing_period` post-processing doesn't shave the second arm's
    // tail. The emit-level fix being verified here is that BOTH predicate
    // tokens are substituted, not just one (Finding 3 / codex bug).
    let source = r#"block fast_mode()
    description: "Fast processing path"
    flow:
        "Fast work."

block slow_mode()
    description: "Slow processing path"
    flow:
        "Slow work."

skill main()
    description: "Test."
    flow:
        if fast_mode.applies() or slow_mode.applies()
            "Either path."
"#;
    let md = compile_and_read_md("or_compound_emit.glyph", source);
    assert!(
        md.contains("Fast processing path") && md.contains("Slow processing path"),
        "both block descriptions must appear in emitted markdown:\n{md}"
    );
    assert!(
        md.contains(" or "),
        "or-operator must pass through verbatim:\n{md}"
    );
}

#[test]
fn emit_or_composed_string_consts_substitutes_both_bodies() {
    let source = r#"const big = "the change is big"
const small = "the change is small"

skill main()
    description: "Test."
    flow:
        if big or small
            "Either."
"#;
    let md = compile_and_read_md("or_consts_emit.glyph", source);
    assert!(
        md.contains("the change is big") && md.contains("the change is small"),
        "both const bodies must appear:\n{md}"
    );
    assert!(
        md.contains(" or "),
        "or-operator must pass through verbatim:\n{md}"
    );
}

#[test]
fn stub_fill_eq_with_string_rhs_preserves_quotes_and_substitutes() {
    let source = r#"const complex_change = "the requested change is complex"

skill main(risk: String = <"risk level">)
    description: "Test."
    flow:
        if risk == "high" and complex_change
            "Escalate."
        else
            "Proceed."
"#;
    let md = compile_and_read_md("eq_string_rhs.glyph", source);
    assert!(
        md.contains("risk == \"high\""),
        "operand `risk == \"high\"` must render verbatim with quotes:\n{md}"
    );
    assert!(
        md.contains("the requested change is complex"),
        "complex_change must be substituted:\n{md}"
    );
}

#[test]
fn stub_fill_numeric_eq_no_substitution() {
    let source = r#"skill main(max_attempts: Int = <"maximum attempts">)
    description: "Test."
    flow:
        if max_attempts == 3
            "Halt."
        else
            "Continue."
"#;
    let md = compile_and_read_md("numeric_eq.glyph", source);
    assert!(
        md.contains("max_attempts == 3"),
        "numeric == operand must render verbatim:\n{md}"
    );
}

#[test]
fn stub_fill_paren_grouped_predicates_preserves_parens_and_substitutes() {
    let source = r#"const big = "the change is big"
const small = "the change is small"

skill main(reviewable: Bool = <"whether reviewable">)
    description: "Test."
    flow:
        if (big or small) and reviewable
            "Review."
        else
            "Skip."
"#;
    let md = compile_and_read_md("paren_grouped.glyph", source);
    assert!(
        md.contains("the change is big") && md.contains("the change is small"),
        "both const bodies substituted:\n{md}"
    );
    assert!(
        md.contains("(") && md.contains(")"),
        "parens preserved in output:\n{md}"
    );
    assert!(
        md.contains("reviewable"),
        "Boolean token `reviewable` rendered bare:\n{md}"
    );
}

// --- B02 regression: `!=` operator in branch conditions ---
// `GLYPH_LANGUAGE_GUIDE.md` documents `if risk != "low":`. Prior to this fix,
// the tokenizer rejected `!` as `G::parse::unexpected-char`. These tests
// guard the documented surface.

const NEQ_SOURCE: &str = r##"const low = "low"

skill main(risk = <"caller-supplied risk level">)
    description: "Demo."
    flow:
        if risk != "low":
            "Act."
"##;

#[test]
fn not_equals_in_if_condition_check_emits_no_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let src_path = dir.path().join("neq.glyph");
    std::fs::write(&src_path, NEQ_SOURCE).unwrap();
    let result = run_check_json(src_path);
    assert_eq!(
        result.status.code(),
        Some(0),
        "glyph check should succeed for `!=`; stdout={}; stderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let ids = stdout_diagnostic_ids(&result);
    assert!(
        ids.is_empty(),
        "expected no diagnostics for documented `!=`; got: {:?}",
        ids
    );
}

#[test]
fn not_equals_in_if_condition_compiles_with_operator_in_output() {
    let md = compile_and_read_md("neq_compile.glyph", NEQ_SOURCE);
    assert!(
        md.contains("!="),
        "compiled markdown should preserve the `!=` operator in the If header:\n{md}"
    );
}
