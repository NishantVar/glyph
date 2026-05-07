//! Regression test for issue #85: an imported Tier-1 callee's
//! `output_contract` must drive both the expand-time return-fold gate and
//! the emit-time suffix template, exactly as a same-file callee would.
//!
//! Codex flagged that `block_has_output_contract()` only walked
//! `IrNode::Block` entries in the consumer arena, which never sees imported
//! `<150`-word export blocks (they live in `resolved_imports.block_bodies`).
//! The fix hoists the imported callee's OC onto `IrCall.callee_output_contract`
//! during the cross-file fix-up step, so the gates read the same field
//! regardless of import boundary.

use std::path::PathBuf;
use std::process::Command;

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

#[test]
fn imported_tier1_callee_uses_locked_identifier_suffix() {
    let dir = tempfile::tempdir().unwrap();
    let lib_path = dir.path().join("lib.glyph");
    let consumer_path = dir.path().join("consumer.glyph");

    std::fs::write(
        &lib_path,
        "\
export block helper() -> BranchName
    flow:
        \"Probe the working tree.\"
        return <current_branch>
",
    )
    .unwrap();

    std::fs::write(
        &consumer_path,
        "\
import \"./lib.glyph\" { helper }

skill main()
    description: \"Demo.\"
    flow:
        helper()
",
    )
    .unwrap();

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(dir.path())
        .output()
        .expect("failed to spawn glyph binary");

    assert!(
        output.status.success(),
        "compile failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let md = std::fs::read_to_string(dir.path().join("consumer.md")).unwrap();
    assert!(
        md.contains(", and return that as your result."),
        "imported Tier-1 callee should append the locked Identifier suffix:\n{md}"
    );
    assert!(
        !md.contains("Return the result of"),
        "must not fall back to the legacy `Return the result of …` fold:\n{md}"
    );
    assert!(
        !md.contains("<current_branch>"),
        "compiled Markdown must not leak the literal output-target token:\n{md}"
    );
}

#[test]
fn imported_return_only_tier1_callee_uses_standalone_template() {
    let dir = tempfile::tempdir().unwrap();
    let lib_path = dir.path().join("lib.glyph");
    let consumer_path = dir.path().join("consumer.glyph");

    // Return-only helper: body_text is empty so resolved_imports.block_bodies
    // doesn't include the helper. The cross-file fix-up still hoists the OC
    // onto the Call and materializes an empty resolved_body so the Tier-1
    // inline path runs and routes through the standalone return template.
    std::fs::write(
        &lib_path,
        "\
export block helper() -> BranchName
    flow:
        return <current_branch>
",
    )
    .unwrap();

    std::fs::write(
        &consumer_path,
        "\
import \"./lib.glyph\" { helper }

skill main()
    description: \"Demo.\"
    flow:
        helper()
",
    )
    .unwrap();

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(dir.path())
        .output()
        .expect("failed to spawn glyph binary");

    assert!(
        output.status.success(),
        "compile failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let md = std::fs::read_to_string(dir.path().join("consumer.md")).unwrap();
    assert!(
        md.contains("1. Return current branch as your result."),
        "imported return-only Tier-1 callee should produce a standalone return step:\n{md}"
    );
    assert!(
        !md.contains("1. , and return"),
        "must not emit a leading-comma malformed suffix:\n{md}"
    );
}
