# BUG-030: Fixed `<path>.tmp` temp name makes atomic_write unsafe under concurrent invocations on the same output

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/lib.rs:1874-1893`
**Found by:** x-io | **Audit date:** unknown-date

## Description

`tmp_path_for` derives the temp file name by appending a constant `.tmp` to the final output path, so every process compiling the same source/output uses the identical `foo.md.tmp`. If two `glyph compile` invocations run concurrently against the same file or overlapping directory (e.g. an editor-on-save hook firing while a manual build runs, or a parallel CI matrix sharing a workspace), they race on one tmp path: process A's `std::fs::remove_file(&tmp_path)` can delete process B's in-progress tmp, `std::fs::write` interleaves partial content, and a `rename` can publish a half-written or truncated `foo.md`.

The documented crash-safety story ("user never observes a half-written .md", cli.md:223) silently does not extend to concurrency because the tmp name is not unique per process. A unique suffix (pid + counter, or `tempfile::NamedTempFile` in the target dir) would make each writer's tmp private and keep `rename` the only shared step.

## Trigger / Reproduction

Run two concurrent `glyph compile` invocations targeting the same output file (e.g. editor on-save hook + manual build, or parallel CI jobs sharing a workspace). Both processes share the same `.tmp` intermediate path; the race between `remove_file`, `write`, and `rename` can corrupt the output.

## Evidence

```rust
fn tmp_path_for(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".tmp");
    PathBuf::from(s)
}
```

## Recommended Resolution

Make the temp name unique per writer — e.g. append `.tmp.<pid>.<rand>` or use `tempfile::NamedTempFile::new_in(parent)` then persist/rename onto the target. The `tempfile` crate is already listed as a workspace dependency in `glyph-core/Cargo.toml`, making the `NamedTempFile` approach straightforward. Keep any existing stale-`.tmp` sweep but match it against the unique pattern carefully, or rely solely on rename atomicity.

## Verification Notes

The code at lib.rs:1876-1893 confirms the fixed `.tmp` suffix: `tmp_path_for` appends a constant `.tmp`, so two concurrent invocations targeting the same output share the identical `foo.md.tmp`. The `atomic_write` function's three-step sequence (`remove_file`, `write`, `rename`) is not atomic across processes. The project's own integration test at `crates/glyph-cli/tests/walking_skeleton.rs:24-25` explicitly documents this problem: "Copy the corpus source to a tempdir to avoid parallel-test races on the shared output file (atomic_write uses a `.tmp` intermediate)" — the maintainers already hit this in the test suite and worked around it by isolating each test to its own unique directory. The docs at `docs/reference/cli.md:223` describe single-process crash-safety but are silent on concurrency.
