# BUG-065: BuildResult.exit_code doc comment contradicts actual repairable-only exit code (says 1, code returns 2)

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/lib.rs:1449`
**Found by:** lib-1 | **Audit date:** unknown-date

## Description

The `BuildResult.exit_code` field is documented as `0 = all ok, 1 = any failure/skip`, but the implementation can return exit code `2`. In `compile_directory_with_layout`, `exit_code = if any_failure { 1 } else { diag_worst }`. When a build has only repairable diagnostics and no hard errors or dependent skips, `any_failure` stays false and `diag_worst` is `2`, so the method returns `2`. The doc comment silently omits this third tier. The runtime value `2` is correct per the authoritative CLI contract (`docs/reference/cli.md`: repairable-only → exit 2); the defect is solely the misleading struct doc comment, which would lead a maintainer to assume any skip implies exit 1.

Note: the specific two-file trigger in the original report (b.glyph imports a.glyph where a has repairable-only diagnostics) does NOT produce exit 2 — it produces exit 1, because line 1685 unconditionally sets `any_failure = true` whenever any file is skipped. The `2` case only occurs when a repairable-only file has no consumers in the same build.

## Trigger / Reproduction

Build a directory containing a single `.glyph` file that has only a repairable diagnostic (e.g. `G::analyze::duplicate-import`) and no other files that import it. The build records `FileOutcome::Failed` but `any_failure` stays false; `exit_code = diag_worst = 2`. The doc comment claims any failure/skip yields exit 1, but exit 2 is returned.

## Evidence

```rust
/// Overall exit code for the build (0 = all ok, 1 = any failure/skip).
pub exit_code: u8,

// ...later in compile_directory_with_layout:
let exit_code = if any_failure { 1 } else { diag_worst };
```

## Recommended Resolution

Update the doc comment on `BuildResult.exit_code` to describe the real three-tier contract, referencing `docs/reference/cli.md` so it stays aligned with the CLI exit-code matrix:

```rust
/// Overall exit code for the build:
/// - 0 = all ok (or warnings only)
/// - 1 = any hard error or dependency-error skip
/// - 2 = repairable diagnostics only (no hard errors, no skips)
/// See docs/reference/cli.md for the authoritative exit-code matrix.
pub exit_code: u8,
```

## Verification Notes

The doc comment at lib.rs:1449 reads `0 = all ok, 1 = any failure/skip` but omits the `2 = repairable only` case entirely. The logic at line 1867 (`let exit_code = if any_failure { 1 } else { diag_worst }`) confirms `2` is reachable when `any_failure` is false and `diag_worst = 2`. The two-file scenario described in the original claim is wrong (line 1685 sets `any_failure = true` for any skipped file, confirmed by test at lib.rs:7180 asserting exit_code=1 for a directory compile failure). The real `2` case is a repairable-only single-file-or-no-consumers build. No code change needed — only the doc comment requires correction.

## Independent Agent Finding

### Verdict

Reproduced. The production bug is real as a documentation defect: `BuildResult.exit_code` can be `2` for repairable-only diagnostics even though the struct field comment documents only `0` and `1`. The dependent-skip trigger is refuted: when a repairable-only failed file has a dependent in the same directory build, the dependent is skipped and the aggregate exit code is `1`.

### Reproduction/Refutation

I used Graphify first to locate `BuildResult` and `compile_directory_with_layout`, then checked only the narrow implementation regions around `BuildResult`, `any_failure`, `diag_worst`, and the exit-code calculation.

Single repairable-only file:

```text
$ cargo run --quiet --manifest-path tmp/bug-065-repro/Cargo.toml -- crates/glyph-cli/tests/corpus/repairable/missing_description.glyph
exit_code=2
/Users/nishantvarshney/genesis/glyph/crates/glyph-cli/tests/corpus/repairable/missing_description.glyph failed diag_exit_code=2
  G::analyze::missing-description Repairable
```

Dependent skip case:

```text
$ cargo run --quiet --manifest-path tmp/bug-065-repro/Cargo.toml -- tmp/bug-065-repro/a.glyph tmp/bug-065-repro/b.glyph
exit_code=1
/Users/nishantvarshney/genesis/glyph/tmp/bug-065-repro/a.glyph failed diag_exit_code=2
  G::analyze::missing-description Repairable
/Users/nishantvarshney/genesis/glyph/tmp/bug-065-repro/b.glyph skipped failed_dep=/Users/nishantvarshney/genesis/glyph/tmp/bug-065-repro/a.glyph
```

### Evidence

- `crates/glyph-core/src/lib.rs` defines `BuildResult.exit_code` with the doc comment `0 = all ok, 1 = any failure/skip`, omitting `2`.
- `compile_directory_with_layout` initializes `any_failure = false`, sets it to `true` for skipped files and hard-error failures, but not for repairable-only `FileOutcome::Failed` diagnostics.
- The aggregate calculation is `let exit_code = if any_failure { 1 } else { diag_worst };`, so a repairable-only failed outcome with `diag_worst = 2` and no skips returns `2`.
- `docs/reference/cli.md` documents the authoritative three-tier matrix: `0` success, `1` hard errors, `2` repairable diagnostics only, and states that `1` wins over `2`.

### Resolution Input

Preserve the existing suggested resolution. No source behavior change is indicated; update only the `BuildResult.exit_code` doc comment so it describes the three-tier contract and points maintainers to `docs/reference/cli.md`.
