# BUG-055: glyph fmt writes source files non-atomically, risking truncation/data loss on a mid-write IO error

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-cli/src/main.rs:248-254`
**Found by:** cli | **Audit date:** unknown-date

## Description

`run_fmt` rewrites each `.glyph` source file in place with `std::fs::write(&file, &result.output)`. `std::fs::write` opens the file with create+truncate and then writes the buffer. If the write fails partway (e.g. disk full, a transient IO error, or the process is killed between truncate and the final flush), the original source file has already been truncated and is left partially written or empty. Because `fmt` operates on the user's *source* `.glyph` file (not a derived artifact), this is a data-loss path: the author's source is corrupted/lost rather than regenerable. The reference CLI contract (`docs/reference/cli.md` §'Atomic emission') promises an atomic write-tmp-then-rename pattern only for `compile`; `fmt`'s in-place write has no such protection, so a failed format silently destroys the input. The `atomic_write` helper already exists in `glyph-core` and is used by `compile`.

## Trigger / Reproduction

1. Run `glyph fmt big.glyph` (or `glyph fmt dir/`) where the target filesystem returns a write error after the file has been truncated (e.g. disk becomes full after the truncate syscall).
2. `std::fs::write` returns `Err(e)`.
3. The original `.glyph` source is now empty or partially written.
4. `glyph fmt` prints the error and exits 3 — but the user's source is already gone.

## Evidence

```rust
if result.changed {
    any_changed = true;
    if !check {
        if let Err(e) = std::fs::write(&file, &result.output) {
            eprintln!("glyph: cannot write `{}`: {}", file.display(), e);
            return ExitCode::from(3);
        }
    }
}
```

`glyph-core/src/lib.rs` defines `pub atomic_write` (write to `.tmp` sibling, then rename) used by `compile` at lines 545, 1769, 2508, 3169 — but `run_fmt` never calls it.

## Recommended Resolution

Use the same atomic-rename pattern as `compile`: write `result.output` to a sibling temp file (e.g. `<file>.tmp`) in the same directory, fsync if desired, then `std::fs::rename(tmp, file)`. On any error, remove the temp file and leave the original untouched. Since `glyph_core::atomic_write` is already `pub` and imported from the same crate, the fix is a one-line change: replace `std::fs::write(&file, &result.output)` with `glyph_core::atomic_write(&file, &result.output)`.

## Verification Notes

`crates/glyph-cli/src/main.rs` line 250 confirms `run_fmt` uses `std::fs::write` (bare, non-atomic) directly on the user's `.glyph` source. `glyph-core/src/lib.rs` defines `pub atomic_write` (write-to-.tmp-then-rename) used by `compile`. `docs/reference/cli.md` §"On-Failure Disk State Guarantee" explicitly covers only `compile`'s atomic guarantee and says nothing about `fmt`, confirming this is a gap. The trigger requires an unusual OS/hardware condition (mid-write IO error after truncation); it cannot happen on a normal successful or fully-failed write. The proposed fix is correct and complete.

## Independent Agent Finding

**Verdict:** Reproduced / confirmed.

**Reproduction/Refutation:** I reproduced the destructive write behavior in `tmp/bug055/` by running the existing CLI binary under a file-size limit. The scratch `.glyph` input was 12,000 bytes and contained tab-indented lines so `glyph fmt` had to rewrite it. Running `(ulimit -f 1; target/debug/glyph fmt tmp/bug055/repro.glyph)` caused the formatter process to fail with status 153 (SIGXFSZ on this system), after which the source file was no longer equal to the original and had been truncated to 512 bytes. This matches the report's risk case: once `std::fs::write` has opened/truncated the source path, a mid-write failure or process death leaves the user's source damaged rather than untouched.

**Evidence:** Graphify maps `run_fmt()` to `crates/glyph-cli/src/main.rs` and `atomic_write()` to `crates/glyph-core/src/lib.rs`. A targeted `ast-grep` search found exactly one `std::fs::write(&file, &result.output)` match at `crates/glyph-cli/src/main.rs:250`. Bounded source read confirmed `run_fmt` calls `std::fs::write(&file, &result.output)` inside the `if result.changed && !check` branch, while `glyph-core/src/lib.rs:1876-1885` defines `pub fn atomic_write(path: &Path, content: &str)` using sibling-temp write followed by rename. Reproduction command summary: initial size `12000`, formatter status `153`, final size `512`, `cmp` against the saved original returned different, and the first bytes of the corrupted file were formatted spaces plus `line`, proving partial formatted output replaced the original.

**Resolution Input:** Keep the existing recommended resolution: replace the in-place `std::fs::write(&file, &result.output)` in `run_fmt` with the existing atomic write helper, `glyph_core::atomic_write(&file, &result.output)`, so write failure leaves the original source file untouched.
