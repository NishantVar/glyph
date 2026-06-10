# BUG-066: Stale `.tmp` left behind when a file that previously crashed mid-write now fails to compile

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/lib.rs:1876-1886`
**Found by:** x-io | **Audit date:** unknown-date

## Description

The stale-`.tmp` cleanup (`std::fs::remove_file(&tmp_path)`) only runs inside `atomic_write`, which is only called when a file successfully reaches the write step. `docs/architecture/compiler-pipeline.md:131` states "Before writing any new `.tmp`, Phase 7 scans the output paths it is about to write and deletes any pre-existing `.tmp` siblings for those paths." If a source compiled successfully once — leaving a leftover `foo.md.tmp` from a crash between write and rename — and then is edited to be invalid so it now fails compilation, the build never calls `atomic_write` for `foo.md`, so the orphaned `foo.md.tmp` is never swept. It persists indefinitely across failed rebuilds. The atomic_emission tests only cover the success-after-stale-tmp case (`stale_tmp_cleaned_on_rebuild`) and a no-prior-tmp failed rebuild, leaving this leak untested.

## Trigger / Reproduction

1. Build a `.glyph` file successfully (output `foo.md` written).
2. Simulate a crash between the `atomic_write` temp-write and the rename, leaving `foo.md.tmp` on disk.
3. Edit `foo.glyph` to introduce a compile error.
4. Run the build again. Compilation fails, `atomic_write` is never called, and `foo.md.tmp` is never removed — it persists indefinitely.

## Evidence

```rust
pub fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    let tmp_path = tmp_path_for(path);
    let _ = std::fs::remove_file(&tmp_path);  // cleanup only runs on success path
    std::fs::write(&tmp_path, content)?;
    // ...rename follows
}
```

## Recommended Resolution

Either sweep stale `.tmp` siblings for all planned output paths up-front in `compile_directory_with_layout` before the compile loop begins (matching the documented "before writing any new .tmp" wording), or accept the current lazy-cleanup behaviour and soften the doc comment in `compiler-pipeline.md:131` to reflect that the sweep is per-file-on-success rather than a startup scan. The upfront sweep approach better matches the documented intent.

## Verification Notes

Code confirms the bug exactly. `atomic_write` at lib.rs:1876 performs stale `.tmp` removal (line 1879) only at the moment of write — i.e., only on the success path. All call sites for `.md` output are inside `CompileOutcome::Compiled` branches. When a file fails compilation, `atomic_write` is never called, so any pre-existing orphaned `.tmp` is never removed. The test `stale_tmp_cleaned_on_rebuild` only covers the success case; no test covers stale-tmp + failed-rebuild. The doc at `compiler-pipeline.md:131` claims an upfront sweep that does not exist. The bug is real but low severity because `.tmp` files are cosmetic artifacts and do not affect compilation correctness.
