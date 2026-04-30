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
