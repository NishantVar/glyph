# BUG-028: compile path silently ignores unresolvable imports that `check` rejects with missing-file

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/lib.rs:2854-2861`
**Found by:** lib-2 | **Audit date:** unknown-date

## Description

In `build_resolved_imports`, an import path that fails to resolve is dropped silently (`let resolved = match resolve_import_path(consumer, &import.path) { Some(r) => r, None => continue }`), and the same is true in the Phase-1 DAG builder (`compile_directory_with_layout`, ~line 1591, which only pushes resolved deps). The directory/compile pipeline therefore never emits `G::analyze::missing-file`, whereas the check pipeline (`check_one_file`, lib.rs ~lines 1109-1123) emits it as a hard error.

The CLI `compile` command goes straight to `compile_directory_with_layout` without running check first, so the divergence is user-reachable. Concrete trigger: a consumer file `import "missing.glyph" (helper)` where `helper` is never called in the flow. `glyph check` reports `G::analyze::missing-file` and exits 1; `glyph compile` silently produces compiled markdown and exits 0. (When the imported name IS used, the consumer instead trips `G::analyze::undefined-call`, masking the issue — but the unused-import case slips through entirely.) This violates the documented contract that imports must resolve and that check/compile agree on validity.

## Trigger / Reproduction

Create a consumer file with `import "missing.glyph" { helper }` where `missing.glyph` does not exist and `helper` is never called in the flow. Run `glyph check` (exits 1, emits `G::analyze::missing-file`) then `glyph compile` (exits 0, produces compiled markdown with no diagnostic). The divergence is confirmed end-to-end.

## Evidence

```rust
let resolved = match resolve_import_path(consumer, &import.path) {
    Some(r) => r,
    None => continue,
};
```

## Recommended Resolution

Detect unresolvable filesystem imports on the compile path and surface `G::analyze::missing-file` (mirroring `check_one_file`). Easiest approach: push a diagnostic into `result.import_diagnostics` in the `None` arm of `build_resolved_imports` (and/or fail the file in the Phase-1 DAG builder) so `compile_file_with_resolved_imports` returns it instead of routing the file to the no-import fast path, matching the check pipeline's behavior and exit code.

## Verification Notes

In `build_resolved_imports` (lib.rs L2854-2857), when `resolve_import_path` returns `None` for a filesystem import, the code does `None => continue` with no diagnostic pushed to `result.import_diagnostics`. In contrast, `check_one_file` (L1109-1122) pushes `G::analyze::missing-file` in the same `None` arm. Since `compile_file_with_resolved_imports` (L3125-3129) only returns an error outcome when `import_diagnostics` is non-empty, a consumer file with an unresolvable import compiles successfully and exits 0. The `compute_import_closure` path also silently skips non-resolving imports (L1511). The only test for `missing-file` (L6260-6283) calls `check_file`, not the compile pipeline, so this divergence has no test coverage. Reproduced end-to-end: `glyph check` exits 1 with `G::analyze::missing-file`; `glyph compile` exits 0 and produces valid `consumer.md`.

## Independent Agent Finding

**Verdict:** Reproduced.

**Reproduction/Refutation:** Copied the existing missing-import fixture into `tmp/bug-028-repro/consumer.glyph`. The fixture imports `./nonexistent.glyph` selectively and never references the imported `something` name in its flow. Running the same file through `check` and `compile` reproduced the reported check/compile divergence.

**Evidence:**

- `cargo run -q -p glyph-cli -- check tmp/bug-028-repro/consumer.glyph --format json` exited 1 and emitted `G::analyze::missing-file` with message `imported file './nonexistent.glyph' not found`.
- `cargo run -q -p glyph-cli -- compile tmp/bug-028-repro/consumer.glyph --format json` exited 0 and emitted no diagnostic output.
- `compile` produced `tmp/bug-028-repro/consumer.md` containing frontmatter for `main` and a single compiled step, `Do something.`
- Graphify located the relevant implementation nodes at `check_one_file`, `resolve_import_path`, and `compile_directory_with_layout`. Bounded source reads confirmed the same root cause described above: `check_one_file` pushes `G::analyze::missing-file` when `resolve_import_path` returns `None`, while the compile DAG/import-resolution path skips unresolved filesystem imports.

**Resolution Input:** Preserve the existing suggested resolution. The compile path should emit `G::analyze::missing-file` for unresolved filesystem imports and fail consistently with `check`, including the unused-import case where no later undefined-call diagnostic masks the missing file. Add regression coverage that runs both `check` and `compile` against an unused selective import of a nonexistent `.glyph` file and asserts matching nonzero behavior.
