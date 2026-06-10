# BUG-075: Canonical default section order in compiled-output.md swaps Constraints/Context vs implementation

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `docs/reference/compiled-output.md:55`
**Found by:** x-contract | **Audit date:** unknown-date

## Description

`compiled-output.md` line 55 documents the canonical default order as `Goal, Parameters, Context, Constraints, Steps`. The emitter assigns canonical slots Goal=1, Parameters=2, Constraints=3, Context=4, Flow/Steps=5 (`emit/scaffold.rs:28-48`, slot constants confirmed at `mod.rs:638/649/659/682`), so the real default order is `Goal, Parameters, Constraints, Context, Steps` — Constraints precedes Context, the reverse of the doc. Both `cli.md` line 53 and `GLYPH_LANGUAGE_GUIDE.md` §7 (recommended order `... constraints, context, flow`) agree with the implementation, making `compiled-output.md` the sole outlier. A skill that declares neither `constraints:` nor `context:` at an explicit source position (so both take their canonical slots) emits Constraints before Context, contradicting this reference contract.

## Trigger / Reproduction

Compile a skill that declares both `constraints:` and `context:` sections without explicit source-position overrides and observe that Constraints appears before Context in the compiled output. Compare against the canonical order listed in `compiled-output.md:55`, which incorrectly shows Context at slot 3 and Constraints at slot 4.

## Evidence

```rust
// emit/scaffold.rs:28 — canonical slot assignments per spec §4.1.5
// "Canonical slot per spec §4.1.5: Goal=1, Parameters=2, Constraints=3, Context=4, Flow=5."
//
// mod.rs slot constant assignments (lines 638/649/659/682):
//   Parameters  => Some(2)
//   Constraints => Some(3)   // Constraints is slot 3
//   Context     => Some(4)   // Context is slot 4
//   Flow        => Some(5)
```

```markdown
<!-- compiled-output.md line 55 — INCORRECT order -->
sub-sections not declared keep the canonical default order
(`Goal`, `Parameters`, `Context`, `Constraints`, `Steps`)
--                      ^^^^^^^^^  ^^^^^^^^^^^
--                      slot 3?    slot 4?  — reversed vs. implementation
```

## Recommended Resolution

Correct `compiled-output.md` line 55 to `Goal, Parameters, Constraints, Context, Steps` to match the emitter's canonical slots and the other two reference documents.

## Verification Notes

`scaffold.rs` explicitly documents the slot order in its `RenderUnit` struct comment. `docs/architecture/ir-semantics.md` lines 79-80 and 144-145 independently confirm Constraints=slot 3, Context=slot 4. Both `cli.md` line 53 and `GLYPH_LANGUAGE_GUIDE.md` §7 list the recommended source order as `constraints:` before `context:`, matching the implementation. `compiled-output.md` is the sole outlier. This is a documentation-only inconsistency with no runtime or compiler behavior impact.
