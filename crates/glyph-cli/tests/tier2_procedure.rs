//! Integration tests for Tier 2 same-file procedure projection (slice 14).

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

fn compile_fixture(name: &str) -> String {
    let source = corpus_dir().join(format!("{}.glyph", name));
    let compiled = corpus_dir().join(format!("{}.md", name));
    let _ = std::fs::remove_file(&compiled);

    let result = Command::new(glyph_bin())
        .arg("compile")
        .arg(&source)
        .output()
        .expect("failed to spawn glyph binary");
    assert!(
        result.status.success(),
        "glyph compile failed. stderr={:?}",
        String::from_utf8_lossy(&result.stderr),
    );

    std::fs::read_to_string(&compiled).expect("compiled .md missing")
}

#[test]
fn explicit_blocks_tier2_projection() {
    // Compile the fixture once to avoid parallel-test race conditions
    // (compile_fixture deletes and recreates the output file).
    let output = compile_fixture("explicit_blocks");

    // Four-plus-statement block emits a procedure section.
    assert!(
        output.contains("### Procedure: review-code"),
        "expected ### Procedure: review-code section in output:\n{}",
        output
    );

    // Caller steps reference the procedure by name.
    assert!(
        output.contains("review-code procedure"),
        "expected caller step to reference 'review-code procedure' in output:\n{}",
        output
    );

    // small_helper has only 1 statement — should inline (Tier 1), no procedure section.
    assert!(
        !output.contains("### Procedure: small-helper"),
        "small_helper should NOT get a procedure section (Tier 1 inline):\n{}",
        output
    );
    assert!(
        output.contains("Do a quick check."),
        "small_helper body should be inlined in Steps:\n{}",
        output
    );

    // Procedure section ordering is deterministic across runs.
    let output2 = compile_fixture("explicit_blocks");
    assert_eq!(
        output, output2,
        "procedure section ordering should be deterministic across runs"
    );
}

#[test]
fn branch_only_tier2_emits_procedure_section() {
    let output = compile_fixture("branch_only_procedure");

    assert!(
        output.contains("Follow the deep-clean procedure"),
        "expected sub-step reference to deep-clean procedure; got:\n{}",
        output
    );

    assert!(
        output.contains("### Procedure: deep-clean"),
        "expected ### Procedure: deep-clean section; got:\n{}",
        output
    );
}

#[test]
fn two_hop_nested_tier2_emits_inner_procedure() {
    let output = compile_fixture("nested_branch_only_procedure");

    // (1) Outer procedure section is present exactly once.
    let outer_count = output.matches("### Procedure: outer").count();
    assert_eq!(
        outer_count, 1,
        "expected `### Procedure: outer` exactly once, got {outer_count}; output:\n{output}"
    );

    // (2) Inner (two-hop) procedure section is present exactly once.
    let bar_count = output.matches("### Procedure: bar").count();
    assert_eq!(
        bar_count, 1,
        "expected `### Procedure: bar` exactly once, got {bar_count}; output:\n{output}"
    );

    // (3) Parent-before-child ordering: outer header appears before bar header.
    let outer_idx = output
        .find("### Procedure: outer")
        .expect("outer header missing");
    let bar_idx = output
        .find("### Procedure: bar")
        .expect("bar header missing");
    assert!(
        outer_idx < bar_idx,
        "expected `### Procedure: outer` to appear before `### Procedure: bar` (parent-before-child); outer_idx={outer_idx} bar_idx={bar_idx}"
    );

    // (4) `bar`'s section contains four numbered top-level steps (1.–4.)
    // matching the source flow text.
    // Numbering, not lettering — lettering only applies inside branch arms of
    // another procedure; `bar`'s own section renders its flow as numbered steps.
    let bar_section = {
        let after_header = &output[bar_idx + "### Procedure: bar".len()..];
        let next = after_header
            .find("### Procedure:")
            .unwrap_or(after_header.len());
        &after_header[..next]
    };
    for (n, text) in [
        "First inner step.",
        "Second inner step.",
        "Third inner step.",
        "Fourth inner step.",
    ]
    .iter()
    .enumerate()
    {
        let needle = format!("{}. {}", n + 1, text);
        assert!(
            bar_section.contains(&needle),
            "expected `bar` procedure body to contain `{needle}`; section was:\n{bar_section}"
        );
    }

    // (5) No regression: `outer`'s body still references `bar` as a procedure.
    assert!(
        output.contains("Follow the bar procedure"),
        "expected `outer` body to keep its `Follow the bar procedure.` reference; output:\n{output}"
    );
}
