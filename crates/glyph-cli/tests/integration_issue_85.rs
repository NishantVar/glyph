//! Integration tests for issue #85 — output target identifier returns.

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

fn run_compile_emit_ir(file: &Path) -> Output {
    Command::new(glyph_bin())
        .arg("compile")
        .arg(file)
        .arg("--emit-ir")
        .output()
        .expect("failed to spawn glyph binary")
}

fn run_fmt(file: &Path) -> Output {
    Command::new(glyph_bin())
        .arg("fmt")
        .arg(file)
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

fn ir_json_path(source: &Path) -> PathBuf {
    let parent = source.parent().unwrap();
    let stem = source
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_suffix(".glyph.md")
        .unwrap();
    parent.join(format!("{}.ir.json", stem))
}

fn md_path(source: &Path) -> PathBuf {
    let parent = source.parent().unwrap();
    let stem = source
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_suffix(".glyph.md")
        .unwrap();
    parent.join(format!("{}.md", stem))
}

#[test]
fn output_target_compile_and_emit_ir_shape() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("current.glyph.md");
    std::fs::write(
        &path,
        "\
skill current() -> BranchName
    description: \"Return the current branch.\"
    flow:
        return <current_branch>
",
    )
    .unwrap();

    let result = run_compile_emit_ir(&path);
    assert_eq!(
        result.status.code(),
        Some(0),
        "compile should succeed; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );

    let md = std::fs::read_to_string(md_path(&path)).unwrap();
    assert!(
        md.contains("Produce current branch as the final output."),
        "compiled Markdown should contain natural output-target prose:\n{md}"
    );
    assert!(
        !md.contains("<current_branch>"),
        "compiled Markdown must not leak the literal output target:\n{md}"
    );

    let ir: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ir_json_path(&path)).unwrap()).unwrap();
    let oc = &ir["skill"]["output_contract"];
    assert_eq!(oc["kind"], "output_contract");
    assert_eq!(oc["form"], "identifier");
    assert_eq!(oc["target_name"], "current_branch");
    assert_eq!(oc["ty"], serde_json::json!({ "domain_type": "branchname" }));
    assert_eq!(oc["source"], "synthesized_by_agent");
}

#[test]
fn inline_block_output_contract_survives_emit_ir() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inline_block.glyph.md");
    std::fs::write(
        &path,
        "\
block helper() -> BranchName
    flow:
        return <current_branch>

skill current()
    description: \"Return the current branch.\"
    flow:
        helper()
",
    )
    .unwrap();

    let result = run_compile_emit_ir(&path);
    assert_eq!(
        result.status.code(),
        Some(0),
        "compile should succeed; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );

    let ir: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ir_json_path(&path)).unwrap()).unwrap();
    let flow = ir["skill"]["flow"].as_array().unwrap();
    let call = &flow[0];
    assert_eq!(call["kind"], "call");
    assert_eq!(call["projection_mode"], "inline");
    let oc = &call["callee_output_contract"];
    assert_eq!(oc["kind"], "output_contract");
    assert_eq!(oc["form"], "identifier");
    assert_eq!(oc["target_name"], "current_branch");
}

#[test]
fn export_block_accepts_output_target_identifier_form() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lib.glyph.md");
    std::fs::write(
        &path,
        "\
export block compute() -> ResultName
    description: \"Compute result.\"
    flow:
        return <result_name>
",
    )
    .unwrap();

    let result = run_check(&path);
    assert_eq!(
        result.status.code(),
        Some(0),
        "export block output target should check cleanly; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn output_target_diagnostics_surface_through_check() {
    let dir = tempfile::tempdir().unwrap();

    let malformed = dir.path().join("malformed.glyph.md");
    std::fs::write(
        &malformed,
        "\
skill malformed()
    description: \"Bad target.\"
    flow:
        return <a.b>
",
    )
    .unwrap();
    let result = run_check(&malformed);
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        ndjson_contains_id(&stdout, "G::parse::malformed-output-target"),
        "expected malformed-output-target diagnostic, got:\n{stdout}"
    );

    let trailing = dir.path().join("trailing.glyph.md");
    std::fs::write(
        &trailing,
        "\
skill trailing()
    description: \"Bad target.\"
    flow:
        return <result>bar
",
    )
    .unwrap();
    let result = run_check(&trailing);
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        ndjson_contains_id(&stdout, "G::parse::malformed-output-target"),
        "expected malformed-output-target diagnostic for trailing text, got:\n{stdout}"
    );

    let outside = dir.path().join("outside.glyph.md");
    std::fs::write(
        &outside,
        "\
skill outside() -> BranchName
    description: \"Bad target placement.\"
    flow:
        return <current_branch>
        \"continue\"
",
    )
    .unwrap();
    let result = run_check(&outside);
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        ndjson_contains_id(&stdout, "G::parse::output-target-outside-return"),
        "expected output-target-outside-return diagnostic, got:\n{stdout}"
    );

    let export_outside = dir.path().join("export_outside.glyph.md");
    std::fs::write(
        &export_outside,
        "\
export block outside() -> BranchName
    description: \"Bad target placement.\"
    flow:
        return <current_branch>
        \"continue\"
",
    )
    .unwrap();
    let result = run_check(&export_outside);
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        ndjson_contains_id(&stdout, "G::parse::output-target-outside-return"),
        "expected export block output-target-outside-return diagnostic, got:\n{stdout}"
    );

    let shadow = dir.path().join("shadow.glyph.md");
    std::fs::write(
        &shadow,
        "\
const current_branch = \"main\"

skill shadow() -> BranchName
    description: \"Bad target name.\"
    flow:
        return <current_branch>
",
    )
    .unwrap();
    let result = run_check(&shadow);
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        ndjson_contains_id(&stdout, "G::analyze::output-target-shadows-binding"),
        "expected output-target-shadows-binding diagnostic, got:\n{stdout}"
    );
}

#[test]
fn placeholder_string_return_check_then_fmt_then_check_clean() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("placeholder.glyph.md");
    std::fs::write(
        &path,
        "\
skill current() -> BranchName
    description: \"Return the current branch.\"
    flow:
        return \"<current_branch>\"
",
    )
    .unwrap();

    let first = run_check(&path);
    assert_eq!(first.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&first.stdout);
    assert!(
        ndjson_contains_id(&stdout, "G::analyze::placeholder-string-return"),
        "expected placeholder-string-return diagnostic, got:\n{stdout}"
    );

    let fmt = run_fmt(&path);
    assert_eq!(
        fmt.status.code(),
        Some(0),
        "fmt should succeed; stderr={:?}",
        String::from_utf8_lossy(&fmt.stderr)
    );
    let rewritten = std::fs::read_to_string(&path).unwrap();
    assert!(rewritten.contains("return <current_branch>"));
    assert!(!rewritten.contains("return \"<current_branch>\""));

    let second = run_check(&path);
    assert_eq!(
        second.status.code(),
        Some(0),
        "post-fmt check should be clean; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
}

/// Issue #86 AC13: `--emit-ir` JSON shape for descriptive output target.
/// Verifies that `return <"…">` produces `form: "description"` + `description`
/// key in the output_contract node, with no `target_name` field.
#[test]
fn emit_ir_descriptive_output_contract_shape() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("diagnose.glyph.md");
    std::fs::write(
        &path,
        "\
skill diagnose_issue() -> Confirmation
    description: \"Diagnose the issue.\"
    flow:
        return <\"root cause analysis including affected files and severity\">
",
    )
    .unwrap();

    let result = run_compile_emit_ir(&path);
    assert_eq!(
        result.status.code(),
        Some(0),
        "compile should succeed; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );

    let ir: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ir_json_path(&path)).unwrap()).unwrap();
    let oc = &ir["skill"]["output_contract"];
    assert_eq!(oc["kind"], "output_contract");
    assert_eq!(oc["form"], "description");
    assert_eq!(
        oc["description"],
        "root cause analysis including affected files and severity"
    );
    assert!(
        oc.get("target_name").is_none() || oc["target_name"].is_null(),
        "descriptive form must not emit target_name; got: {:?}",
        oc
    );
    assert_eq!(oc["source"], "synthesized_by_agent");
}
