//! Regression test for B04 — pretty diagnostics point to the wrong columns.
//!
//! JSON diagnostics carry the start column from `Span::start.col`. The pretty
//! renderer turns that (line, col) back into a byte range and hands it to
//! codespan-reporting, which prints `path:line:col` in the diagnostic header
//! and draws a caret under that column. Before the B04 fix, the byte range was
//! computed by a `locate_byte` walker that bailed at the requested *line*
//! before advancing through the column, so pretty output collapsed every span
//! whose start column was > 1 to column 2.
//!
//! This test runs `glyph check` twice against a fixture with multiple
//! diagnostics whose start columns are deeper into their lines, and asserts
//! that the `line:col` in the pretty header matches the JSON `start` line/col
//! for every diagnostic id.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, Output};

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn corpus(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("invalid")
        .join("case-violation")
        .join(name)
}

fn run_check(file: &PathBuf, format: &str) -> Output {
    Command::new(glyph_bin())
        .arg("check")
        .arg(file)
        .arg("--format")
        .arg(format)
        .output()
        .expect("failed to spawn glyph binary")
}

/// Strip CSI ANSI escape sequences (`ESC [ ... <final byte>`). codespan-
/// reporting colorizes the pretty output even when stderr is not a TTY in
/// some configurations; tests must not rely on color state.
fn strip_ansi(s: &str) -> String {
    // Strip CSI escape sequences (`ESC [ ... <final byte 0x40-0x7E>`).
    // Iterate by char (not byte) so multi-byte UTF-8 — e.g. the codespan-
    // reporting `┌─` box-drawing glyphs — survives the strip intact.
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next(); // consume '['
            while let Some(&c) = chars.peek() {
                if ('\u{30}'..='\u{3F}').contains(&c) {
                    chars.next();
                } else {
                    break;
                }
            }
            while let Some(&c) = chars.peek() {
                if ('\u{20}'..='\u{2F}').contains(&c) {
                    chars.next();
                } else {
                    break;
                }
            }
            chars.next(); // consume the final byte (or stop if exhausted)
        } else {
            out.push(c);
        }
    }
    out
}

fn collect_json_starts(stdout: &str) -> BTreeMap<String, (u32, u32)> {
    let mut out = BTreeMap::new();
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each NDJSON line must parse as JSON");
        let id = v
            .get("id")
            .and_then(|x| x.as_str())
            .expect("diagnostic id")
            .to_string();
        let start = v
            .get("span")
            .and_then(|s| s.get("start"))
            .expect("span.start");
        let l = start.get("line").and_then(|x| x.as_u64()).unwrap() as u32;
        let c = start.get("col").and_then(|x| x.as_u64()).unwrap() as u32;
        out.entry(id).or_insert((l, c));
    }
    out
}

fn collect_pretty_starts(stderr_plain: &str) -> BTreeMap<String, (u32, u32)> {
    let mut out = BTreeMap::new();
    let lines: Vec<&str> = stderr_plain.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let id = parse_title_id(line);
        if let Some(id) = id {
            let mut j = i + 1;
            while j < lines.len() {
                let l = lines[j];
                if let Some((lno, col)) = parse_header_linecol(l) {
                    out.entry(id.clone()).or_insert((lno, col));
                    break;
                }
                if parse_title_id(l).is_some() {
                    break;
                }
                j += 1;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

fn parse_title_id(line: &str) -> Option<String> {
    let line = line.trim_start();
    for prefix in ["error[", "warning[", "note[", "help["] {
        if let Some(rest) = line.strip_prefix(prefix) {
            if let Some(end) = rest.find(']') {
                return Some(rest[..end].to_string());
            }
        }
    }
    None
}

fn parse_header_linecol(line: &str) -> Option<(u32, u32)> {
    let marker = "┌─ ";
    let idx = line.find(marker)?;
    let after = &line[idx + marker.len()..];
    let after = after.trim_end();
    let (rest, col) = after.rsplit_once(':')?;
    let (_path, lno) = rest.rsplit_once(':')?;
    Some((lno.parse().ok()?, col.parse().ok()?))
}

#[test]
fn pretty_caret_columns_match_json() {
    let src = corpus("param_annotation_snake_case.glyph");

    let json_out = run_check(&src, "json");
    let json_stdout = String::from_utf8(json_out.stdout).expect("json stdout should be UTF-8");
    let json_starts = collect_json_starts(&json_stdout);

    assert!(
        json_starts.values().any(|(_, c)| *c > 2),
        "fixture should expose at least one diagnostic with start col > 2; got {json_starts:?}\nstdout:\n{json_stdout}",
    );

    let pretty_out = run_check(&src, "pretty");
    let pretty_stderr_raw =
        String::from_utf8(pretty_out.stderr).expect("pretty stderr should be UTF-8");
    let pretty_stderr = strip_ansi(&pretty_stderr_raw);
    let pretty_starts = collect_pretty_starts(&pretty_stderr);

    for (id, (jl, jc)) in &json_starts {
        let (pl, pc) = pretty_starts.get(id).unwrap_or_else(|| {
            panic!(
                "pretty output missing diagnostic `{id}`\n--- pretty stderr ---\n{pretty_stderr}\n--- json stdout ---\n{json_stdout}"
            )
        });
        assert_eq!(
            (jl, jc),
            (pl, pc),
            "pretty `line:col` mismatched JSON for id `{id}`: json={jl}:{jc}, pretty={pl}:{pc}\n--- pretty stderr ---\n{pretty_stderr}\n--- json stdout ---\n{json_stdout}",
        );
    }
}
