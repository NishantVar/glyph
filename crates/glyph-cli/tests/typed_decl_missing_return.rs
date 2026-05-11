use std::path::PathBuf;
use std::process::Command;

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn check_source(source: &str) -> (Option<i32>, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.glyph");
    std::fs::write(&path, source).unwrap();
    let output = Command::new(glyph_bin())
        .arg("check")
        .arg(&path)
        .output()
        .expect("failed to spawn glyph binary");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Must-error #1: typed skill with `flow:` but no `return`.
#[test]
fn typed_skill_flow_no_return_fires() {
    let src = "\
skill foo() -> Plan
    flow:
        \"do the thing\"
";
    let (code, stderr) = check_source(src);
    assert!(
        stderr.contains("G::analyze::typed-decl-missing-return"),
        "expected typed-decl-missing-return; stderr: {}",
        stderr
    );
    assert_eq!(code, Some(1), "expected exit 1 (Error); stderr: {}", stderr);
}

/// Must-error #2: typed private block with single-string shorthand.
/// Per parse.rs:1881, this lands as `FlowStmt::InlineString` — non-empty
/// flow but no `Return`, so the predicate correctly returns false.
#[test]
fn typed_block_shorthand_fires() {
    let src = "\
skill caller() -> Plan
    flow:
        ctx = resolve(\"x\")
        return ctx

block resolve(scope) -> Diagnosis
    \"inspect and diagnose\"
";
    let (code, stderr) = check_source(src);
    assert!(
        stderr.contains("G::analyze::typed-decl-missing-return"),
        "expected typed-decl-missing-return for typed block shorthand; stderr: {}",
        stderr
    );
    assert_eq!(code, Some(1), "expected exit 1; stderr: {}", stderr);
}

/// Must-error #3: typed private block with `flow:` and no `return`.
#[test]
fn typed_block_flow_no_return_fires() {
    let src = "\
skill caller() -> AgentList
    flow:
        xs = enumerate()
        return xs

block enumerate() -> AgentList
    flow:
        \"scan home directory\"
        \"list found dirs\"
";
    let (code, stderr) = check_source(src);
    assert!(
        stderr.contains("G::analyze::typed-decl-missing-return"),
        "expected typed-decl-missing-return for typed block flow no-return; stderr: {}",
        stderr
    );
    assert_eq!(code, Some(1), "expected exit 1; stderr: {}", stderr);
}

/// Must-error #4: typed export block whose body has `return none`.
/// Export blocks have no structured `flow: Vec<FlowStmt>`; we reuse the
/// parser-computed `has_meaningful_return` (false for `return none`).
#[test]
fn typed_export_block_return_none_fires() {
    let src = "\
export block load(scope = \".\") -> Plan
    flow:
        \"read scope\"
        return none
";
    let (code, stderr) = check_source(src);
    assert!(
        stderr.contains("G::analyze::typed-decl-missing-return"),
        "expected typed-decl-missing-return for typed export block return-none; stderr: {}",
        stderr
    );
    assert_eq!(code, Some(1), "expected exit 1; stderr: {}", stderr);
}

/// Must-error #5: typed skill ending in a bare `return`. Bare `return`
/// parses as `ReturnExpr::None` per `parse.rs:2630`.
#[test]
fn typed_skill_bare_return_fires() {
    let src = "\
skill bar() -> Diagnosis
    flow:
        \"investigate\"
        return
";
    let (code, stderr) = check_source(src);
    assert!(
        stderr.contains("G::analyze::typed-decl-missing-return"),
        "expected typed-decl-missing-return for typed skill bare return; stderr: {}",
        stderr
    );
    assert_eq!(code, Some(1), "expected exit 1; stderr: {}", stderr);
}

/// Must-error #6: typed skill ending in `return None` (capital N). Pins
/// the case-insensitive `none` handling — `parse_return_expr` only maps
/// lowercase `none` to `ReturnExpr::None`, so `return None` lands as
/// `ReturnExpr::Name(Spanned{node: "None"})`. Without the case-insensitive
/// branch in `flow_has_meaningful_return`, this case would slip through.
#[test]
fn typed_skill_return_capital_none_fires() {
    let src = "\
skill bar() -> Diagnosis
    flow:
        \"investigate\"
        return None
";
    let (code, stderr) = check_source(src);
    assert!(
        stderr.contains("G::analyze::typed-decl-missing-return"),
        "expected typed-decl-missing-return for typed skill `return None`; stderr: {}",
        stderr
    );
    assert_eq!(code, Some(1), "expected exit 1; stderr: {}", stderr);
}

/// Overlap: a typed export block with NO `return` at all must fire only
/// the new rule, not the legacy Repairable `missing-return`. B1 suppression.
#[test]
fn typed_export_block_no_return_fires_new_only() {
    let src = "\
export block plan(scope = \".\") -> Plan
    flow:
        \"read scope\"
";
    let (code, stderr) = check_source(src);
    assert!(
        stderr.contains("G::analyze::typed-decl-missing-return"),
        "expected new rule to fire; stderr: {}",
        stderr
    );
    assert!(
        !stderr.contains("G::analyze::missing-return"),
        "legacy missing-return must be suppressed for typed export blocks; stderr: {}",
        stderr
    );
    assert_eq!(code, Some(1), "expected exit 1; stderr: {}", stderr);
}

/// Overlap: an untyped export block with NO `return` still fires the
/// legacy Repairable `missing-return`. Behavior preserved.
#[test]
fn untyped_export_block_no_return_still_fires_legacy() {
    let src = "\
export block plan(scope = \".\")
    flow:
        \"read scope\"
";
    let (_code, stderr) = check_source(src);
    assert!(
        stderr.contains("G::analyze::missing-return"),
        "legacy missing-return must still fire for untyped export blocks; stderr: {}",
        stderr
    );
    assert!(
        !stderr.contains("G::analyze::typed-decl-missing-return"),
        "new rule must NOT fire for untyped declarations; stderr: {}",
        stderr
    );
}

/// Must-pass A: untyped block, single-string shorthand. No `-> Type`,
/// so the new rule must not fire.
#[test]
fn untyped_block_shorthand_clean() {
    let src = "\
skill caller()
    flow:
        log_progress()

block log_progress()
    \"print status\"
";
    let (_code, stderr) = check_source(src);
    assert!(
        !stderr.contains("G::analyze::typed-decl-missing-return"),
        "new rule must not fire on untyped block; stderr: {}",
        stderr
    );
}

/// Must-pass B: typed private block with a value-producing `return`.
#[test]
fn typed_block_meaningful_return_clean() {
    let src = "\
skill caller() -> Diagnosis
    flow:
        d = resolve()
        return d

block resolve() -> Diagnosis
    flow:
        \"investigate\"
        return <\"the diagnosis text\">
";
    let (_code, stderr) = check_source(src);
    assert!(
        !stderr.contains("G::analyze::typed-decl-missing-return"),
        "new rule must not fire on typed block with meaningful return; stderr: {}",
        stderr
    );
}

/// Must-pass C: typed export block whose flow ends in a named-binding
/// return (`return ctx`). The parser sets `has_meaningful_return = true`
/// for this shape (parse.rs:1150 + L1153 catch-all `_ => true`).
#[test]
fn typed_export_block_named_return_clean() {
    let src = "\
export block plan(scope = \".\") -> Plan
    flow:
        ctx = inspect(scope)
        return ctx

block inspect(s) -> Plan
    flow:
        \"look at\"
        return <\"a plan summary\">
";
    let (_code, stderr) = check_source(src);
    assert!(
        !stderr.contains("G::analyze::typed-decl-missing-return"),
        "new rule must not fire on typed export block with named return; stderr: {}",
        stderr
    );
}
