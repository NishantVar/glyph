# BUG-038: False procedure-step-count-mismatch when a same-file procedure block ends with `return`

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/validate_output.rs:1308-1324`
**Found by:** validate-1 | **Audit date:** unknown-date

## Description

`check_procedures` compares the number of rendered numbered items in a `### Procedure:` section (`proc_section.items.len()`) against `find_callee_flow_count`, which returns `callee_flow.len()` — the raw length of the serialized block flow. `emit_ir.rs::serialize_call` (line 448) serializes EVERY `IrBlockFlowItem` into `callee_flow`, including `IrBlockFlowItem::Return`. But the procedure renderer (`emit/scaffold.rs` lines 950-983) computes `visible_count` by filtering out `Return` items and folds the return sentence into the last visible step (`append_return_sentence`) rather than emitting a separate numbered item.

So for a same-file procedure callee block that contains a `return` statement, the rendered section has V numbered items while `callee_flow.len()` = V+1. `find_callee_flow_count` returns V+1, so `proc_section.items.len() (V) != callee_flow_count (V+1)` fires a spurious `G::expand::procedure-step-count-mismatch` on perfectly valid compiled output.

The return-only case (V=0, a `return`-only block) is unaffected because the scaffold emits 1 standalone step and `callee_flow.len()` is also 1. The bug only manifests with at least one visible step plus a `return`.

## Trigger / Reproduction

A `.glyph` file with a private block called via same-file-procedure projection where the block ends in `return <name>`:

```
block fetch(url)
    flow:
        "make HTTP request to {url}"
        return <result>

export skill run(url)
    flow:
        call fetch(url)
```

Running `glyph validate-output` on the compiled output reports a non-existent `G::expand::procedure-step-count-mismatch`.

## Evidence

```rust
if let Some(callee_flow_count) = find_callee_flow_count(flow, proc_name) {
    if proc_section.items.len() != callee_flow_count { ... }
    // callee_flow_count counts the Return node the MD section never numbers
}
```

`find_callee_flow_count` returns raw `callee_flow.len()` (V+1), while `proc_section.items.len()` is V (the rendered numbered items excluding the folded return sentence).

## Recommended Resolution

In `find_callee_flow_count`, count only step-projecting `callee_flow` nodes, mirroring the emitter's `visible_count`: exclude `return` nodes (and any other non-step-projecting kinds). Concretely, count `callee_flow` entries whose kind is not `"return"` (or whichever sentinel value `IrBlockFlowItem::Return` serializes to).

## Verification Notes

In `emit_ir.rs` lines 448-477, `serialize_call` maps ALL `IrBlockFlowItem` variants — including `IrBlockFlowItem::Return` — into the `callee_flow` JSON array without filtering. In `scaffold.rs` lines 950-953, the procedure renderer explicitly filters out `Return` items from `visible_count` and folds the return sentence into the last visible step via `append_return_sentence`. In `validate_output.rs` line 1440, `find_callee_flow_count` returns raw `callee_flow.len()` (V+1), while `proc_section.items.len()` is V. The mismatch check fires a spurious error. The `predicate_branch_last_in_block_procedure_with_return` fixture passes because when the last visible step is a Branch, the return sentence IS emitted as a new numbered item (scaffold.rs line 987), keeping counts equal — confirming the bug only triggers when the last visible step is a non-Branch item.
