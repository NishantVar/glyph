//! Integration tests for issue #86 AC12 — descriptive output-target form,
//! exercising the full pipeline at the CLI binary level.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn run_compile(file: &Path) -> Output {
    Command::new(glyph_bin())
        .arg("compile")
        .arg(file)
        .output()
        .expect("failed to spawn glyph binary")
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

fn run_fmt(file: &Path) -> Output {
    Command::new(glyph_bin())
        .arg("fmt")
        .arg(file)
        .output()
        .expect("failed to spawn glyph binary")
}

/// Strip `.glyph` suffix and return the sibling `.md` path that `compile` writes to.
fn md_path(source: &Path) -> PathBuf {
    let parent = source.parent().unwrap();
    let stem = source
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_suffix(".glyph")
        .unwrap();
    parent.join(format!("{}.md", stem))
}

fn ndjson_contains_id(stdout: &str, id: &str) -> bool {
    stdout
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .any(|v| v.get("id").and_then(|x| x.as_str()) == Some(id))
}

/// AC12 (a): A private block with `return <"…">` compiles cleanly and the
/// description text is incorporated into the compiled Markdown prose.
#[test]
fn descriptive_output_target_in_block_compiles_and_prose_carries_description() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("block_desc.glyph");
    std::fs::write(
        &path,
        "\
block helper() -> BranchName
    flow:
        return <\"branch name as currently checked out\">

skill main()
    description: \"Use the helper.\"
    flow:
        helper()
",
    )
    .unwrap();

    let result = run_compile(&path);
    assert_eq!(
        result.status.code(),
        Some(0),
        "compile should succeed; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );

    let md = std::fs::read_to_string(md_path(&path)).unwrap();
    assert!(
        md.contains("branch name as currently checked out"),
        "compiled Markdown must incorporate the description text:\n{md}"
    );
    assert!(
        !md.contains("<\"branch name as currently checked out\">"),
        "compiled Markdown must not leak the descriptive output-target token:\n{md}"
    );
}

/// AC12 (b): An `export block` with `return <"…">` compiles/checks cleanly.
#[test]
fn descriptive_output_target_in_export_block_compiles() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("export_block_desc.glyph");
    std::fs::write(
        &path,
        "\
export block compute() -> Confirmation
    description: \"Compute a result.\"
    flow:
        return <\"detailed confirmation of the completed operation\">
",
    )
    .unwrap();

    let result = run_check(&path);
    assert_eq!(
        result.status.code(),
        Some(0),
        "export block with descriptive output target should check cleanly; \
         stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

/// AC12 (c): Two separate blocks returning the SAME domain type but with
/// different descriptions both compile cleanly — the types match nominally,
/// not by description text.
#[test]
fn same_domain_type_with_different_descriptions_compiles_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("two_desc_blocks.glyph");
    std::fs::write(
        &path,
        "\
block first() -> BranchName
    flow:
        return <\"current branch name\">

block second() -> BranchName
    flow:
        return <\"target branch name for the merge\">

skill main()
    description: \"Demonstrate two descriptive returns of the same type.\"
    flow:
        first()
        second()
",
    )
    .unwrap();

    let result = run_compile(&path);
    assert_eq!(
        result.status.code(),
        Some(0),
        "two blocks with same domain type but different descriptions should compile; \
         stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

/// AC12 (d): A placeholder string return whose inner content is not a valid
/// identifier (contains spaces/words) is repaired by `glyph fmt` into the
/// descriptive form `return <"…">`, and the file then checks cleanly.
#[test]
fn placeholder_string_return_repairs_to_descriptive_form() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("placeholder_desc.glyph");
    std::fs::write(
        &path,
        "\
skill diagnose() -> Confirmation
    description: \"Diagnose the issue.\"
    flow:
        return \"<root cause and severity>\"
",
    )
    .unwrap();

    // First check: must emit placeholder-string-return (exit 2).
    let first = run_check(&path);
    assert_eq!(first.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&first.stdout);
    assert!(
        ndjson_contains_id(&stdout, "G::analyze::placeholder-string-return"),
        "expected placeholder-string-return diagnostic, got:\n{stdout}"
    );

    // fmt must repair the placeholder into the descriptive form.
    let fmt = run_fmt(&path);
    assert_eq!(
        fmt.status.code(),
        Some(0),
        "fmt should succeed; stderr={:?}",
        String::from_utf8_lossy(&fmt.stderr)
    );
    let rewritten = std::fs::read_to_string(&path).unwrap();
    assert!(
        rewritten.contains("return <\"root cause and severity\">"),
        "fmt must rewrite to descriptive form; got:\n{rewritten}"
    );
    assert!(
        !rewritten.contains("return \"<root cause and severity>\""),
        "placeholder string return must be gone after fmt; got:\n{rewritten}"
    );

    // Second check: must be clean after the repair.
    let second = run_check(&path);
    assert_eq!(
        second.status.code(),
        Some(0),
        "post-fmt check should be clean; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
}
