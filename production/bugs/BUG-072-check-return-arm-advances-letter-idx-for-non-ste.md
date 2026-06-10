# BUG-072: check_return_arm advances letter_idx for non-step arm nodes, misaligning the Output sub-step lookup

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/validate_output.rs:1769-1800`
**Found by:** validate-1 | **Audit date:** unknown-date

## Description

In `check_return_arm`, `letter_idx` is incremented for every `call`/`inline_instruction`/`instruction_ref` node (lines 1773-1775) WITHOUT checking `role == "step"`, whereas the rendered lettered sub-items only include step-projecting nodes (`count_step_projecting_nodes` requires `role == "step"`). If an arm body contains a non-step `inline_instruction` or `call` node (e.g. a constraint- or context-role node), `letter_idx` over-counts and `md_item.sub_items.get(letter_idx)` looks at the wrong sub-item (or returns `None`), producing a wrong/false `G::expand::malformed-markdown` for an arm-local Return. This path is currently dormant because arm-local Returns are post-MVP and not yet emitted (documented in the function's own comment: "arm-local returns are not yet emitted, so this is a no-op until the parser allows them"), but the index logic is inconsistent with the rest of the module and will misbehave once arm returns are enabled.

## Trigger / Reproduction

This bug is not currently reachable in production — `lower_flow_body` does not handle `FlowStmt::Return`, so `IrNode::Return` nodes are never emitted inside arm bodies. It will trigger once arm-local returns are enabled: a branch arm containing a non-step `inline_instruction` (e.g. from a `ConstraintMarker` statement) followed by a `return` will cause `letter_idx` to over-count, making the sub-item lookup reference the wrong lettered step or return `None`.

## Evidence

```rust
// check_return_arm: increments letter_idx without role == "step" guard
"call" | "inline_instruction" | "instruction_ref" => {
    letter_idx += 1;  // no role == "step" guard, unlike count_step_projecting_nodes
}

// count_step_projecting_nodes (the reference implementation, correctly guarded):
"call" | "inline_instruction" | "instruction_ref" if node.role == "step" => {
    count += 1;
}
```

## Recommended Resolution

Gate the `letter_idx += 1` for `call`/`inline_instruction`/`instruction_ref` on `role == "step"`, mirroring `count_step_projecting_nodes`:

```rust
"call" | "inline_instruction" | "instruction_ref" if node.role == "step" => {
    letter_idx += 1;
}
```

This keeps the sub-item index aligned with what was actually rendered when arm-local returns are eventually enabled. Note: `"instruction_ref"` is not a real IR node kind (no `IrInstructionRef` struct exists), so that arm is dead code regardless and may be removed.

## Verification Notes

The inconsistency is confirmed by code trace: `count_step_projecting_nodes` at line 815-816 guards `call | inline_instruction | instruction_ref` with `role == "step"`, but `check_return_arm` at lines 1773-1775 increments `letter_idx` for those same kinds without any role guard. `inline_instruction` nodes CAN have non-step roles (e.g. `Role::Constraint` from `ConstraintMarker` statements). The function's own doc comment at lines 1745-1748 explicitly acknowledges the code is a no-op until arm-local returns are emitted. The bug is latent but the logic error is real and will cause false `G::expand::malformed-markdown` violations once arm returns are enabled with mixed-role arm bodies. The fix is correct and complete.
