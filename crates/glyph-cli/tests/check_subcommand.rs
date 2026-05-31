//! Slice 3 integration tests — `glyph check <path>`.
//!
//! Acceptance criteria from `mvp-issues.md` slice 3:
//!   1. `glyph check valid/update_docs.glyph` exits 0 with no files written.
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
    let src = corpus_path("valid", "update_docs.glyph");
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
                panic!("check must not have written {}", sibling_md.display());
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
        let cls = v
            .get("classification")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        assert_eq!(
            cls, "warning",
            "json mode on a valid file should emit only warnings on stdout, got classification {:?} in: {}",
            cls, line,
        );
    }
}

#[test]
fn check_repairable_exits_two_with_diagnostic_on_stdout() {
    let src = corpus_path("repairable", "tab_indent.glyph");
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
    let src = corpus_path("repairable", "tab_indent.glyph");
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
    let src = corpus_path("invalid", "empty.glyph");
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
    // The repairable fixture lives in its own dir with no other .glyph siblings.
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
    let src = corpus_path("valid", "update_docs.glyph");
    let result = run_check_no_format(src);
    assert_eq!(result.status.code(), Some(0));
}

/// Strip ANSI SGR escape sequences so caret column math is on visible characters.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for n in chars.by_ref() {
                if n == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ----------------------------------------------------------------------------
// B05 regression: imported-file diagnostics must render against the imported
// file (its own path + source text), and a shared dependency must not have its
// diagnostics duplicated when a directory is checked.
// ----------------------------------------------------------------------------

/// Pretty mode: an entry file imports `dep.glyph`, which contains a skill with
/// no `description:`. That diagnostic originates in `dep.glyph` and must be
/// rendered with `dep.glyph`'s path in the file header and `dep.glyph`'s own
/// source text in the snippet -- never the entry file's.
#[test]
fn b05_imported_diagnostic_renders_against_dependency_file_pretty() {
    let entry = corpus_path("imported-diagnostics", "single/main.glyph");
    let result = run_check(entry, "pretty");
    let stderr = strip_ansi(&String::from_utf8_lossy(&result.stderr));

    let lines: Vec<&str> = stderr.lines().collect();
    let header_idx = lines
        .iter()
        .position(|l| l.contains("G::analyze::missing-description"))
        .unwrap_or_else(|| {
            panic!("expected a missing-description diagnostic from the dependency:\n{stderr}")
        });

    // The file-location line immediately follows the diagnostic title. It must
    // name the dependency file, not the entry file.
    let location_line = lines[header_idx + 1];
    assert!(
        location_line.contains("dep.glyph"),
        "imported diagnostic must render against `dep.glyph`, got header: {location_line:?}\n{stderr}"
    );
    assert!(
        !location_line.contains("main.glyph"),
        "imported diagnostic must NOT render against the entry file `main.glyph`, \
         got header: {location_line:?}\n{stderr}"
    );

    // The rendered snippet must show the dependency's own source -- the skill
    // that is actually missing a description -- not entry-file lines.
    let snippet: String = lines[header_idx..]
        .iter()
        .take(8)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        snippet.contains("dep_skill_without_description"),
        "imported diagnostic must render the dependency's own source text, got:\n{snippet}"
    );
    assert!(
        !snippet.contains("Entry skill that imports a dependency"),
        "imported diagnostic must NOT render the entry file's source text, got:\n{snippet}"
    );
}

/// JSON mode, single entry: the imported diagnostic's span must identify the
/// dependency file, not the entry file.
#[test]
fn b05_imported_diagnostic_span_identifies_dependency_file_json() {
    let entry = corpus_path("imported-diagnostics", "single/main.glyph");
    let result = run_check(entry, "json");
    let stdout = String::from_utf8_lossy(&result.stdout);

    let mut saw = false;
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each NDJSON line must parse as JSON");
        if v.get("id").and_then(|x| x.as_str()) == Some("G::analyze::missing-description") {
            saw = true;
            let file = v
                .get("span")
                .and_then(|s| s.get("file"))
                .and_then(|f| f.as_str())
                .unwrap_or("");
            assert!(
                file.contains("dep.glyph"),
                "imported diagnostic span must identify the dependency file, got file={file:?}"
            );
            assert!(
                !file.ends_with("main.glyph"),
                "imported diagnostic span must NOT identify the entry file, got file={file:?}"
            );
        }
    }
    assert!(
        saw,
        "expected a missing-description diagnostic from the dependency in:\n{stdout}"
    );
}

/// Directory mode: the directory holds an entry that imports a shared
/// dependency. The dependency's diagnostic must be reported exactly once, not
/// once per importing root.
#[test]
fn b05_shared_dependency_diagnostic_not_duplicated_in_directory_mode() {
    let dir = corpus_path("imported-diagnostics", "dir");
    let result = run_check(dir, "json");
    let stdout = String::from_utf8_lossy(&result.stdout);

    let count = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter(|l| {
            let v: serde_json::Value =
                serde_json::from_str(l).expect("each NDJSON line must parse as JSON");
            v.get("id").and_then(|x| x.as_str()) == Some("G::analyze::missing-description")
        })
        .count();

    assert_eq!(
        count, 1,
        "the shared dependency's diagnostic must appear exactly once in directory mode, \
         found {count} occurrences in:\n{stdout}"
    );
}

/// JSON mode: an entry file imports two *different* dependency files that
/// happen to share the basename `dep.glyph` (under `dep_a/` and `dep_b/`).
/// Each carries a description-less skill. Their diagnostics' `span.file`
/// values must be distinct -- a bare basename cannot tell the two apart, so
/// the rendered path must include enough directory context.
#[test]
fn b05_same_basename_dependency_diagnostics_have_distinct_span_files_json() {
    let entry = corpus_path("imported-diagnostics", "samename/main.glyph");
    let result = run_check(entry, "json");
    let stdout = String::from_utf8_lossy(&result.stdout);

    let mut span_files: Vec<String> = Vec::new();
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each NDJSON line must parse as JSON");
        if v.get("id").and_then(|x| x.as_str()) == Some("G::analyze::missing-description") {
            let file = v
                .get("span")
                .and_then(|s| s.get("file"))
                .and_then(|f| f.as_str())
                .unwrap_or("")
                .to_string();
            span_files.push(file);
        }
    }

    assert_eq!(
        span_files.len(),
        2,
        "expected one missing-description diagnostic per dependency in:\n{stdout}"
    );
    assert_ne!(
        span_files[0], span_files[1],
        "two distinct dependency files sharing the basename `dep.glyph` must have \
         distinguishable `span.file` values, got {span_files:?}"
    );
}

/// B04 regression: `--format pretty` diagnostic carets must point at the column
/// the diagnostic span actually starts at, not near column 2.
///
/// Fixture `param_annotation_snake_case.glyph` line 1 is `block foo(x: link_mode)`;
/// the `type-case-violation` diagnostic spans `link_mode` (1-indexed byte columns
/// 14..=22), so the rendered caret must sit directly under `link_mode`.
#[test]
fn pretty_caret_points_at_diagnostic_start_column() {
    let src = corpus_path(
        "invalid",
        "case-violation/param_annotation_snake_case.glyph",
    );
    let result = run_check(src, "pretty");
    let stderr = strip_ansi(&String::from_utf8_lossy(&result.stderr));
    let lines: Vec<&str> = stderr.lines().collect();

    // Anchor on the single-line `type-case-violation` diagnostic, whose span is
    // `link_mode` at byte columns 14..=22 on source line 1.
    let header_idx = lines
        .iter()
        .position(|l| l.contains("G::analyze::type-case-violation"))
        .unwrap_or_else(|| panic!("did not find type-case-violation diagnostic in:\n{stderr}"));

    // Within that diagnostic block, find the rendered source line for line 1 and
    // the caret line that immediately follows it.
    let src_line_idx = lines[header_idx..]
        .iter()
        .position(|l| l.contains("block foo(x: link_mode)"))
        .map(|off| header_idx + off)
        .unwrap_or_else(|| panic!("did not find rendered source line 1 in:\n{stderr}"));
    let src_line = lines[src_line_idx];
    let caret_line = lines
        .get(src_line_idx + 1)
        .unwrap_or_else(|| panic!("no caret line after source line in:\n{stderr}"));
    assert!(
        caret_line.contains("^"),
        "expected a caret line after the source line, got: {caret_line:?}\n{stderr}"
    );

    // The caret line shares the same gutter width as the source line, so the
    // byte offset of the first caret must equal the byte offset of `link_mode`.
    let token_col = src_line
        .find("link_mode")
        .expect("rendered source line should contain `link_mode`");
    let caret_col = caret_line
        .find("^")
        .expect("caret line should contain a caret");

    assert_eq!(
        caret_col, token_col,
        "pretty caret must sit under `link_mode` (rendered col {token_col}), but it \
         rendered at col {caret_col}\n--- src   : {src_line:?}\n--- caret : {caret_line:?}\n{stderr}"
    );
}
