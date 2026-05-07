//! Slice 12 integration tests — multi-file build orchestration.
//!
//! Covers all five acceptance criteria via the CLI binary:
//!   1. `glyph compile dir/` processes every `.glyph` even if not transitively reached
//!   2. Files compile in topological order (libraries before consumers)
//!   3. Failure in b.glyph skips c.glyph (which imports it) with the build warning
//!   4. Stale c.md left untouched on disk after c.glyph skip; stderr note emitted
//!   5. Build exits 1 if any file failed; partial output present for successful files

use std::path::PathBuf;
use std::process::Command;

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

/// AC1: `glyph compile dir/` processes every `.glyph` even if not transitively reached.
#[test]
fn ac1_directory_compile_all_files() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(dir.path().join("a.glyph"), "\
skill alpha()
    description: \"Alpha.\"
    flow:
        \"Do alpha.\"
").unwrap();

    std::fs::write(dir.path().join("b.glyph"), "\
skill beta()
    description: \"Beta.\"
    flow:
        \"Do beta.\"
").unwrap();

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(dir.path())
        .output()
        .expect("failed to spawn glyph binary");

    assert_eq!(
        output.status.code(),
        Some(0),
        "should exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(dir.path().join("a.md").exists(), "a.md should be produced");
    assert!(dir.path().join("b.md").exists(), "b.md should be produced");
}

/// AC2: Files compile in topological order (libraries before consumers).
/// We verify this indirectly: if lib compiles first, consumer can succeed.
#[test]
fn ac2_topological_order() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(dir.path().join("lib.glyph"), "\
export const greeting = \"Hello.\"
").unwrap();

    std::fs::write(dir.path().join("consumer.glyph"), "\
import \"./lib.glyph\" { greeting }

skill main()
    description: \"Main.\"
    flow:
        \"Go.\"
").unwrap();

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(dir.path())
        .output()
        .expect("failed to spawn glyph binary");

    // Both files should produce output (lib might produce nothing since it has
    // no skill, but it should not fail).
    let stderr = String::from_utf8_lossy(&output.stderr);
    // No hard errors expected.
    assert_ne!(
        output.status.code(),
        Some(3),
        "should not be invocation error, stderr: {}",
        stderr
    );

    assert!(dir.path().join("consumer.md").exists(), "consumer.md should be produced");
}

/// AC3: Failure in b.glyph skips c.glyph (which imports it) with the build warning.
#[test]
fn ac3_failure_skips_dependent() {
    let dir = tempfile::tempdir().unwrap();

    // a — valid standalone
    std::fs::write(dir.path().join("a.glyph"), "\
skill alpha()
    description: \"Alpha.\"
    flow:
        \"Do alpha.\"
").unwrap();

    // b — broken
    std::fs::write(dir.path().join("b.glyph"), "\
this is broken!!!
").unwrap();

    // c — imports b
    std::fs::write(dir.path().join("c.glyph"), "\
import \"./b.glyph\" { something }

skill gamma()
    description: \"Gamma.\"
    flow:
        \"Do gamma.\"
").unwrap();

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(dir.path())
        .output()
        .expect("failed to spawn glyph binary");

    assert_eq!(output.status.code(), Some(1), "should exit 1");

    // a.md should exist (partial output).
    assert!(dir.path().join("a.md").exists(), "a.md should exist");

    // c.md should NOT exist (skipped).
    assert!(!dir.path().join("c.md").exists(), "c.md should not exist");

    // stderr should contain the skipped-due-to-failed-import warning.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("skipped-due-to-failed-import"),
        "stderr should contain skipped warning: {}",
        stderr
    );
}

/// AC4: Stale c.md left untouched on disk after c.glyph skip; stderr note emitted.
#[test]
fn ac4_stale_md_untouched_with_note() {
    let dir = tempfile::tempdir().unwrap();

    let stale_content = "# Previous output\nStale.";
    std::fs::write(dir.path().join("c.md"), stale_content).unwrap();

    // b — broken
    std::fs::write(dir.path().join("b.glyph"), "\
this is broken!!!
").unwrap();

    // c — imports b, will be skipped
    std::fs::write(dir.path().join("c.glyph"), "\
import \"./b.glyph\" { something }

skill gamma()
    description: \"Gamma.\"
    flow:
        \"Do gamma.\"
").unwrap();

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(dir.path())
        .output()
        .expect("failed to spawn glyph binary");

    assert_eq!(output.status.code(), Some(1));

    // c.md should still contain the stale content.
    let c_md = std::fs::read_to_string(dir.path().join("c.md")).unwrap();
    assert_eq!(c_md, stale_content, "stale c.md should be untouched");

    // stderr should contain the stale note.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("was not regenerated"),
        "stderr should note stale .md: {}",
        stderr
    );
}

/// AC5: Build exits 1 if any file failed; partial output present for successful files.
#[test]
fn ac5_exit_1_partial_output() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(dir.path().join("good.glyph"), "\
skill good()
    description: \"Good.\"
    flow:
        \"Do good.\"
").unwrap();

    std::fs::write(dir.path().join("bad.glyph"), "\
this is broken!!!
").unwrap();

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(dir.path())
        .output()
        .expect("failed to spawn glyph binary");

    assert_eq!(output.status.code(), Some(1), "should exit 1");
    assert!(dir.path().join("good.md").exists(), "good.md should exist");
    assert!(!dir.path().join("bad.md").exists(), "bad.md should not exist");
}
