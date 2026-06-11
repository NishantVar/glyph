# BUG-029: IR JSON (--emit-ir) write errors swallowed; reported as compiled while artifact missing

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/lib.rs:1766-1769`
**Found by:** x-errswallow | **Audit date:** unknown-date

## Description

In `compile_directory_with_layout`, when `emit_ir` is set the serialized IR JSON is written with `atomic_write(&ir_path, &ir_json).ok();` and the parent `create_dir_all` is `let _ =`. Both errors are discarded and the file's outcome is unconditionally pushed as `FileOutcome::Compiled` (lib.rs:1775), so `glyph compile --emit-ir <dir>` exits 0 even when the `.ir.json` was never written.

Concrete trigger: a read-only or full output directory. The user believes IR was emitted; a subsequent `glyph validate-output <ir.json> <md>` then fails to find the IR file or silently validates against a stale prior IR, producing a confusing/incorrect result with no indication the emit step failed. Lower severity than the procedure markdown case because IR JSON is a secondary/debug artifact, but it is still a failed write reported as success.

## Trigger / Reproduction

Run `glyph compile --emit-ir` targeting a read-only output directory. The compile exits 0 and reports the file as compiled, but no `.ir.json` artifact is written. Any subsequent `glyph validate-output` run referencing the expected IR path will either fail to find the file or validate against a stale prior artifact.

## Evidence

```rust
if let Some(parent) = ir_path.parent() {
    let _ = std::fs::create_dir_all(parent);
}
atomic_write(&ir_path, &ir_json).ok();
```

## Recommended Resolution

Surface the write/`create_dir_all` failure: attach an error diagnostic to the file's outcome (so it becomes `Failed` and the CLI exits non-zero) or thread the `io::Result` up, instead of dropping it with `.ok()` and `let _ =`. By contrast, the primary `.md` artifact write at lines 545 and 3169 uses `atomic_write(...).map_err(|e| CompileError::Write {...})?` which properly propagates the error — the IR write should match that pattern.

## Verification Notes

The code at lines 1766-1769 of `lib.rs` exactly matches the claimed pattern: `let _ = std::fs::create_dir_all(parent)` and `atomic_write(&ir_path, &ir_json).ok()` both discard errors, and line 1775 unconditionally pushes `FileOutcome::Compiled`. The exit code is only set non-zero via `any_failure`, which is never set from an IR write failure. The same silent discard pattern exists for library procedure files at line 2508, but the IR case is more impactful because it is the input to `validate-output`. There is no upstream guard that would prevent a read-only output directory from reaching this code path.

## Independent Agent Finding

Verdict: Reproduced. `glyph compile --emit-ir` reports success when the expected IR JSON sidecar cannot be written, leaving the primary Markdown artifact present and the IR artifact missing.

Reproduction/Refutation: I created `tmp/bug029/input.glyph` as a minimal valid skill, created `tmp/bug029/out/renamed.ir.json` as a directory to block the expected sidecar path, then ran:

```sh
cargo run -q -p glyph-cli -- compile tmp/bug029/input.glyph --output tmp/bug029/out/renamed.md --emit-ir --format json
```

The command exited `0` with no diagnostic output. A follow-up artifact check showed `tmp/bug029/out/renamed.md` as a regular file and `tmp/bug029/out/renamed.ir.json` still as a directory, so no IR JSON file was produced. Running the downstream validator against the expected path failed with exit `3` and `glyph: cannot read \`tmp/bug029/out/renamed.ir.json\`: Is a directory (os error 21)`.

Evidence: The implementation still drops both error paths in `crates/glyph-core/src/lib.rs`: `let _ = std::fs::create_dir_all(parent);` followed by `atomic_write(&ir_path, &ir_json).ok();`, then unconditionally records `FileOutcome::Compiled`. The reproduction demonstrates the swallowed `atomic_write` failure with a directory/file conflict at the IR sidecar path.

Resolution Input: Preserve the existing suggested resolution: surface the `create_dir_all` and IR `atomic_write` failures instead of discarding them. Either attach an error diagnostic so the file outcome becomes `Failed` and the CLI exits non-zero, or thread the `io::Result` upward. The IR sidecar write should follow the primary Markdown write pattern that maps `atomic_write` errors into `CompileError::Write`.
