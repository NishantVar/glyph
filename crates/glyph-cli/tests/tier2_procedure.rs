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
    let source = corpus_dir().join(format!("{}.glyph.md", name));
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
    assert_eq!(output, output2, "procedure section ordering should be deterministic across runs");
}
