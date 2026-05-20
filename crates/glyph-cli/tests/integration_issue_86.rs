//! Integration tests for issue #86 — output target descriptive-form diagnostics.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn run_check(file: &Path) -> Output {
    Command::new(glyph_bin())
        .arg("check")
        .arg(file)
        .arg("--format")
        .arg("json")
        .output()
        .expect("failed to spawn glyph binary")
}

fn ndjson_contains_id(stdout: &str, id: &str) -> bool {
    stdout
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .any(|v| v.get("id").and_then(|x| x.as_str()) == Some(id))
}

/// `<"">` — empty description — must emit `malformed-output-target`.
#[test]
fn descriptive_form_empty_emits_malformed_output_target() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty_desc.glyph");
    std::fs::write(
        &path,
        "\
skill s() -> Confirmation
    flow:
        return <\"\">
",
    )
    .unwrap();
    let result = run_check(&path);
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        ndjson_contains_id(&stdout, "G::parse::malformed-output-target"),
        "expected malformed-output-target for empty description, got:\n{stdout}"
    );
}

#[test]
fn descriptive_form_unterminated_emits_unterminated_string() {
    // B01: previously this case (an unterminated string inside a
    // descriptive output target) was the silent-tokenize-error escape
    // hatch — `glyph check` printed nothing and exited 0, while
    // `glyph compile` later failed with a generic build error. The
    // tokenizer now maps `TokenizeError::UnterminatedString` to a
    // structured `G::parse::unterminated-string` diagnostic so the
    // source is surfaced as invalid at check time.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unterminated_desc.glyph");
    std::fs::write(
        &path,
        // The closing `">` is intentionally absent — the newline terminates
        // the inner string literal before the output-target `>` is seen.
        "skill s() -> Confirmation\n    flow:\n        return <\"oops\n",
    )
    .unwrap();
    let result = run_check(&path);
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        ndjson_contains_id(&stdout, "G::parse::unterminated-string"),
        "expected G::parse::unterminated-string for unterminated descriptive form, got:\n{stdout}"
    );
}

/// `return <"oops">` followed by a non-terminal statement must emit
/// `output-target-outside-return` (same as the identifier form).
#[test]
fn descriptive_form_in_non_terminal_position_emits_outside_return() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("outside_return_desc.glyph");
    std::fs::write(
        &path,
        "\
skill s() -> Confirmation
    flow:
        return <\"oops\">
        \"continue\"
",
    )
    .unwrap();
    let result = run_check(&path);
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        ndjson_contains_id(&stdout, "G::parse::output-target-outside-return"),
        "expected output-target-outside-return for descriptive form outside terminal return, got:\n{stdout}"
    );
}

/// B01 regression: an unexpected character in flow position (e.g. `@`)
/// previously fell through to the tokenizer's catch-all error path,
/// returning `None` from the parser without pushing any diagnostic.
/// `glyph check` then printed nothing and exited 0 while `glyph compile`
/// later failed with a generic `G::build::compile-error`. The tokenizer
/// now maps every `TokenizeError` variant to a structured diagnostic so
/// invalid source is rejected at check time with a non-zero exit and a
/// diagnostic classified as `error`.
#[test]
fn unexpected_char_in_flow_emits_diagnostic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unexpected_char.glyph");
    std::fs::write(
        &path,
        "skill main()\n    description: \"Demo.\"\n    flow:\n        @\n",
    )
    .unwrap();
    let result = run_check(&path);
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected non-zero exit; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        ndjson_contains_id(&stdout, "G::parse::unexpected-char"),
        "expected G::parse::unexpected-char for `@` in flow, got:\n{stdout}"
    );
    // Every diagnostic emitted in this case must be classification `error`
    // (no silent or warning-only path).
    let has_error_class = stdout
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .any(|v| v.get("classification").and_then(|x| x.as_str()) == Some("error"));
    assert!(
        has_error_class,
        "expected at least one diagnostic with classification=error, got:\n{stdout}"
    );
}
