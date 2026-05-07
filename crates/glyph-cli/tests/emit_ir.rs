//! Integration tests for `glyph compile --emit-ir` (Slice 17).
//!
//! Verifies the `--emit-ir` flag produces a `.ir.json` sidecar file that
//! conforms to `design/ir-json-schema.md`.

use std::path::PathBuf;
use std::process::Command;

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn run_compile_emit_ir(source: &std::path::Path) -> std::process::Output {
    Command::new(glyph_bin())
        .arg("compile")
        .arg(source)
        .arg("--emit-ir")
        .output()
        .expect("failed to spawn glyph binary")
}

/// Copy a corpus source to a tempdir to avoid parallel-test races.
fn setup_tempdir(filename: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("valid")
        .join(filename);
    let tmp_src = dir.path().join(filename);
    std::fs::copy(&src, &tmp_src).unwrap();
    (dir, tmp_src)
}

fn ir_json_path(source: &std::path::Path) -> PathBuf {
    let parent = source.parent().unwrap();
    let stem = source
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_suffix(".glyph")
        .unwrap();
    parent.join(format!("{}.ir.json", stem))
}

#[test]
fn emit_ir_produces_ir_json_file() {
    let (_dir, src) = setup_tempdir("update_docs.glyph");
    let result = run_compile_emit_ir(&src);
    assert!(
        result.status.success(),
        "glyph compile --emit-ir exited non-zero. stderr={:?}",
        String::from_utf8_lossy(&result.stderr),
    );

    let ir_path = ir_json_path(&src);
    assert!(ir_path.exists(), "expected {} to exist", ir_path.display());

    // Parse it as valid JSON.
    let content = std::fs::read_to_string(&ir_path).unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&content).expect("ir.json should be valid JSON");

    // Check top-level envelope fields.
    assert_eq!(v["ir_version"], 2);
    assert!(v["compiler"].as_str().unwrap().starts_with("glyph "));
    assert_eq!(v["source_file"].as_str().unwrap(), "update_docs.glyph");
    assert_eq!(v["skill"]["kind"], "skill");
    assert_eq!(v["skill"]["name"], "update_docs");
}

#[test]
fn emit_ir_is_byte_identical_across_runs() {
    let (_dir, src) = setup_tempdir("update_docs.glyph");
    let ir_path = ir_json_path(&src);

    let r1 = run_compile_emit_ir(&src);
    assert!(r1.status.success());
    let bytes1 = std::fs::read(&ir_path).unwrap();

    let r2 = run_compile_emit_ir(&src);
    assert!(r2.status.success());
    let bytes2 = std::fs::read(&ir_path).unwrap();

    assert_eq!(
        bytes1, bytes2,
        "IR JSON should be byte-identical across runs"
    );
}

/// Write a source string to a tempdir, compile with --emit-ir, and return the parsed IR JSON.
fn compile_and_read_ir(filename: &str, source: &str) -> serde_json::Value {
    let dir = tempfile::tempdir().unwrap();
    let src_path = dir.path().join(filename);
    std::fs::write(&src_path, source).unwrap();
    let result = run_compile_emit_ir(&src_path);
    assert!(
        result.status.success(),
        "compile failed. stderr={:?}",
        String::from_utf8_lossy(&result.stderr),
    );
    let ir_path = ir_json_path(&src_path);
    let content = std::fs::read_to_string(&ir_path)
        .unwrap_or_else(|_| panic!("expected {} to exist", ir_path.display()));
    serde_json::from_str(&content).expect("ir.json should be valid JSON")
}

#[test]
fn emit_ir_includes_site_modifier_for_with_calls() {
    let source = r#"block inspect()
    flow:
        "Inspect the thing."

skill fix()
    description: "Fix it."
    flow:
        inspect() with "focus on auth"
"#;
    let v = compile_and_read_ir("with_mod.glyph", source);
    let flow = v["skill"]["flow"].as_array().unwrap();
    let call = &flow[0];
    assert_eq!(call["kind"], "call");
    assert_eq!(call["projection_mode"], "inline");
    assert_eq!(call["site_modifier"], "focus on auth");
}

#[test]
fn emit_ir_includes_projection_mode_for_calls() {
    // Tier 2 call (>= 4 flow statements).
    let source = r#"block review_code()
    flow:
        "Scan for style violations."
        "Check for security issues."
        "Check for performance issues."
        "Compile findings."

skill fix()
    description: "Fix it."
    flow:
        review_code()
"#;
    let v = compile_and_read_ir("proj_mode.glyph", source);
    let flow = v["skill"]["flow"].as_array().unwrap();
    let call = &flow[0];
    assert_eq!(call["kind"], "call");
    assert_eq!(call["projection_mode"], "same_file_procedure");
}

#[test]
fn emit_ir_inline_calls_have_inline_projection_mode() {
    // Tier 1 inline: small block.
    let source = r#"block helper()
    flow:
        "Do a quick check."

skill fix()
    description: "Fix it."
    flow:
        helper()
"#;
    let v = compile_and_read_ir("proj_inline.glyph", source);
    let flow = v["skill"]["flow"].as_array().unwrap();
    // Tier 1 calls keep their Call shape in IR so call-site metadata remains
    // available to validators and downstream tooling.
    let node = &flow[0];
    assert_eq!(node["kind"], "call");
    assert_eq!(node["projection_mode"], "inline");
}

#[test]
fn emit_ir_includes_description_on_block_in_call() {
    // Block/ExportBlock don't appear as separate nodes in the JSON — their
    // content is inlined into Call nodes. When a described block is called
    // via a non-inline (Tier 2+) projection, the callee's description should
    // appear as `callee_description` on the Call node. When absent, the field
    // is omitted entirely.
    let source = r#"block review_code()
    description: "Review code thoroughly."
    flow:
        "Scan for style violations."
        "Check for security issues."
        "Check for performance issues."
        "Compile findings."

skill fix()
    description: "Fix it."
    flow:
        review_code()
"#;
    let v = compile_and_read_ir("desc.glyph", source);
    let flow = v["skill"]["flow"].as_array().unwrap();
    let call = &flow[0];
    assert_eq!(call["kind"], "call");
    assert_eq!(call["target"], "review_code");
    assert_eq!(call["projection_mode"], "same_file_procedure");
    // callee_description present because the block has a description set.
    assert_eq!(
        call["callee_description"], "Review code thoroughly.",
        "callee_description should surface the block's description on non-inline calls"
    );
}

#[test]
fn emit_ir_omits_callee_description_when_absent() {
    // When a block has no description, callee_description is omitted from the Call node.
    let source = r#"block review_code()
    flow:
        "Scan for style violations."
        "Check for security issues."
        "Check for performance issues."
        "Compile findings."

skill fix()
    description: "Fix it."
    flow:
        review_code()
"#;
    let v = compile_and_read_ir("desc_absent.glyph", source);
    let flow = v["skill"]["flow"].as_array().unwrap();
    let call = &flow[0];
    assert_eq!(call["kind"], "call");
    assert!(
        call.get("callee_description").is_none() || call["callee_description"].is_null(),
        "callee_description should be absent when block has no description"
    );
}

#[test]
fn emit_ir_includes_resolved_predicates_on_branch() {
    let (_dir, src) = setup_tempdir("branching.glyph");
    let result = run_compile_emit_ir(&src);
    assert!(result.status.success());
    let ir_path = ir_json_path(&src);
    let content = std::fs::read_to_string(&ir_path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();

    // Find the branch node in flow.
    let flow = v["skill"]["flow"].as_array().unwrap();
    let branch = flow.iter().find(|n| n["kind"] == "branch").unwrap();
    // branching.glyph uses mode == "fast" / mode == "slow", not .applies().
    // So resolved_predicates should be null.
    assert!(
        branch["resolved_predicates"].is_null(),
        "resolved_predicates should be null when no .applies() used"
    );
    // predicate_shape reflects the actual classification from Analyze.
    // branching.glyph uses `mode == "fast"`: per design/data-flow.md §327, the
    // entire `==` form is a boolean comparison — operands do NOT contribute to
    // summary flags. Only `==` itself fires `has_boolean_token` (and
    // `has_comparison_operator`). `mode` and `"fast"` are operands so they
    // never set `has_predicate_token`. `==` is not a compositional operator
    // (and/or/not).
    let shape = &branch["predicate_shape"];
    assert!(shape.is_object(), "predicate_shape should be an object");
    assert_eq!(shape["has_boolean_token"], true);
    assert_eq!(shape["has_predicate_token"], false);
    assert_eq!(shape["has_compositional_operator"], false);
}

#[test]
fn emit_ir_includes_resolved_predicates_with_applies_calls() {
    let source = r#"block fast_mode()
    description: "When the user wants fast processing."
    flow:
        "Do fast processing."

block slow_mode()
    description: "When the user wants thorough processing."
    flow:
        "Do slow processing."

skill main()
    description: "A skill that branches."
    flow:
        if fast_mode.applies()
            "Do the fast thing."
        elif slow_mode.applies()
            "Do the slow thing."
        else
            "Do the default thing."
"#;
    let v = compile_and_read_ir("applies.glyph", source);
    let flow = v["skill"]["flow"].as_array().unwrap();
    let branch = flow.iter().find(|n| n["kind"] == "branch").unwrap();
    let rp = &branch["resolved_predicates"];
    assert!(rp.is_object(), "resolved_predicates should be an object");
    assert_eq!(rp["fast_mode"], "When the user wants fast processing.");
    assert_eq!(rp["slow_mode"], "When the user wants thorough processing.");
    // predicate_shape should always be present.
    let shape = &branch["predicate_shape"];
    assert!(shape.is_object(), "predicate_shape should be an object");
}

#[test]
fn emit_ir_includes_local_refs_on_resolved_call() {
    // local_refs is always present as an array (may be empty).
    let source = r#"block review_code()
    flow:
        "Scan for style violations."
        "Check for security issues."
        "Check for performance issues."
        "Compile findings."

skill fix()
    description: "Fix it."
    flow:
        review_code()
"#;
    let v = compile_and_read_ir("local_refs.glyph", source);
    let flow = v["skill"]["flow"].as_array().unwrap();
    let call = &flow[0];
    assert_eq!(call["kind"], "call");
    let local_refs = call["local_refs"].as_array().unwrap();
    assert!(
        local_refs.is_empty(),
        "local_refs should be empty when no local bindings"
    );
}

#[test]
fn emit_ir_role_enum_includes_context_value() {
    // A skill with context: section entries and flow-level context markers
    // should produce context nodes with the right role.
    let source = r#"skill fix(scope = ".")
    description: "Fix a bug."
    context:
        "This is project context."
    flow:
        "Do the fix."
"#;
    let v = compile_and_read_ir("context_role.glyph", source);
    let context = v["skill"]["context"].as_array().unwrap();
    assert!(!context.is_empty(), "context array should not be empty");
    assert_eq!(context[0]["kind"], "context");
    assert_eq!(context[0]["text"], "This is project context.");
}

#[test]
fn emit_ir_skill_carries_context_array() {
    // Even when empty, context array should be present.
    let source = r#"skill simple()
    description: "A simple skill."
    flow:
        "Do the thing."
"#;
    let v = compile_and_read_ir("ctx_empty.glyph", source);
    let context = v["skill"]["context"].as_array().unwrap();
    assert!(
        context.is_empty(),
        "context should be empty when no context declared"
    );
}

#[test]
fn emit_ir_call_carries_callee_context_for_non_inline_call() {
    // For Tier 2+ calls, callee_context should be an array.
    let source = r#"block review_code()
    flow:
        "Scan for style violations."
        "Check for security issues."
        "Check for performance issues."
        "Compile findings."

skill fix()
    description: "Fix it."
    flow:
        review_code()
"#;
    let v = compile_and_read_ir("callee_ctx.glyph", source);
    let flow = v["skill"]["flow"].as_array().unwrap();
    let call = &flow[0];
    assert_eq!(call["kind"], "call");
    assert_eq!(call["projection_mode"], "same_file_procedure");
    // callee_context should be an array (not null) for non-inline calls.
    let cc = &call["callee_context"];
    assert!(
        cc.is_array(),
        "callee_context should be array for non-inline. got: {:?}",
        cc
    );
}

#[test]
fn emit_ir_context_node_serializes_correctly() {
    let source = r#"skill fix()
    description: "Fix a bug."
    context:
        "This codebase follows a monorepo layout."
    flow:
        "Do the fix."
"#;
    let v = compile_and_read_ir("ctx_node.glyph", source);
    let context = v["skill"]["context"].as_array().unwrap();
    assert_eq!(context.len(), 1);
    let cn = &context[0];
    assert!(cn["node_id"].as_str().unwrap().starts_with("n"));
    assert_eq!(cn["kind"], "context");
    assert_eq!(cn["text"], "This codebase follows a monorepo layout.");
    // Inline-string context entries have no source name.
    assert!(
        cn.get("name").is_none(),
        "inline-string context must not serialize a name field"
    );
}

#[test]
fn emit_ir_context_node_carries_name_for_nameref_entry() {
    // A NameRef context entry resolves to the const's text but should also
    // carry the source name in the IR JSON so downstream tooling (and the
    // emitter) can render a per-entry label.
    let source = r#"const project_overview = "This codebase is a monorepo."

skill fix()
    description: "Fix a bug."
    context:
        project_overview
    flow:
        "Do the fix."
"#;
    let v = compile_and_read_ir("ctx_named.glyph", source);
    let context = v["skill"]["context"].as_array().unwrap();
    assert_eq!(context.len(), 1);
    let cn = &context[0];
    assert_eq!(cn["kind"], "context");
    assert_eq!(cn["text"], "This codebase is a monorepo.");
    assert_eq!(cn["name"], "project_overview");
}

#[test]
fn emit_ir_params_serialize_with_correct_default() {
    let source = r#"skill summarize(scope = ".", target)
    description: "Summarize."
    flow:
        "Inspect files under {scope}."
        "Write to {target}."
"#;
    let v = compile_and_read_ir("params.glyph", source);
    let params = v["skill"]["params"].as_array().unwrap();
    assert_eq!(params.len(), 2);

    // scope has default "."
    let scope = &params[0];
    assert_eq!(scope["name"], "scope");
    assert_eq!(scope["default"]["kind"], "string");
    assert_eq!(scope["default"]["value"], ".");

    // target has no default (required)
    let target = &params[1];
    assert_eq!(target["name"], "target");
    assert!(target.get("default").map_or(true, |d| d.is_null()));
}

#[test]
fn emit_ir_conforms_to_schema_full_skill() {
    let (_dir, src) = setup_tempdir("with_context.glyph");
    let result = run_compile_emit_ir(&src);
    assert!(
        result.status.success(),
        "stderr={:?}",
        String::from_utf8_lossy(&result.stderr),
    );
    let ir_path = ir_json_path(&src);
    let content = std::fs::read_to_string(&ir_path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();

    // Envelope
    assert_eq!(v["ir_version"], 2);
    assert!(v["compiler"].is_string());
    assert!(v["source_file"].is_string());

    // Skill
    let skill = &v["skill"];
    assert!(skill["node_id"].is_string());
    assert_eq!(skill["kind"], "skill");
    assert!(skill["name"].is_string());
    assert!(skill["description"].is_string());
    assert!(skill["params"].is_array());
    assert!(skill["effects"].is_array());
    assert!(skill["context"].is_array());
    assert!(skill["constraints"].is_array());
    assert!(skill["flow"].is_array());

    // Context should have entries (with_context.glyph has context section).
    let ctx = skill["context"].as_array().unwrap();
    assert!(!ctx.is_empty());
    for c in ctx {
        assert_eq!(c["kind"], "context");
        assert!(c["node_id"].is_string());
        assert!(c["text"].is_string());
    }
}

#[test]
fn predicate_shape_reflects_predicate_token_classification() {
    let source = r#"block helper()
    description: "A helper block."
    flow:
        "Do helper work."

skill main()
    description: "A test skill."
    flow:
        if helper.applies()
            "Do work."
"#;
    let v = compile_and_read_ir("classification_propagation.glyph", source);
    let flow = v["skill"]["flow"].as_array().unwrap();
    let branch = flow.iter().find(|n| n["kind"] == "branch").unwrap();
    let shape = &branch["predicate_shape"];
    assert_eq!(shape["has_predicate_token"], true);
    assert_eq!(shape["has_boolean_token"], false);
    assert_eq!(shape["has_compositional_operator"], false);
}

#[test]
fn imported_string_const_resolves_in_arena_consts() {
    let dir = tempfile::tempdir().unwrap();
    let imported_path = dir.path().join("imported.glyph");
    std::fs::write(
        &imported_path,
        r#"export const big_change = "the change is big"
"#,
    )
    .unwrap();
    let main_path = dir.path().join("main.glyph");
    std::fs::write(
        &main_path,
        r#"import "./imported.glyph" { big_change }

skill main()
    description: "A test skill."
    flow:
        if big_change
            "Do work."
"#,
    )
    .unwrap();

    // Compile the whole directory so the multi-file pipeline resolves the
    // import edge and routes through `lower_with_imports`. (Single-file compile
    // on this branch does not walk imports.)
    let result = run_compile_emit_ir(dir.path());
    assert!(
        result.status.success(),
        "compile failed: {:?}",
        String::from_utf8_lossy(&result.stderr)
    );
    let ir_path = ir_json_path(&main_path);
    let content = std::fs::read_to_string(&ir_path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();

    let flow = v["skill"]["flow"].as_array().unwrap();
    let branch = flow.iter().find(|n| n["kind"] == "branch").unwrap();
    let rp = &branch["resolved_predicates"];
    assert!(rp.is_object(), "resolved_predicates should be populated");
    assert_eq!(rp["big_change"], "the change is big");
}

/// Task 6 — Decl::Block flow walking: a numeric bare-condition inside a
/// private block's flow must fire the
/// `G::analyze::condition-non-boolean-non-predicate` diagnostic. Pre-fix,
/// `check_file_numeric_conditions` only walked `Decl::Skill`, so the
/// equivalent condition inside `block helper` slipped past analyze.
#[test]
fn block_flow_numeric_condition_fires_diagnostic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("block_numeric.glyph");
    std::fs::write(
        &path,
        r#"const max_attempts = 3

block helper()
    description: "A helper block."
    flow:
        if max_attempts
            "Loop."
        else
            "Stop."

skill main()
    description: "Test."
    flow:
        helper()
"#,
    )
    .unwrap();

    let out = std::process::Command::new(glyph_bin())
        .arg("check")
        .arg(&path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("failed to spawn glyph");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let ids: Vec<String> = stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
        .collect();
    assert!(
        ids.contains(&"G::analyze::condition-non-boolean-non-predicate".to_string()),
        "expected diagnostic for bare numeric in block flow; got: {:?}",
        ids
    );
}

#[test]
fn expand_skips_eq_operand_from_resolved_predicates() {
    let source = r#"const complex_change = "the requested change is complex"

skill main(risk: String)
    description: "Test."
    flow:
        if risk == "high" and complex_change
            "Escalate."
        else
            "Proceed."
"#;
    let v = compile_and_read_ir("eq_operand_skipped.glyph", source);
    let flow = v["skill"]["flow"].as_array().unwrap();
    let branch = flow.iter().find(|n| n["kind"] == "branch").unwrap();
    let rp = &branch["resolved_predicates"];
    assert!(rp.is_object(), "resolved_predicates should be populated");
    assert_eq!(
        rp["complex_change"], "the requested change is complex",
        "complex_change must resolve"
    );
    assert!(
        rp.get("high").is_none() && rp.get("\"high\"").is_none(),
        "operand `\"high\"` must NOT enter resolved_predicates; got keys: {:?}",
        rp.as_object().unwrap().keys().collect::<Vec<_>>()
    );
}

#[test]
fn expand_resolves_both_predicates_in_or_compound() {
    let source = r#"block fast_mode()
    description: "Fast processing path."
    flow:
        "Fast work."

block slow_mode()
    description: "Slow processing path."
    flow:
        "Slow work."

skill main()
    description: "Test."
    flow:
        if fast_mode.applies() or slow_mode.applies()
            "Either path."
"#;
    let v = compile_and_read_ir("or_compound_predicates.glyph", source);
    let flow = v["skill"]["flow"].as_array().unwrap();
    let branch = flow.iter().find(|n| n["kind"] == "branch").unwrap();
    let rp = &branch["resolved_predicates"];
    assert_eq!(rp["fast_mode"], "Fast processing path.");
    assert_eq!(rp["slow_mode"], "Slow processing path.");
}
