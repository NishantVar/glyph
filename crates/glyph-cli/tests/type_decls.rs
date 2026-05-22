//! Tests for `type` declarations and library-only export semantics.

use std::path::PathBuf;
use std::process::Command;

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn fixture(kind: &str, name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/corpus")
        .join(kind)
        .join(name)
}

fn run_compile(src: std::path::PathBuf) -> std::process::Output {
    Command::new(glyph_bin())
        .arg("compile")
        .arg(&src)
        .output()
        .expect("failed to spawn glyph binary")
}

/// A type-only library (no `skill`, only `export type`) must satisfy the
/// library-export rule and compile with exit 0.
#[test]
fn type_only_library_compiles_cleanly() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("types_only.glyph"),
        "export type RepoContext = <\"the inspected repo state, including file tree and dependencies\">\n\
         export type RiskLevel = <\"one of: low, medium, high; severity of the change\">\n",
    )
    .unwrap();

    let output = Command::new(glyph_bin())
        .arg("check")
        .arg(dir.path().join("types_only.glyph"))
        .output()
        .expect("failed to spawn glyph binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("G::analyze::no-exports-in-library"),
        "type-only file should satisfy library-export rule; stderr: {}",
        stderr
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "expected exit 0; stderr: {}",
        stderr
    );
}

#[test]
fn type_level_description_applies_when_param_has_no_per_param_description() {
    let src = fixture("valid", "type_level_lookup.glyph");
    let out = src.with_file_name("type_level_lookup.md");
    let _ = std::fs::remove_file(&out);
    let result = run_compile(src.clone());
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let md = std::fs::read_to_string(&out).expect("compiled .md file is missing");
    assert!(
        md.contains("- **risk** (RiskLevel): one of: low, medium, high. Default: \"medium\"."),
        "expected type-level fallback; got md=\n{}",
        md
    );
}

#[test]
fn per_param_description_overrides_type_level() {
    let src = fixture("valid", "type_level_override.glyph");
    let out = src.with_file_name("type_level_override.md");
    let _ = std::fs::remove_file(&out);
    let result = run_compile(src.clone());
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let md = std::fs::read_to_string(&out).expect("compiled .md file is missing");
    assert!(
        md.contains(
            "- **risk** (RiskLevel): raise to 'high' if fix touches auth. Default: \"medium\"."
        ),
        "per-param description should win; got md=\n{}",
        md
    );
}

#[test]
fn type_decl_imported_selectively_drives_param_description() {
    // Cross-file imports are only resolved by directory-mode compile
    // (`run_compile_directory` → `compile_directory_with_options`); single-file
    // `glyph compile <file.glyph>` runs `compile_source_with_effects`, which has
    // no import graph. Run the test out of a tempdir copy of the fixtures so we
    // can compile the directory without polluting the corpus tree.
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer_src = std::fs::read_to_string(fixture("valid", "imports/consumer.glyph"))
        .expect("read consumer.glyph fixture");
    let types_src = std::fs::read_to_string(fixture("valid", "imports/types.glyph"))
        .expect("read types.glyph fixture");
    let consumer_path = tmp.path().join("consumer.glyph");
    std::fs::write(&consumer_path, &consumer_src).unwrap();
    std::fs::write(tmp.path().join("types.glyph"), &types_src).unwrap();

    let result = Command::new(glyph_bin())
        .arg("compile")
        .arg(tmp.path())
        .output()
        .expect("failed to spawn glyph binary");
    assert_eq!(
        result.status.code(),
        Some(0),
        "stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let out = consumer_path.with_file_name("consumer.md");
    let md = std::fs::read_to_string(&out).expect("compiled .md");
    assert!(
        md.contains("- **level** (Severity): one of: low, medium, high. Default: \"medium\"."),
        "imported type-level description should apply; got md=\n{}",
        md
    );
}

/// An imported type used only in a `-> ReturnType` annotation is a use of the
/// import: the unused-import sweep must not fire.
#[test]
fn imported_type_used_only_in_return_type_is_not_unused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        tmp.path().join("types.glyph"),
        "export type Diagnosis = <\"root cause and severity\">\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("consumer.glyph"),
        r#"import "./types.glyph" { Diagnosis }

export block triage() -> Diagnosis
    description: "Triage an issue."
    flow:
        "Inspect the issue."
        return "diagnosis text"
"#,
    )
    .unwrap();

    let result = Command::new(glyph_bin())
        .arg("check")
        .arg("--format")
        .arg("json")
        .arg(tmp.path())
        .output()
        .expect("failed to spawn glyph binary");

    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        !stdout.contains("G::analyze::unused-import")
            && !stderr.contains("G::analyze::unused-import"),
        "must NOT fire G::analyze::unused-import — `-> Diagnosis` is a use of the import; \
         stdout={stdout}\nstderr={stderr}"
    );
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0; stdout={stdout}\nstderr={stderr}"
    );
}

/// Tier 3 procedure files (export blocks with body word count >= 150) must
/// render the `## Parameters` bullet using the shared shape — including
/// per-param description, type-level description fallback, and the typed
/// `(Foo)` suffix per `compiled-output.md` §`## Parameters`.
#[test]
fn tier3_procedure_file_renders_param_with_type_and_description() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // 16 substantive content words per "blah" line keeps the Glyph
    // word counter happy without bloating the fixture.
    let big_step = "\"Inspect the project layout, dependency graph, configuration files, \
        test suites, documentation, structural conventions, build targets, \
        platform integrations, observability hooks, package metadata, security \
        boundaries, deployment manifests, environment variables, secrets \
        handling, runtime expectations, and observability targets thoroughly.\"";
    let lib_src = format!(
        r#"export type RepoPath = <"the path or glob to inspect">

export block inspect_repo(scope: RepoPath = ".") -> Report
    description: "Inspect the repository structure."
    flow:
        {big_step}
        {big_step}
        {big_step}
        {big_step}
        {big_step}
        return "report"
"#
    );
    std::fs::write(tmp.path().join("repo_tools.glyph"), lib_src).unwrap();

    let result = Command::new(glyph_bin())
        .arg("compile")
        .arg(tmp.path())
        .output()
        .expect("failed to spawn glyph binary");
    assert!(
        result.status.success(),
        "compile failed: stdout={} stderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );

    let proc_md = std::fs::read_to_string(tmp.path().join("repo_tools").join("inspect-repo.md"))
        .expect("expected Tier 3 procedure file");
    assert!(
        proc_md.contains("- **scope** (RepoPath): the path or glob to inspect. Default: \".\"."),
        "procedure file should pull type-level description and render typed bullet:\n{proc_md}"
    );
}

// --- §8.4 locked return-prose templates: one fixture+test per spec row. ---

/// Compile a fixture and return the resulting `.md`. Panics on non-zero exit.
fn compile_and_read(name: &str) -> String {
    let src = fixture("valid", name);
    let out = src.with_extension("md");
    let _ = std::fs::remove_file(&out);
    let result = run_compile(src.clone());
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0 for {name}; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    std::fs::read_to_string(&out).expect("compiled .md missing")
}

#[test]
fn return_row2_named_with_type_decl_includes_description() {
    let md = compile_and_read("return_row2_named_with_type_decl.glyph");
    // ADR 0026: return renders as its own `Output:` step with name + producer step.
    // For row 2 (named return + `-> Type` + `type Type` decl), description metadata
    // lives only on `skill.output_contract`; the rendered step just names the binding.
    assert!(
        md.contains("Output: diagnosis."),
        "row 2 Output line should appear:\n{md}"
    );
}

#[test]
fn return_row3_named_with_type_no_decl_omits_description() {
    let md = compile_and_read("return_row3_named_with_type_no_decl.glyph");
    // ADR 0026: identifier return renders as `Output: <name>.` (no description
    // since row 3 has no `type Foo` decl to source one from).
    assert!(
        md.contains("Output: diagnosis."),
        "row 3 Output line should appear:\n{md}"
    );
    assert!(
        !md.contains("`Diagnosis`):"),
        "row 3 must not include a colon-led description:\n{md}"
    );
}

#[test]
fn return_row5_expr_with_type_decl_includes_description() {
    let md = compile_and_read("return_row5_expr_with_type_decl.glyph");
    assert!(
        md.contains("Inspect the scope. Return a `Diagnosis`: root cause and severity."),
        "row 5 sentence should appear:\n{md}"
    );
    assert!(
        !md.contains("Return the result of"),
        "legacy fold must not fire when `-> Foo` is in scope:\n{md}"
    );
}

#[test]
fn return_row6_expr_with_type_no_decl_omits_description() {
    let md = compile_and_read("return_row6_expr_with_type_no_decl.glyph");
    assert!(
        md.contains("Inspect the scope. Return a `Diagnosis`."),
        "row 6 sentence should appear:\n{md}"
    );
    assert!(
        !md.contains("Return a `Diagnosis`:"),
        "row 6 must not include a colon-led description:\n{md}"
    );
    assert!(
        !md.contains("Return the result of"),
        "legacy fold must not fire when `-> Foo` is in scope:\n{md}"
    );
}

#[test]
fn return_row7_return_only_body_uses_standalone_sentence() {
    // Row 7: return-only body with `return expr` (no markers). Routes to the
    // matching `return expr` shape — row 5 here because the fixture has a
    // `type Diagnosis` decl in scope. Emitted standalone since there is no
    // leading body to fold into.
    let md = compile_and_read("return_row7_return_only_body.glyph");
    assert!(
        md.contains("1. Return a `Diagnosis`: root cause and severity."),
        "row 7 standalone sentence should appear:\n{md}"
    );
    assert!(
        !md.contains("1. , and"),
        "row 7 must not emit a leading-comma malformed line:\n{md}"
    );
}

#[test]
fn return_row8_no_type_no_target_emits_no_sentence() {
    let md = compile_and_read("return_row8_no_type_no_target.glyph");
    assert!(
        md.contains("1. Inspect the scope.\n"),
        "row 8 should leave the final step unmodified:\n{md}"
    );
    assert!(
        !md.contains("Produce"),
        "row 8 must not append any §8.4 sentence:\n{md}"
    );
    assert!(
        !md.contains("Return a "),
        "row 8 must not append a `Return a` sentence:\n{md}"
    );
}
