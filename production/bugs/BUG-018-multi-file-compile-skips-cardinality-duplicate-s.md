# BUG-018: Multi-file compile skips cardinality/duplicate-section checks for private blocks

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/analyze.rs:4472-4595`
**Found by:** analyze-2 | **Audit date:** unknown-date

## Description

In the single-file path (`analyze_with_diagnostics`, `Decl::Block` arm), a private `block`'s
freeform sections are validated by `check_section_cardinality(&spanned.node.freeform_sections,
...)` at line 3079 (emits `G::analyze::cardinality-violation`, e.g. a `goal:` section with
!=1 items) and `check_duplicate_sections(...)` at line 3086 (emits
`G::analyze::duplicate-section`, e.g. two `quality:` sub-sections).

The import-aware path `analyze_with_imports` `Decl::Block` arm (4472-4595) calls neither;
it only runs `check_block_freeform_slots`.

Trigger: a project using `import` with a private `block` whose body has a duplicated freeform
sub-section or a single-item-cardinality section with the wrong item count. Single-file
compile flags it; multi-file compile silently accepts it. Both diagnostic IDs vanish on the
multi-file path even though the violation is identical.

## Trigger / Reproduction

Create a multi-file project with an `import` declaration. Define a private `block` with two
`goal:` entries (cardinality violation) or two identically named freeform sub-sections
(duplicate-section violation). Run `glyph compile`. Neither
`G::analyze::cardinality-violation` nor `G::analyze::duplicate-section` is emitted. Compile
the same block without any `import` declaration and both errors are correctly emitted.

## Evidence

```rust
// single-file Block arm (lines 3079-3091):
check_section_cardinality(&spanned.node.freeform_sections, spanned.span, ...); // line 3079
check_duplicate_sections(&spanned.node.freeform_sections, ...);                // line 3086

// imports-path Block arm (lines 4472-4595):
// -- only check_block_freeform_slots is called --
// -- neither check_section_cardinality nor check_duplicate_sections is present --
```

## Recommended Resolution

In `analyze_with_imports`'s `Decl::Block` arm, add the same two calls used in the single-file
path after the existing `check_block_freeform_slots` call:

```rust
check_section_cardinality(&spanned.node.freeform_sections, spanned.span, ...);
check_duplicate_sections(&spanned.node.freeform_sections, ...);
```

mirroring lines 3079 and 3086 of the single-file path.

## Verification Notes

Direct code inspection confirms the asymmetry. In `analyze_with_diagnostics`, the
`Decl::Block` arm (lines 3064-3091) calls `check_block_freeform_slots` at line 3064,
`check_section_cardinality` at line 3079, and `check_duplicate_sections` at line 3086. In
`analyze_with_imports`, the `Decl::Block` arm (lines 4472-4595) calls only
`check_block_freeform_slots` at line 4585. All existing test fixtures for cardinality and
duplicate-section (`block_cardinality_violation.glyph`, `duplicate_section_freeform.glyph`)
are single-file with no imports, so no test exercises the gap. The proposed fix is correct
and complete.

## Independent Agent Finding

### Verdict

Reproduced. The current CLI reports both diagnostics on the single-file path, but accepts the
same private-block violations when a valid import forces the import-aware path.

### Reproduction/Refutation

Created scratch fixtures under `tmp/bug018-independent-agent/` with a valid imported const,
a no-import control, and matching imported variants for a private `block helper()`.

Commands run:

```sh
cargo run -q -p glyph-cli -- compile tmp/bug018-independent-agent/single.glyph --format json
cargo run -q -p glyph-cli -- compile tmp/bug018-independent-agent/with_import.glyph --format json
cargo run -q -p glyph-cli -- compile tmp/bug018-independent-agent/single_duplicate.glyph --format json
cargo run -q -p glyph-cli -- compile tmp/bug018-independent-agent/with_import_duplicate.glyph --format json
rg -n "check_section_cardinality\\(|check_duplicate_sections\\(|check_block_freeform_slots\\(" crates/glyph-core/src/analyze.rs
```

### Evidence

`single.glyph` exited 1 and emitted `G::analyze::cardinality-violation` with message
"section goal: accepts only one item but 2 were provided". `with_import.glyph` exited 0
with no diagnostics.

`single_duplicate.glyph` exited 1 and emitted `G::analyze::duplicate-section` with message
"duplicate quality: sub-section". `with_import_duplicate.glyph` exited 0 with no
diagnostics.

Graphify located `analyze_with_imports`, `check_block_freeform_slots`,
`check_section_cardinality`, and `check_duplicate_sections` in
`crates/glyph-core/src/analyze.rs`. The bounded `rg` check corroborated the asymmetry:
single-file `Decl::Block` calls appear at lines 3064, 3079, and 3086, while the imports-path
private block arm has only `check_block_freeform_slots` at line 4585.

### Resolution Input

Keep the existing suggested resolution. Add imports-path parity for private `Decl::Block` by
calling `check_section_cardinality` and `check_duplicate_sections` after
`check_block_freeform_slots`, and add regression coverage for both imported private-block
cardinality and duplicate-section cases.
