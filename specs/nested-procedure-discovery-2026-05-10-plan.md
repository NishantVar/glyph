# Nested-procedure discovery fix — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Glyph compiler emit `### Procedure: <name>` sections for Tier-2 blocks reachable through two-or-more hops (e.g. `skill → outer → bar`), not just one. Today the call-site reference renders but the body is silently dropped.

**Architecture:** Replace the one-shot recursion in `collect_tier2_targets` with a worklist BFS that, after seeding from `skill.steps`, drains a queue of discovered Tier-2 procedure names — opening each procedure's `branch_steps` and `outgoing_calls` to find further Tier-2 callees. Cycle-safe via `HashSet`; deterministic via sorted iteration of `HashMap<usize, NodeId>` branch maps; correct for block top-level outgoing edges via a metadata-based tier classifier (block top-level calls don't lower to `IrCall` so the IrCall-tier map gives false negatives there).

**Tech Stack:** Rust 2021, `cargo nextest`, single production file `crates/glyph-core/src/emit/scaffold.rs`, integration test in `crates/glyph-cli/tests/tier2_procedure.rs`, fixture in `crates/glyph-cli/tests/corpus/valid/`.

**Spec:** [`nested-procedure-discovery-2026-05-10.md`](./nested-procedure-discovery-2026-05-10.md)

---

## File Structure

| Path | Action | Responsibility |
|------|--------|----------------|
| `crates/glyph-cli/tests/corpus/valid/nested_branch_only_procedure.glyph` | Create | New fixture: skill → `outer` (1-hop, branch arm) → `bar` (2-hop, multi-step). Sibling to existing `branch_only_procedure.glyph`. |
| `crates/glyph-cli/tests/tier2_procedure.rs` | Modify (append a new test fn) | Adds `two_hop_nested_tier2_emits_inner_procedure` with the 5 ordered assertions from the spec. |
| `crates/glyph-core/src/emit/scaffold.rs` | Modify (one production file) | Replace `collect_tier2_targets` body + the call site at L444 with worklist-BFS discovery; add `record` and `classifies_as_tier2` helpers; extend imports. |

No other files touched. Existing fixtures `branch_only_procedure.glyph` and `explicit_blocks.glyph` continue to exercise the one-hop and Tier-2/Tier-1 split paths and must keep passing (regression).

---

## Task 1: Add failing fixture + integration test (RED)

**Files:**
- Create: `crates/glyph-cli/tests/corpus/valid/nested_branch_only_procedure.glyph`
- Modify: `crates/glyph-cli/tests/tier2_procedure.rs:91` (append a new `#[test]` after the existing `branch_only_tier2_emits_procedure_section` function)

- [ ] **Step 1.1: Create the fixture file**

Path: `crates/glyph-cli/tests/corpus/valid/nested_branch_only_procedure.glyph`

```
skill foo()
    description: "Two-hop nested procedure discovery test."
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

Why this shape:
- `outer` has a branch arm calling `bar`, so `bar` only becomes reachable by walking *into* `outer`'s body (the bug today).
- `bar` has 4 single-statement flows → satisfies expand's `stmt_count >= 4` Tier-2 rule.
- Const-resolved single-statement bodies in `bar` give predictable, lint-stable text for the assertions.
- `bar.applies()` is the same idiom used by the fork-terminal repro; if your environment surfaces a parse error here, fall back to a free condition (`if condition` or `if some_flag == "go"`) — the structural bug is independent of the predicate shape.

- [ ] **Step 1.2: Append the failing test function**

Path: `crates/glyph-cli/tests/tier2_procedure.rs` — append AFTER the existing `branch_only_tier2_emits_procedure_section` function (so the file ends with the new test):

```rust
#[test]
fn two_hop_nested_tier2_emits_inner_procedure() {
    let output = compile_fixture("nested_branch_only_procedure");

    // (1) Outer procedure section is present exactly once.
    let outer_count = output.matches("### Procedure: outer").count();
    assert_eq!(
        outer_count, 1,
        "expected `### Procedure: outer` exactly once, got {outer_count}; output:\n{output}"
    );

    // (2) Inner (two-hop) procedure section is present exactly once.
    let bar_count = output.matches("### Procedure: bar").count();
    assert_eq!(
        bar_count, 1,
        "expected `### Procedure: bar` exactly once, got {bar_count}; output:\n{output}"
    );

    // (3) Parent-before-child ordering: outer header appears before bar header.
    let outer_idx = output
        .find("### Procedure: outer")
        .expect("outer header missing");
    let bar_idx = output
        .find("### Procedure: bar")
        .expect("bar header missing");
    assert!(
        outer_idx < bar_idx,
        "expected `### Procedure: outer` to appear before `### Procedure: bar` (parent-before-child); outer_idx={outer_idx} bar_idx={bar_idx}"
    );

    // (4) `bar`'s section contains four numbered top-level steps (1.–4.).
    // Numbering, not lettering — lettering only applies inside branch arms of
    // another procedure; `bar`'s own section renders its flow as numbered steps.
    let bar_section = {
        let after_header = &output[bar_idx + "### Procedure: bar".len()..];
        let next = after_header
            .find("### Procedure:")
            .unwrap_or(after_header.len());
        &after_header[..next]
    };
    for n in 1..=4 {
        let needle = format!("{n}. ");
        assert!(
            bar_section.contains(&needle),
            "expected `bar` procedure body to contain numbered step `{needle}`; section was:\n{bar_section}"
        );
    }

    // (5) No regression: `outer`'s body still references `bar` as a procedure.
    assert!(
        output.contains("Follow the bar procedure"),
        "expected `outer` body to keep its `Follow the bar procedure.` reference; output:\n{output}"
    );
}
```

- [ ] **Step 1.3: Run the new test and confirm it fails for the right reason**

Run: `cargo nextest run -p glyph-cli two_hop_nested_tier2_emits_inner_procedure`

Expected: FAIL on assertion **(2)** — `expected \`### Procedure: bar\` exactly once, got 0`. (Assertions (1) and (5) should pass even without the fix; (2) is the load-bearing one.)

If the fixture itself fails to compile (e.g. parse/analyze error on `bar.applies()`), swap the predicate to `if condition` and re-run. The structural assertion shape is unchanged.

- [ ] **Step 1.4: Commit the RED checkpoint**

```bash
git add crates/glyph-cli/tests/corpus/valid/nested_branch_only_procedure.glyph crates/glyph-cli/tests/tier2_procedure.rs
git commit -m "test: add failing fixture for two-hop tier-2 procedure discovery"
```

Note: per project convention (CLAUDE.md), do NOT add `Co-Authored-By` trailers.

---

## Task 2: Implement worklist BFS in scaffold.rs (GREEN)

**Files:**
- Modify: `crates/glyph-core/src/emit/scaffold.rs:6-12` (extend `use` declarations)
- Modify: `crates/glyph-core/src/emit/scaffold.rs:285-310` (extend `collect_tier2_targets` signature with `queue` parameter)
- Modify: `crates/glyph-core/src/emit/scaffold.rs:444` (replace one-shot call with worklist BFS body; add the two helper functions adjacent to `collect_tier2_targets`)

- [ ] **Step 2.1: Extend imports**

Replace `crates/glyph-core/src/emit/scaffold.rs` lines 6-12:

```rust
use super::templates;
use crate::ir::{
    BranchPredicateShape, IrArena, IrBlock, IrCall, IrNode, LocalRef, NodeId, OutputTargetForm,
};
use crate::slot;
use std::collections::BTreeMap;
use std::collections::HashSet;
```

with:

```rust
use super::templates;
use crate::ir::{
    BranchPredicateShape, IrArena, IrBlock, IrCall, IrNode, LocalRef, NodeId, OutputTargetForm,
};
use crate::slot;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
```

(Adds `IrBlock`, `HashMap`, `VecDeque` — needed by the BFS and helpers below. `IrBlock` may already be unused elsewhere in this file; the import is needed for the helper signatures.)

- [ ] **Step 2.2: Replace `collect_tier2_targets` and add the two helpers**

Replace `crates/glyph-core/src/emit/scaffold.rs` lines 285-310 (the existing `collect_tier2_targets` function):

```rust
fn collect_tier2_targets(
    nodes: &[NodeId],
    arena: &IrArena,
    seen: &mut HashSet<String>,
    order: &mut Vec<String>,
) {
    for nid in nodes {
        match arena.get(*nid) {
            IrNode::Call(c) if c.projection_tier == Some(2) => {
                if seen.insert(c.target.clone()) {
                    order.push(c.target.clone());
                }
            }
            IrNode::Branch(b) => {
                collect_tier2_targets(&b.then_body, arena, seen, order);
                for elif in &b.elif_branches {
                    collect_tier2_targets(&elif.body, arena, seen, order);
                }
                if let Some(else_body) = &b.else_body {
                    collect_tier2_targets(else_body, arena, seen, order);
                }
            }
            _ => {}
        }
    }
}
```

with this revised walker (signature gains `queue`; recording is funneled through `record`):

```rust
/// Walk a slice of `NodeId`s and record any Tier-2 `IrCall.target` reached by
/// recursing through `Branch.then_body` / `elif_branches` / `else_body`. Used
/// both as the seed (over `skill.steps`) and as the per-procedure expansion
/// step (over a discovered block's `branch_steps`) of the worklist BFS in
/// `build()`.
fn collect_tier2_targets(
    nodes: &[NodeId],
    arena: &IrArena,
    seen: &mut HashSet<String>,
    order: &mut Vec<String>,
    queue: &mut VecDeque<String>,
) {
    for nid in nodes {
        match arena.get(*nid) {
            IrNode::Call(c) if c.projection_tier == Some(2) => {
                record(&c.target, seen, order, queue);
            }
            IrNode::Branch(b) => {
                collect_tier2_targets(&b.then_body, arena, seen, order, queue);
                for elif in &b.elif_branches {
                    collect_tier2_targets(&elif.body, arena, seen, order, queue);
                }
                if let Some(else_body) = &b.else_body {
                    collect_tier2_targets(else_body, arena, seen, order, queue);
                }
            }
            _ => {}
        }
    }
}

/// BFS bookkeeping: register a newly-discovered Tier-2 procedure name into
/// `seen` (for cycle safety), `order` (for parent-before-child render order),
/// and `queue` (for transitive expansion). Idempotent on already-seen names.
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
    let Some(b) = blocks_by_name.get(name) else {
        return false;
    };
    let stmt_count = b.flow_statements.len();
    let has_branches = !b.branch_steps.is_empty();
    let wc = b.resolved_word_count.unwrap_or(0) as usize;
    stmt_count >= 4 || has_branches || wc >= 150
}
```

- [ ] **Step 2.3: Replace the seed call with the worklist BFS**

Find this block in `crates/glyph-core/src/emit/scaffold.rs` (around line 442-444):

```rust
    // ### Steps
    let mut procedure_order: Vec<String> = Vec::new();
    let mut procedure_seen: HashSet<String> = HashSet::new();
    collect_tier2_targets(&skill.steps, arena, &mut procedure_seen, &mut procedure_order);
```

Replace with:

```rust
    // ### Steps
    //
    // Procedure discovery (Tier 2) is a transitive closure: a procedure
    // reachable only by walking through another procedure's body must still
    // get its `### Procedure: <name>` section emitted, otherwise the call-site
    // `Follow the <X> procedure.` reference dangles. We seed from `skill.steps`
    // and then drain a queue, opening each discovered procedure's
    // `branch_steps` (structural branches) and `outgoing_calls` (top-level
    // call edges) to find further Tier-2 callees. Cycle-safe via `seen`.
    // See specs/nested-procedure-discovery-2026-05-10.md.
    let mut procedure_order: Vec<String> = Vec::new();
    let mut procedure_seen: HashSet<String> = HashSet::new();
    let mut procedure_queue: VecDeque<String> = VecDeque::new();

    // Pre-compute lookup maps off `arena.nodes()` once.
    // - `target_to_tier` is authoritative for any callee reached via an
    //   `IrCall` (skill flow + branch arms); insert only `Some(tier)` entries
    //   and prefer `2` if duplicates ever appear (expand keeps tiers
    //   consistent per target — this just makes the map robust).
    // - `blocks_by_name` lets the BFS open a discovered procedure's body.
    let mut target_to_tier: HashMap<String, u8> = HashMap::new();
    let mut blocks_by_name: HashMap<&str, &IrBlock> = HashMap::new();
    for node in arena.nodes() {
        match node {
            IrNode::Call(c) => {
                if let Some(tier) = c.projection_tier {
                    let entry = target_to_tier.entry(c.target.clone()).or_insert(tier);
                    if tier == 2 {
                        *entry = 2;
                    }
                }
            }
            IrNode::Block(b) => {
                blocks_by_name.insert(b.name.as_str(), b);
            }
            _ => {}
        }
    }

    // Seed: walk skill.steps with the existing recursion through Branch nodes.
    collect_tier2_targets(
        &skill.steps,
        arena,
        &mut procedure_seen,
        &mut procedure_order,
        &mut procedure_queue,
    );

    // Drain the worklist: open each discovered procedure's body and discover
    // further Tier-2 callees transitively.
    while let Some(name) = procedure_queue.pop_front() {
        let Some(block) = blocks_by_name.get(name.as_str()).copied() else {
            // Imported / cross-file block — the existing library-procedures
            // path handles those separately. Skip.
            continue;
        };

        // Sort branch_steps by usize key (original flow_statements index)
        // before walking. The field is `HashMap<usize, NodeId>`, so raw
        // iteration is nondeterministic — sorting preserves source order
        // and gives deterministic procedure_order output.
        let mut indexed: Vec<(usize, NodeId)> = block
            .branch_steps
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect();
        indexed.sort_by_key(|(idx, _)| *idx);
        let sorted_branch_ids: Vec<NodeId> =
            indexed.into_iter().map(|(_, v)| v).collect();
        collect_tier2_targets(
            &sorted_branch_ids,
            arena,
            &mut procedure_seen,
            &mut procedure_order,
            &mut procedure_queue,
        );

        // Walk top-level outgoing_calls: these are block-level call edges
        // that DO NOT become IrCall nodes (they live as `outgoing_calls`
        // strings + the `"call <name>"` placeholder in `flow_statements`),
        // so target_to_tier alone misses them. Use the metadata-based
        // classifier as a fallback.
        for callee in &block.outgoing_calls {
            if classifies_as_tier2(callee, &target_to_tier, &blocks_by_name) {
                record(
                    callee,
                    &mut procedure_seen,
                    &mut procedure_order,
                    &mut procedure_queue,
                );
            }
        }
    }
```

- [ ] **Step 2.4: Format and type-check**

Run in parallel:
```bash
cargo fmt
cargo check -p glyph-core
```

Expected: `cargo check` succeeds with no errors. Warnings about unused `HashMap`/`VecDeque` would indicate the BFS body wasn't pasted into `build()`; warnings about unused `IrBlock` would indicate the helper signatures didn't take effect.

- [ ] **Step 2.5: Run the new test and confirm GREEN**

Run: `cargo nextest run -p glyph-cli two_hop_nested_tier2_emits_inner_procedure`

Expected: PASS.

If still failing on assertion (2), inspect the compiled output by hand:
```bash
cargo run -p glyph-cli -- compile crates/glyph-cli/tests/corpus/valid/nested_branch_only_procedure.glyph
cat crates/glyph-cli/tests/corpus/valid/nested_branch_only_procedure.md
```
Confirm `### Procedure: outer` and `### Procedure: bar` are both present, with `outer` first.

- [ ] **Step 2.6: Run the full regression suite**

Per spec verification section + project CLAUDE.md ("verification scales with blast radius" — this is single-file private logic so the targeted set is sufficient, but `glyph-cli` integration tests are required since `-p glyph-core` does not pick them up):

```bash
cargo nextest run -p glyph-core
cargo nextest run -p glyph-cli
```

Expected: ALL PASS, including the existing `branch_only_tier2_emits_procedure_section` and `explicit_blocks_tier2_projection` tests (these exercise the one-hop and Tier-2/Tier-1-split paths and must keep passing).

If a previously-passing fixture's compiled `.md` now contains a NEW `### Procedure:` section that wasn't there before, that's likely a real bug-fix side-effect (the fix may discover legitimate Tier-2 procedures that were silently dropped). Inspect the diff: if the new section's body matches the corresponding source `block`, update the fixture's `.md` (those `.md` files in `tests/corpus/valid/` are the recorded output and they're regenerated by `compile_fixture`, but if any test asserts against a snapshot it'll need updating). If the new section is for a block that *shouldn't* be Tier-2, that's a real regression — investigate `classifies_as_tier2`.

- [ ] **Step 2.7: Commit the GREEN implementation**

```bash
git add crates/glyph-core/src/emit/scaffold.rs
git commit -m "core: discover tier-2 procedures transitively via worklist BFS

Replaces the one-shot collect_tier2_targets walk over skill.steps with a
worklist that drains discovered procedure bodies (branch_steps +
outgoing_calls) for further Tier-2 callees. Fixes the dangling
\`Follow the <X> procedure.\` reference that occurred when a multi-step
block was reachable only via two-or-more hops (skill -> A -> B).

Block top-level outgoing_calls don't lower to IrCall nodes, so a metadata-
based classifier (stmt_count >= 4 || has_branches || wc >= 150) is used
as the fallback tier check for those edges.

See specs/nested-procedure-discovery-2026-05-10.md."
```

Note: per project convention (CLAUDE.md), do NOT add `Co-Authored-By` trailers.

---

## Out of scope (per spec)

The procedure body emitter at `crates/glyph-core/src/emit/scaffold.rs:698` renders block top-level steps from raw `flow_statements` strings. For a Tier-2 callee reached only via a block's top-level `outgoing_calls`, this fix correctly emits `### Procedure: <name>`, but the parent block's body will still show the raw `"call <name>"` placeholder string rather than `Follow the <X> procedure.`

Both the fork-terminal repro and the new Example-B fixture exercise the **branch-arm** path (which goes through `branch::emit_to_scaffold` → `emit_lettered_substeps` and renders correctly), so they exit clean. A follow-up issue should be filed if a real-world fixture exposes the renderer-side gap. Do not address it in this plan.

---

## Done criteria

- `cargo nextest run -p glyph-cli two_hop_nested_tier2_emits_inner_procedure` passes.
- `cargo nextest run -p glyph-core` passes (no regressions).
- `cargo nextest run -p glyph-cli` passes (no regressions; in particular, `branch_only_tier2_emits_procedure_section` and `explicit_blocks_tier2_projection` still green).
- Two commits on the branch: the RED test commit and the GREEN implementation commit.
- The compiled output of `crates/glyph-cli/tests/corpus/valid/nested_branch_only_procedure.glyph` contains `### Procedure: outer` followed by `### Procedure: bar`, in that order, each exactly once.
