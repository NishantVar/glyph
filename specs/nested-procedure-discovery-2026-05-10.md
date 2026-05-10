# Nested-procedure discovery — design spec

Date: 2026-05-10
Status: draft, awaiting user review
Owner: Nishant
Related code: `crates/glyph-core/src/emit/scaffold.rs` (`collect_tier2_targets`, ~L285)
Related test crate: `crates/glyph-cli/tests/tier2_procedure.rs`
Earlier partial fix: commit `b8aefad` ("core: recognize procedure references inside lettered sub-steps", 2026-05-08)

## Problem

When a multi-step block (Tier 2) is reachable only by going **two or more hops** through other blocks, the compiler emits the call-site reference text but never emits the corresponding `### Procedure: <name>` section. The reference dangles and the body is lost in the round-trip.

### Example A — works today (one hop)

```
skill foo()
    flow:
        if bar.applies()
            bar()
        else
            "do nothing"

block bar()
    flow:
        step_one()
        step_two()
        step_three()
        step_four()
```

Compiles correctly: `### Procedure: bar` is emitted.

### Example B — still broken (two hops)

```
skill foo()
    flow:
        outer()

block outer()
    flow:
        if bar.applies()
            bar()
        else
            "do nothing"

block bar()
    flow:
        step_one()
        step_two()
        step_three()
        step_four()
```

Compiles to: `### Procedure: outer` ✅, `outer` body says `a. Follow the bar procedure.` ⚠️, no `### Procedure: bar` section ❌.

The fork-terminal repro originally filed against the compiler is exactly Example B's shape: skill → `dispatch_new_fork` → `execute_fork_with_plan`.

## Root cause

`collect_tier2_targets` at `crates/glyph-core/src/emit/scaffold.rs:285` walks the **skill's** flow tree (recursing through `Branch.then_body`, `elif_branches`, `else_body`) and seeds `procedure_seen` / `procedure_order` with every Tier-2 target it finds along the way. It never opens up a discovered procedure's own body to see whether *that* block also calls Tier-2 blocks. Discovery stops at the skill-flow boundary; the May 8 fix (`b8aefad`) only added recursion through the skill's own branch arms, not transitive walking across block boundaries.

The branch-arm renderer at `crates/glyph-core/src/emit/branch.rs:319` emits `Follow the <kebab> procedure.` for any Tier-2 IrCall it encounters — but emitting that text does not register the callee in `procedure_order`. The procedure-emission loop at `scaffold.rs:678` walks `procedure_order`, so missed targets stay missed.

## Fix design — transitive-closure procedure discovery

### Algorithm

Replace the one-shot `collect_tier2_targets(&skill.steps, …)` call at `scaffold.rs:444` with a worklist BFS:

1. **Pre-compute lookup maps** off `arena.nodes()` once:
   - `target_to_tier: HashMap<String, u8>` — every IrCall's target → its `projection_tier`. Tier is a function of the callee block, so any single call site is authoritative for the IrCall-reachable subset. **Construction rule:** insert only calls whose `projection_tier` is `Some(tier)`; if the same target is seen with multiple tiers (shouldn't happen — expand keeps tiers consistent per target — but be robust), prefer `2`. Skips ungated `None` entries cleanly.
   - `blocks_by_name: HashMap<&str, &IrBlock>` — for body walking.
2. **Seed**: walk `skill.steps` with the existing recursion through `Branch` nodes, recording every `IrCall` whose `projection_tier == Some(2)` via the `record(name)` helper below.
3. **Drain the worklist**: for each name dequeued, look up its `IrBlock`. If absent (imported / cross-file), `continue` — the existing library-procedures path handles those.  Otherwise:
   - Sort `block.branch_steps` by its `usize` key (the original `flow_statements` index) before walking. The field is `HashMap<usize, NodeId>`, so raw iteration is nondeterministic — sorting preserves source order and gives deterministic output.
   - For each branch in source order, run the same skill-flow walker on the IrBranch (recurses through `then_body` / `elif_branches` / `else_body`).
   - Walk `block.outgoing_calls` (top-level call edges); for each name, run `classifies_as_tier2` (helper below) and `record` if it returns `true`.
4. **Cycle safety**: the existing `seen: HashSet<String>` short-circuits re-insertion. Mutual recursion between two Tier-2 blocks is therefore safe.
5. **Order**: BFS appending into `Vec<String>` produces parent-before-child, so the rendered file shows `outer` above `bar`.

### Helper

```rust
fn record(
    name: &str,
    seen: &mut HashSet<String>,
    order: &mut Vec<String>,
    queue: &mut VecDeque<String>,
) {
    if seen.insert(name.to_string()) {
        order.push(name.to_string());
        queue.push_back(name.to_string());
    }
}

/// Emit-time approximation for block-only outgoing edges, matching the expand
/// criteria available from `IrBlock` metadata. Block top-level calls do not
/// become `IrCall` nodes (they live as `outgoing_calls` strings + the
/// `"call <name>"` placeholder in `flow_statements`), so `target_to_tier`
/// gives a false negative for any block reached only via top-level outgoing
/// edges. The structural legs of expand's Tier-2 rule
/// (`stmt_count >= 4 || has_branches || wc >= 150`) are derivable from
/// `IrBlock` metadata and are checked here. The `freq >= 2` leg is
/// intentionally omitted: `freq` counts `IrCall` nodes, so if it would have
/// fired, `target_to_tier` already carries the entry.
fn classifies_as_tier2(
    name: &str,
    target_to_tier: &HashMap<String, u8>,
    blocks_by_name: &HashMap<&str, &IrBlock>,
) -> bool {
    if target_to_tier.get(name) == Some(&2) {
        return true;
    }
    let Some(b) = blocks_by_name.get(name) else { return false; };
    let stmt_count = b.flow_statements.len();
    let has_branches = !b.branch_steps.is_empty();
    let wc = b.resolved_word_count.unwrap_or(0) as usize;
    stmt_count >= 4 || has_branches || wc >= 150
}
```

### Files touched

- `crates/glyph-core/src/emit/scaffold.rs` — production change (one file).
- `crates/glyph-cli/tests/tier2_procedure.rs:78` — new assertion exercising the two-hop case end-to-end.
- `crates/glyph-cli/tests/corpus/valid/nested_branch_only_procedure.glyph` — new fixture sibling to the existing `branch_only_procedure.glyph`.

Estimated diff: ~50 lines of Rust + one fixture + one test.

## Test plan

Fixture (Example B's shape):

```
skill foo()
    flow:
        outer()

block outer()
    description: "Outer wrapper that branches into bar."
    flow:
        if bar.applies()
            bar()
        else
            "do nothing"

block bar()
    description: "Multi-step inner work."
    flow:
        step_one_text
        step_two_text
        step_three_text
        step_four_text

const step_one_text   = "First inner step."
const step_two_text   = "Second inner step."
const step_three_text = "Third inner step."
const step_four_text  = "Fourth inner step."
```

Assertions in `tier2_procedure.rs`:

1. The compiled `.md` contains `### Procedure: outer` exactly once.
2. The compiled `.md` contains `### Procedure: bar` exactly once.
3. The byte offset of `### Procedure: outer` is **less than** the byte offset of `### Procedure: bar` (parent-before-child ordering).
4. The slice between `### Procedure: bar` and the next `### Procedure:` (or EOF) contains four numbered top-level steps (`1.`–`4.`) matching `bar`'s flow text. (Numbering, not lettering — lettering only applies when a body is rendered inside a branch arm of another procedure; `bar`'s own `### Procedure: bar` section renders its flow as numbered steps directly.)
5. The `outer` procedure body still contains `Follow the bar procedure.` (no regression).

## Verification

Per project CLAUDE.md scope rules, this is single-file private logic. Run:

```
cargo fmt
cargo check -p glyph-core
cargo nextest run -p glyph-core
cargo nextest run -p glyph-cli      # NB: required — -p glyph-core does NOT pick up the integration test
```

## Out of scope / known follow-up gap

The procedure body emitter at `crates/glyph-core/src/emit/scaffold.rs:698` renders block top-level steps from raw `flow_statements` strings. For a Tier-2 callee reached only via a block's top-level `outgoing_calls`, this fix correctly emits `### Procedure: bar`, but the parent block's body will still contain the raw `"call bar"` placeholder string from `flow_statements` rather than `Follow the bar procedure.` The existing fork-terminal-style repro and the new Example-B fixture both exercise the **branch-arm** path (which goes through `branch::emit_to_scaffold` → `emit_lettered_substeps` and renders correctly), so they exit clean.

Renderer-side fix for top-level Tier-2 calls inside block flows is a separate change to the procedure-emission loop in `scaffold.rs` and is **not** addressed by this spec. File a follow-up issue if a real-world fixture exposes it.

## Approvals

- [ ] Design reviewed and approved (user)
- [ ] Implementation plan generated via `superpowers:writing-plans`
