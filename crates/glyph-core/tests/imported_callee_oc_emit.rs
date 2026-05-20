//! B10 regression: `--emit-ir` JSON must populate `callee_output_contract`
//! for imported Tier-1 calls when the imported block declares a return type.

use glyph_core::compile_directory_with_options;

const LIB_GLYPH: &str = "\
export block helper() -> BranchName
    flow:
        return <current_branch>
";

const MAIN_GLYPH: &str = "\
import \"./lib.glyph\" { helper }

skill main() -> BranchName
    description: \"Main.\"
    flow:
        helper()
        return <current_branch>
";

#[test]
fn imported_tier1_callee_output_contract_is_serialized() {
    let dir = tempfile::tempdir().unwrap();
    let lib_path = dir.path().join("lib.glyph");
    let main_path = dir.path().join("main.glyph");
    std::fs::write(&lib_path, LIB_GLYPH).unwrap();
    std::fs::write(&main_path, MAIN_GLYPH).unwrap();

    let result = compile_directory_with_options(
        &[lib_path.clone(), main_path.clone()],
        /* emit_ir = */ true,
        /* enable_effects = */ false,
    );

    let outcome = result
        .outcomes
        .into_iter()
        .find(|(p, _)| p.file_name() == main_path.file_name())
        .map(|(_, o)| o)
        .expect("main.glyph outcome present");

    match outcome {
        glyph_core::FileOutcome::Compiled { diagnostics } => {
            assert!(
                !diagnostics.has_error(),
                "main.glyph must compile cleanly; got diagnostics: {:?}",
                diagnostics.sorted()
            );
        }
        glyph_core::FileOutcome::Failed { diagnostics } => panic!(
            "main.glyph must compile cleanly; got Failed: {:?}",
            diagnostics.sorted()
        ),
        glyph_core::FileOutcome::Skipped { .. } => panic!("main.glyph should not be skipped"),
    }

    let ir_path = main_path.with_extension("ir.json");
    let ir_text = std::fs::read_to_string(&ir_path)
        .unwrap_or_else(|e| panic!("expected IR JSON at {}: {e}", ir_path.display()));
    let ir: serde_json::Value =
        serde_json::from_str(&ir_text).expect("emitted IR must be valid JSON");

    let flow = ir
        .get("skill")
        .and_then(|s| s.get("flow"))
        .and_then(|f| f.as_array())
        .expect("skill.flow array");

    let helper_call = flow
        .iter()
        .find(|n| {
            n.get("kind").and_then(|k| k.as_str()) == Some("call")
                && n.get("target").and_then(|t| t.as_str()) == Some("helper")
        })
        .expect("helper() call node must appear in skill.flow");

    let oc = helper_call
        .get("callee_output_contract")
        .expect("call must carry the callee_output_contract field");
    assert!(
        !oc.is_null(),
        "imported helper() callee_output_contract must not be null; full call = {helper_call}",
    );

    assert_eq!(
        oc.get("form").and_then(|v| v.as_str()),
        Some("identifier"),
        "imported callee OC must surface as identifier form: {oc}"
    );
    assert_eq!(
        oc.get("target_name").and_then(|v| v.as_str()),
        Some("current_branch"),
        "imported callee OC target_name must match producer: {oc}"
    );
}
