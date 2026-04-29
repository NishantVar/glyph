//! Slice 3 integration tests — `glyph check <path>`.
//!
//! Acceptance criteria from `mvp-issues.md` slice 3:
//!   1. `glyph check valid/update_docs.glyph.md` exits 0 with no files written.
//!   2. `glyph check repairable/<file>` exits 2 with diagnostics on stdout (JSON)
//!      or stderr (pretty).
//!   3. `glyph check invalid/<file>` exits 1.
//!   4. Subcommand parsing accepts file or directory paths.

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

fn run_check(file: PathBuf, format: &str) -> Output {
    Command::new(glyph_bin())
        .arg("check")
        .arg(&file)
        .arg("--format")
        .arg(format)
        .output()
        .expect("failed to spawn glyph binary")
}

fn run_check_no_format(file: PathBuf) -> Output {
    Command::new(glyph_bin())
        .arg("check")
        .arg(&file)
        .output()
        .expect("failed to spawn glyph binary")
}

#[test]
fn check_valid_exits_zero_and_writes_no_md() {
    let src = corpus_path("valid", "update_docs.glyph.md");
    let sibling_md = src.with_file_name("update_docs.md");

    // Snapshot: was the .md present *before* this test? `walking_skeleton` may have
    // produced one. We must preserve the pre-existing state — check itself MUST
    // NOT touch the file. Capture the bytes (or absence) and restore them.
    let pre = std::fs::read(&sibling_md).ok();

    // If a .md existed pre-test, leave it. If not, ensure it doesn't appear.
    let result = run_check(src, "json");

    let post_exists = sibling_md.exists();
    // Restore prior state to avoid bleeding between tests.
    match pre {
        Some(_bytes) => assert!(
            post_exists,
            "check must not delete a pre-existing {}",
            sibling_md.display()
        ),
        None => {
            if post_exists {
                let _ = std::fs::remove_file(&sibling_md);
                panic!(
                    "check must not have written {}",
                    sibling_md.display()
                );
            }
        }
    }

    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    // Warnings (e.g., over-declared effects) may appear on stdout in JSON mode
    // and are acceptable alongside exit 0. Only errors/repairables would be a
    // problem (but those change the exit code to 1 or 2).
    let stdout = String::from_utf8_lossy(&result.stdout);
    for line in stdout.trim().lines() {
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each NDJSON line must parse as JSON");
        let cls = v.get("classification").and_then(|x| x.as_str()).unwrap_or("");
        assert_eq!(
            cls, "warning",
            "json mode on a valid file should emit only warnings on stdout, got classification {:?} in: {}",
            cls, line,
        );
    }
}

#[test]
fn check_repairable_exits_two_with_diagnostic_on_stdout() {
    let src = corpus_path("repairable", "tab_indent.glyph.md");
    let result = run_check(src, "json");
    assert_eq!(
        result.status.code(),
        Some(2),
        "expected exit 2; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    let trimmed = stdout.trim_end_matches('\n');
    assert!(
        !trimmed.is_empty(),
        "expected at least one repairable diagnostic on stdout"
    );
    let mut saw_repairable = false;
    for line in trimmed.lines() {
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each NDJSON line must parse as JSON");
        if v.get("classification").and_then(|x| x.as_str()) == Some("repairable") {
            saw_repairable = true;
        }
    }
    assert!(
        saw_repairable,
        "expected a repairable-classified diagnostic, got: {}",
        stdout
    );
}

#[test]
fn check_repairable_pretty_renders_to_stderr() {
    let src = corpus_path("repairable", "tab_indent.glyph.md");
    let result = run_check(src, "pretty");
    assert_eq!(result.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let stderr = String::from_utf8_lossy(&result.stderr).to_string();
    assert!(
        stdout.trim().is_empty(),
        "pretty mode must not write to stdout, got: {:?}",
        stdout
    );
    assert!(
        stderr.contains("G::parse::tab-indent") || stderr.contains("tab-indent"),
        "stderr should include the diagnostic id, got: {:?}",
        stderr
    );
}

#[test]
fn check_invalid_exits_one() {
    let src = corpus_path("invalid", "empty.glyph.md");
    let result = run_check(src, "json");
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit 1; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn check_accepts_directory_path() {
    // The repairable fixture lives in its own dir with no other .glyph.md siblings.
    // A directory-mode check should pick up the one file and exit 2.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("repairable");
    let result = run_check(dir, "json");
    assert_eq!(
        result.status.code(),
        Some(2),
        "expected exit 2 from directory walk; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn check_default_format_is_pretty() {
    // Calling with no `--format` flag should default to `pretty` and still exit
    // with the right code on a clean file.
    let src = corpus_path("valid", "update_docs.glyph.md");
    let result = run_check_no_format(src);
    assert_eq!(result.status.code(), Some(0));
}
