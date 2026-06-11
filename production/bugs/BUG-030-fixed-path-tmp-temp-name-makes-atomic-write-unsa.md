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

## Independent Agent Finding

**Verdict:** Reproduced. The current implementation is unsafe under concurrent writers targeting the same output path.

**Reproduction/Refutation:** I first used Graphify to locate the implementation context; it identified `atomic_write()` and `tmp_path_for()` in `crates/glyph-core/src/lib.rs`. Bounded source inspection confirmed that `atomic_write` computes one shared tmp path, removes it, writes to it, then renames it, while `tmp_path_for` appends the constant `.tmp` suffix. I then ran a scratch harness under `tmp/bug030-repro` that links to `glyph-core` and starts 16 threads against one output path: one writer uses a 256 MiB payload and 15 writers use distinct small payloads.

**Evidence:** The targeted command was `cargo run --quiet --manifest-path tmp/bug030-repro/Cargo.toml`. It reproduced on round 0 with 15 `No such file or directory (os error 2)` failures from `atomic_write`, which is consistent with one writer renaming the shared tmp file while the other writers still expect it to exist. The final file was also byte-corrupt relative to every complete writer payload:

```text
round=0
failures=15
failed_writer=BIG err=No such file or directory (os error 2)
...
final_len=4126
final_matches_writer=<none>
final_first_line=writer=SMALL-1
```

Follow-up inspection of the final output showed it began as `SMALL-1` but ended with an extra trailing byte from another writer (`END=SMALL-1\n4`), so the observed output was not just last-writer-wins; it was a mixed payload produced by the shared tmp inode race. `rg -n "atomic_write\(" crates/glyph-core/src/lib.rs` also shows this function is used for compile output paths at lines 545 and 3169, plus emitted IR/library outputs at lines 1769 and 2508.

**Resolution Input:** Preserve the existing suggested resolution. Each writer needs a private temp file in the target directory, followed by a single rename/persist step onto the final path. `tempfile::NamedTempFile::new_in(parent)` or a suffix containing enough uniqueness such as pid plus random/counter would address the reproduced collision; a fixed `.tmp` sibling cannot provide concurrent safety.
