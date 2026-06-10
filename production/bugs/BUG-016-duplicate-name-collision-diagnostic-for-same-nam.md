# BUG-016: Duplicate `name-collision` diagnostic for same-named exports (redundant seen_exports sweep)

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/analyze.rs:3099-3121`
**Found by:** analyze-1 | **Audit date:** unknown-date

## Description

`analyze_with_diagnostics` runs a legacy `seen_exports` sweep (lines 3099-3121) that emits `G::analyze::name-collision` ("duplicate export name `X`") for two export blocks / exported consts with the same name. Immediately afterward it calls `sweep_value_name_collisions` (line 3148), which records ALL consts, blocks, export blocks, skills and import aliases under a canonical key and emits its own `G::analyze::name-collision` ("X collides with earlier X (canonically equal)") for the very same exact-name duplicate. `DiagBag::push` does no de-duplication. Result: a file with two `export block foo` (or two `export const foo`) produces TWO Error-tier `name-collision` diagnostics at the same span. The `seen_exports` set is keyed by raw name (not canonical), records only ExportBlock + exported Const, and is a strict subset of what `sweep_value_name_collisions` already covers, so it catches nothing that sweep_value misses. The redundant diagnostic it emits is also lower quality — it carries no `related` span, unlike the sweep_value one. The identical redundant sweep is duplicated in the imports path at lines 4601-4621 (just before the `sweep_value` call at 4636), so both compile paths double-report.

## Trigger / Reproduction

Create a `.glyph` file with two declarations named identically, e.g. two `export block foo()` blocks. Run `glyph check`. The output contains two `G::analyze::name-collision` errors at the same span — one from the `seen_exports` sweep ("duplicate export name `foo`") and one from `sweep_value_name_collisions` ("export block `foo` collides with earlier `foo` (canonically equal)").

## Evidence

```rust
// G::analyze::name-collision — duplicate export names.
{
    let mut seen_exports: HashMap<&str, Span> = HashMap::new();
    for decl in &file.decls {
        let (name, span) = match decl {
            Decl::ExportBlock(b) => (b.node.name.as_str(), b.span),
            Decl::Const(c) if c.node.exported => (c.node.name.as_str(), c.span),
            _ => continue,
        };
        if let Some(_prev_span) = seen_exports.get(name) {
            bag.push(Diagnostic::error("G::analyze::name-collision",
                format!("duplicate export name `{}`", name), ...), span);
        } else { seen_exports.insert(name, span); }
    }
}
// ... then sweep_value_name_collisions(&file, ...) also emits name-collision for the same pair
```

## Recommended Resolution

Delete the redundant `seen_exports` block in `analyze_with_diagnostics` (lines 3099-3121) and the identical one in `analyze_with_imports` (lines 4601-4621). `sweep_value_name_collisions` already detects every value-namespace collision these blocks covered (export blocks + exported consts are a subset of all decl types it handles), with a better message and a `related` span pointing to the first occurrence. If an export-specific message is desired, gate or special-case the wording inside `sweep_value_name_collisions` instead of emitting a second diagnostic.

## Verification Notes

Confirmed by direct code inspection and live reproduction: running `glyph check` on a file with two `export block walkthrough` declarations produces exactly two `G::analyze::name-collision` errors at the same span — one from each code path. `DiagBag::push` at `diagnostic.rs:197` is a plain `Vec::push` with no deduplication. `sweep_value_name_collisions` records all `Decl::Const` (not just exported ones) and `Decl::ExportBlock`, which is a strict superset of what `seen_exports` covers. The `seen_exports` sweep is entirely redundant and emits a lower-quality diagnostic (no `related` span). The same pattern repeats in the imports path. The existing tests use `contains` assertions and will continue to pass after the redundant blocks are deleted.
