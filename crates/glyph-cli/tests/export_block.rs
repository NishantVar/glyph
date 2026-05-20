//! B03: export-block flow is fully semantic-checked.
//!
//! Each repro below exercises a single diagnostic that previously did not fire
//! for an `export block` (only for `block` / `skill`). The negative control
//! ensures a well-formed export block still exits 0 with no diagnostics.
//!
//! Bug audit ID: B03 (2026-05-20).

use std::path::PathBuf;
use std::process::{Command, Output};

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

/// Write `src` to a fresh tempdir as `repro.glyph` and run `glyph check
/// <path> --format json` on it. NDJSON on stdout is the contract used by the
/// existing `check_subcommand` tests.
fn run_check_on_source(src: &str) -> (Output, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().join("repro.glyph");
    std::fs::write(&path, src).expect("write fixture");
    let output = Command::new(glyph_bin())
        .arg("check")
        .arg(&path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("failed to spawn glyph binary");
    (output, dir)
}

/// Assert that `output.stdout` contains a JSON diagnostic with `id == diag_id`
/// and that `glyph check` exited with `expected_code`.
fn assert_diagnostic(output: &Output, expected_code: i32, diag_id: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(expected_code),
        "expected exit {expected_code} for diagnostic {diag_id};\nstdout={stdout}\nstderr={stderr}",
    );
    let mut found = false;
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each NDJSON line must parse as JSON");
        if v.get("id").and_then(|x| x.as_str()) == Some(diag_id) {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "expected diagnostic id {diag_id} in stdout;\nstdout={stdout}\nstderr={stderr}",
    );
}

// -- Repro 1 --------------------------------------------------------------
// `return missing()` calls an undeclared block. Should surface
// `G::analyze::undefined-call` (repairable -> exit 2).
#[test]
fn b03_repro1_return_undefined_call() {
    let src = "\
export block caller() -> Report
    flow:
        return missing()
";
    let (output, _dir) = run_check_on_source(src);
    assert_diagnostic(&output, 2, "G::analyze::undefined-call");
}

// -- Repro 2 --------------------------------------------------------------
// `return foo()` against a callee that requires arg `x`. Should surface
// `G::analyze::missing-required-arg` (error -> exit 1).
//
// Codex GAP 2 follow-up: the diagnostic span must pin the callee
// identifier (`foo`, 3 chars), not the entire export-block decl. The
// skill/block path already passes `target.span`; the export-block
// terminal-return path must match.
#[test]
fn b03_repro2_return_call_missing_required_arg() {
    let src = "\
export block foo(x: Input) -> Report
    flow:
        return \"ok\"

export block caller() -> Report
    flow:
        return foo()
";
    let (output, _dir) = run_check_on_source(src);
    assert_diagnostic(&output, 1, "G::analyze::missing-required-arg");

    // Assert the span pins the callee identifier — `foo`, length 3.
    // SourceSpan serializes as `{file, start: {line, col}, end: {line, col}}`,
    // so we check that start.line == end.line (single-line span) and
    // end.col - start.col == 3 (callee identifier width).
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut saw_callee_pinned = false;
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).expect("ndjson");
        if v.get("id").and_then(|x| x.as_str()) != Some("G::analyze::missing-required-arg") {
            continue;
        }
        let span = v.get("span").expect("diagnostic must carry a span");
        let start_line = span["start"]["line"].as_u64().expect("span.start.line");
        let end_line = span["end"]["line"].as_u64().expect("span.end.line");
        let start_col = span["start"]["col"].as_u64().expect("span.start.col");
        let end_col = span["end"]["col"].as_u64().expect("span.end.col");
        assert_eq!(
            start_line, end_line,
            "callee-pinned span must be single-line; got start_line={start_line} end_line={end_line};\nstdout={stdout}",
        );
        let width = end_col - start_col + 1;
        assert_eq!(
            width, 3,
            "expected missing-required-arg span width 3 (callee `foo`); got start_col={start_col} end_col={end_col} width={width};\nstdout={stdout}",
        );
        saw_callee_pinned = true;
        break;
    }
    assert!(
        saw_callee_pinned,
        "no missing-required-arg diagnostic with a span; stdout={stdout}"
    );
}

// -- Repro 3 --------------------------------------------------------------
// Caller declared `-> Report` but `return foo()` where `foo() -> Plan`.
// Should surface `G::analyze::nominal-mismatch` (error -> exit 1).
#[test]
fn b03_repro3_return_call_nominal_mismatch() {
    let src = "\
export block foo() -> Plan
    flow:
        return \"plan\"

export block caller() -> Report
    flow:
        return foo()
";
    let (output, _dir) = run_check_on_source(src);
    assert_diagnostic(&output, 1, "G::analyze::nominal-mismatch");
}

// -- Repro 4 --------------------------------------------------------------
// `require missing_text` where `missing_text` is not a declared `const`.
// Should surface `G::analyze::undefined-name` (error -> exit 1).
#[test]
fn b03_repro4_require_undefined_name() {
    let src = "\
export block caller() -> Report
    require missing_text
    flow:
        return \"ok\"
";
    let (output, _dir) = run_check_on_source(src);
    assert_diagnostic(&output, 1, "G::analyze::undefined-name");
}

// -- Repro 5 --------------------------------------------------------------
// Two `return` statements at the flow root. Should surface
// `G::parse::multiple-returns` (error -> exit 1).
#[test]
fn b03_repro5_multiple_returns() {
    let src = "\
export block caller() -> Report
    flow:
        return \"a\"
        return \"b\"
";
    let (output, _dir) = run_check_on_source(src);
    assert_diagnostic(&output, 1, "G::parse::multiple-returns");
}

// -- Repro 6 --------------------------------------------------------------
// `return` followed by another root-level statement. Should surface
// `G::parse::return-not-terminal` (error -> exit 1).
#[test]
fn b03_repro6_return_not_terminal() {
    let src = "\
export block caller() -> Report
    flow:
        return \"a\"
        \"after\"
";
    let (output, _dir) = run_check_on_source(src);
    assert_diagnostic(&output, 1, "G::parse::return-not-terminal");
}

// -- Repro 7 --------------------------------------------------------------
// `return` nested inside an `if` branch body. Should surface
// `G::parse::return-in-branch` (error -> exit 1).
#[test]
fn b03_repro7_return_in_branch() {
    let src = "\
export block caller() -> Report
    flow:
        if \"x\":
            return \"a\"
        return \"b\"
";
    let (output, _dir) = run_check_on_source(src);
    assert_diagnostic(&output, 1, "G::parse::return-in-branch");
}

// -- Repro 8 (Codex GAP 1) -----------------------------------------------
// A standalone (non-return) flow-call at the flow root that targets an
// undeclared block. Should surface `G::analyze::undefined-call`
// (repairable -> exit 2). Pre-fix the export-block path only validated
// the terminal return; standalone calls passed silently.
#[test]
fn b03_repro8_root_nonreturn_undefined_call() {
    let src = "\
export block caller() -> Report
    flow:
        missing()
        return \"ok\"
";
    let (output, _dir) = run_check_on_source(src);
    assert_diagnostic(&output, 2, "G::analyze::undefined-call");
}

// -- Repro 9 (Codex GAP 1) -----------------------------------------------
// A standalone (non-return) flow-call at the flow root that targets a
// known callee but omits a required positional argument. Should surface
// `G::analyze::missing-required-arg` (error -> exit 1).
#[test]
fn b03_repro9_root_nonreturn_missing_required_arg() {
    let src = "\
export block foo(x: Input) -> Report
    flow:
        return \"ok\"

export block caller() -> Report
    flow:
        foo()
        return \"ok\"
";
    let (output, _dir) = run_check_on_source(src);
    assert_diagnostic(&output, 1, "G::analyze::missing-required-arg");
}

// -- Repro 10 (Codex GAP 1) ----------------------------------------------
// A non-return flow-call inside an `if` branch body that targets an
// undeclared block. Should surface `G::analyze::undefined-call`
// (repairable -> exit 2). Symmetric to repro 8 but exercises branch-
// body collection — the scanner must include calls nested inside
// `if`/`elif`/`else` bodies.
#[test]
fn b03_repro10_branch_body_nonreturn_undefined_call() {
    let src = "\
export block caller() -> Report
    flow:
        if \"x\":
            missing()
        return \"ok\"
";
    let (output, _dir) = run_check_on_source(src);
    assert_diagnostic(&output, 2, "G::analyze::undefined-call");
}

// -- Negative control ----------------------------------------------------
// A well-formed export block that calls another well-formed export block at
// `return` should produce no diagnostics and exit 0. This guards against the
// fix being over-broad.
#[test]
fn b03_negative_well_formed_export_block_is_clean() {
    let src = "\
export block helper() -> Report
    flow:
        return \"ok\"

export block caller() -> Report
    flow:
        return helper()
";
    let (output, _dir) = run_check_on_source(src);
    assert_eq!(
        output.status.code(),
        Some(0),
        "well-formed export block must exit 0;\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().is_empty(),
        "well-formed export block must produce no diagnostics, got: {stdout}",
    );
}

/// Run `glyph check` against `main.glyph`, which imports from `dep.glyph`.
/// Both files are written to a fresh tempdir so the relative `import "./dep.glyph"`
/// path resolves. Returns stdout/stderr from the JSON-format check.
fn run_check_on_two_files(dep_src: &str, main_src: &str) -> (Output, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let dep_path = dir.path().join("dep.glyph");
    let main_path = dir.path().join("main.glyph");
    std::fs::write(&dep_path, dep_src).expect("write dep.glyph");
    std::fs::write(&main_path, main_src).expect("write main.glyph");
    let output = Command::new(glyph_bin())
        .arg("check")
        .arg(&main_path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("failed to spawn glyph binary");
    (output, dir)
}

/// Repro 11 — B03 GAP 3, dotted-method case.
///
/// `helper.applies()` in flow position must NOT be harvested as a standalone
/// `applies()` call. Pre-fix, the body walker's `Ident` arm saw `applies`
/// followed by `(` and collected it into `flow_calls` because the previous-
/// token `Dot` was ignored, firing `G::analyze::undefined-call` for `applies`.
/// Post-fix, the harvest gate skips identifiers whose previous token is
/// `TokenKind::Dot`, so the diagnostic does NOT fire.
#[test]
fn b03_repro11_dotted_method_not_collected_as_call() {
    let src = "\
export block helper() -> Report
    flow:
        return \"ok\"

export block caller() -> Report
    flow:
        helper.applies()
        return \"ok\"
";
    let (output, _dir) = run_check_on_source(src);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each NDJSON line must parse as JSON");
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
        let msg = v.get("message").and_then(|x| x.as_str()).unwrap_or("");
        if id == "G::analyze::undefined-call" && msg.contains("applies") {
            panic!(
                "post-fix must not emit undefined-call for `applies` (dotted-method position);\nstdout={stdout}\nstderr={stderr}",
            );
        }
    }
}

/// Repro 12 — B03 GAP 4, terminal_return case.
///
/// An imported block consumed in an export block's `terminal_return` must
/// mark the import as used. Pre-fix, `ExportBlockDecl` had no
/// `flow: Vec<FlowStmt>` so the per-Skill / per-Block `track_flow_usage`
/// sweep did not reach export blocks; the lib.rs unused-import emission step
/// then fired `G::analyze::unused-import` (Repairable, exit 2) on
/// `dep_helper`. Post-fix, the GAP-4 export-block usage sweep marks
/// `dep_helper` as used.
#[test]
fn b03_repro12_imported_return_call_marks_import_used() {
    let dep_src = "\
export block dep_helper() -> Report
    flow:
        return \"ok\"
";
    let main_src = "\
import \"./dep.glyph\" { dep_helper }

export block runner() -> Report
    flow:
        return dep_helper()
";
    let (output, _dir) = run_check_on_two_files(dep_src, main_src);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each NDJSON line must parse as JSON");
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
        let msg = v.get("message").and_then(|x| x.as_str()).unwrap_or("");
        if id == "G::analyze::unused-import" && msg.contains("dep_helper") {
            panic!(
                "post-fix must not emit unused-import for `dep_helper` (used in terminal_return);\nstdout={stdout}\nstderr={stderr}",
            );
        }
    }
}

/// Repro 13 — B03 GAP 4, flow_calls (non-return) case.
///
/// An imported block consumed in an export block's non-return
/// `flow_calls` (e.g. a standalone root-level call before the terminal
/// `return`) must mark the import as used. Same diagnostic gap as
/// repro 12 — closed by the same GAP-4 sweep over `eb.flow_calls`.
#[test]
fn b03_repro13_imported_nonreturn_call_marks_import_used() {
    let dep_src = "\
export block dep_side_effect() -> Report
    flow:
        return \"ok\"
";
    let main_src = "\
import \"./dep.glyph\" { dep_side_effect }

export block runner() -> Report
    flow:
        dep_side_effect()
        return \"ok\"
";
    let (output, _dir) = run_check_on_two_files(dep_src, main_src);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each NDJSON line must parse as JSON");
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
        let msg = v.get("message").and_then(|x| x.as_str()).unwrap_or("");
        if id == "G::analyze::unused-import" && msg.contains("dep_side_effect") {
            panic!(
                "post-fix must not emit unused-import for `dep_side_effect` (used in flow_calls);\nstdout={stdout}\nstderr={stderr}",
            );
        }
    }
}

/// Repro 14 — B03 GAP 4, body_constraints case.
///
/// An imported text const consumed in an export block's
/// `body_constraints` (e.g. `require imported_const`) must mark the
/// import as used. Same diagnostic gap as repros 12/13 — closed by
/// the GAP-4 sweep over `eb.body_constraints`.
#[test]
fn b03_repro14_imported_constraint_marks_import_used() {
    let dep_src = "\
export const dep_rule = \"Be accurate.\"
";
    let main_src = "\
import \"./dep.glyph\" { dep_rule }

export block runner() -> Report
    require dep_rule
    flow:
        return \"ok\"
";
    let (output, _dir) = run_check_on_two_files(dep_src, main_src);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each NDJSON line must parse as JSON");
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
        let msg = v.get("message").and_then(|x| x.as_str()).unwrap_or("");
        if id == "G::analyze::unused-import" && msg.contains("dep_rule") {
            panic!(
                "post-fix must not emit unused-import for `dep_rule` (used in body_constraints);\nstdout={stdout}\nstderr={stderr}",
            );
        }
    }
}

/// Repro 15 — B03 GAP 5, applies-on-non-block (unknown receiver).
///
/// An `if missing.applies():` condition in an export-block flow must run
/// the same semantic check as Skill/Block flow conditions. Pre-fix,
/// `analyze_export_block` did not iterate condition expressions, so the
/// `.applies()` validator (`check_applies_in_condition`) never ran and
/// `G::analyze::applies-on-non-block` (Error) was NOT emitted. Post-fix,
/// the export-block analyzer walks `decl.condition_refs` and invokes the
/// validator with an empty flow-local-type map.
#[test]
fn b03_repro15_condition_applies_on_unknown_receiver() {
    let src = "\
export block caller() -> Report
    flow:
        if missing.applies():
            \"branch-noop\"
        return \"ok\"
";
    let (output, _dir) = run_check_on_source(src);
    assert_diagnostic(&output, 1, "G::analyze::applies-on-non-block");
}

/// Repro 16 — B03 GAP 5, applies-on-non-block (text-const receiver).
///
/// `if greeting.applies():` where `greeting` is an `export const` (a text
/// declaration, not a block) must fire `G::analyze::applies-on-non-block`.
/// Same diagnostic gap as repro 15 — closed by the same condition_refs
/// sweep through `check_applies_in_condition`.
#[test]
fn b03_repro16_condition_applies_on_text_const_receiver() {
    let src = "\
export const greeting = \"Hello\"

export block caller() -> Report
    flow:
        if greeting.applies():
            \"branch-noop\"
        return \"ok\"
";
    let (output, _dir) = run_check_on_source(src);
    assert_diagnostic(&output, 1, "G::analyze::applies-on-non-block");
}

/// Repro 17 — B03 GAP 5, imported-block usage in condition.
///
/// An imported block consumed ONLY in an export-block's if/elif
/// condition (`if ready.applies():`) must mark the import as used.
/// Pre-fix, the GAP-4 sweep walked terminal_return / flow_calls /
/// body_constraints / body_context but NOT condition expressions, so
/// `G::analyze::unused-import` (Repairable, exit 2) fired on `ready`.
/// Post-fix, the GAP-5 extension to the sweep walks condition_refs and
/// inserts every imported block/text referenced in a condition into
/// `used_import_names`.
#[test]
fn b03_repro17_imported_block_used_in_condition_not_unused() {
    let dep_src = "\
export block ready() -> Report
    description: \"always ready\"
    flow:
        return \"yes\"
";
    let main_src = "\
import \"./dep.glyph\" { ready }

export block runner() -> Report
    flow:
        if ready.applies():
            \"branch-noop\"
        return \"idle\"
";
    let (output, _dir) = run_check_on_two_files(dep_src, main_src);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // B03 GAP 6 strengthening: the pre-strengthening assertion only checked
    // that `unused-import` was absent — it did NOT catch
    // `G::analyze::applies-on-undescribed-block` (which fires when GAP-5's
    // condition validator sees an empty `imported_block_descriptions` map).
    // Demand exit 0 + zero diagnostic lines so the absence of a description
    // plumb is now a hard failure.
    assert_eq!(
        output.status.code(),
        Some(0),
        "imported described block used in if-condition must be clean (exit 0);\nstdout={stdout}\nstderr={stderr}",
    );
    assert!(
        stdout.trim().is_empty(),
        "expected zero diagnostics for imported-described-block-in-condition;\nstdout={stdout}\nstderr={stderr}",
    );
}

/// B03 GAP 7 — Codex round-5 repro: the export-block import-usage sweep
/// previously split `cref.raw` on non-identifier chars, which matched
/// identifiers inside string literal contents. So an imported const whose
/// name happens to appear only inside a `"..."` literal in a condition
/// (e.g. `if risk == "ready":`) was wrongly marked as used and
/// `G::analyze::unused-import` never fired.
///
/// Post-fix the sweep uses `condition::tokenize_condition`, which yields
/// string literals as a single `"..."` token (quotes preserved). The
/// receiver-extraction below then probes the QUOTED token against the
/// import sets and finds no match, so the imported name correctly stays
/// unused.
#[test]
fn b03_repro18_imported_const_in_string_literal_only_still_unused() {
    let dep_src = "\
export const ready = \"yes\"
";
    let main_src = "\
import \"./dep.glyph\" { ready }

export block runner(risk: String) -> Report
    flow:
        if risk == \"ready\":
            \"branch-noop\"
        return \"idle\"
";
    let (output, _dir) = run_check_on_two_files(dep_src, main_src);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut found_unused = false;
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each NDJSON line must parse as JSON");
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
        let msg = v.get("message").and_then(|x| x.as_str()).unwrap_or("");
        if id == "G::analyze::unused-import" && msg.contains("ready") {
            found_unused = true;
            break;
        }
    }
    assert!(
        found_unused,
        "expected G::analyze::unused-import for `ready` (only present inside a string literal);\nstdout={stdout}\nstderr={stderr}",
    );
}

/// B03 GAP 7 — Codex round-5 repro: the export-block import-usage sweep
/// previously matched any bare identifier in `cref.raw`, including the
/// method name following `.` in a `name.applies()` call. So an imported
/// name appearing only as a `.applies` method token (e.g. `if cond.applies():`
/// where `cond` is a local block and `applies` is an imported const) was
/// wrongly marked as used.
///
/// Post-fix the sweep uses `condition::tokenize_condition`, which keeps
/// `name.applies()` as ONE token, then strips the `.applies()` suffix to
/// recover just `name`. So an imported name that only appears as the
/// method token is correctly never marked as used.
#[test]
fn b03_repro19_imported_name_as_method_token_only_still_unused() {
    let dep_src = "\
export const applies = \"yes\"
";
    let main_src = "\
import \"./dep.glyph\" { applies }

block cond() -> Report
    description: \"check\"
    flow:
        return \"yes\"

export block runner() -> Report
    flow:
        if cond.applies():
            \"branch-noop\"
        return \"idle\"
";
    let (output, _dir) = run_check_on_two_files(dep_src, main_src);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut found_unused = false;
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each NDJSON line must parse as JSON");
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
        let msg = v.get("message").and_then(|x| x.as_str()).unwrap_or("");
        if id == "G::analyze::unused-import" && msg.contains("applies") {
            found_unused = true;
            break;
        }
    }
    assert!(
        found_unused,
        "expected G::analyze::unused-import for `applies` (only present as the `.applies()` method token);\nstdout={stdout}\nstderr={stderr}",
    );
}

/// B03 GAP 8 — Codex round-6 repro: whole-module alias (`import ... as
/// dep`) where the export-block condition uses the dotted receiver
/// `dep.ready.applies()` per GLYPH_LANGUAGE_GUIDE §8.7. Pre-fix the
/// substring scanner stripped to just `ready`, then attempted to resolve
/// it against same-file `block_names`/`text_names`, which fail, so it
/// fired `applies-on-non-block` and (because the bare alias `dep` was
/// never marked used by the sweep) `unused-import` on `dep`.
///
/// Post-fix `check_applies_in_condition` walks the tokenized condition
/// so the receiver is the FULL `dep.ready` (one token), which matches
/// the `dep.ready` compound key recorded in `imported_block_descriptions`.
/// The sweep also marks the bare alias `dep` as used whenever the
/// receiver contains `.`. So this should be a clean exit-0 run with
/// zero diagnostic lines.
#[test]
fn b03_repro20_whole_module_alias_applies_resolves() {
    let dep_src = "\
export block ready() -> Report
    description: \"always ready\"
    flow:
        return \"yes\"
";
    let main_src = "\
import \"./dep.glyph\" as dep

export block runner() -> Report
    flow:
        if dep.ready.applies():
            \"branch-noop\"
        return \"idle\"
";
    let (output, _dir) = run_check_on_two_files(dep_src, main_src);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "whole-module-alias `.applies()` on described imported block must be clean (exit 0);\nstdout={stdout}\nstderr={stderr}",
    );
    assert!(
        stdout.trim().is_empty(),
        "expected zero diagnostics for whole-module-alias `.applies()` on described imported block;\nstdout={stdout}\nstderr={stderr}",
    );
}

/// B03 GAP 10 — Codex round-7 repro: whole-module-alias `.applies()`
/// receiver resolves to an imported block that lacks a `description:`
/// sub-section. Pre-fix, `check_applies_in_condition`'s dotted-receiver
/// branch only consulted `imported_block_descriptions` — so an undescribed
/// imported block (NOT in the description map) fell through to the same
/// `applies-on-non-block` diagnostic the validator emits for genuinely
/// unresolved receivers. The correct ID is
/// `applies-on-undescribed-block` (matching the selective-import branch).
///
/// Post-fix, the dotted-receiver branch does a three-way check:
///   - `imported_block_descriptions` hit → OK
///   - `imported_blocks` hit (no description)
///       → `G::analyze::applies-on-undescribed-block` (Error,
///         imported-no-repair — repair is single-file)
///   - neither → existing `G::analyze::applies-on-non-block`
#[test]
fn b03_repro24_whole_module_undescribed_block_undescribed_diagnostic() {
    let dep_src = "\
export block ready() -> Report
    flow:
        return \"yes\"
";
    let main_src = "\
import \"./dep.glyph\" as dep

export block runner() -> Report
    flow:
        if dep.ready.applies():
            \"branch-noop\"
        return \"idle\"
";
    let (output, _dir) = run_check_on_two_files(dep_src, main_src);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "whole-module-alias `.applies()` on undescribed imported block must exit 1;\nstdout={stdout}\nstderr={stderr}",
    );
    let mut found = false;
    let mut wrong_ids: Vec<String> = Vec::new();
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each NDJSON line must parse as JSON");
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
        if id == "G::analyze::applies-on-undescribed-block" {
            found = true;
        } else if id == "G::analyze::applies-on-non-block" {
            wrong_ids.push(id.to_string());
        }
    }
    assert!(
        wrong_ids.is_empty(),
        "undescribed imported block must NOT emit `applies-on-non-block` (that's the unresolved-receiver shape); saw {wrong_ids:?}\nstdout={stdout}\nstderr={stderr}",
    );
    assert!(
        found,
        "expected G::analyze::applies-on-undescribed-block diagnostic;\nstdout={stdout}\nstderr={stderr}",
    );
}

/// B03 GAP 9 — Codex round-6 repro: malformed `.applies` in an
/// export-block `if` condition. `.applies` without parens
/// (e.g. `if x.applies:`) is invalid per `parse_branch_condition`'s
/// validator, but the export-block body-walker path never reached
/// that validator — so the diagnostic was silently dropped (exit 0).
///
/// Post-fix, parse.rs walks the captured if/elif condition tokens
/// after the outer if-let-Ident closes and fires
/// `G::parse::applies-no-parens` when an `Ident("applies")` token
/// preceded by `Dot` is NOT followed by `Lparen`.
#[test]
fn b03_repro21_export_condition_applies_no_parens() {
    let src = "\
block cond() -> Report
    description: \"check\"
    flow:
        return \"yes\"

export block runner() -> Report
    flow:
        if cond.applies:
            \"branch-noop\"
        return \"idle\"
";
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().join("main.glyph");
    std::fs::write(&path, src).expect("write main.glyph");
    let output = Command::new(glyph_bin())
        .arg("check")
        .arg(&path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("failed to spawn glyph binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "malformed `.applies` (no parens) in export-block condition must exit 1;\nstdout={stdout}\nstderr={stderr}",
    );
    let mut found = false;
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each NDJSON line must parse as JSON");
        if v.get("id").and_then(|x| x.as_str()) == Some("G::parse::applies-no-parens") {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "expected G::parse::applies-no-parens diagnostic;\nstdout={stdout}\nstderr={stderr}",
    );
}

/// B03 GAP 9 — Codex round-6 repro: malformed `.applies` in an
/// export-block `if` condition. `.applies(arg)` with arguments is
/// invalid per `parse_branch_condition`'s validator, but the
/// export-block body-walker path never reached that validator — so
/// the diagnostic was silently dropped (exit 0).
///
/// Post-fix, parse.rs walks the captured if/elif condition tokens
/// after the outer if-let-Ident closes and fires
/// `G::parse::applies-with-args` when an `Ident("applies")` token
/// preceded by `Dot` is followed by `Lparen` whose immediate next
/// token is NOT `Rparen`.
#[test]
fn b03_repro22_export_condition_applies_with_args() {
    let src = "\
block cond() -> Report
    description: \"check\"
    flow:
        return \"yes\"

export block runner() -> Report
    flow:
        if cond.applies(\"foo\"):
            \"branch-noop\"
        return \"idle\"
";
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().join("main.glyph");
    std::fs::write(&path, src).expect("write main.glyph");
    let output = Command::new(glyph_bin())
        .arg("check")
        .arg(&path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("failed to spawn glyph binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "malformed `.applies(...)` (with args) in export-block condition must exit 1;\nstdout={stdout}\nstderr={stderr}",
    );
    let mut found = false;
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each NDJSON line must parse as JSON");
        if v.get("id").and_then(|x| x.as_str()) == Some("G::parse::applies-with-args") {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "expected G::parse::applies-with-args diagnostic;\nstdout={stdout}\nstderr={stderr}",
    );
}

/// B03 GAP 8 bonus — `check_applies_in_condition` previously
/// substring-scanned for `.applies()` in the raw condition text, so
/// a string literal like `if x == ".applies()":` triggered a false
/// positive `G::analyze::applies-on-non-block`. Post-fix, the
/// validator walks `condition::tokenize_condition`, which yields
/// string literals as one quoted `"..."` token — the `tok.starts_with('"')`
/// skip means substring scans inside literal values are now ignored.
#[test]
fn b03_repro23_string_literal_applies_no_false_positive() {
    let src = "\
export block runner(risk: String) -> Report
    flow:
        if risk == \"ready.applies()\":
            \"branch-noop\"
        return \"idle\"
";
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().join("main.glyph");
    std::fs::write(&path, src).expect("write main.glyph");
    let output = Command::new(glyph_bin())
        .arg("check")
        .arg(&path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("failed to spawn glyph binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each NDJSON line must parse as JSON");
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
        assert_ne!(
            id, "G::analyze::applies-on-non-block",
            "string-literal containing `.applies()` must not trigger applies-on-non-block;\nstdout={stdout}\nstderr={stderr}",
        );
    }
}
