//! Integration tests for `G::expand::llm-required-for-param-description`.
//! Modeled on `callbodyshape_span.rs`. Public API only:
//! `compile_source_with_effects`, `CompileOutcome`, `DiagBag`.

use glyph_core::{compile_source_with_effects, CompileOutcome};

/// Test 2: an inline `<"...">` description compiles silently.
#[test]
fn described_param_is_silent() {
    let src = r#"skill diagnose(scope = "." <"directory to inspect">) -> Report
    description: "Demo."
    flow:
        "Look at {scope}."
        return context
"#;
    let outcome = compile_source_with_effects(src, 0, "demo.glyph", false)
        .expect("compile_source_with_effects must not return CompileError here");
    match outcome {
        CompileOutcome::Compiled { markdown, .. } => {
            assert!(
                markdown.contains("directory to inspect"),
                "described param prose must appear in markdown:\n{markdown}"
            );
        }
        CompileOutcome::Diagnostics(bag) => {
            panic!("expected Compiled, got diagnostics: {:#?}", bag.sorted())
        }
    }
}

/// Test 3: type-registry fallback compiles silently.
#[test]
fn type_registry_fallback_is_silent() {
    let src = r#"type Scope = <"a filesystem path">

skill diagnose(s: Scope = ".") -> Report
    description: "Demo."
    flow:
        "Look at {s}."
        return context
"#;
    let outcome = compile_source_with_effects(src, 0, "demo.glyph", false)
        .expect("compile_source_with_effects must not return CompileError here");
    match outcome {
        CompileOutcome::Compiled { markdown, .. } => {
            assert!(
                markdown.contains("a filesystem path"),
                "type-registry description must appear in markdown:\n{markdown}"
            );
        }
        CompileOutcome::Diagnostics(bag) => {
            panic!("expected Compiled, got diagnostics: {:#?}", bag.sorted())
        }
    }
}

/// Test 1: un-described param hard-fails at fill time.
#[test]
fn undescribed_param_hard_fails() {
    let src = r#"skill foo(scope = ".") -> Report
    description: "Demo."
    flow:
        "Look at {scope}."
        return context
"#;
    let outcome = compile_source_with_effects(src, 0, "demo.glyph", false)
        .expect("compile_source_with_effects must not return CompileError here");
    let bag = match outcome {
        CompileOutcome::Diagnostics(bag) => bag,
        CompileOutcome::Compiled { markdown, .. } => {
            panic!("expected diagnostics, got Compiled:\n{markdown}")
        }
    };
    let diags: Vec<_> = bag.sorted();
    assert_eq!(
        diags.len(),
        1,
        "expected exactly one diagnostic, got: {diags:#?}"
    );
    assert_eq!(
        diags[0].id.as_str(),
        "G::expand::llm-required-for-param-description"
    );
    assert!(
        diags[0].message.contains("scope"),
        "diagnostic must name the parameter:\n{}",
        diags[0].message
    );
}

// ------------------------------------------------------------------
// Procedure-path (Tier-3 directory-pipeline) tests
// ------------------------------------------------------------------

/// Helper: build a Tier-3-qualifying export block body. The body must
/// cross `body_word_count >= 150` (see `lib.rs:2231`) so the block
/// routes through `emit_library_procedures` -> `emit_procedure`.
fn long_tier3_body() -> String {
    let mut s = String::new();
    for i in 0..20 {
        s.push_str(&format!(
            "        \"Step {} of the inspection: carefully examine the repository structure and contents for noteworthy patterns.\"\n",
            i + 1
        ));
    }
    s
}

/// Test 4 -- directory pipeline: a library file with a >=150-word
/// export block (qualifies for Tier-3 at `lib.rs:2231`) and an
/// un-described param routes through `emit_library_procedures` ->
/// `emit_procedure` and surfaces the new diagnostic in the library
/// file's DiagBag.
#[test]
fn procedure_param_undescribed_directory_pipeline() {
    let dir = tempfile::tempdir().unwrap();
    let lib_src = format!(
        "export block inspect_repo(scope = \".\") -> Report\n    description: \"Inspect the repository for issues.\"\n    flow:\n{}        return context\n",
        long_tier3_body()
    );
    let lib_path = dir.path().join("repo_tools.glyph");
    std::fs::write(&lib_path, &lib_src).unwrap();

    let result =
        glyph_core::compile_directory_with_options(std::slice::from_ref(&lib_path), false, false);
    let outcome = result
        .outcomes
        .into_iter()
        .find(|(p, _)| p.file_name() == lib_path.file_name())
        .map(|(_, o)| o)
        .expect("repo_tools.glyph outcome present");
    let bag = match outcome {
        glyph_core::FileOutcome::Compiled { diagnostics }
        | glyph_core::FileOutcome::Failed { diagnostics } => diagnostics,
        glyph_core::FileOutcome::Skipped { .. } => panic!("library file should not be skipped"),
    };
    let sorted = bag.sorted();
    let matches: Vec<_> = sorted
        .iter()
        .filter(|d| d.id.as_str() == "G::expand::llm-required-for-param-description")
        .collect();
    assert!(
        !matches.is_empty(),
        "expected at least one llm-required-for-param-description diagnostic; got: {sorted:#?}"
    );
    assert!(
        matches.iter().any(|d| d.message.contains("scope")),
        "diagnostic must name the parameter `scope`: {matches:#?}"
    );

    // The procedure .md must NOT be written when the diagnostic fires.
    let proc_md = dir.path().join("repo_tools").join("inspect-repo.md");
    assert!(
        !proc_md.exists(),
        "procedure .md must not be written when hard-fail diagnostic fires: {}",
        proc_md.display()
    );
}

/// Test 5 -- directory pipeline: same shape as Test 4 but with a
/// `<"...">` description on the param. Compile succeeds, no
/// `llm-required-for-param-description` diagnostic surfaces.
#[test]
fn procedure_param_described_directory_pipeline() {
    let dir = tempfile::tempdir().unwrap();
    let lib_src = format!(
        "export block inspect_repo(scope = \".\" <\"directory to inspect\">) -> Report\n    description: \"Inspect the repository for issues.\"\n    flow:\n{}        return context\n",
        long_tier3_body()
    );
    let lib_path = dir.path().join("repo_tools.glyph");
    std::fs::write(&lib_path, &lib_src).unwrap();

    let result =
        glyph_core::compile_directory_with_options(std::slice::from_ref(&lib_path), false, false);
    let outcome = result
        .outcomes
        .into_iter()
        .find(|(p, _)| p.file_name() == lib_path.file_name())
        .map(|(_, o)| o)
        .expect("repo_tools.glyph outcome present");
    let bag = match outcome {
        glyph_core::FileOutcome::Compiled { diagnostics } => diagnostics,
        glyph_core::FileOutcome::Failed { diagnostics } => panic!(
            "described param library must compile cleanly; got Failed with: {:#?}",
            diagnostics.sorted()
        ),
        glyph_core::FileOutcome::Skipped { .. } => panic!("library file should not be skipped"),
    };
    let sorted = bag.sorted();
    assert!(
        !sorted
            .iter()
            .any(|d| d.id.as_str() == "G::expand::llm-required-for-param-description"),
        "described param must not surface llm-required-for-param-description; got: {sorted:#?}"
    );
}

// ---------- Test 6 ----------------------------------------------------------
// Mixed source: skill foo's `scope` param has no description (fires
// llm-required-for-param-description) AND the call to `inspect(scope) with
// "extra modifier"` fires llm-required-for-call. Both diagnostics must
// coexist in the bag.
#[test]
fn mixed_param_and_call_hard_fails_emit_both_diagnostics() {
    let src = "block inspect(scope = \".\" <\"directory\">) -> Report\n    description: \"Inspect.\"\n    flow:\n        \"Look at {scope}.\"\n        return context\n\nskill foo(scope = \".\") -> Report\n    description: \"Demo.\"\n    flow:\n        inspect(scope) with \"extra modifier\"\n        return context\n";
    let outcome = compile_source_with_effects(src, 0, "demo.glyph", false)
        .expect("compile_source_with_effects must not return CompileError here");
    let bag = match outcome {
        CompileOutcome::Diagnostics(bag) => bag,
        CompileOutcome::Compiled { markdown, .. } => {
            panic!("expected diagnostics, got Compiled:\n{markdown}")
        }
    };
    let diags: Vec<_> = bag.sorted();
    let ids: Vec<&str> = diags.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(
        diags.len(),
        2,
        "expected exactly two diagnostics, got: {ids:?}"
    );
    assert!(ids.contains(&"G::expand::llm-required-for-call"));
    assert!(ids.contains(&"G::expand::llm-required-for-param-description"));
}

// ---------- Test 7 ----------------------------------------------------------
// Two params in one source: typed `s: Scope` (Scope has no registry entry,
// so type-registry fallback misses) and untyped `flag`. Both hard-fail.
// Typed-param message must include `type <Type> = <\"...\">` registry
// remediation; untyped must not (and must not mention "type registry").
#[test]
fn typed_vs_untyped_param_remediation_wording_diverges() {
    let src = "skill demo(s: Scope = \".\", flag = \"\") -> Report\n    description: \"Demo.\"\n    flow:\n        \"Look at {s} with {flag}.\"\n        return context\n";
    let outcome = compile_source_with_effects(src, 0, "demo.glyph", false)
        .expect("compile_source_with_effects must not return CompileError here");
    let bag = match outcome {
        CompileOutcome::Diagnostics(bag) => bag,
        CompileOutcome::Compiled { markdown, .. } => {
            panic!("expected diagnostics, got Compiled:\n{markdown}")
        }
    };
    let diags: Vec<_> = bag.sorted();
    let param_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.id.as_str() == "G::expand::llm-required-for-param-description")
        .collect();
    assert_eq!(
        param_diags.len(),
        2,
        "expected two param-description diagnostics, got: {param_diags:#?}"
    );
    let typed_msg = param_diags
        .iter()
        .find(|d| d.message.contains("`s`"))
        .map(|d| d.message.clone())
        .expect("typed-param `s` diagnostic must be present");
    let untyped_msg = param_diags
        .iter()
        .find(|d| d.message.contains("`flag`"))
        .map(|d| d.message.clone())
        .expect("untyped-param `flag` diagnostic must be present");
    assert!(
        typed_msg.contains("type <Type> = <\"...\">"),
        "typed-param message must include registry remediation:\n{typed_msg}"
    );
    assert!(
        !untyped_msg.contains("type <Type> = <\"...\">"),
        "untyped-param message must not include registry remediation:\n{untyped_msg}"
    );
    assert!(
        !untyped_msg.contains("type registry"),
        "untyped-param message must not mention the type registry:\n{untyped_msg}"
    );
}
