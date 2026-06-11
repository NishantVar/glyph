# BUG-002: infer_effects_for_skill ignores calls inside branch bodies and return-position calls, defeating effect under-declaration checks

**Severity:** high | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/analyze.rs:6042-6094`
**Found by:** analyze-3 | **Audit date:** unknown-date

## Description

`infer_effects_for_skill` only walks top-level `FlowStmt::Call` nodes. The seed worklist (lines 6051–6058) filters `skill.flow` for `FlowStmt::Call` and skips `FlowStmt::Return(ReturnExpr::Call)`. When expanding a referenced block it iterates only the block's top-level `FlowStmt::Call` (lines 6072–6076). Neither pass descends into `FlowStmt::Branch` then_body / elif / else bodies.

The returned `inferred` set drives three diagnostics: `G::analyze::effects-under-declared` (Error), `effects-over-declared` (Warning), and `missing-effects` (Repairable). A skill that declares `effects: none` but whose only effectful call is inside a branch body will have an empty `inferred` set, so the `effects-under-declared` error never fires even though the skill triggers the effect. The same omission applies to a terminal `return dangerous()` at flow root.

This silently passes validation for an under-declared effect set — the entire purpose of the effects gate. It also diverges from the sibling walker `infer_effects_for_flow` (lines 7524–7569, used by `glyph fmt`) which correctly recurses into branches and handles `Return(Call)`, so `analyze` and `glyph fmt` disagree about a skill's effects.

## Trigger / Reproduction

A skill declaring `effects: none` with an effectful `block dangerous()` call inside an `if` branch:

```
block dangerous()
    effects: writes_fs
    flow:
        // ... effectful operations

skill my_skill()
    effects: none
    flow:
        if some_condition:
            dangerous()
```

Running `cargo run -p glyph-cli -- check --enable-effects` emits no `effects-under-declared` error. Moving the `dangerous()` call to the top level of the skill flow correctly fires `G::analyze::effects-under-declared`.

## Evidence

```rust
// Seed worklist — flat filter, Branch and Return(Call) silently dropped:
let mut worklist: Vec<String> = skill.flow
    .iter()
    .filter_map(|stmt| match stmt {
        FlowStmt::Call { target, .. } => Some(target.node.clone()),
        _ => None   // FlowStmt::Branch and FlowStmt::Return(ReturnExpr::Call) ignored
    })
    .collect();

// Block-expansion inner loop — same flat-only walk:
for stmt in &block.flow {
    if let FlowStmt::Call { target: inner, .. } = stmt {
        worklist.push(inner.node.clone());
        // FlowStmt::Branch nested calls never reached
    }
}
```

## Recommended Resolution

Replace the top-level-only collection with a recursive flow walker mirroring `infer_effects_for_flow` at lines 7524–7569. The recursive walker must:

1. Descend into `FlowStmt::Branch` `then_body`, `elif_branches`, and `else_body`.
2. Match `FlowStmt::Return(ReturnExpr::Call { target, .. })` and add `target.node` to the worklist.

Apply the same recursion both to the skill's own flow when seeding the worklist and to each referenced block's flow when expanding transitive calls.

## Verification Notes

Confirmed end-to-end by running `cargo run -p glyph-cli -- check --enable-effects` on a skill with an effectful call inside a branch — zero `effects-under-declared` diagnostics emitted. Same skill with the call at top level correctly fires the error. Code trace confirms the flat `filter_map` at lines 6051–6058. The sibling `infer_effects_for_flow` uses a recursive `walk()` that correctly handles all three cases, confirming the divergence is real and `analyze` is the defective pass.

## Independent Agent Finding

### Verdict

Confirmed.

### Reproduction/Refutation

Created targeted scratch fixtures under `tmp/` for a branch-nested call, a top-level control call, and a return-position call. Ran:

- `cargo run -q -p glyph-cli -- check --enable-effects --format json tmp/bug002_branch.glyph`
- `cargo run -q -p glyph-cli -- check --enable-effects --format json tmp/bug002_top_level.glyph`
- `cargo run -q -p glyph-cli -- check --enable-effects --format json tmp/bug002_return.glyph`

The branch-nested and return-position fixtures both exited `0` with no diagnostics even though each called a `block dangerous()` declaring `effects: writes_fs` from a `skill` declaring `effects: none`. The top-level control exited `1` and emitted `G::analyze::effects-under-declared` with message `` `effects: none` declared but call graph infers: writes_fs ``.

### Evidence

Graphify located the relevant implementation at `crates/glyph-core/src/analyze.rs:6042`. A bounded source read confirmed `infer_effects_for_skill` seeds its worklist with only `FlowStmt::Call` and expands block flows with only top-level `FlowStmt::Call`. A bounded read of `infer_effects_for_flow` confirmed the sibling walker recurses into `FlowStmt::Branch` bodies and handles `FlowStmt::Return(ReturnExpr::Call { .. })`, matching the report's claimed divergence.

### Resolution Input

Use a shared recursive call-target collector for both skill-flow seeding and block-flow expansion in `infer_effects_for_skill`. It should visit branch `then`, `elif`, and `else` bodies and include `ReturnExpr::Call` targets, then preserve the existing worklist/visited behavior for transitive block effects and stdlib effects.
