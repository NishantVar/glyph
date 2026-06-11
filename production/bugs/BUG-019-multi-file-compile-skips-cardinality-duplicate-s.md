# BUG-019: Multi-file compile skips cardinality/duplicate-section checks for export blocks

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/analyze.rs:4299-4471`
**Found by:** analyze-2 | **Audit date:** unknown-date

## Description

In the single-file path (`analyze_with_diagnostics`, `Decl::ExportBlock` arm), an `export
block`'s freeform sections are validated by `check_section_cardinality(
&spanned.node.freeform_sections, ...)` at line 2946 and `check_duplicate_sections(
&spanned.node.freeform_sections, ...)` at line 2953.

The import-aware path `analyze_with_imports` `Decl::ExportBlock` arm (4299-4471) runs
usage-tracking, `check_return_call_no_value`, and `check_block_freeform_slots`, but neither
`check_section_cardinality` nor `check_duplicate_sections`. The `analyze_export_block` helper
(line 7013) was also confirmed to contain neither check.

Trigger: a project using `import` with an `export block` whose body has a duplicate freeform
sub-section or a cardinality-one section (e.g. `goal:`) with the wrong item count.
Single-file compile emits `G::analyze::cardinality-violation` /
`G::analyze::duplicate-section`; multi-file compile emits nothing. Same root cause as the
private-block gap (BUG-018): the imports-path Block/ExportBlock arms never mirror these
freeform-section validators.

## Trigger / Reproduction

Create a multi-file project with an `import` declaration. Define an `export block` with two
`goal:` sub-sections (cardinality violation) or two duplicate freeform sub-section names.
Run `glyph compile`. Neither diagnostic is emitted. Compile the same export block without
any `import` and both errors surface correctly.

## Evidence

```rust
// single-file ExportBlock arm (lines 2946-2958):
check_section_cardinality(&spanned.node.freeform_sections, spanned.span, ...); // line 2946
check_duplicate_sections(&spanned.node.freeform_sections, ...);                // line 2953

// imports-path ExportBlock arm (lines 4299-4471):
// dispatches to analyze_export_block (line 7013), plus:
//   usage tracking, check_return_call_no_value, check_block_freeform_slots
// -- neither check_section_cardinality nor check_duplicate_sections is present --
// -- analyze_export_block helper (line 7013) also contains neither check --
```

## Recommended Resolution

In `analyze_with_imports`'s `Decl::ExportBlock` arm, add:

```rust
check_section_cardinality(&spanned.node.freeform_sections, spanned.span, ...);
check_duplicate_sections(&spanned.node.freeform_sections, ...);
```

after the `check_block_freeform_slots` call, mirroring the single-file ExportBlock arm
(lines 2946/2953).

## Verification Notes

Both code paths were traced directly. In `analyze_with_diagnostics`, the `Decl::ExportBlock`
arm at lines 2946-2958 explicitly calls both checks on `spanned.node.freeform_sections`. In
`analyze_with_imports`, the `Decl::ExportBlock` arm at lines 4299-4471 dispatches to
`analyze_export_block` plus several inline checks, but neither the inline arm nor the helper
contains any call to `check_section_cardinality` or `check_duplicate_sections`. All existing
test fixtures for these validators are single-file with no imports, so the gap is untested.

## Independent Agent Finding

### Verdict

Reproduced. On 2026-06-10, a scratch import-aware project with an `export block` containing
either a two-item `goal:` section or duplicate `quality:` sections compiled with exit 0 and no
JSON diagnostics. The equivalent single-file `export block` inputs emitted the expected hard
errors.

### Reproduction/Refutation

Created transient fixtures under `tmp/bug019-repro/`:

- `cardinality-single/main.glyph`: standalone `export block bad_export()` with two `goal:`
  items.
- `cardinality-multi/main.glyph`: same invalid `export block`, plus
  `import "./lib.glyph" { shared_context }` and `context shared_context`.
- `duplicate-single/main.glyph`: standalone `export block bad_export()` with duplicate
  `quality:` sections.
- `duplicate-multi/main.glyph`: same duplicate sections, plus the import/context reference.

Command results:

```text
cargo run -q -p glyph-cli -- compile tmp/bug019-repro/cardinality-single/main.glyph --format json
exit 1; stdout contains G::analyze::cardinality-violation

cargo run -q -p glyph-cli -- compile tmp/bug019-repro/duplicate-single/main.glyph --format json
exit 1; stdout contains G::analyze::duplicate-section

cargo run -q -p glyph-cli -- compile tmp/bug019-repro/cardinality-multi --format json
exit 0; stdout empty

cargo run -q -p glyph-cli -- compile tmp/bug019-repro/duplicate-multi --format json
exit 0; stdout empty
```

This confirms the reported behavior rather than refuting it: the validators fire in the
single-file path and are skipped in the import-aware path.

### Evidence

Graphify query for `analyze_with_imports ExportBlock check_section_cardinality
check_duplicate_sections` showed `analyze_with_imports()` calls `analyze_export_block()` and
`check_block_freeform_slots()`, while `check_section_cardinality()` and
`check_duplicate_sections()` are separate analyzer helpers.

Bounded source checks in `crates/glyph-core/src/analyze.rs` match the runtime result:

- `rg -n "Decl::ExportBlock|check_section_cardinality|check_duplicate_sections|analyze_export_block|check_block_freeform_slots" crates/glyph-core/src/analyze.rs`
  shows the single-file `Decl::ExportBlock` arm around lines 2892-2953 has
  `check_block_freeform_slots`, `check_section_cardinality`, and `check_duplicate_sections`.
- The import-aware `Decl::ExportBlock` arm around lines 4299-4469 calls
  `analyze_export_block`, tracks import usage, checks `return` calls, then calls
  `check_block_freeform_slots` and closes the arm with no cardinality or duplicate-section
  check.
- `analyze_export_block` starts around line 7013 and handles return-type/output/call
  validation; the inspected body does not contain these freeform-section validators.

### Resolution Input

Keep the existing suggested resolution: add `check_section_cardinality` and
`check_duplicate_sections` to the `analyze_with_imports` `Decl::ExportBlock` arm immediately
after `check_block_freeform_slots`, mirroring the single-file arm. Add regression coverage that
compiles an import-bearing directory containing an invalid `export block` and asserts
`G::analyze::cardinality-violation` / `G::analyze::duplicate-section` are emitted.
