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

## Independent Agent Finding

### Verdict

Confirmed as a validator-level latent bug. The false `G::expand::malformed-markdown` is reproducible through the public `glyph validate-output` command with a schema-compatible synthetic IR containing an arm-local `return` after a non-step `inline_instruction`. Current compiler emission reachability is refuted for normal `.glyph` input because `lower_flow_body` still drops `FlowStmt::Return(_)` in branch bodies instead of emitting `IrNode::Return`.

### Reproduction/Refutation

Reproduction used a scratch IR/Markdown pair under `tmp/bug072/`: a branch `then_body` with `inline_instruction` role `"constraint"` followed by a `return`, and Markdown with exactly one lettered sub-step, `a. Output: a concise answer.` Since constraint-role inline instructions are not step-projecting, that Markdown has the correct sub-step count for the arm-local return.

Command/output summary:

- `mcp__graphify.query_graph("Glyph validate_output check_return_arm letter_idx count_step_projecting_nodes ...")` located `count_step_projecting_nodes()` and `check_return_arm()` in `crates/glyph-core/src/validate_output.rs`.
- `awk 'NR>=790 && NR<=835 ...' crates/glyph-core/src/validate_output.rs` showed `count_step_projecting_nodes` counts `call | inline_instruction | instruction_ref` only when `role == "step"`, while always counting `"return"`.
- `awk 'NR>=1738 && NR<=1810 ...' crates/glyph-core/src/validate_output.rs` showed `check_return_arm` increments `letter_idx` for `call | inline_instruction | instruction_ref` without a role guard before looking up the return's `md_item.sub_items`.
- `awk 'NR>=481 && NR<=650 ...' crates/glyph-core/src/lower.rs` showed `lower_flow_body` handles `FlowStmt::Return(_) | FlowStmt::BareName(_)` by emitting no IR node, with the comment that branch-body return is caught by validation.
- `cargo run -p glyph-cli -- validate-output tmp/bug072/ir.json tmp/bug072/out.md --format json` exited `1` and printed `{"classification":"error","id":"G::expand::malformed-markdown","message":"missing `Output:` sub-step for arm-local Return"}`.

This reproduces the reported index drift: the non-step inline instruction consumes `letter_idx` in `check_return_arm`, so the return checks sub-item index `1` even though the only rendered/expected sub-item is index `0`.

### Evidence

The checker's two counters disagree on the same IR body. `count_step_projecting_nodes` aligns with the rendering contract by excluding non-step inline instructions from the lettered sub-step count, but `check_return_arm` advances its flattened sub-step cursor for those non-step nodes. The scratch validation result demonstrates the practical consequence: a structurally correct single `Output:` sub-step is reported as missing.

The production compile path remains dormant for this exact arm-local-return shape today: `lower_flow_body` recurses through branch arms but drops `FlowStmt::Return(_)`, so normal compiler-emitted IR should not currently contain the `return` arm node needed to trigger this from source input.

### Resolution Input

Preserve the existing suggested resolution: gate `letter_idx += 1` for `call`/`inline_instruction`/`instruction_ref` on `role == "step"`, matching `count_step_projecting_nodes`. This is still the right fix before arm-local returns are emitted by the live lowering path. The note that `"instruction_ref"` appears to be dead IR-kind handling can remain separate from the minimal bug fix.
