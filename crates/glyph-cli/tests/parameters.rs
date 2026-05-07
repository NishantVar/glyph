//! Slice 4 integration tests — parameters and `## Parameters` section.
//!
//! Covers the original slice-4 acceptance criteria from `mvp-issues.md`,
//! updated by PRD #103 / Slice 2 (#105):
//!   1. Skill parameters with defaults parse and lower without diagnostics.
//!   2. Compiled output emits a `## Parameters` section between frontmatter and
//!      `## Instructions` whose entries match the design contract:
//!      `(default: <value>)` for defaulted parameters, `(required)` for skill
//!      parameters without a default.
//!   3. `export block` parameters without a default are now permitted
//!      (`G::analyze::missing-param-default` retired by #105). Call-site
//!      enforcement lives under `G::analyze::missing-required-arg` —
//!      validated for private blocks (#104), same-file export blocks (#105
//!      slice A), and imported export blocks (#105 slice C, see
//!      `imports.rs::imported_export_block_missing_required_arg_exit_1`).
//!   4. `{name}` slot inside an instruction string that does not match a
//!      declared parameter surfaces `G::analyze::unknown-param-slot`.
//!   5. `{name}` slot inside `description:` (non-instruction-bearing) surfaces
//!      `G::parse::param-slot-in-non-instruction-string` (repairable).
//!   6. Parameterless skill (`update_docs`) snapshot is unchanged — no
//!      `## Parameters` section is emitted when the skill declares no params.

use std::path::PathBuf;
use std::process::{Command, Output};

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn fixture(subdir: &str, name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join(subdir)
        .join(name)
}

fn run_compile(path: PathBuf, format: &str) -> Output {
    Command::new(glyph_bin())
        .arg("compile")
        .arg(path)
        .arg("--format")
        .arg(format)
        .output()
        .expect("failed to spawn glyph binary")
}

fn run_check(path: PathBuf, format: &str) -> Output {
    Command::new(glyph_bin())
        .arg("check")
        .arg(path)
        .arg("--format")
        .arg(format)
        .output()
        .expect("failed to spawn glyph binary")
}

fn assert_has_diagnostic_id(stdout: &str, id: &str) {
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
fn skill_with_params_compiles_and_emits_parameters_section() {
    let src = fixture("valid", "with_params.glyph");
    // Clean any previous artifact.
    let out = src.with_file_name("with_params.md");
    let _ = std::fs::remove_file(&out);

    let result = run_compile(src.clone(), "json");
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let md = std::fs::read_to_string(&out).expect("compiled .md file is missing");

    // `## Parameters` appears between frontmatter and `## Instructions`.
    let frontmatter_end = md.find("---\n\n").expect("frontmatter terminator");
    let params_idx = md.find("## Parameters").expect("`## Parameters` section");
    let instructions_idx = md.find("## Instructions").expect("`## Instructions` section");
    assert!(
        frontmatter_end < params_idx && params_idx < instructions_idx,
        "section ordering: frontmatter -> ## Parameters -> ## Instructions; got md=\n{}",
        md
    );

    // Defaulted parameter renders with `. Default: <X>.`; required parameter
    // renders with `. Required.`.
    assert!(
        md.contains("- **scope**. Default: \".\"."),
        "expected defaulted scope parameter; got md=\n{}",
        md
    );
    assert!(
        md.contains("- **target**. Required."),
        "expected required target parameter; got md=\n{}",
        md
    );
}

#[test]
fn export_block_without_default_compiles_cleanly() {
    // PRD #103 / Slice 2 (#105): export-block parameters may now omit a
    // default. The retired rule `G::analyze::missing-param-default` no
    // longer fires; an export block declared without a default value for a
    // parameter compiles successfully.
    let src = fixture("valid", "export_block_no_default.glyph");
    // Clean any previous artifact.
    let out = src.with_file_name("export_block_no_default.md");
    let _ = std::fs::remove_file(&out);

    let result = run_compile(src, "json");
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert!(
        !stdout.contains("G::analyze::missing-param-default"),
        "retired diagnostic must not appear in stdout: {}",
        stdout
    );
}

#[test]
fn missing_required_arg_at_call_site_emits_analyze_diagnostic() {
    // PRD #103 / Slice 1 (#104): a private `block` with a required parameter
    // called with no argument must surface `G::analyze::missing-required-arg`
    // (hard error → exit 1). The diagnostic span must pin the offending
    // callee identifier (line 5 in the fixture), not the enclosing skill
    // header (line 1) — otherwise a skill with multiple calls cannot tell
    // the IDE which call is broken.
    let src = fixture("invalid", "missing_required_arg.glyph");
    let result = run_compile(src, "json");
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit 1; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_has_diagnostic_id(&stdout, "G::analyze::missing-required-arg");

    let diag = stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v.get("id").and_then(|x| x.as_str()) == Some("G::analyze::missing-required-arg"))
        .expect("diagnostic must be present");
    let span = &diag["span"];
    assert_eq!(
        span["start"]["line"].as_u64(),
        Some(5),
        "diagnostic must pin the call line (5), got span={}",
        span
    );
}

#[test]
fn export_block_missing_required_arg_at_call_site_emits_analyze_diagnostic() {
    // PRD #103 / Slice 2 (#105): export-block parameters may now omit a
    // default. A caller that omits the corresponding positional argument
    // surfaces `G::analyze::missing-required-arg` (hard error → exit 1)
    // at the call site span — mirrors the private-`block` behavior wired
    // by Slice 1 (#104).
    let src = fixture("invalid", "export_block_missing_required_arg.glyph");
    let result = run_compile(src, "json");
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit 1; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_has_diagnostic_id(&stdout, "G::analyze::missing-required-arg");

    let diag = stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v.get("id").and_then(|x| x.as_str()) == Some("G::analyze::missing-required-arg"))
        .expect("diagnostic must be present");
    let span = &diag["span"];
    assert_eq!(
        span["start"]["line"].as_u64(),
        Some(5),
        "diagnostic must pin the call line (5), got span={}",
        span
    );
}

#[test]
fn return_call_missing_required_arg_emits_analyze_diagnostic() {
    // Codex P2 follow-up to #105: a `return helper()` whose callee has a
    // required parameter must surface `G::analyze::missing-required-arg`,
    // not silently compile. Pre-fix the FlowStmt::Return arm only ran the
    // undefined-call + nominal-mismatch checks.
    let src = fixture("invalid", "return_call_missing_required_arg.glyph");
    let result = run_compile(src, "json");
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit 1; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_has_diagnostic_id(&stdout, "G::analyze::missing-required-arg");
}

#[test]
fn branch_call_missing_required_arg_emits_analyze_diagnostic() {
    // Codex P2 follow-up to #105: a call inside an `if` branch body must
    // run the same required-arg check as a top-level call. Pre-fix the
    // branch-body walker only verified name resolution.
    let src = fixture("invalid", "branch_call_missing_required_arg.glyph");
    let result = run_compile(src, "json");
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit 1; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_has_diagnostic_id(&stdout, "G::analyze::missing-required-arg");
}

#[test]
fn return_to_same_file_export_block_with_type_mismatch_emits_nominal_mismatch() {
    // Codex P2 follow-up to #105: now that same-file export blocks are
    // legal call targets, `return helper()` must run the nominal-match
    // check against the export-block's return type. Pre-fix the
    // local_callee_return_types map was built from `Decl::Block` only,
    // so export-block returns silently skipped the check.
    let src = fixture("invalid", "return_export_block_nominal_mismatch.glyph");
    let result = run_compile(src, "json");
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit 1; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_has_diagnostic_id(&stdout, "G::analyze::nominal-mismatch");
}

#[test]
fn unknown_param_slot_emits_analyze_diagnostic() {
    let src = fixture("invalid", "unknown_param_slot.glyph");
    let result = run_compile(src, "json");
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit 1; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_has_diagnostic_id(&stdout, "G::analyze::unknown-param-slot");
}

#[test]
fn slot_in_description_emits_repairable_parse_diagnostic() {
    let src = fixture("repairable", "slot_in_description.glyph");
    let result = run_check(src, "json");
    assert_eq!(
        result.status.code(),
        Some(2),
        "expected exit 2 (repairable); stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_has_diagnostic_id(&stdout, "G::parse::param-slot-in-non-instruction-string");
}
