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
    let path = dir.path().join("empty_desc.glyph.md");
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

/// `<"oops\n` — unterminated string — the tokenizer raises `UnterminatedString`
/// which falls through to `Err(_) => return None` in `parse_with_diagnostics`
/// without pushing any diagnostic ID into the bag. As a result `glyph check`
/// produces no JSON output and exits 0 (empty diag bag).
///
/// Cached design fact: no new diagnostic ID for this case; accept the current
/// silent behaviour. This test pins that contract so any future change (e.g.
/// promoting the tokenizer error to a structured diagnostic) gets noticed.
#[test]
fn descriptive_form_unterminated_produces_no_structured_diagnostic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unterminated_desc.glyph.md");
    std::fs::write(
        &path,
        // The closing `">` is intentionally absent — the newline terminates
        // the inner string literal before the output-target `>` is seen.
        "skill s() -> Confirmation\n    flow:\n        return <\"oops\n",
    )
    .unwrap();
    let result = run_check(&path);
    let stdout = String::from_utf8_lossy(&result.stdout);
    // No structured diagnostic is emitted — the tokenizer bails silently.
    assert!(
        stdout.trim().is_empty(),
        "expected no structured diagnostics for unterminated descriptive form, got:\n{stdout}"
    );
}

/// `return <"oops">` followed by a non-terminal statement must emit
/// `output-target-outside-return` (same as the identifier form).
#[test]
fn descriptive_form_in_non_terminal_position_emits_outside_return() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("outside_return_desc.glyph.md");
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
