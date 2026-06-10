# BUG-017: Multi-file compile skips block flow-assign rejection (single/multi-file diagnostic divergence)

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/analyze.rs:4472-4595`
**Found by:** analyze-2 | **Audit date:** unknown-date

## Description

The single-file analysis path (`analyze_with_diagnostics`, `Decl::Block` arm) calls
`check_block_flow_assign_rejected(&spanned.node.flow, ...)` at line 3058, which emits
`G::analyze::flow-assign-in-block-unsupported` (Error) whenever a private `block`'s flow
contains a flow-position assignment `x = foo()` (a `FlowStmt::Call` with `bound_name: Some(..)`).

The import-aware path `analyze_with_imports` (`Decl::Block` arm, lines 4472-4595) does NOT
call this check. None of the walkers it does call (`track_flow_usage`,
`walk_return_of_no_value_call`, `check_flow_output_target_shadows_binding`,
`check_block_return_calls`, `check_flow_placeholder_string_returns`,
`check_block_freeform_slots`, `warn_if_banned_return_type`) reject the binding.

Trigger: any project that uses `import` (routing through `analyze_with_imports`, invoked
from lib.rs:1397 and lib.rs:3229) containing a private `block helper() { flow: x = foo() }`.
Single-file compilation of the same block emits the error; multi-file compilation silently
accepts it. Lowering ignores `bound_name` (never read in lower.rs), so the assignment is
silently dropped from output rather than erroring. This is a reachable divergence between the
single-file and multi-file pipelines on the same input.

## Trigger / Reproduction

Create a multi-file project with an `import` declaration. Define a private `block` whose
`flow:` section contains a flow-position assignment: `x = foo()`. Run `glyph compile`. The
error `G::analyze::flow-assign-in-block-unsupported` is NOT emitted. Compile the same block
in a single-file (no `import`) and the error is correctly emitted.

## Evidence

```rust
// single-file path, Decl::Block arm (line 3058):
check_block_flow_assign_rejected(&spanned.node.flow, file_label, line_index, bag);

// imports path, Decl::Block arm (lines 4472-4595):
// -- no equivalent call to check_block_flow_assign_rejected present --
// walkers called: track_flow_usage, walk_return_of_no_value_call,
//   check_flow_output_target_shadows_binding, check_block_return_calls,
//   check_flow_placeholder_string_returns, check_block_freeform_slots,
//   warn_if_banned_return_type
```

## Recommended Resolution

In `analyze_with_imports`'s `Decl::Block` arm, add:

```rust
check_block_flow_assign_rejected(&spanned.node.flow, file_label, line_index, bag);
```

mirroring line 3058 of the single-file path, so block flow-position assignments are rejected
identically on both pipelines.

## Verification Notes

The knowledge graph confirms the exact asymmetry: `analyze_with_diagnostics` (L2599) has a
direct call edge to `check_block_flow_assign_rejected` (L1938), while `analyze_with_imports`
(L3871) has 34 outgoing call edges and `check_block_flow_assign_rejected` is absent from all
of them. The function has degree 3 total — defined twice and called exactly once, only by
`analyze_with_diagnostics`. A private block with a flow-position assignment in a multi-file
project routed through `analyze_with_imports` will never trigger the error that single-file
analysis correctly emits.
