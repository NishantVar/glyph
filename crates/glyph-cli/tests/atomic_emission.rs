//! Slice 16 integration tests — atomic emission and stale `.tmp` cleanup.
//!
//! Acceptance criteria:
//!   1. Mid-pipeline crash leaves no `.tmp` files and no half-written `.md`
//!   2. Prior successful `.md` survives a failed re-build
//!   3. Stale `.tmp` from a SIGINT'd previous run is cleaned at startup
//!   4. Same rules apply uniformly to `.md`, `.ir.json`, and procedure files

use std::path::PathBuf;
use std::process::Command;

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

/// AC1: A file that fails compilation leaves no `.tmp` and no half-written `.md`.
#[test]
fn failed_compile_leaves_no_tmp_and_no_md() {
    let dir = tempfile::tempdir().unwrap();

    // Write a glyph file with a syntax error (no skill declaration — will error).
    std::fs::write(
        dir.path().join("broken.glyph.md"),
        "this is not valid glyph syntax and has no skill declaration\n",
    )
    .unwrap();

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(dir.path().join("broken.glyph.md"))
        .output()
        .expect("failed to run glyph");

    assert_ne!(output.status.code(), Some(0));

    // No .tmp file should exist
    assert!(
        !dir.path().join("broken.md.tmp").exists(),
        "tmp file should not exist after failed compile"
    );
    // No .md output should exist
    assert!(
        !dir.path().join("broken.md").exists(),
        "output .md should not exist after failed compile"
    );
}

/// AC2: Prior successful `.md` survives a failed re-build.
#[test]
fn prior_md_survives_failed_rebuild() {
    let dir = tempfile::tempdir().unwrap();

    let source_path = dir.path().join("hello.glyph.md");
    let output_path = dir.path().join("hello.md");

    // Write a valid glyph file and compile it successfully.
    std::fs::write(
        &source_path,
        "\
skill hello()
    description: \"Say hello.\"

    flow:
        \"Greet the user warmly.\"
",
    )
    .unwrap();

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(&source_path)
        .output()
        .expect("failed to run glyph");

    assert_eq!(output.status.code(), Some(0), "first compile should succeed");
    assert!(output_path.exists(), "output .md should exist after successful compile");

    let original_content = std::fs::read_to_string(&output_path).unwrap();

    // Now break the source and recompile.
    std::fs::write(
        &source_path,
        "this is broken and has no skill declaration\n",
    )
    .unwrap();

    let output2 = Command::new(glyph_bin())
        .arg("compile")
        .arg(&source_path)
        .output()
        .expect("failed to run glyph");

    assert_ne!(output2.status.code(), Some(0), "second compile should fail");

    // Prior .md should still exist with original content.
    assert!(output_path.exists(), "prior .md must survive failed rebuild");
    let after_content = std::fs::read_to_string(&output_path).unwrap();
    assert_eq!(
        original_content, after_content,
        "prior .md content must be unchanged after failed rebuild"
    );

    // No .tmp should be left behind.
    assert!(
        !dir.path().join("hello.md.tmp").exists(),
        "no .tmp should remain after failed rebuild"
    );
}

/// AC3: Stale `.tmp` from a SIGINT'd previous run is cleaned at startup.
#[test]
fn stale_tmp_cleaned_on_rebuild() {
    let dir = tempfile::tempdir().unwrap();

    let source_path = dir.path().join("greet.glyph.md");

    std::fs::write(
        &source_path,
        "\
skill greet()
    description: \"Greet the user.\"

    flow:
        \"Say hi to the user.\"
",
    )
    .unwrap();

    // Simulate a stale .tmp from a previous SIGINT'd run.
    std::fs::write(dir.path().join("greet.md.tmp"), "stale tmp content\n").unwrap();
    assert!(dir.path().join("greet.md.tmp").exists());

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(&source_path)
        .output()
        .expect("failed to run glyph");

    assert_eq!(output.status.code(), Some(0), "compile should succeed");

    // Stale .tmp should be gone.
    assert!(
        !dir.path().join("greet.md.tmp").exists(),
        "stale .tmp should be cleaned up during build"
    );

    // Fresh .md should exist.
    assert!(dir.path().join("greet.md").exists(), "output .md should exist");
}

/// AC4: Procedure files also get atomic emission — stale .tmp cleaned.
#[test]
fn procedure_files_stale_tmp_cleaned() {
    let dir = tempfile::tempdir().unwrap();

    // Create a library file with a large export block (>= 150 words) to trigger
    // Tier 3 procedure emission.
    // Each flow statement is one "word" in body_word_count, so we need 150+ statements.
    let flow_lines: String = (0..160)
        .map(|i| format!("        \"Step number {}.\"", i))
        .collect::<Vec<_>>()
        .join("\n");
    let source = format!(
        "\
export text shared_val = \"x\"

export block setup_env(shell = \"bash\")
    flow:
{}
        return shell
",
        flow_lines,
    );

    let lib_path = dir.path().join("mylib.glyph.md");
    std::fs::write(&lib_path, &source).unwrap();

    // Create the subdir and a stale .tmp for the procedure file.
    let proc_dir = dir.path().join("mylib");
    std::fs::create_dir_all(&proc_dir).unwrap();
    std::fs::write(proc_dir.join("setup-env.md.tmp"), "stale procedure tmp\n").unwrap();

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(dir.path())
        .output()
        .expect("failed to run glyph");

    // Library compile exits 0
    assert_eq!(output.status.code(), Some(0));

    // Stale .tmp should be cleaned
    assert!(
        !proc_dir.join("setup-env.md.tmp").exists(),
        "stale procedure .tmp should be cleaned"
    );

    // Procedure .md should exist
    assert!(
        proc_dir.join("setup-env.md").exists(),
        "procedure .md should be emitted"
    );
}

/// AC4 continued: directory-mode compile also cleans stale .tmp for .md outputs.
#[test]
fn directory_mode_cleans_stale_tmp() {
    let dir = tempfile::tempdir().unwrap();

    let source_path = dir.path().join("task.glyph.md");
    std::fs::write(
        &source_path,
        "\
skill task()
    description: \"Do the task.\"

    flow:
        \"Execute the task steps.\"
",
    )
    .unwrap();

    // Plant a stale .tmp
    std::fs::write(dir.path().join("task.md.tmp"), "stale\n").unwrap();

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(dir.path())
        .output()
        .expect("failed to run glyph");

    assert_eq!(output.status.code(), Some(0));
    assert!(
        !dir.path().join("task.md.tmp").exists(),
        "directory-mode compile should clean stale .tmp"
    );
    assert!(dir.path().join("task.md").exists());
}
