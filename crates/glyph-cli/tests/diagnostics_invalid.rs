//! Slice 2 integration tests — diagnostic infrastructure end-to-end.
//!
//! Covers the six acceptance criteria from the slice spec:
//!   1. `glyph compile invalid/empty.glyph.md` exits 1 with `G::parse::empty-file`
//!   2. `glyph compile invalid/empty_flow.glyph.md` exits 1 with `G::parse::empty-flow`
//!   3. `--format json` produces JSON diagnostics on stdout
//!   4. Pretty output renders span, message, and source caret to stderr
//!   5. Re-running over identical input produces byte-identical JSON
//!   6. Exit-code rules hold — `1` wins over `2`
//!
//! The 1-wins-over-2 rule is exercised at the `DiagBag` API layer in
//! `glyph-core::diagnostic::tests` (unit). Here we exercise the fixtures
//! end-to-end through the binary.

use std::path::PathBuf;
use std::process::{Command, Output};

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("invalid")
        .join(name)
}

fn run_compile(file: &str, format: &str) -> Output {
    Command::new(glyph_bin())
        .arg("compile")
        .arg(fixture(file))
        .arg("--format")
        .arg(format)
        .output()
        .expect("failed to spawn glyph binary")
}

fn assert_contains_diagnostic_id(stdout: &str, id: &str) {
    let mut found = false;
    for line in stdout.lines() {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => panic!("non-JSON line on stdout: {:?}", line),
        };
        if v.get("id").and_then(|x| x.as_str()) == Some(id) {
            found = true;
        }
    }
    assert!(
        found,
        "expected diagnostic `{}` in JSON output, got:\n{}",
        id, stdout
    );
}

#[test]
fn empty_file_exits_one_with_empty_file_diagnostic() {
    let result = run_compile("empty.glyph.md", "json");
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit code 1; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_contains_diagnostic_id(&stdout, "G::parse::empty-file");
}

#[test]
fn empty_flow_exits_one_with_empty_flow_diagnostic() {
    let result = run_compile("empty_flow.glyph.md", "json");
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit code 1; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_contains_diagnostic_id(&stdout, "G::parse::empty-flow");
}

#[test]
fn json_format_produces_ndjson_on_stdout() {
    let result = run_compile("empty.glyph.md", "json");
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    let trimmed = stdout.trim_end_matches('\n');
    assert!(!trimmed.is_empty(), "expected diagnostic on stdout");
    // Each line must parse as a complete JSON object.
    for line in trimmed.lines() {
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each NDJSON line must parse as JSON");
        let obj = v.as_object().expect("each diagnostic must be a JSON object");
        for required in ["id", "classification", "message", "span"] {
            assert!(
                obj.contains_key(required),
                "diagnostic missing required field `{}`: {}",
                required,
                line
            );
        }
        // span shape
        let span = obj.get("span").and_then(|s| s.as_object()).unwrap();
        for required in ["file", "start", "end"] {
            assert!(
                span.contains_key(required),
                "span missing field `{}` in {}",
                required,
                line
            );
        }
    }
}

#[test]
fn pretty_format_renders_to_stderr() {
    let result = run_compile("empty.glyph.md", "pretty");
    assert_eq!(result.status.code(), Some(1));
    // stdout should be empty (or carry no diagnostics) under pretty mode.
    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let stderr = String::from_utf8_lossy(&result.stderr).to_string();
    assert!(
        stdout.trim().is_empty(),
        "pretty mode should not write to stdout, got: {:?}",
        stdout
    );
    // codespan-reporting writes the diagnostic id, the message, and a caret line.
    assert!(
        stderr.contains("G::parse::empty-file"),
        "stderr should include the diagnostic id, got: {:?}",
        stderr
    );
    assert!(
        stderr.contains("source file has no declarations"),
        "stderr should include the message, got: {:?}",
        stderr
    );
    // codespan-reporting renders carets as `^` on a separate line.
    assert!(
        stderr.contains('^'),
        "stderr should include a caret indicator, got: {:?}",
        stderr
    );
}

#[test]
fn json_output_is_byte_identical_across_runs() {
    // Run twice over the same fixture; the NDJSON stream must be byte-identical.
    let first = run_compile("empty_flow.glyph.md", "json").stdout;
    let second = run_compile("empty_flow.glyph.md", "json").stdout;
    assert_eq!(
        first, second,
        "JSON output must be byte-identical across runs"
    );
}

#[test]
fn empty_flow_does_not_emit_md_file() {
    let _ = run_compile("empty_flow.glyph.md", "json");
    let unwanted = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("invalid")
        .join("empty_flow.md");
    assert!(
        !unwanted.exists(),
        "should not have written `{}` for a failing compile",
        unwanted.display()
    );
}
