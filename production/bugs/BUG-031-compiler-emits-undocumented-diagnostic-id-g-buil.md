# BUG-031: Compiler emits undocumented diagnostic ID G::build::compile-error

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Note:** likely already tracked — diagnostics-todos.md (`G::build::compile-error` undocumented generic fallback violates catalog completeness rule)
**Location:** `crates/glyph-core/src/lib.rs:1841-1849`
**Found by:** x-contract | **Audit date:** unknown-date

## Description

`diagnostics.md` §Catalog Completeness Rule (lines 68-76) and §Build phase (lines 187-190) make the catalog the exhaustive, stable contract of compiler-emitted IDs, and `mvp-acceptance.md` (lines 79-80) asserts "Every diagnostic ID the compiler emits at MVP is listed in the diagnostics reference doc." The build pipeline emits `G::build::compile-error` (classification error) as a catch-all for Read/Parse/Lower/Validate failures not wired to a structured ID.

This ID is absent from diagnostics.md's catalog (only `G::build::skipped-due-to-failed-import` and `G::build::import-outside-out-dir` are documented under Build). It is a real, reachable, error-class diagnostic that reaches CLI output via `FileOutcome::Failed{diagnostics}` -> `emit_diagnostics`, so a tool keying on the documented ID set will encounter an unlisted ID. Particularly acute for `ValidateError`: `ValidateError::diagnostic_id()` already maps each variant to documented `G::validate::*` IDs, but that method is never called — errors bypass all structured diagnostics and become `G::build::compile-error` at the catch-all.

## Trigger / Reproduction

Any invalid `.glyph` file that reaches the catch-all `Err(e)` handler in the build pipeline. For example, a file with a validate-phase error will emit `G::build::compile-error` to CLI output rather than the structured `G::validate::*` ID documented in diagnostics.md. The integration test at `integration_issue_86.rs:107` references this ID in a comment, confirming it is a known, exercised path.

## Evidence

```rust
Diagnostic::error(
    "G::build::compile-error",
    format!("compile pipeline failed: {:?}", e),
    SourceSpan::from_byte_span(&label, span, &li),
)
```

## Recommended Resolution

Either add `G::build::compile-error` to the diagnostics.md Build-phase catalog (with a note that it is a generic catch-all), or replace the catch-all with the specific structured IDs the underlying Read/Parse/Lower/Validate errors should carry. For `ValidateError` specifically, `ValidateError::diagnostic_id()` already provides the correct structured IDs — these should be called and emitted instead of falling through to the catch-all. The comment in the handler admits it is a stand-in for unwired errors.

## Verification Notes

The catch-all `Err(e)` arm at lib.rs line 1828 synthesizes `G::build::compile-error` for any `CompileError` variant that isn't `CompileError::Lower(LowerError::NoSkill)`. Confirmed reachable via `CompileError::Validate(...)` (from `validate::validate(&arena).map_err(CompileError::Validate)?`), `CompileError::Lower(LowerError::UndefinedConstraintRef)`, `UndefinedContextRef`, and `CompileError::Read`/`Write` for I/O failures. The diagnostics.md catalog (lines 174-181) lists six `G::validate::*` IDs as the stable documented contract, but these IDs are never actually emitted by the compiler — they are short-circuited through the catch-all. `G::build::compile-error` is absent from the catalog, violating the Catalog Completeness Rule and `mvp-acceptance.md §Diagnostic IDs`.

## Independent Agent Finding

Verdict: Reproduced. The current compiler still emits undocumented error diagnostic ID `G::build::compile-error` through the build catch-all.

Reproduction/Refutation: I reproduced the fallback via the `CompileError::Read` path using an isolated scratch file in `tmp/`: create a valid `.glyph` file, make it unreadable with `chmod 000`, then run `cargo run --quiet -p glyph-cli -- compile --format json tmp/bug031_unreadable.glyph`. The command exited `1` and emitted NDJSON with `id:"G::build::compile-error"` and `classification:"error"`. I also tested the report's undefined-constraint example shape; in the current checkout that no longer reaches the build fallback and instead emits `G::analyze::undefined-name`, so that specific example is stale even though the underlying bug is still real.

Evidence: `crates/glyph-core/src/lib.rs:1839-1849` still synthesizes `Diagnostic::error("G::build::compile-error", format!("compile pipeline failed: {:?}", e), ...)` for non-`NoSkill` `CompileError`s. `docs/reference/diagnostics.md` still lists only `G::build::skipped-due-to-failed-import` and `G::build::import-outside-out-dir` under the Build phase; `rg -n "G::build::compile-error" docs/reference/diagnostics.md` returned no matches. The reproduction command output was: `{"id":"G::build::compile-error","classification":"error","message":"compile pipeline failed: Read { ... PermissionDenied ... }", ...}`.

Resolution Input: Preserve the existing recommended resolution. Either document `G::build::compile-error` as an intentional build-phase fallback, or replace the catch-all with structured diagnostics for each underlying `Read`/`Write`/`Parse`/`Lower`/`Validate` failure path. If replacing it, include the read/write failure paths in addition to the Validate-specific `diagnostic_id()` mapping noted above.
