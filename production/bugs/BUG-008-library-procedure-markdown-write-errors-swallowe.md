# BUG-008: Library procedure markdown write errors swallowed; build reports success with missing/stale output

**Severity:** high | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/lib.rs:2505-2510`
**Found by:** x-errswallow | **Audit date:** unknown-date

## Description

In `emit_library_procedures` (the directory-build path used by `glyph compile <dir>` via `compile_directory_with_layout`), the compiled procedure markdown is written with `atomic_write(&out_path, &markdown).ok()`, and the preceding `create_dir_all(parent)` is `let _ =`. Both I/O errors are fully discarded.

Immediately after, the code does `emitted.push((eb.node.name.clone(), out_path))`, so the procedure is recorded as a successfully emitted Tier-3 reference target regardless of whether the file was actually written. The caller (lib.rs:1804–1805) records those paths and the file's `BuildOutcome` stays `Compiled`, so the CLI exits 0.

Concrete trigger conditions: a read-only output directory, a full disk, or a path component that is an existing file. The procedure `.md` is never written (or a stale prior version is left on disk), yet `glyph compile` reports success and any skill that imports the procedure compiles against output that does not exist.

This is inconsistent with `compile_file` and `compile_file_with_resolved_imports` (lib.rs:545 and 3169) which correctly propagate the identical write via `CompileError::Write`, confirming writes are meant to be surfaced.

## Trigger / Reproduction

Compile a library `.glyph` file to a read-only output directory:

```sh
chmod 555 ./out/
glyph compile ./my_library.glyph --out-dir ./out/
# exits 0, reports Compiled — procedure .md silently not written
```

Or reproduce on a full disk or when a path component is an existing file.

## Evidence

```rust
if let Some(parent) = out_path.parent() {
    let _ = std::fs::create_dir_all(parent);  // I/O error discarded
}
atomic_write(&out_path, &markdown).ok();       // write error discarded

emitted.push((eb.node.name.clone(), out_path)); // unconditionally recorded as emitted
```

## Recommended Resolution

Propagate write and mkdir failures instead of swallowing them, mirroring the handling already used in `compile_file` at lib.rs:540–548:

1. Change `let _ = std::fs::create_dir_all(parent)` to propagate or push a diagnostic on failure.
2. Change `atomic_write(&out_path, &markdown).ok()` to `atomic_write(&out_path, &markdown).map_err(|e| CompileError::Write { path: out_path.clone(), source: e })?` (or push an equivalent error diagnostic into `diags`).
3. Only push to `emitted` when the write succeeds.

This ensures `lib_diags.has_error()` returns true on write failure, `FileOutcome` is set to `Failed`, and the CLI exits non-zero.

## Verification Notes

Code at lib.rs:2505–2510 confirmed: both `create_dir_all` and `atomic_write` discard errors, and `emitted.push` is unconditional. The caller inserts all emitted paths into `procedure_paths` and gates `FileOutcome::Failed` only on `proc_diags.has_error()` — write errors produce no diagnostics, so `any_failure` is never set. Contrast with `compile_file` (lines 540–548) which uses `.map_err(|e| CompileError::Write{...})?`. Concrete trigger confirmed: compiling to a read-only output directory exits 0 with the procedure .md not written.

## Independent Agent Finding

### Verdict

Reproduced. In the current tree, library procedure write failures are still swallowed: the CLI exits 0 with no diagnostics while the expected procedure Markdown file is absent.

### Reproduction/Refutation

Used the deterministic "path component is an existing file" trigger rather than a permission-dependent read-only directory:

1. Created `tmp/bug-008/input/lib.glyph` with one minimal library export, `export block do_thing()`.
2. Created `tmp/bug-008/out/lib` as a regular file. The expected procedure output path is `tmp/bug-008/out/lib/do-thing.md`, so the parent path cannot be created as a directory.
3. Ran `cargo run -q -p glyph-cli -- compile tmp/bug-008/input --out-dir tmp/bug-008/out`.
   Output summary: exit `0`; stdout `0` bytes; stderr `0` bytes; `tmp/bug-008/out/lib/do-thing.md` did not exist; `tmp/bug-008/out/lib` remained a file.
4. Ran `cargo run -q -p glyph-cli -- compile tmp/bug-008/input --out-dir tmp/bug-008/out --format json`.
   Output summary: exit `0`; stdout `0` bytes; stderr `0` bytes.

### Evidence

Graphify orientation for `library procedure markdown write errors swallowed` did not identify a precise owning implementation node, so I narrowed with `rg` and bounded line reads. Source inspection confirmed:

- `crates/glyph-core/src/lib.rs:2505-2510` discards `create_dir_all(parent)` with `let _ =`, discards `atomic_write(&out_path, &markdown)` with `.ok()`, then unconditionally pushes `(block_name, out_path)` into `emitted`.
- `crates/glyph-core/src/lib.rs:1804-1810` inserts all returned procedure paths and only fails the library when `proc_diags.has_error()` is true; the swallowed I/O failure creates no diagnostic.
- `crates/glyph-core/src/lib.rs:2111-2128` resolves out-dir procedure outputs as `<out-dir>/<relative-parent>/<lib-stem>/<block-kebab>.md`, matching the reproduced missing path.
- `crates/glyph-cli/src/main.rs:520-537` validates/creates only the top-level `--out-dir`; nested procedure parent failures occur later in core emission.

### Resolution Input

The existing recommended resolution is correct: propagate the nested `create_dir_all` and `atomic_write` failures as build/write errors, and only record a procedure path in `emitted` after the write succeeds. A focused regression should compile a library directory with a regular file occupying the would-be procedure parent path and assert non-zero exit plus a surfaced diagnostic.
