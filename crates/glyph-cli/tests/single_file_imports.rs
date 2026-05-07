//! Single-file `glyph compile` import resolution.
//!
//! Locks in the bug fix described in
//! `obsidian://plans/single-file-import-resolution-design-2026-05-07.md`:
//! `glyph compile <file.glyph>` must resolve relative imports the same way
//! `glyph compile <dir>/` does.

use std::path::PathBuf;
use std::process::{Command, Output};

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn run_compile(file: &std::path::Path, extra_args: &[&str]) -> Output {
    let mut cmd = Command::new(glyph_bin());
    cmd.arg("compile").arg(file);
    for a in extra_args {
        cmd.arg(a);
    }
    cmd.output().expect("failed to spawn glyph binary")
}

/// Bug repro: a single-file compile of a skill that imports a const from a
/// sibling library file must resolve the import (no `undefined-name`,
/// exit 0, `.md` written).
#[test]
fn single_file_resolves_relative_const_import() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("lib.glyph");
    let entry = dir.path().join("entry.glyph");

    std::fs::write(&lib, "export const greeting = \"Hello, world\"\n").unwrap();
    std::fs::write(
        &entry,
        "import \"./lib.glyph\" { greeting }\n\
         \n\
         skill greeter()\n\
         \x20\x20\x20\x20description: \"Says hi.\"\n\
         \x20\x20\x20\x20require greeting\n",
    )
    .unwrap();

    let out = run_compile(&entry, &["--format", "json"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    for line in stdout.lines() {
        let v: serde_json::Value = serde_json::from_str(line).expect("non-JSON diagnostic line");
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
        assert_ne!(
            id, "G::analyze::undefined-name",
            "imported const should resolve in single-file mode\nstdout: {stdout}\nstderr: {stderr}"
        );
        assert_ne!(
            id, "G::analyze::undefined-call",
            "imported block should resolve in single-file mode\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
    assert_eq!(
        out.status.code(),
        Some(0),
        "single-file compile of skill+import should exit 0\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        dir.path().join("entry.md").is_file(),
        "entry.md should be written"
    );
}

/// Approach A: when an entry skill imports a const from another skill (a
/// `.glyph` file with both `skill ...` and `export const`), both skills' `.md`s
/// are emitted from a single-file invocation. Confirms the closure walker
/// pulls the imported skill into the pipeline.
#[test]
fn single_file_emits_md_for_each_skill_in_closure() {
    let dir = tempfile::tempdir().unwrap();
    let lib_skill = dir.path().join("lib_skill.glyph");
    let entry = dir.path().join("entry.glyph");

    // lib_skill is itself a skill *and* exports a const.
    std::fs::write(
        &lib_skill,
        "skill helper()\n\
         \x20\x20\x20\x20description: \"helper skill\"\n\
         \x20\x20\x20\x20flow:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\"Do helper work.\"\n\
         \n\
         export const motto = \"do good work\"\n",
    )
    .unwrap();
    // entry imports the const and `require`s it (so unused-import does not fire).
    std::fs::write(
        &entry,
        "import \"./lib_skill.glyph\" { motto }\n\
         \n\
         skill main()\n\
         \x20\x20\x20\x20description: \"top-level\"\n\
         \x20\x20\x20\x20require motto\n",
    )
    .unwrap();

    let out = run_compile(&entry, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "single-file compile of skill+import should exit 0; stderr: {stderr}"
    );
    assert!(
        dir.path().join("entry.md").is_file(),
        "entry.md should be emitted"
    );
    assert!(
        dir.path().join("lib_skill.md").is_file(),
        "lib_skill.md should be emitted (closure includes the imported skill)"
    );
}

/// IO failure: pre-create the output `.md` as a directory so atomic_write's
/// rename fails. Single-file CLI must still exit 3 and emit the
/// `cannot write` stderr.
#[test]
fn single_file_md_write_failure_exits_three() {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("entry.glyph");
    std::fs::write(
        &entry,
        "skill demo()\n\
         \x20\x20\x20\x20description: \"d\"\n",
    )
    .unwrap();

    // Block the .md output by pre-creating it as a directory.
    std::fs::create_dir(dir.path().join("entry.md")).unwrap();

    let out = run_compile(&entry, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(3),
        "expected exit 3 on .md write failure; stderr: {stderr}"
    );
    assert!(
        stderr.contains("cannot write"),
        "stderr must contain 'cannot write'; got: {stderr}"
    );
    assert!(
        stderr.contains("entry.md"),
        "stderr must mention the failing path; got: {stderr}"
    );
}

/// IO failure under `--emit-ir`: pre-create `entry.ir.json` as a directory.
/// Even though the `.md` writes successfully, the IR write fails and the CLI
/// must still exit 3 with `cannot write` for the IR path. (Today's directory
/// pipeline silently swallows this; the unified pipeline surfaces it.)
#[test]
fn single_file_ir_write_failure_exits_three() {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("entry.glyph");
    std::fs::write(
        &entry,
        "skill demo()\n\
         \x20\x20\x20\x20description: \"d\"\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("entry.ir.json")).unwrap();

    let out = run_compile(&entry, &["--emit-ir"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(3),
        "expected exit 3 on .ir.json write failure; stderr: {stderr}"
    );
    assert!(
        stderr.contains("cannot write") && stderr.contains("entry.ir.json"),
        "stderr must mention IR path 'cannot write'; got: {stderr}"
    );
}
