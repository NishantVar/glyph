//! Flow-position assignment corpus tests.
//!
//! Covers `.flow-assign-spec.md` §11 (test corpus list) and §12.1
//! (acceptance: `valid/imports/fix_bug.glyph` round-trip).
//!
//! Valid fixtures must compile cleanly (exit 0) and emit a `.md` whose
//! contents satisfy the spec-derived assertions: producer-step naming
//! sentence, `{name}` → bare-name substitution, and bound-name return prose.
//!
//! Invalid fixtures must fire the diagnostic specified in §11 (and §10
//! Diagnostics catalog). The `truly_unknown` fixture verifies the
//! precedence rule in §6.3 — a `{name}` slot whose name is never bound
//! anywhere fires `unknown-param-slot`, NOT `use-before-bind`.

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

fn diagnostic_ids(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
        .collect()
}

fn assert_has_diagnostic_id(stdout: &str, id: &str) {
    let ids = diagnostic_ids(stdout);
    assert!(
        ids.iter().any(|x| x == id),
        "expected diagnostic `{}`; got {:?}\nraw stdout:\n{}",
        id,
        ids,
        stdout,
    );
}

// ── Valid fixtures ─────────────────────────────────────────────────

/// §12.1 acceptance: `valid/imports/fix_bug.glyph` round-trips after
/// flow-position assignments land. Emits a stable `.md` with the
/// producer-step naming sentence and bound-name return prose.
#[test]
fn fix_bug_round_trips_with_flow_assign() {
    let src = fixture("valid", "imports/fix_bug.glyph");
    let out = src.with_file_name("fix_bug.md");
    let _ = std::fs::remove_file(&out);

    let result = run_compile(src, "json");
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );

    let md = std::fs::read_to_string(&out).expect(".md file is missing after compile");
    // Producer-step naming sentence (§9.1, value-shape).
    assert!(
        md.contains("Refer to this result as ctx."),
        "expected value-binding naming sentence; got md=\n{md}"
    );
    // Bound-name return prose (§9.3).
    assert!(
        md.contains("Your result is ctx"),
        "expected bound-name return prose; got md=\n{md}"
    );
    // Codex M4: when the §9.3 return-prose step exists, the §8.4
    // generic "Return a `<T>`." suffix must NOT also fire on the
    // producer step — that would duplicate the return statement.
    assert!(
        !md.contains("Return a `Report`."),
        "expected no duplicate §8.4 return suffix on the producer step; got md=\n{md}"
    );
}

/// §11 valid: bind, reference inside an inline string, return.
#[test]
fn flow_assign_compiles_and_substitutes_inline_slot() {
    let src = fixture("valid", "flow_assign.glyph");
    let out = src.with_file_name("flow_assign.md");
    let _ = std::fs::remove_file(&out);

    let result = run_compile(src, "json");
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );

    let md = std::fs::read_to_string(&out).expect(".md file is missing after compile");
    // §9.1 naming sentence.
    assert!(
        md.contains("Refer to this result as ctx."),
        "expected naming sentence; got md=\n{md}"
    );
    // §9.2 substitution: `{ctx}` in the inline-string step renders as bare `ctx`.
    assert!(
        md.contains("Summarize the issues you found in ctx."),
        "expected `{{ctx}}` slot substituted to bare `ctx`; got md=\n{md}"
    );
    assert!(
        !md.contains("{ctx}"),
        "no literal `{{ctx}}` should remain in inline text; got md=\n{md}"
    );
    // §9.3 bound-name return prose.
    assert!(
        md.contains("Your result is ctx"),
        "expected bound-name return prose; got md=\n{md}"
    );
    // Codex round-2 M1: when the §9.3 return-prose step exists, the
    // §8.4 generic "Return a `<T>`." suffix must NOT also fire on the
    // last *inline-string* step. (The earlier round only gated the
    // tier-1 Call last-step path; the inline path was missed.)
    assert!(
        !md.contains("Return a `Report`."),
        "expected no duplicate §8.4 return suffix on the last inline step; got md=\n{md}"
    );
}

/// §11: `<name> = <call>(scope) with "..."` now requires LLM-grade
/// expansion. The deterministic stub filler refuses, so compile exits
/// non-zero with `G::expand::llm-required-for-call` and writes no `.md`.
/// See docs/superpowers/specs/2026-05-18-callbodyshape-span-emission-design.md.
#[test]
fn flow_assign_with_modifier_hard_fails_under_stub_filler() {
    let src = fixture("valid", "flow_assign_with_modifier.glyph");
    let out = src.with_file_name("flow_assign_with_modifier.md");
    let _ = std::fs::remove_file(&out);

    let result = run_compile(src, "json");
    assert_ne!(
        result.status.code(),
        Some(0),
        "expected non-zero exit; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let stderr = String::from_utf8_lossy(&result.stderr).to_string();
    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let combined = format!("{stdout}\n{stderr}");
    assert!(
        combined.contains("G::expand::llm-required-for-call"),
        "expected llm-required-for-call diagnostic; got combined output:\n{combined}"
    );
    assert!(
        !out.exists(),
        ".md file must not be written when CallBodyShape stub filler hard-fails: found {}",
        out.display()
    );
}

/// §11 valid: outer binding referenced INSIDE both `if`/`else` arms
/// AND from the post-arm position. The binding declared before the
/// arm survives across arm boundaries — `{ctx}` slots inside each
/// arm rewrite to the bare `ctx` flow-local (Codex L6 — exercises
/// the H1+H2 child-FlowScope path). The post-arm `return ctx` then
/// resolves to the same outer flow-local.
#[test]
fn flow_assign_outer_visible_in_arm_compiles() {
    let src = fixture("valid", "flow_assign_outer_visible_in_arm.glyph");
    let out = src.with_file_name("flow_assign_outer_visible_in_arm.md");
    let _ = std::fs::remove_file(&out);

    let result = run_compile(src, "json");
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );

    let md = std::fs::read_to_string(&out).expect(".md file is missing after compile");
    assert!(
        md.contains("Refer to this result as ctx."),
        "expected naming sentence; got md=\n{md}"
    );
    // Both arms render and the outer `ctx` binding survives to the
    // post-arm return.
    assert!(
        md.contains("Your result is ctx"),
        "expected bound-name return prose; got md=\n{md}"
    );
    assert!(md.contains("If mode"), "expected branch arm; got md=\n{md}");
    assert!(md.contains("Otherwise"), "expected else arm; got md=\n{md}");
    // Codex L6: `{ctx}` slots inside both arms rewrite to bare `ctx`,
    // proving the outer flow-local is visible *inside* each arm
    // (the H1+H2 child-FlowScope inherits parent bindings).
    assert!(
        md.contains("Quickly summarize ctx."),
        "expected `{{ctx}}` substituted to `ctx` in `if` arm; got md=\n{md}"
    );
    assert!(
        md.contains("Carefully summarize ctx."),
        "expected `{{ctx}}` substituted to `ctx` in `else` arm; got md=\n{md}"
    );
    // No leftover `{ctx}` braces — the slot must be substituted.
    assert!(
        !md.contains("{ctx}"),
        "expected no unsubstituted `{{ctx}}` in arm bodies; got md=\n{md}"
    );
}

/// Codex L6: a skill with both an outer flow-local binding AND an
/// arm-local flow-local binding compiles cleanly. The arm-local
/// binding is consumed *inside* the same arm via a `{local_summary}`
/// slot (proving the H1+H2 child FlowScope sees the arm-local), the
/// outer binding is referenced from the OTHER arm via `{outer_ctx}`
/// (proving the child FlowScope still inherits the parent's
/// bindings), and `return outer_ctx` resolves to the outer binding.
#[test]
fn flow_assign_arm_local_used_in_arm_compiles() {
    let src = fixture("valid", "flow_assign_arm_local_used_in_arm.glyph");
    let out = src.with_file_name("flow_assign_arm_local_used_in_arm.md");
    let _ = std::fs::remove_file(&out);

    let result = run_compile(src, "json");
    assert_eq!(
        result.status.code(),
        Some(0),
        "expected exit 0; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );

    let md = std::fs::read_to_string(&out).expect(".md file is missing after compile");
    // Outer binding produced before the branch — has the §9.1 naming
    // sentence at the top-level producer step.
    assert!(
        md.contains("Refer to this result as outer_ctx."),
        "expected outer naming sentence; got md=\n{md}"
    );
    // Arm-local `{local_summary}` slot in the `if` arm rewrites to
    // the bare `local_summary` flow-local — only possible if the
    // arm's child FlowScope holds the arm-local binding (Codex H1+H2).
    assert!(
        md.contains("Use local_summary for the fast path."),
        "expected `{{local_summary}}` substituted in `if` arm; got md=\n{md}"
    );
    // Codex round-2 M2: the arm-local producer step itself must carry
    // the §9.1 naming sentence so the bare `local_summary` reference
    // in the next substep has a stated antecedent. Mirrors
    // `scaffold.rs`'s skill-flow convention (action sentence + naming
    // sentence in the same Step).
    assert!(
        md.contains("Refer to this result as local_summary."),
        "expected arm-local producer naming sentence; got md=\n{md}"
    );
    // Outer `{outer_ctx}` slot inside the `else` arm rewrites — proves
    // the child FlowScope inherits the parent's outer binding.
    assert!(
        md.contains("Carefully summarize outer_ctx."),
        "expected `{{outer_ctx}}` substituted in `else` arm; got md=\n{md}"
    );
    // No leftover arm-local braces.
    assert!(
        !md.contains("{local_summary}") && !md.contains("{outer_ctx}"),
        "expected no unsubstituted slots; got md=\n{md}"
    );
    // Skill returns the outer binding.
    assert!(
        md.contains("Your result is outer_ctx"),
        "expected outer-binding return prose; got md=\n{md}"
    );
}

// ── Invalid fixtures ───────────────────────────────────────────────

/// §11 invalid: `return ctx` referenced before `ctx = ...` in the same
/// scope. Spec §6.3 requires the specialized `use-before-bind` here
/// because the name IS bound elsewhere in the skill.
#[test]
fn flow_assign_use_before_bind_fires_use_before_bind() {
    let src = fixture("invalid", "flow_assign_use_before_bind.glyph");
    let result = run_compile(src, "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert_has_diagnostic_id(&stdout, "G::analyze::use-before-bind");
}

/// Codex round-2 M3: an unresolved RHS callee (e.g. `x = unknown(...)`)
/// must surface as a *single* repairable `G::analyze::undefined-call`
/// at the call site. It must NOT also fire a secondary
/// `G::analyze::use-before-bind` (Error tier) for the later `return x`,
/// which would upgrade the repairable single-fault failure to an exit-1
/// cascade. The `SkillBindingTrace` only registers names whose RHS
/// callee actually resolves — names whose RHS is unknown are not
/// considered "ever bound" because nothing was ever materialized for
/// downstream lookups (`handle_flow_assign` skips registration for the
/// same reason).
#[test]
fn flow_assign_unknown_rhs_emits_single_repairable_diag() {
    let src = fixture("invalid", "flow_assign_unknown_rhs.glyph");
    let result = run_compile(src, "json");
    // Repairable (exit 2) — see `Classification::exit_code`.
    assert_eq!(
        result.status.code(),
        Some(2),
        "expected exit 2 (Repairable); got status={:?}",
        result.status
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    let ids = diagnostic_ids(&stdout);
    assert_eq!(
        ids.len(),
        1,
        "expected exactly one diagnostic; got {ids:?}\nraw stdout:\n{stdout}"
    );
    assert!(
        ids.iter().any(|x| x == "G::analyze::undefined-call"),
        "expected `undefined-call`; got {ids:?}"
    );
    assert!(
        !ids.iter().any(|x| x == "G::analyze::use-before-bind"),
        "must NOT fire `use-before-bind` for an unresolved-RHS binding; got {ids:?}",
    );
}

/// §6.3 precedence: `{never_bound}` slot whose name is never bound
/// anywhere fires the existing `unknown-param-slot`, NOT the
/// specialized `use-before-bind`.
#[test]
fn flow_assign_truly_unknown_fires_unknown_param_slot() {
    let src = fixture("invalid", "flow_assign_truly_unknown.glyph");
    let result = run_compile(src, "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    let ids = diagnostic_ids(&stdout);
    assert!(
        ids.iter().any(|x| x == "G::analyze::unknown-param-slot"),
        "expected `unknown-param-slot`; got {ids:?}",
    );
    assert!(
        !ids.iter().any(|x| x == "G::analyze::use-before-bind"),
        "must NOT fire `use-before-bind` for a truly-unknown name; got {ids:?}",
    );
}

/// §11 invalid: `x = "literal"` is rejected at parse with the
/// repairable `assign-rhs-not-call`. Recovery returns `BareName(x)`.
#[test]
fn flow_assign_rhs_not_call_fires_parse_assign_rhs_not_call() {
    let src = fixture("invalid", "flow_assign_rhs_not_call.glyph");
    let result = run_compile(src, "json");
    // Repairable → exit 2 (per `Classification::exit_code`).
    assert_eq!(result.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert_has_diagnostic_id(&stdout, "G::parse::assign-rhs-not-call");
}

/// §11 invalid: binding name shadows an enclosing parameter →
/// `redeclared-flow-binding`. Hint identifies the prior decl as a
/// parameter (kind disambiguation per §6.2 a).
#[test]
fn flow_assign_redecl_param_fires_redeclared_flow_binding() {
    let src = fixture("invalid", "flow_assign_redecl_param.glyph");
    let result = run_compile(src, "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert_has_diagnostic_id(&stdout, "G::analyze::redeclared-flow-binding");
    assert!(
        stdout.contains("parameter"),
        "hint should identify the prior decl as a parameter; got {stdout}"
    );
}

/// §11 invalid: same name bound twice in one scope.
#[test]
fn flow_assign_redecl_self_fires_redeclared_flow_binding() {
    let src = fixture("invalid", "flow_assign_redecl_self.glyph");
    let result = run_compile(src, "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert_has_diagnostic_id(&stdout, "G::analyze::redeclared-flow-binding");
    assert!(
        stdout.contains("flow-local binding"),
        "hint should identify the prior decl as a flow-local binding; got {stdout}"
    );
}

/// §11 invalid: binding name collides with a `const`.
#[test]
fn flow_assign_redecl_const_fires_redeclared_flow_binding() {
    let src = fixture("invalid", "flow_assign_redecl_const.glyph");
    let result = run_compile(src, "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert_has_diagnostic_id(&stdout, "G::analyze::redeclared-flow-binding");
    assert!(
        stdout.contains("const"),
        "hint should identify the prior decl as a const; got {stdout}"
    );
}

/// §11 invalid: binding declared inside an `if` arm, referenced from
/// the post-arm position. Spec §6.3: arm leak is `use-before-bind`.
#[test]
fn flow_assign_arm_leak_fires_use_before_bind() {
    let src = fixture("invalid", "flow_assign_arm_leak.glyph");
    let result = run_compile(src, "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert_has_diagnostic_id(&stdout, "G::analyze::use-before-bind");
}

/// §11 invalid: RHS callee declares no return type (`-> Type` absent).
/// Spec §6.2 b: `assignment-rhs-has-no-value`.
#[test]
fn flow_assign_no_value_fires_assignment_rhs_has_no_value() {
    let src = fixture("invalid", "flow_assign_no_value.glyph");
    let result = run_compile(src, "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert_has_diagnostic_id(&stdout, "G::analyze::assignment-rhs-has-no-value");
}

/// §11 invalid: bound name returned, declared types don't match.
/// Reuses the existing `nominal-mismatch` diagnostic.
#[test]
fn flow_assign_return_type_mismatch_fires_nominal_mismatch() {
    let src = fixture("invalid", "flow_assign_return_type_mismatch.glyph");
    let result = run_compile(src, "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert_has_diagnostic_id(&stdout, "G::analyze::nominal-mismatch");
}

/// Codex H3: a flow-local binding passed as a positional argument to
/// a callee whose param has a `:Type` annotation that doesn't
/// nominal-match the binding's recorded type fires
/// `G::analyze::call-arg-type-mismatch` at the call site.
#[test]
fn flow_assign_call_arg_type_mismatch_fires_call_arg_type_mismatch() {
    let src = fixture("invalid", "flow_assign_call_arg_type_mismatch.glyph");
    let result = run_compile(src, "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert_has_diagnostic_id(&stdout, "G::analyze::call-arg-type-mismatch");
}

/// §11 invalid: flow-position assignment inside a `block` flow body.
/// Spec §6.1 + §10: `flow-assign-in-block-unsupported`. Locks the MVP
/// scoping decision in tests so any future block-flow support shows up
/// as a snapshot/test churn (Codex Round 3 High 2).
#[test]
fn flow_assign_in_block_fires_flow_assign_in_block_unsupported() {
    let src = fixture("invalid", "flow_assign_in_block.glyph");
    let result = run_compile(src, "json");
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert_has_diagnostic_id(&stdout, "G::analyze::flow-assign-in-block-unsupported");
}
