//! Integration tests for the CallBodyShape hard-fail plumbing.
//! Covers the lib-level Err → CompileOutcome::Diagnostics conversion and
//! the explicit IR-node-id-ascending ordering of the resulting bag.
//!
//! NOTE: the end-to-end test below is `#[ignore]` after Task 4 because the
//! Result-plumbing only fires when an emit site actually pushes a
//! `CallBodyShape` span. Tasks 5–7 are the ones that flip the emit sites from
//! literal emission to span emission for non-trivial Calls. Un-ignore in
//! Task 7 (or when its dependencies all land) and the test should pass.

use glyph_core::{compile_source_with_effects, CompileOutcome};

const SRC_WITH_MODIFIER: &str = r#"block inspect_repo(scope = ".") -> Report
    description: "Inspect the repository at the given scope."
    flow:
        "Examine the repository at {scope} and produce a report."
        return context

skill diagnose(scope = ".") -> Report
    description: "Inspect the scope with a focus area."
    flow:
        ctx = inspect_repo(scope) with "focus on lint failures"
        return ctx
"#;

#[test]

fn with_modifier_produces_llm_required_diagnostic() {
    let outcome = compile_source_with_effects(SRC_WITH_MODIFIER, 0, "test.glyph", false)
        .expect("compile_source_with_effects must not return CompileError here");
    match outcome {
        CompileOutcome::Diagnostics(bag) => {
            let sorted = bag.sorted();
            let llm_diags: Vec<_> = sorted
                .iter()
                .filter(|d| d.id == "G::expand::llm-required-for-call")
                .collect();
            assert_eq!(
                llm_diags.len(),
                1,
                "expected exactly one G::expand::llm-required-for-call; got bag={sorted:?}"
            );
            let msg = &llm_diags[0].message;
            assert!(
                msg.contains("inspect_repo"),
                "message must name the target: {msg}"
            );
            assert!(
                msg.contains("with modifier"),
                "message must mention with modifier: {msg}"
            );
        }
        CompileOutcome::Compiled { markdown, .. } => panic!(
            "expected Diagnostics outcome for with-modifier Call; got Compiled markdown:\n{markdown}"
        ),
    }
}

fn count_llm_required(src: &str) -> (usize, Vec<String>) {
    let outcome = compile_source_with_effects(src, 0, "test.glyph", false).unwrap();
    match outcome {
        CompileOutcome::Diagnostics(bag) => {
            let sorted = bag.sorted();
            let llms: Vec<_> = sorted
                .iter()
                .filter(|d| d.id == "G::expand::llm-required-for-call")
                .map(|d| d.message.clone())
                .collect();
            (llms.len(), llms)
        }
        CompileOutcome::Compiled { markdown, .. } => {
            panic!("expected Diagnostics; got Compiled markdown:\n{markdown}")
        }
    }
}

const TIER1_TOPLEVEL: &str = "block inspect_repo(scope = \".\") -> Report\n    description: \"Inspect the repository.\"\n    flow:\n        \"Examine the repository at {scope}.\"\n        return context\n\nskill diagnose(scope = \".\") -> Report\n    description: \"Inspect with focus.\"\n    flow:\n        inspect_repo(scope) with \"focus on lint failures\"\n        return context\n";

#[test]
fn site_tier1_toplevel_with_modifier_hard_fails() {
    let (n, msgs) = count_llm_required(TIER1_TOPLEVEL);
    assert_eq!(n, 1, "got msgs={msgs:?}");
    assert!(msgs[0].contains("inspect_repo"));
    assert!(msgs[0].contains("with modifier"));
}

const TIER2_TOPLEVEL: &str = "block summarize_findings(scope = \".\") -> Report\n    description: \"Summarize the recent findings about the repository structure and surface anything notable for follow-up.\"\n    flow:\n        \"Read recent notes about {scope}.\"\n        \"Group them by topic.\"\n        \"Highlight items needing follow-up.\"\n        return context\n\nskill diagnose(scope = \".\") -> Report\n    description: \"Inspect with focus.\"\n    flow:\n        summarize_findings(scope) with \"focus on lint failures\"\n        return context\n";

#[test]
fn site_tier2_toplevel_with_modifier_hard_fails() {
    let (n, msgs) = count_llm_required(TIER2_TOPLEVEL);
    assert_eq!(n, 1, "got msgs={msgs:?}");
    assert!(msgs[0].contains("summarize_findings"));
    assert!(msgs[0].contains("with modifier"));
}

const TIER3_TOPLEVEL: &str = "export block shared_inspect(scope = \".\") -> Report\n    description: \"Shared inspection routine that walks the repository and reports notable findings.\"\n    flow:\n        \"Read recent notes about {scope}.\"\n        \"Group them by topic.\"\n        \"Highlight items needing follow-up.\"\n        return context\n\nskill diagnose(scope = \".\") -> Report\n    description: \"Inspect with focus.\"\n    flow:\n        shared_inspect(scope) with \"focus on lint failures\"\n        return context\n";

#[test]
fn site_tier3_toplevel_with_modifier_hard_fails() {
    let (n, msgs) = count_llm_required(TIER3_TOPLEVEL);
    assert_eq!(n, 1, "got msgs={msgs:?}");
    assert!(msgs[0].contains("shared_inspect"));
    assert!(msgs[0].contains("with modifier"));
}

const STDLIB_TOPLEVEL: &str = "import \"@glyph/std\" { subagent }\n\nskill delegate(scope = \".\") -> Report\n    description: \"Delegate work to a subagent.\"\n    flow:\n        subagent(scope) with \"focus on lint failures\"\n        return context\n";

#[test]
#[ignore = "stdlib end-to-end blocked on PR #149 / flow-assign-subagent-cli — re-enable when build_resolved_imports wires @glyph/std; remove #[ignore]"]
fn site_stdlib_toplevel_with_modifier_hard_fails() {
    // End-to-end regression scaffold: gated #[ignore] until Task 9 / PR #149
    // teaches `compile_directory_with_options` to resolve `@glyph/std`. Once
    // the import path lands, removing the #[ignore] should make this pass.
    let dir = tempfile::tempdir().unwrap();
    let main_path = dir.path().join("delegate.glyph");
    std::fs::write(&main_path, STDLIB_TOPLEVEL).unwrap();
    let result = glyph_core::compile_directory_with_options(&[main_path.clone()], false, false);
    let bag = result
        .outcomes
        .into_iter()
        .find(|(p, _)| p.file_name() == main_path.file_name())
        .map(|(_, outcome)| match outcome {
            glyph_core::FileOutcome::Compiled { diagnostics }
            | glyph_core::FileOutcome::Failed { diagnostics } => diagnostics,
            glyph_core::FileOutcome::Skipped { .. } => {
                panic!("file should not be skipped");
            }
        })
        .expect("delegate.glyph outcome present");
    let sorted = bag.sorted();
    let llms: Vec<_> = sorted
        .iter()
        .filter(|d| d.id == "G::expand::llm-required-for-call")
        .map(|d| d.message.clone())
        .collect();
    assert_eq!(llms.len(), 1, "got msgs={llms:?} sorted={sorted:?}");
    assert!(llms[0].contains("subagent"));
    assert!(llms[0].contains("with modifier"));
}

const TIER2_IN_ARM: &str = "block summarize_findings(scope = \".\") -> Report\n    description: \"Summarize the recent findings about the repository structure and surface anything notable for follow-up.\"\n    flow:\n        \"Read recent notes about {scope}.\"\n        \"Group them by topic.\"\n        \"Highlight items needing follow-up.\"\n        return context\n\nskill diagnose(scope = \".\") -> Report\n    description: \"Inspect with focus.\"\n    flow:\n        if scope == \".\":\n            summarize_findings(scope) with \"focus on lint failures\"\n        return context\n";

#[test]
fn site_tier2_in_arm_with_modifier_hard_fails() {
    let (n, msgs) = count_llm_required(TIER2_IN_ARM);
    assert_eq!(n, 1, "got msgs={msgs:?}");
    assert!(msgs[0].contains("summarize_findings"));
    assert!(msgs[0].contains("with modifier"));
}

const TIER3_IN_ARM: &str = "export block shared_inspect(scope = \".\") -> Report\n    description: \"Shared inspection routine that walks the repository and reports notable findings.\"\n    flow:\n        \"Read recent notes about {scope}.\"\n        \"Group them by topic.\"\n        \"Highlight items needing follow-up.\"\n        return context\n\nskill diagnose(scope = \".\") -> Report\n    description: \"Inspect with focus.\"\n    flow:\n        if scope == \".\":\n            shared_inspect(scope) with \"focus on lint failures\"\n        return context\n";

#[test]
fn site_tier3_in_arm_with_modifier_hard_fails() {
    let (n, msgs) = count_llm_required(TIER3_IN_ARM);
    assert_eq!(n, 1, "got msgs={msgs:?}");
    assert!(msgs[0].contains("shared_inspect"));
    assert!(msgs[0].contains("with modifier"));
}

const TIER1_LOCAL_REFS: &str = "block prep(scope = \".\") -> Report\n    description: \"Prep.\"\n    flow:\n        \"Read {scope}.\"\n        return context\n\nblock inspect(ctx = \"default\") -> Report\n    description: \"Inspect.\"\n    flow:\n        \"Look at {ctx}.\"\n        return context\n\nskill diagnose(scope = \".\") -> Report\n    description: \"Demo.\"\n    flow:\n        ctx = prep(scope)\n        inspect(ctx)\n        return context\n";

#[test]
fn local_refs_alone_hard_fails_with_local_ref_reason() {
    let (n, msgs) = count_llm_required(TIER1_LOCAL_REFS);
    assert_eq!(n, 1, "got msgs={msgs:?}");
    assert!(msgs[0].contains("local-ref cross-references"));
}

const COMBINED: &str = "block prep(scope = \".\") -> Report\n    description: \"Prep.\"\n    flow:\n        \"Read {scope}.\"\n        return context\n\nblock inspect(ctx = \"default\") -> Report\n    description: \"Inspect.\"\n    flow:\n        \"Look at {ctx}.\"\n        return context\n\nskill diagnose(scope = \".\") -> Report\n    description: \"Demo.\"\n    flow:\n        ctx = prep(scope)\n        inspect(ctx) with \"focus on lint\"\n        return context\n";

#[test]
fn modifier_plus_local_refs_yields_single_diagnostic_with_both_reasons() {
    let (n, msgs) = count_llm_required(COMBINED);
    assert_eq!(n, 1, "exactly one diagnostic per failing Call");
    assert!(msgs[0].contains("a with modifier and local-ref cross-references"));
    assert!(msgs[0].contains("the with modifier / rewrite the local reference"));
}

const TWO_CALLS: &str = "block a(scope = \".\") -> Report\n    description: \"A.\"\n    flow:\n        \"Look at {scope}.\"\n        return context\n\nblock b(scope = \".\") -> Report\n    description: \"B.\"\n    flow:\n        \"Look at {scope}.\"\n        return context\n\nskill diagnose(scope = \".\") -> Report\n    description: \"Demo.\"\n    flow:\n        a(scope) with \"m1\"\n        b(scope) with \"m2\"\n        return context\n";

#[test]
fn multiple_failing_calls_ordered_by_ir_node_id() {
    let (n, msgs) = count_llm_required(TWO_CALLS);
    assert_eq!(n, 2);
    assert!(msgs[0].contains("`a`"));
    assert!(msgs[1].contains("`b`"));
}

const IF_ELSE_SAME_TARGET: &str = "block build_walkthrough(scope = \".\") -> Report\n    description: \"Walkthrough of the relevant scope, naming each construct and the instruction it produces.\"\n    flow:\n        \"Walk {scope}.\"\n        return context\n\nskill diagnose(scope = \".\") -> Report\n    description: \"Demo.\"\n    flow:\n        if scope == \".\":\n            build_walkthrough(scope) with \"name each construct and show it beside the instruction it creates\"\n        else:\n            build_walkthrough(scope)\n        return context\n";

#[test]
fn if_arms_same_target_only_modifier_arm_hard_fails() {
    let (n, msgs) = count_llm_required(IF_ELSE_SAME_TARGET);
    assert_eq!(n, 1, "got msgs={msgs:?}");
    assert!(msgs[0].contains("build_walkthrough"));
    assert!(msgs[0].contains("with modifier"));
}

// ---------------------------------------------------------------------
// Trivial-Call regression tests (spec §6.1).
//
// Safety net: a trivial Call (no `site_modifier`, no `local_refs`) must
// still render byte-identically to today's deterministic output for each
// of the seven emit sites — i.e. it must compile cleanly (no
// `G::expand::llm-required-for-call`) and the rendered Markdown must
// contain the expected anchor / inline-body substring.

fn compile_to_md(src: &str) -> String {
    match compile_source_with_effects(src, 0, "test.glyph", false).unwrap() {
        CompileOutcome::Compiled { markdown, .. } => markdown,
        CompileOutcome::Diagnostics(bag) => panic!(
            "trivial Call must compile cleanly; got diagnostics:\n{:?}",
            bag.sorted()
        ),
    }
}

#[test]
fn trivial_tier1_toplevel_renders_inline_body() {
    let src = r#"block inspect(scope = ".") -> Report
    description: "Inspect."
    flow:
        "Look at {scope}."
        return context

skill diagnose(scope = ".") -> Report
    description: "Demo."
    flow:
        inspect(scope)
        return context
"#;
    let md = compile_to_md(src);
    assert!(
        md.contains("Look at"),
        "trivial tier-1 inline body must render in md:\n{md}"
    );
}

#[test]
fn trivial_tier2_toplevel_renders_follow_procedure() {
    let src = r#"block do_steps()
    description: "Steps."
    flow:
        "Do thing one."
        "Do thing two."
        "Do thing three."
        "Do thing four."

skill demo()
    description: "Demo."
    flow:
        do_steps()
"#;
    let md = compile_to_md(src);
    assert!(
        md.contains("Follow the do-steps procedure below."),
        "trivial tier-2 anchor must render in md:\n{md}"
    );
}

#[test]
fn trivial_tier3_toplevel_renders_follow_procedure() {
    // Tier-3 = external library file whose `export block` body crosses the
    // 150-word threshold. We drive this through the directory-level compile
    // API (same harness pattern as
    // `site_stdlib_toplevel_with_modifier_hard_fails`) because the
    // single-file `compile_source_with_effects` helper cannot resolve
    // `import "./..."` paths. The body below is deliberately verbose so
    // `emit_library_procedures` writes a sibling procedure file and the
    // call lowers to `projection_tier = Some(3)`.
    let dir = tempfile::tempdir().unwrap();
    let helper_path = dir.path().join("helper.glyph");
    let main_path = dir.path().join("main.glyph");
    let helper_src = "export block shared_inspect(scope = \".\") -> Report\n    description: \"Shared inspection routine that walks the repository at the given scope and reports notable findings to the orchestrator skill, suitable for downstream triage workflows.\"\n    flow:\n        \"Open the repository at {scope} and enumerate every tracked file, paying particular attention to top-level configuration, dependency manifests, build scripts, and CI workflow definitions.\"\n        \"Read the contents of each manifest and configuration file in turn, taking careful notes about declared dependencies, environment variables, feature flags, language toolchain versions, and any other facts that downstream auditors will want to inspect.\"\n        \"Group the collected notes by topic — runtime dependencies, build tooling, deployment configuration, observability instrumentation, security posture — and within each topic sort entries by severity so the most important findings appear first.\"\n        \"Cross-reference the grouped notes with any historical lint, security-scan, or test-failure reports already present in the repository to flag regressions, recurrent themes, and items the team has previously chosen to defer.\"\n        \"Highlight items needing follow-up by tagging each one with a clear owner, an estimated effort level, and a short rationale explaining why the team should prioritise resolving it before the next release.\"\n        return context\n";
    std::fs::write(&helper_path, helper_src).unwrap();
    let main_src = "import \"./helper.glyph\" { shared_inspect }\n\nskill diagnose(scope = \".\") -> Report\n    description: \"Demo.\"\n    flow:\n        shared_inspect(scope)\n        return context\n";
    std::fs::write(&main_path, main_src).unwrap();
    let result = glyph_core::compile_directory_with_options(
        &[helper_path.clone(), main_path.clone()],
        false,
        false,
    );
    let outcome = result
        .outcomes
        .into_iter()
        .find(|(p, _)| p.file_name() == main_path.file_name())
        .map(|(_, o)| o)
        .expect("main.glyph outcome present");
    match outcome {
        glyph_core::FileOutcome::Compiled { diagnostics } => {
            let sorted = diagnostics.sorted();
            assert!(
                sorted.is_empty(),
                "trivial tier-3 Call must compile cleanly; got diagnostics: {sorted:?}"
            );
            let md = std::fs::read_to_string(main_path.with_extension("md"))
                .expect("compiled .md must exist for trivial tier-3 Call");
            assert!(
                md.contains("shared-inspect.md"),
                "trivial tier-3 anchor must reference the external procedure file: {md}"
            );
        }
        glyph_core::FileOutcome::Failed { diagnostics } => panic!(
            "trivial tier-3 Call must compile cleanly; got Failed: {:?}",
            diagnostics.sorted()
        ),
        glyph_core::FileOutcome::Skipped { .. } => {
            panic!("main.glyph should not be skipped");
        }
    }
}

#[test]
#[ignore = "stdlib resolution blocked on PR #149 — same gate as site_stdlib_toplevel_with_modifier_hard_fails"]
fn trivial_stdlib_toplevel_renders_follow_procedure() {
    let src = "import \"@glyph/std\" { subagent }\n\nskill delegate(scope = \".\") -> Report\n    description: \"Delegate work to a subagent.\"\n    flow:\n        subagent(scope)\n        return context\n";
    let dir = tempfile::tempdir().unwrap();
    let main_path = dir.path().join("delegate.glyph");
    std::fs::write(&main_path, src).unwrap();
    let result = glyph_core::compile_directory_with_options(&[main_path.clone()], false, false);
    let outcome = result
        .outcomes
        .into_iter()
        .find(|(p, _)| p.file_name() == main_path.file_name())
        .map(|(_, o)| o)
        .expect("delegate.glyph outcome present");
    match outcome {
        glyph_core::FileOutcome::Compiled { diagnostics } => {
            assert!(
                diagnostics.sorted().is_empty(),
                "trivial stdlib-bound Call must compile cleanly"
            );
            let md = std::fs::read_to_string(main_path.with_extension("md"))
                .expect("compiled .md must exist");
            assert!(
                md.contains("subagent"),
                "trivial stdlib-bound Call must render a subagent reference:\n{md}"
            );
        }
        other => panic!("expected Compiled outcome; got {other:?}"),
    }
}

#[test]
fn trivial_tier1_in_arm_renders_inline_body() {
    let src = r#"block inspect(scope = ".") -> Report
    description: "Inspect."
    flow:
        "Look at {scope}."
        return context

skill diagnose(scope = ".") -> Report
    description: "Demo."
    flow:
        if scope == ".":
            inspect(scope)
        return context
"#;
    let md = compile_to_md(src);
    assert!(
        md.contains("Look at"),
        "trivial tier-1 in-arm inline body must render in md:\n{md}"
    );
}

#[test]
fn trivial_tier2_in_arm_renders_follow_procedure() {
    let src = r#"block do_steps()
    description: "Steps."
    flow:
        "Do thing one."
        "Do thing two."
        "Do thing three."
        "Do thing four."

skill demo(scope = ".")
    description: "Demo."
    flow:
        if scope == ".":
            do_steps()
"#;
    let md = compile_to_md(src);
    assert!(
        md.contains("Follow the do-steps procedure"),
        "trivial tier-2 in-arm anchor must render in md:\n{md}"
    );
}

#[test]
fn trivial_tier3_in_arm_renders_follow_procedure() {
    let dir = tempfile::tempdir().unwrap();
    let helper_path = dir.path().join("helper.glyph");
    let main_path = dir.path().join("main.glyph");
    let helper_src = "export block shared_inspect(scope = \".\") -> Report\n    description: \"Shared inspection routine that walks the repository at the given scope and reports notable findings to the orchestrator skill, suitable for downstream triage workflows.\"\n    flow:\n        \"Open the repository at {scope} and enumerate every tracked file, paying particular attention to top-level configuration, dependency manifests, build scripts, and CI workflow definitions.\"\n        \"Read the contents of each manifest and configuration file in turn, taking careful notes about declared dependencies, environment variables, feature flags, language toolchain versions, and any other facts that downstream auditors will want to inspect.\"\n        \"Group the collected notes by topic — runtime dependencies, build tooling, deployment configuration, observability instrumentation, security posture — and within each topic sort entries by severity so the most important findings appear first.\"\n        \"Cross-reference the grouped notes with any historical lint, security-scan, or test-failure reports already present in the repository to flag regressions, recurrent themes, and items the team has previously chosen to defer.\"\n        \"Highlight items needing follow-up by tagging each one with a clear owner, an estimated effort level, and a short rationale explaining why the team should prioritise resolving it before the next release.\"\n        return context\n";
    std::fs::write(&helper_path, helper_src).unwrap();
    let main_src = "import \"./helper.glyph\" { shared_inspect }\n\nskill diagnose(scope = \".\") -> Report\n    description: \"Demo.\"\n    flow:\n        if scope == \".\":\n            shared_inspect(scope)\n        return context\n";
    std::fs::write(&main_path, main_src).unwrap();
    let result = glyph_core::compile_directory_with_options(
        &[helper_path.clone(), main_path.clone()],
        false,
        false,
    );
    let outcome = result
        .outcomes
        .into_iter()
        .find(|(p, _)| p.file_name() == main_path.file_name())
        .map(|(_, o)| o)
        .expect("main.glyph outcome present");
    match outcome {
        glyph_core::FileOutcome::Compiled { diagnostics } => {
            let sorted = diagnostics.sorted();
            assert!(
                sorted.is_empty(),
                "trivial tier-3 in-arm Call must compile cleanly; got diagnostics: {sorted:?}"
            );
            let md = std::fs::read_to_string(main_path.with_extension("md"))
                .expect("compiled .md must exist for trivial tier-3 in-arm Call");
            assert!(
                md.contains("shared-inspect.md"),
                "trivial tier-3 in-arm anchor must reference the external procedure file: {md}"
            );
        }
        glyph_core::FileOutcome::Failed { diagnostics } => panic!(
            "trivial tier-3 in-arm Call must compile cleanly; got Failed: {:?}",
            diagnostics.sorted()
        ),
        glyph_core::FileOutcome::Skipped { .. } => {
            panic!("main.glyph should not be skipped");
        }
    }
}
