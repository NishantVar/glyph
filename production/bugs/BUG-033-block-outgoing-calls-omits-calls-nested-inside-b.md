# BUG-033: Block outgoing_calls omits calls nested inside branch bodies, defeating validate's recursive-call cycle detection

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/lower.rs:866-875`
**Found by:** lower-1 | **Audit date:** unknown-date

## Description

When lowering a block, `outgoing_calls` is built by scanning only top-level flow statements (`FlowStmt::Call` and `FlowStmt::Return(Call)`); calls that appear inside `if`/elif/else branch bodies are never added. `validate.rs` builds its block call-graph adjacency entirely from `IrBlock.outgoing_calls` (validate.rs L61: `adjacency.insert(&b.name, &b.outgoing_calls)`) and runs DFS cycle detection on it.

Concrete trigger: `block a() { if cond { call a } }` — the recursive `call a` is branch-nested, so `outgoing_calls` for `a` is empty, the adjacency has no self-edge, DFS finds no back-edge, and `ValidateError::RecursiveCall` is never raised. A genuinely recursive/cyclic skill passes validation. The code comment at this site explicitly documents `outgoing_calls` as the cycle-check edge set (and even added the `Return(Call)` arm for exactly that reason), so the branch-nested omission is an incompleteness in that safety check, not a deliberate exclusion.

## Trigger / Reproduction

Define a block with a recursive call inside a branch body:

```
block a()
    flow:
        if some-condition
            call a
```

Run `glyph check` or `glyph compile`. No `G::validate::recursive-call` error is raised despite the genuine cycle.

## Evidence

```rust
let outgoing_calls: Vec<String> = block
    .flow
    .iter()
    .filter_map(|stmt| match stmt {
        FlowStmt::Call { target, .. } => Some(target.node.clone()),
        FlowStmt::Return(ReturnExpr::Call { target, .. }) => Some(target.node.clone()),
        _ => None,
    })
    .collect();
```

The `_ => None` arm silently drops `FlowStmt::Branch` variants without descending into their `then_body`, `elif_branches[*].body`, or `else_body` fields (all `Vec<FlowStmt>`).

## Recommended Resolution

Recursively walk branch `then`/elif/else bodies (and any nested branches) when collecting `outgoing_calls` so call edges inside conditional arms participate in cycle detection. Implement a small recursive helper over `FlowStmt` that descends into `Branch` bodies and also covers `Return(Call)` within them, replacing the current flat `filter_map`.

## Verification Notes

The code at lower.rs lines 867-875 builds `outgoing_calls` with a flat `.iter().filter_map()` over `block.flow` that only matches top-level `FlowStmt::Call` and `FlowStmt::Return(ReturnExpr::Call)` — the `FlowStmt::Branch` variant falls through to `_ => None`. Since `FlowStmt::Branch` has `then_body: Vec<FlowStmt>`, `elif_branches[*].body: Vec<FlowStmt>`, and `else_body: Option<Vec<FlowStmt>>` which can each contain `FlowStmt::Call` entries, any calls nested inside branch bodies are silently omitted from `outgoing_calls`. `validate.rs` line 92 builds its DFS adjacency map exclusively from `b.outgoing_calls`, so branch-nested calls never appear as edges and cycles through them are missed. No existing test exercises this path — all cycle-detection tests use only top-level flow statements.
