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
