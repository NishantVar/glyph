//! Slice 4 integration tests — parameters and `## Parameters` section.
//!
//! Covers the six acceptance criteria from `mvp-issues.md` slice 4:
//!   1. Skill parameters with defaults parse and lower without diagnostics.
//!   2. Compiled output emits a `## Parameters` section between frontmatter and
//!      `## Instructions` whose entries match the design contract:
//!      `(default: <value>)` for defaulted parameters, `(required)` for skill
//!      parameters without a default.
//!   3. `export block` parameters without a default surface
//!      `G::analyze::missing-param-default` (compile error).
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
    let src = fixture("valid", "with_params.glyph.md");
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

    // Defaulted parameter renders with `(default: ".")`; required parameter
    // renders with `(required)`.
    assert!(
        md.contains("- **scope** (default: \".\")"),
        "expected defaulted scope parameter; got md=\n{}",
        md
    );
    assert!(
        md.contains("- **target** (required)"),
        "expected required target parameter; got md=\n{}",
        md
    );
}

#[test]
fn export_block_missing_default_emits_analyze_diagnostic() {
    let src = fixture("invalid", "export_block_no_default.glyph.md");
    let result = run_compile(src, "json");
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit 1; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_has_diagnostic_id(&stdout, "G::analyze::missing-param-default");
}

#[test]
fn unknown_param_slot_emits_analyze_diagnostic() {
    let src = fixture("invalid", "unknown_param_slot.glyph.md");
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
    let src = fixture("repairable", "slot_in_description.glyph.md");
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
