# BUG-058: emit_procedure (Tier 3) drops the §8.4 return sentence when the last flow item is a Branch

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/emit/mod.rs:291-301`
**Found by:** emit-core | **Audit date:** unknown-date

## Description

In `emit_procedure` the `Branch` arm of the flow loop ignores `is_last` and the computed `return_sentence`: it calls `branch::emit_to_scaffold` and nothing else. The sibling Tier-2 emitter (`scaffold.rs::build`, lines 982-989) explicitly handles this case — when the final visible step is a branch and a return contract exists, it emits a trailing standalone numbered step `{step_num + 1}. {sentence}`.

The design contracts Tier-2 and Tier-3 procedure projections as byte-identical (`docs/architecture/ir-semantics.md`: "byte-identical Tier 2 / Tier 3 shape"). So an export block whose flow ends in a branch and which declares a `return`/`-> Type` contract renders WITH the §8.4 sentence when inlined (Tier 2) but WITHOUT it when externalized to its own `.md` file (Tier 3) — the return contract is silently lost on disk.

The bug is currently latent: the only production call site in `lib.rs:2466-2471` synthesizes only `IrBlockFlowItem::Inline` items. However, the `Branch` arm is live code in a public function and a maintainer wiring branch lowering into the library path would hit the silent drop.

## Trigger / Reproduction

Wire `IrBlockFlowItem::Branch` into the Tier-3 path (e.g. extend `lib.rs` to pass branch items to `emit_procedure`) for a block whose flow ends on a branch and which declares a return contract. The Tier-3 `.md` file will be missing the `{n}. Produce …` step present in the Tier-2 inline rendering.

## Evidence

```rust
// emit/mod.rs Branch arm — no is_last / return_sentence handling:
crate::ir::IrBlockFlowItem::Branch { node_id } => {
    if let crate::ir::IrNode::Branch(br) = arena.get(*node_id) {
        branch::emit_to_scaffold(&mut scaffold, arena, br, step_num, &mut next_span_id);
    }
}

// vs scaffold.rs:985 — Tier-2 sibling does append the return sentence:
// if is_last { if let Some(sent) = proc_sentence... s.push_literal(format!("{}. {}\n", step_num + 1, sent)); }
```

## Recommended Resolution

Mirror the Tier-2 path: after `branch::emit_to_scaffold`, when `is_last` is `true` and `return_sentence` is `Some`, push a trailing `{step_num + 1}. {sentence}\n` literal into the local scaffold:

```rust
crate::ir::IrBlockFlowItem::Branch { node_id } => {
    if let crate::ir::IrNode::Branch(br) = arena.get(*node_id) {
        branch::emit_to_scaffold(&mut scaffold, arena, br, step_num, &mut next_span_id);
    }
    if is_last {
        if let Some(sent) = return_sentence.as_deref() {
            scaffold.push_literal(format!("{}. {}\n", step_num + 1, sent));
        }
    }
}
```

## Verification Notes

The code divergence is confirmed: `mod.rs` `Branch` arm calls `emit_to_scaffold` with no `is_last`/`return_sentence` check; `scaffold.rs` Tier-2 emitter lines 985-989 explicitly appends the trailing step when `is_last`. The `docs/architecture/ir-semantics.md` line 103 contracts "byte-identical Tier 2 / Tier 3 shape", making this a genuine contract violation. The bug is latent in production because `lib.rs:2466-2471` currently synthesizes only `IrBlockFlowItem::Inline` items, but the Branch arm is live public code. The proposed fix is correct and complete.

## Independent Agent Finding

**Verdict:** Reproduced.

**Reproduction/Refutation:** I created a temporary standalone Cargo harness under `tmp/bug058-repro` that calls the public `glyph_core::emit::emit_procedure` API with a single synthetic `IrBlockFlowItem::Branch` and `OutputTargetForm::Identifier("current_branch")`. The harness expected the Tier-3 procedure output to include the trailing §8.4 sentence ``2. Produce `current_branch`.`` after the branch step. It instead emitted only the branch opener and reported the expected return sentence missing. The temporary harness was removed after capture.

**Evidence:**

- Graphify located `emit_procedure()` at `crates/glyph-core/src/emit/mod.rs:93` and the branch rendering path at `crates/glyph-core/src/emit/branch.rs`.
- Bounded source reads confirmed the current Tier-3 `Branch` arm at `crates/glyph-core/src/emit/mod.rs:291-301` still only calls `branch::emit_to_scaffold(...)`; it does not check `is_last` or append `return_sentence`.
- The sibling Tier-2 path at `crates/glyph-core/src/emit/scaffold.rs:973-989` still appends `format!("{}. {}\n", step_num + 1, sent)` when the final visible item is a branch.
- `crates/glyph-core/src/lib.rs:2462-2472` still synthesizes only `IrBlockFlowItem::Inline` items for the current production call site, so the bug remains latent through that route.
- Command run:

```console
$ cargo run --quiet --manifest-path tmp/bug058-repro/Cargo.toml
--- emitted procedure ---
---
name: helper
kind: procedure
description: Returns the branch.
---

## Steps

1. Decide whether the change needs review applies and, if so:
--- end ---
REPRODUCED: missing expected trailing return sentence: "2. Produce `current_branch`.\n"
```

**Resolution Input:** Preserve the existing recommended resolution. Mirror the Tier-2 branch handling in `emit_procedure`: after `branch::emit_to_scaffold`, if `is_last` and `return_sentence` is `Some`, push a trailing `{step_num + 1}. {sentence}\n` literal into the local scaffold. A focused regression test should construct a branch-backed `IrBlockFlowItem` as the final visible procedure item and assert that the emitted Tier-3 markdown includes the standalone §8.4 return sentence.
