//! Slice 2 integration tests — diagnostic infrastructure end-to-end.
//!
//! Covers the six acceptance criteria from the slice spec:
//!   1. `glyph compile invalid/empty.glyph` exits 1 with `G::parse::empty-file`
//!   2. `glyph compile invalid/empty_flow.glyph` exits 1 with `G::parse::empty-flow`
//!   3. `--format json` produces JSON diagnostics on stdout
//!   4. Pretty output renders span, message, and source caret to stderr
//!   5. Re-running over identical input produces byte-identical JSON
//!   6. Exit-code rules hold — `1` wins over `2`
//!
//! The 1-wins-over-2 rule is exercised at the `DiagBag` API layer in
//! `glyph-core::diagnostic::tests` (unit). Here we exercise the fixtures
//! end-to-end through the binary.

use std::path::PathBuf;
use std::process::{Command, Output};

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("invalid")
        .join(name)
}

fn run_compile(file: &str, format: &str) -> Output {
    Command::new(glyph_bin())
        .arg("compile")
        .arg(fixture(file))
        .arg("--format")
        .arg(format)
        .output()
        .expect("failed to spawn glyph binary")
}

fn assert_contains_diagnostic_id(stdout: &str, id: &str) {
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
fn empty_file_exits_one_with_empty_file_diagnostic() {
    let result = run_compile("empty.glyph", "json");
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit code 1; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_contains_diagnostic_id(&stdout, "G::parse::empty-file");
}

#[test]
fn empty_flow_exits_one_with_empty_flow_diagnostic() {
    let result = run_compile("empty_flow.glyph", "json");
    assert_eq!(
        result.status.code(),
        Some(1),
        "expected exit code 1; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_contains_diagnostic_id(&stdout, "G::parse::empty-flow");
}

#[test]
fn json_format_produces_ndjson_on_stdout() {
    let result = run_compile("empty.glyph", "json");
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    let trimmed = stdout.trim_end_matches('\n');
    assert!(!trimmed.is_empty(), "expected diagnostic on stdout");
    // Each line must parse as a complete JSON object.
    for line in trimmed.lines() {
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each NDJSON line must parse as JSON");
        let obj = v
            .as_object()
            .expect("each diagnostic must be a JSON object");
        for required in ["id", "classification", "message", "span"] {
            assert!(
                obj.contains_key(required),
                "diagnostic missing required field `{}`: {}",
                required,
                line
            );
        }
        // span shape
        let span = obj.get("span").and_then(|s| s.as_object()).unwrap();
        for required in ["file", "start", "end"] {
            assert!(
                span.contains_key(required),
                "span missing field `{}` in {}",
                required,
                line
            );
        }
    }
}

#[test]
fn pretty_format_renders_to_stderr() {
    let result = run_compile("empty.glyph", "pretty");
    assert_eq!(result.status.code(), Some(1));
    // stdout should be empty (or carry no diagnostics) under pretty mode.
    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let stderr = String::from_utf8_lossy(&result.stderr).to_string();
    assert!(
        stdout.trim().is_empty(),
        "pretty mode should not write to stdout, got: {:?}",
        stdout
    );
    // codespan-reporting writes the diagnostic id, the message, and a caret line.
    assert!(
        stderr.contains("G::parse::empty-file"),
        "stderr should include the diagnostic id, got: {:?}",
        stderr
    );
    assert!(
        stderr.contains("source file has no declarations"),
        "stderr should include the message, got: {:?}",
        stderr
    );
    // codespan-reporting renders carets as `^` on a separate line.
    assert!(
        stderr.contains('^'),
        "stderr should include a caret indicator, got: {:?}",
        stderr
    );
}

#[test]
fn json_output_is_byte_identical_across_runs() {
    // Run twice over the same fixture; the NDJSON stream must be byte-identical.
    let first = run_compile("empty_flow.glyph", "json").stdout;
    let second = run_compile("empty_flow.glyph", "json").stdout;
    assert_eq!(
        first, second,
        "JSON output must be byte-identical across runs"
    );
}

#[test]
fn empty_flow_does_not_emit_md_file() {
    let _ = run_compile("empty_flow.glyph", "json");
    let unwanted = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("invalid")
        .join("empty_flow.md");
    assert!(
        !unwanted.exists(),
        "should not have written `{}` for a failing compile",
        unwanted.display()
    );
}

/// Run `glyph check <fixture> --format json` (Phases 1+2 only — warnings still
/// surface even when no hard error is emitted). Sibling to `run_compile`, but
/// targets the lint-only subcommand so warning-tier diagnostics like
/// `inconsistent-type-spelling` are observable on stdout without requiring a
/// repairable/error to also fire.
fn run_check_json(file: &str) -> Output {
    Command::new(glyph_bin())
        .arg("check")
        .arg(fixture(file))
        .arg("--format")
        .arg("json")
        .output()
        .expect("failed to spawn glyph binary")
}

/// Sibling of `run_check_json` that targets `tests/corpus/valid/` rather than
/// `tests/corpus/invalid/`. Used by positive-case assertions that expect
/// `glyph check` to exit 0 with no diagnostics on stdout.
fn run_check_json_valid(file: &str) -> Output {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("valid")
        .join(file);
    Command::new(glyph_bin())
        .arg("check")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("failed to spawn glyph binary")
}

/// Spec §"Behavior under the new rule": a `type Foo` (type namespace) and a
/// canonically-equal value-namespace binding (`const foo`, `block foo`, param,
/// flow-local) are LEGAL — they live in disjoint namespaces. This test pins
/// the cross-namespace sweep rewrite (Task 8) by exercising a fixture that
/// pairs `type LinkMode` with a flow-local binding `link_mode` in the same
/// file. Pre-Task 8 this fired `G::analyze::name-collision`.
#[test]
fn type_and_value_same_canonical_compiles_clean() {
    let result = run_check_json_valid("legal_cross_kind.glyph");
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert!(
        lines.is_empty(),
        "expected no diagnostics, got:\n{}",
        stdout
    );
    assert_eq!(result.status.code(), Some(0));
}

#[test]
fn inconsistent_implicit_type_emits_warning() {
    let result = run_check_json("case-violation/inconsistent_implicit_type.glyph");
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert_contains_diagnostic_id(&stdout, "G::analyze::inconsistent-type-spelling");
}

#[test]
fn type_snake_case_emits_type_case_violation() {
    let result = run_check_json("case-violation/type_snake_case.glyph");
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert_contains_diagnostic_id(&stdout, "G::analyze::type-case-violation");
}

#[test]
fn const_pascal_case_emits_value_case_violation() {
    let result = run_check_json("case-violation/value_pascal_case.glyph");
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert_contains_diagnostic_id(&stdout, "G::analyze::value-case-violation");
}

#[test]
fn skill_pascal_case_emits_value_case_violation() {
    let result = run_check_json("case-violation/skill_pascal_case.glyph");
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert_contains_diagnostic_id(&stdout, "G::analyze::value-case-violation");
}

#[test]
fn block_pascal_case_emits_value_case_violation() {
    let result = run_check_json("case-violation/block_pascal_case.glyph");
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert_contains_diagnostic_id(&stdout, "G::analyze::value-case-violation");
}

#[test]
fn param_annotation_snake_case_emits_type_case_violation() {
    let result = run_check_json("case-violation/param_annotation_snake_case.glyph");
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert_contains_diagnostic_id(&stdout, "G::analyze::type-case-violation");
}

#[test]
fn return_annotation_snake_case_emits_type_case_violation() {
    let result = run_check_json("case-violation/return_annotation_snake_case.glyph");
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert_contains_diagnostic_id(&stdout, "G::analyze::type-case-violation");
}

#[test]
fn case_violation_wins_over_collision() {
    let result = run_check_json("case-violation/precedence_case_wins.glyph");
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert_contains_diagnostic_id(&stdout, "G::analyze::value-case-violation");
    // Must NOT also emit a name-collision for the colliding declaration.
    let count_collision = stdout
        .lines()
        .filter(|l| l.contains("\"G::analyze::name-collision\""))
        .count();
    assert_eq!(
        count_collision, 0,
        "case-violation must short-circuit collision sweeps"
    );
}

#[test]
fn value_vs_value_canonical_collision_emits_name_collision() {
    let result = run_check_json("case-violation/same_kind_collision_values.glyph");
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert_contains_diagnostic_id(&stdout, "G::analyze::name-collision");
}

/// Per-arm scope isolation for `sweep_value_name_collisions`: the same flow-
/// binding name set in disjoint branch arms (`if` / `else`) must NOT collide,
/// because the arms are sibling scopes. Pre-fix the walker reused a single
/// `seen` map across arms and falsely emitted `G::analyze::name-collision`.
#[test]
fn branch_arm_same_canonical_compiles_clean() {
    let result = run_check_json_valid("branch_arm_same_canonical_compiles_clean.glyph");
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert!(
        lines.is_empty(),
        "expected no diagnostics, got:\n{}",
        stdout
    );
    assert_eq!(result.status.code(), Some(0));
}

/// Guard against over-isolation: two `result = call()` statements in the
/// SAME branch arm are sequential — they share the arm's scope and must
/// still emit `G::analyze::name-collision`.
#[test]
fn flow_assign_sequential_in_arm_collides() {
    let result = run_check_json("flow_assign_sequential_in_arm.glyph");
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert_contains_diagnostic_id(&stdout, "G::analyze::name-collision");
}

/// `register_type_use` ExplicitDecl arm: a local `type X` that shadows a
/// selective type-import alias `X` must emit ONLY
/// `G::analyze::name-collision` (handled by
/// `sweep_type_decl_name_collisions`). It must NOT also emit
/// `G::analyze::duplicate-type-decl` — the previous registry entry came
/// from an import alias, not an in-file `type` declaration.
#[test]
fn local_type_shadows_import_does_not_emit_duplicate_type_decl() {
    let result = run_check_json("imports/local_type_shadows_import.glyph");
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert_contains_diagnostic_id(&stdout, "G::analyze::name-collision");
    let duplicate_lines: Vec<&str> = stdout
        .lines()
        .filter(|l| l.contains("\"G::analyze::duplicate-type-decl\""))
        .collect();
    assert!(
        duplicate_lines.is_empty(),
        "selective-type-import shadow must NOT emit duplicate-type-decl, got:\n{}",
        stdout
    );
}

// ---------------------------------------------------------------------------
// Task 9: ResolvedImportKind plumbing — alias-case rule and kinded sweeps.
// ---------------------------------------------------------------------------

/// A consumer importing both a type and a value whose consumer-local names
/// are canonical-equal (`Mode` / `mode_name`) must compile clean — the two
/// names live in disjoint namespaces (Type vs Value) per spec §"Behavior
/// under the new rule".
#[test]
fn kinded_aliases_filesystem_compile_clean() {
    let result = run_check_json_valid("imports/kinded_alias_filesystem/main.glyph");
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert!(
        lines.is_empty(),
        "expected no diagnostics, got:\n{}",
        stdout
    );
    assert_eq!(result.status.code(), Some(0));
}

/// Selective type-import alias forced to snake_case — illegal under the
/// alias-case rule (type aliases must be PascalCase).
#[test]
fn selective_type_alias_snake_case_emits_type_case_violation() {
    let result = run_check_json("imports/selective_type_alias_snake_case.glyph");
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert_contains_diagnostic_id(&stdout, "G::analyze::type-case-violation");
}

/// Selective value-import alias forced to PascalCase — illegal (value
/// aliases must be snake_case).
#[test]
fn selective_value_alias_pascal_case_emits_value_case_violation() {
    let result = run_check_json("imports/selective_value_alias_pascal_case.glyph");
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert_contains_diagnostic_id(&stdout, "G::analyze::value-case-violation");
}

/// Whole-module filesystem alias forced to PascalCase — illegal (whole-module
/// aliases bind to the value namespace).
#[test]
fn whole_module_alias_pascal_case_emits_value_case_violation() {
    let result = run_check_json("imports/whole_module_alias_pascal_case.glyph");
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert_contains_diagnostic_id(&stdout, "G::analyze::value-case-violation");
}

/// Stdlib selective alias forced to PascalCase — illegal (stdlib has no
/// types in MVP, so all aliases are Value-kinded).
#[test]
fn stdlib_selective_alias_pascal_case_emits_value_case_violation() {
    let result = run_check_json("imports/stdlib_selective_alias_pascal_case.glyph");
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert_contains_diagnostic_id(&stdout, "G::analyze::value-case-violation");
}

/// Stdlib whole-module alias forced to PascalCase — illegal.
#[test]
fn stdlib_whole_module_alias_pascal_case_emits_value_case_violation() {
    let result = run_check_json("imports/stdlib_whole_module_alias_pascal_case.glyph");
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert_contains_diagnostic_id(&stdout, "G::analyze::value-case-violation");
}

/// Phase 3 / Task 3.15: a flow statement inside a freeform section body
/// surfaces `G::parse::flow-statement-in-freeform`.
#[test]
fn freeform_flow_statement_emits_flow_statement_in_freeform() {
    let result = run_compile("freeform_flow_statement.glyph", "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_contains_diagnostic_id(&stdout, "G::parse::flow-statement-in-freeform");
}

/// Phase 3 / Task 3.15: an unrecognized marker word in a freeform section body
/// surfaces `G::parse::unknown-marker-word`.
#[test]
fn freeform_unknown_marker_emits_unknown_marker_word() {
    let result = run_compile("freeform_unknown_marker.glyph", "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_contains_diagnostic_id(&stdout, "G::parse::unknown-marker-word");
}

/// Phase 3 / Task 3.15: a lone marker word with no operand surfaces
/// `G::parse::marker-missing-operand`.
#[test]
fn freeform_marker_missing_operand_emits_marker_missing_operand() {
    let result = run_compile("freeform_marker_missing_operand.glyph", "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_contains_diagnostic_id(&stdout, "G::parse::marker-missing-operand");
}

/// Phase 6 code-review fix: the inline shorthand form (`goal: <ident>`)
/// must run reserved-word idents through the same classifier as the
/// indented body so they cannot be silently consumed as a bare `NameRef`.
/// A lone `require` inline must surface
/// `G::parse::marker-missing-operand`, not `G::analyze::undefined-name`.
#[test]
fn freeform_inline_marker_emits_marker_missing_operand() {
    let result = run_compile("freeform_inline_marker.glyph", "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_contains_diagnostic_id(&stdout, "G::parse::marker-missing-operand");
}

/// Phase 5: writing `effects:` without `--enable-effects` surfaces the
/// unified `G::parse::gated-section` diagnostic. This is the
/// catalogue-driven id (Phase 5 renamed the legacy `effects-disabled`
/// when the gating moved through the catalogue's `enabled` field).
#[test]
fn gated_section_effects_emits_gated_section() {
    let result = run_compile("gated_section_effects.glyph", "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_contains_diagnostic_id(&stdout, "G::parse::gated-section");
}

/// Phase 6: a `goal:` section with more than one body item violates the
/// catalogue's `cardinality = "one"` rule and fires
/// `G::analyze::cardinality-violation`.
#[test]
fn goal_with_two_items_emits_cardinality_violation() {
    let result = run_compile("goal_cardinality_violation.glyph", "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_contains_diagnostic_id(&stdout, "G::analyze::cardinality-violation");
}

/// Phase 6 code-review fix: cardinality enforcement was previously only
/// wired into `analyze_skill`. Private blocks and export blocks also carry
/// `freeform_sections`, so a `goal:` with two items inside a block must
/// fire `G::analyze::cardinality-violation` as well.
#[test]
fn block_goal_with_two_items_emits_cardinality_violation() {
    let result = run_compile("block_cardinality_violation.glyph", "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_contains_diagnostic_id(&stdout, "G::analyze::cardinality-violation");
}

/// Duplicate freeform colon-keyword section (`quality:` twice in one body)
/// must fire `G::analyze::duplicate-section` per
/// `GLYPH_LANGUAGE_GUIDE.md` "each named sub-section may appear at most
/// once per body".
#[test]
fn duplicate_freeform_section_emits_diag() {
    let result = run_compile("duplicate_section_freeform.glyph", "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_contains_diagnostic_id(&stdout, "G::analyze::duplicate-section");
}

fn assert_does_not_contain_diagnostic_id(stdout: &str, id: &str) {
    for line in stdout.lines() {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("id").and_then(|x| x.as_str()) == Some(id) {
            panic!(
                "expected diagnostic `{}` NOT in JSON output, got:\n{}",
                id, stdout
            );
        }
    }
}

/// Mixed-case section keywords (`Description:`, `Effects:`, `Constraints:`,
/// `Context:`, `Flow:`) must dispatch to the same built-in handler as their
/// canonical lowercase forms. The catalogue lookup and duplicate-check are
/// already case-insensitive; the parser's dispatch must match.
///
/// `Description:` (mixed case) routes through the description-section
/// handler. A `{slot}` inside it must fire the description-specific
/// non-instruction-string diagnostic, not the freeform/unknown-slot path.
#[test]
fn case_insensitive_description_emits_param_slot_in_non_instruction_string() {
    let result = run_compile("case_insensitive_description.glyph", "json");
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_contains_diagnostic_id(&stdout, "G::parse::param-slot-in-non-instruction-string");
}

/// `Effects:` (mixed case) routes through the effects-section handler. With
/// effects gating off by default the catalogue-driven `gated-section`
/// diagnostic fires, proving the keyword reached the effects arm.
#[test]
fn case_insensitive_effects_emits_gated_section() {
    let result = run_compile("case_insensitive_effects.glyph", "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_contains_diagnostic_id(&stdout, "G::parse::gated-section");
}

/// `Constraints:` (mixed case) routes through the constraints-section
/// handler. A bare string in the body (no `require`/`avoid`/`must`/`must
/// avoid` prefix) fires the constraint-marker-required parser error.
#[test]
fn case_insensitive_constraints_emits_marker_required_error() {
    let result = run_compile("case_insensitive_constraints.glyph", "json");
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_contains_diagnostic_id(&stdout, "G::parse::unexpected");
}

/// `Context: "x"` (mixed case) parses as the context sub-section, not as a
/// freeform section. Compile succeeds (exit 0) and no parse errors fire.
#[test]
fn case_insensitive_context_parses_clean() {
    let result = run_compile("case_insensitive_context.glyph", "json");
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit code 0; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}

/// `Flow:` (mixed case) routes through the statements body-grammar parser.
/// An `if`/`else` block inside it is legal flow syntax; if `Flow:` were
/// treated as a freeform section the same content would fire
/// `G::parse::flow-statement-in-freeform`.
#[test]
fn case_insensitive_flow_does_not_emit_flow_statement_in_freeform() {
    let result = run_compile("case_insensitive_flow_with_if.glyph", "json");
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_does_not_contain_diagnostic_id(&stdout, "G::parse::flow-statement-in-freeform");
}

/// End-to-end: `description: "x"` followed by `Description: "y"` in the
/// same skill body collides on the case-insensitive dispatch and fires
/// `G::parse::duplicate-subsection` (the built-in description-section
/// duplicate diagnostic — freeform duplicates fire
/// `G::analyze::duplicate-section`, which is the test above). Proves the
/// parser-to-analyze pipeline honours case-insensitive section identity.
#[test]
fn case_insensitive_duplicate_description_emits_duplicate_subsection() {
    let result = run_compile("case_insensitive_duplicate_description.glyph", "json");
    let stdout = String::from_utf8(result.stdout).expect("stdout should be UTF-8");
    assert_contains_diagnostic_id(&stdout, "G::parse::duplicate-subsection");
}
