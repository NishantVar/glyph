//! Integration tests for --output and --out-dir flags.
use std::path::PathBuf;
use std::process::Command;
use tempfile::tempdir;

fn glyph_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_glyph"))
}

fn write(path: &std::path::Path, body: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, body).unwrap();
}

fn contains_filename(dir: &std::path::Path, name: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if contains_filename(&p, name) {
                return true;
            }
        } else if p.file_name().map(|n| n == name).unwrap_or(false) {
            return true;
        }
    }
    false
}

const TRIVIAL_SKILL: &str = "\
skill main()
    description: \"M.\"
    flow:
        \"Do the thing.\"
";

#[test]
fn out_dir_outside_root_import_warns_pretty() {
    let root = tempdir().unwrap();
    let inside = root.path().join("inside");
    let outside = root.path().join("outside");
    let build = root.path().join("build");
    std::fs::create_dir_all(&inside).unwrap();
    std::fs::create_dir_all(&outside).unwrap();

    std::fs::write(
        outside.join("lib.glyph"),
        "export const greeting = \"Hi.\"\n",
    )
    .unwrap();

    std::fs::write(
        inside.join("main.glyph"),
        "\
import \"../outside/lib.glyph\" { greeting }

skill main()
    description: \"M.\"
    flow:
        context greeting
",
    )
    .unwrap();

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(&inside)
        .arg("--out-dir")
        .arg(&build)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("G::build::import-outside-out-dir"),
        "expected outside-root warning in stderr, got: {}",
        stderr
    );
    assert_eq!(
        stderr.matches("G::build::import-outside-out-dir").count(),
        1
    );
}

#[test]
fn output_writes_exact_path() {
    let root = tempdir().unwrap();
    let src = root.path().join("foo.glyph");
    let out = root.path().join("build").join("renamed.md");
    write(&src, TRIVIAL_SKILL);
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();

    let status = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src)
        .arg("--output")
        .arg(&out)
        .status()
        .unwrap();

    assert_eq!(status.code(), Some(0));
    assert!(out.exists(), "renamed.md should exist");
    assert!(
        !root.path().join("foo.md").exists(),
        "default-named output must not be produced"
    );
}

#[test]
fn output_missing_parent_exits_3() {
    let root = tempdir().unwrap();
    let src = root.path().join("foo.glyph");
    write(&src, TRIVIAL_SKILL);
    let out = root.path().join("does_not_exist").join("renamed.md");

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src)
        .arg("--output")
        .arg(&out)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    assert!(!out.exists());
    assert!(!root.path().join("foo.md").exists());
}

#[test]
fn output_target_is_dir_exits_3() {
    let root = tempdir().unwrap();
    let src = root.path().join("foo.glyph");
    write(&src, TRIVIAL_SKILL);
    let out = root.path().join("a_dir");
    std::fs::create_dir_all(&out).unwrap();

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src)
        .arg("--output")
        .arg(&out)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    assert!(!root.path().join("foo.md").exists());
}

#[test]
fn output_with_directory_input_exits_3() {
    let root = tempdir().unwrap();
    let src_dir = root.path().join("src");
    write(&src_dir.join("foo.glyph"), TRIVIAL_SKILL);
    let out = root.path().join("renamed.md");

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src_dir)
        .arg("--output")
        .arg(&out)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    assert!(!out.exists());
}

#[test]
fn output_and_out_dir_conflict_exits_2() {
    let root = tempdir().unwrap();
    let src = root.path().join("foo.glyph");
    write(&src, TRIVIAL_SKILL);

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src)
        .arg("--output")
        .arg(root.path().join("a.md"))
        .arg("--out-dir")
        .arg(root.path().join("b"))
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn output_with_imports_leaves_dep_in_place() {
    let root = tempdir().unwrap();
    let src_dir = root.path().join("src");
    let lib = src_dir.join("lib.glyph");
    let main = src_dir.join("main.glyph");
    write(&lib, "export const greeting = \"Hi.\"\n");
    write(
        &main,
        "\
import \"./lib.glyph\" { greeting }

skill main()
    description: \"M.\"
    flow:
        context greeting
",
    );
    let out = root.path().join("build").join("renamed.md");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();

    let status = Command::new(glyph_bin())
        .arg("compile")
        .arg(&main)
        .arg("--output")
        .arg(&out)
        .status()
        .unwrap();

    assert_eq!(status.code(), Some(0));
    assert!(out.exists(), "renamed entry .md should exist");
    assert!(
        !src_dir.join("main.md").exists(),
        "entry must not also write in-place"
    );
    // Library files produce no `.md` (no skill); just assert that the build
    // succeeded and didn't write anything spurious next to the renamed entry.
    assert!(!out.parent().unwrap().join("lib.md").exists());
}

#[test]
fn out_dir_mirrors_nested_layout() {
    let root = tempdir().unwrap();
    let src_dir = root.path().join("src");
    write(&src_dir.join("a.glyph"), TRIVIAL_SKILL);
    write(
        &src_dir.join("sub").join("b.glyph"),
        TRIVIAL_SKILL.replace("main", "beta").as_str(),
    );
    let build = root.path().join("build");

    let status = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src_dir)
        .arg("--out-dir")
        .arg(&build)
        .status()
        .unwrap();

    assert_eq!(status.code(), Some(0));
    assert!(build.join("a.md").exists());
    assert!(build.join("sub").join("b.md").exists());
    assert!(!src_dir.join("a.md").exists());
    assert!(!src_dir.join("sub").join("b.md").exists());
}

#[test]
fn out_dir_procedure_path_relative_across_dirs() {
    // skills/main.glyph imports libs/lib.glyph; lib exports a Tier-3
    // procedure used by main. Verify the compiled main.md contains a correct
    // relative reference to the procedure file under build/.
    //
    // Tier-3 projection requires body_word_count >= 150 (see
    // crates/glyph-core/src/lib.rs near `body_word_count < 150`). Pad the
    // export block body with enough steps to comfortably clear that bar.
    let root = tempdir().unwrap();
    let src = root.path().join("src");

    let mut long_body = String::new();
    for i in 0..20 {
        long_body.push_str(&format!(
            "        \"Step {} of the long procedure: carefully examine the repository structure and contents.\"\n",
            i + 1
        ));
    }
    let lib_src = format!(
        "\
export block do_thing()
    flow:
{}        return none
",
        long_body
    );
    write(&src.join("libs").join("lib.glyph"), &lib_src);
    write(
        &src.join("skills").join("main.glyph"),
        "\
import \"../libs/lib.glyph\" { do_thing }

skill main()
    description: \"M.\"
    flow:
        do_thing()
",
    );
    let build = root.path().join("build");

    let status = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src)
        .arg("--out-dir")
        .arg(&build)
        .status()
        .unwrap();

    assert_eq!(status.code(), Some(0));
    let main_md = std::fs::read_to_string(build.join("skills").join("main.md")).unwrap();
    // The procedure_path must navigate up from skills/ and down into libs/lib/.
    assert!(
        main_md.contains("../libs/lib/do-thing.md"),
        "main.md should reference `../libs/lib/do-thing.md`; got:\n{}",
        main_md
    );
    assert!(build.join("libs").join("lib").join("do-thing.md").exists());
}

#[test]
fn out_dir_auto_creates_intermediate_dirs() {
    let root = tempdir().unwrap();
    let src = root.path().join("src");
    write(
        &src.join("sub").join("nested").join("a.glyph"),
        TRIVIAL_SKILL,
    );
    let build = root.path().join("build");

    let status = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src)
        .arg("--out-dir")
        .arg(&build)
        .status()
        .unwrap();

    assert_eq!(status.code(), Some(0));
    assert!(build.join("sub").join("nested").join("a.md").exists());
}

#[test]
fn out_dir_outside_root_warning_in_json() {
    let root = tempdir().unwrap();
    let inside = root.path().join("inside");
    let outside = root.path().join("outside");
    let build = root.path().join("build");
    write(
        &outside.join("lib.glyph"),
        "export const greeting = \"Hi.\"\n",
    );
    write(
        &inside.join("main.glyph"),
        "\
import \"../outside/lib.glyph\" { greeting }

skill main()
    description: \"M.\"
    flow:
        context greeting
",
    );

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(&inside)
        .arg("--out-dir")
        .arg(&build)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout
            .lines()
            .any(|l| l.contains("G::build::import-outside-out-dir")),
        "expected NDJSON line with the warning, got:\n{}",
        stdout
    );
}

#[test]
fn output_with_emit_ir_uses_same_stem() {
    let root = tempdir().unwrap();
    let src = root.path().join("foo.glyph");
    write(&src, TRIVIAL_SKILL);
    let out = root.path().join("build").join("renamed.md");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();

    let status = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src)
        .arg("--output")
        .arg(&out)
        .arg("--emit-ir")
        .status()
        .unwrap();

    assert_eq!(status.code(), Some(0));
    assert!(out.exists());
    assert!(out.parent().unwrap().join("renamed.ir.json").exists());
}

#[test]
fn out_dir_with_emit_ir_places_sidecar_beside_mirrored_md() {
    let root = tempdir().unwrap();
    let src = root.path().join("src");
    write(&src.join("sub").join("a.glyph"), TRIVIAL_SKILL);
    let build = root.path().join("build");

    let status = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src)
        .arg("--out-dir")
        .arg(&build)
        .arg("--emit-ir")
        .status()
        .unwrap();

    assert_eq!(status.code(), Some(0));
    assert!(build.join("sub").join("a.md").exists());
    assert!(build.join("sub").join("a.ir.json").exists());
}

#[test]
fn no_flag_writes_beside_source() {
    let root = tempdir().unwrap();
    let src = root.path().join("foo.glyph");
    write(&src, TRIVIAL_SKILL);

    let status = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src)
        .status()
        .unwrap();

    assert_eq!(status.code(), Some(0));
    assert!(root.path().join("foo.md").exists());
}

#[test]
fn output_works_for_no_import_fast_path() {
    // A skill with zero imports goes through compile_file_with_effects, not
    // compile_file_with_resolved_imports. Ensure --output reaches the fast
    // path.
    let root = tempdir().unwrap();
    let src = root.path().join("foo.glyph");
    write(&src, TRIVIAL_SKILL); // no imports
    let out = root.path().join("renamed.md");

    let status = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src)
        .arg("--output")
        .arg(&out)
        .status()
        .unwrap();

    assert_eq!(status.code(), Some(0));
    assert!(out.exists());
    assert!(!root.path().join("foo.md").exists());
}

#[test]
fn out_dir_library_only_outside_root_still_warns() {
    let root = tempdir().unwrap();
    let inside = root.path().join("inside");
    let outside = root.path().join("outside");
    let build = root.path().join("build");
    // Outside library with an exported procedure. Pad the body so it clears
    // the Tier-3 projection threshold (body_word_count >= 150) and therefore
    // emits a real procedure file the assertion below can find.
    let mut long_body = String::new();
    for i in 0..20 {
        long_body.push_str(&format!(
            "        step \"Step {} of the long procedure: carefully examine the repository structure and contents.\"\n",
            i + 1
        ));
    }
    write(
        &outside.join("lib.glyph"),
        &format!(
            "\
export block do_thing()
    flow:
{}        return none
",
            long_body
        ),
    );
    // Inside skill that imports the library.
    write(
        &inside.join("main.glyph"),
        "\
import \"../outside/lib.glyph\" { do_thing }

skill main()
    description: \"M.\"
    flow:
        do_thing()
",
    );

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(&inside)
        .arg("--out-dir")
        .arg(&build)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.matches("G::build::import-outside-out-dir").count(),
        1,
        "should warn exactly once for the library, even though it produces no Compiled .md"
    );
    // Procedure file went in-place (not under build/):
    assert!(outside.join("lib").join("do-thing.md").exists());
    // The outside library must NOT be mirrored under build/.
    assert!(
        !contains_filename(&build, "do-thing.md"),
        "outside-root procedure must not be mirrored under --out-dir"
    );
}

#[test]
fn output_bare_relative_path_resolves_to_cwd() {
    // A bare relative `--output renamed.md` (no parent component) must be
    // accepted: parent canonicalizes to the current working directory.
    let root = tempdir().unwrap();
    let src = root.path().join("foo.glyph");
    write(&src, TRIVIAL_SKILL);

    let output = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src)
        .arg("--output")
        .arg("renamed.md")
        .current_dir(root.path())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        root.path().join("renamed.md").exists(),
        "renamed.md should exist in the cwd"
    );
    assert!(
        !root.path().join("foo.md").exists(),
        "default-named output must not be produced"
    );
}

#[test]
fn out_dir_single_file_input_mirrors_layout() {
    // Single-file input (not a directory) with --out-dir mirrors layout
    // under the build root: the file lives directly beneath build/.
    let root = tempdir().unwrap();
    let src = root.path().join("foo.glyph");
    write(&src, TRIVIAL_SKILL);
    let build = root.path().join("build");

    let status = Command::new(glyph_bin())
        .arg("compile")
        .arg(&src)
        .arg("--out-dir")
        .arg(&build)
        .status()
        .unwrap();

    assert_eq!(status.code(), Some(0));
    assert!(
        build.join("foo.md").exists(),
        "build/foo.md should exist (mirrored single-file input)"
    );
    assert!(
        !root.path().join("foo.md").exists(),
        "in-place compiled output must not be produced"
    );
}
