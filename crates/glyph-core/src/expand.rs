//! Phase 6 Step 1 (Expand, deterministic) — projection-tier assignment and Tier 1 inline.
//!
//! Per `design/pipeline.md` §Phase 6, Step 1 is deterministic:
//! - Computes `resolved_word_count` per block.
//! - Assigns projection tiers to call sites.
//! - Tier 1 (inline): callee body < 150 words → call keeps inline projection metadata.

use crate::condition::ConditionTokenKind;
use crate::ir::{IrArena, IrNode, LocalRef, NodeId};
use crate::slot;
use std::collections::{BTreeMap, HashMap};

/// Count words in resolved prose per `compiled-output.md` §Word Counting Rule.
///
/// A "word" is a whitespace-separated token. Backticked code spans count as 1
/// word each. Markdown formatting markers (`**`, list bullets, headings) do not
/// count.
pub fn count_words(text: &str) -> u32 {
    let mut count: u32 = 0;
    let mut in_backtick = false;
    for token in text.split_whitespace() {
        // Skip Markdown formatting markers.
        if token == "**" || token == "-" || token.starts_with('#') {
            continue;
        }
        if !in_backtick && token.starts_with('`') && token.ends_with('`') && token.len() >= 2 {
            // Single backticked span like `foo` — 1 word.
            count += 1;
            continue;
        }
        if !in_backtick && token.starts_with('`') {
            in_backtick = true;
            count += 1; // Opening backtick span counts as 1 word.
            continue;
        }
        if in_backtick && token.ends_with('`') {
            in_backtick = false;
            // Closing backtick span — already counted at open.
            continue;
        }
        if in_backtick {
            // Inside backtick span — don't count additional words.
            continue;
        }
        count += 1;
    }
    count
}

pub fn expand_step1(arena: IrArena) -> IrArena {
    expand_step1_with_imported_descriptions(arena, &HashMap::new())
}

pub fn expand_step1_with_imported_descriptions(
    mut arena: IrArena,
    imported_block_descriptions: &HashMap<String, String>,
) -> IrArena {
    // Phase 1: Compute resolved_word_count for each Block node.
    let mut block_word_counts: HashMap<String, u32> = HashMap::new();
    for n in arena.nodes() {
        if let IrNode::Block(b) = n {
            let wc = count_words(&b.body_text);
            block_word_counts.insert(b.name.clone(), wc);
        }
    }

    // Update Block nodes with their word counts.
    for n in arena.nodes_mut() {
        if let IrNode::Block(b) = n {
            if let Some(&wc) = block_word_counts.get(&b.name) {
                b.resolved_word_count = Some(wc);
            }
        }
    }

    // Phase 2: Tier assignment. Tier 1 stays as a Call node in IR and is
    // projected inline by Markdown emit.
    //
    // Collect block metadata for tier decisions:
    // - flow_statement_count: number of individual flow statements
    // - call_count: how many times each block is called in the skill
    // - has_branches: whether the block's flow contains any `if`/elif/else.
    //   Codex review Finding (high): Tier 1 inlines `body_text`, which is
    //   built from `FlowStmt::InlineString` only — branches are dropped
    //   silently. Forcing Tier 2 here keeps the structured branch nodes
    //   reachable through the procedure emit path.
    let mut block_flow_counts: HashMap<String, usize> = HashMap::new();
    let mut block_has_branches: HashMap<String, bool> = HashMap::new();
    for n in arena.nodes() {
        if let IrNode::Block(b) = n {
            block_flow_counts.insert(b.name.clone(), b.flow_statements.len());
            block_has_branches.insert(b.name.clone(), !b.branch_steps.is_empty());
        }
    }

    // Count call frequency per target across all Call nodes.
    let mut call_frequency: HashMap<String, usize> = HashMap::new();
    for n in arena.nodes() {
        if let IrNode::Call(c) = n {
            *call_frequency.entry(c.target.clone()).or_insert(0) += 1;
        }
    }

    let nodes = arena.nodes_mut();
    for i in 0..nodes.len() {
        if let IrNode::Call(c) = &nodes[i] {
            if c.resolved_body.is_none() {
                continue;
            }
            let target = &c.target;
            let wc = block_word_counts
                .get(target)
                .copied()
                .unwrap_or_else(|| count_words(c.resolved_body.as_ref().unwrap()));
            let stmt_count = block_flow_counts.get(target).copied().unwrap_or(0);
            let freq = call_frequency.get(target).copied().unwrap_or(1);
            let has_branches = block_has_branches.get(target).copied().unwrap_or(false);

            // Tier 2 conditions: >= 4 flow statements, called 2+ times,
            // OR the block's flow contains a branch (forced Tier 2 — Tier 1
            // inline drops branch structure, see `block_has_branches` above).
            let is_tier2 = stmt_count >= 4 || freq >= 2 || has_branches;

            if is_tier2 {
                // Mark as Tier 2 — leave the Call node in place.
                let mut c_clone = c.clone();
                c_clone.projection_tier = Some(2);
                nodes[i] = IrNode::Call(c_clone);
            } else if wc >= 150 {
                // Word-count promotion: block has < 4 statements but >= 150
                // words of expanded prose → promote to Tier 2 (same-file
                // procedure) per compiled-output.md §Three-Tier Block Projection.
                let mut c_clone = c.clone();
                c_clone.projection_tier = Some(2);
                nodes[i] = IrNode::Call(c_clone);
            } else {
                // Tier 1: inline projection. Keep the Call node in the IR so
                // `--emit-ir` can preserve call-site metadata such as
                // `site_modifier` and `callee_output_contract`; Markdown emit
                // still projects it as the resolved body text.
                let mut c_clone = c.clone();
                c_clone.projection_tier = Some(1);
                nodes[i] = IrNode::Call(c_clone);
            }
        }
    }

    // Phase 2b: Populate resolved_predicates on Branch nodes.
    // Collect block descriptions into a lookup map.
    let mut block_descriptions: HashMap<String, String> = HashMap::new();
    // Include imported block descriptions first, local descriptions will override.
    for (name, desc) in imported_block_descriptions {
        block_descriptions.insert(name.clone(), desc.clone());
    }
    for n in arena.nodes() {
        if let IrNode::Block(b) = n {
            if let Some(ref desc) = b.description {
                block_descriptions.insert(b.name.clone(), desc.clone());
            }
        }
    }
    // Clone the consts map so we can borrow it below without arena borrow conflict.
    // Merge in the root skill's string-defaulted params: condition.rs:304 classifies
    // those param names as PredicateConst, so resolve_branch_predicates must look
    // them up alongside arena consts. (Same-name shadowing is rejected by Analyze.)
    let mut consts_for_lookup: BTreeMap<String, String> = arena.consts.clone();
    if let Some(root_id) = arena.root_skill() {
        if let IrNode::Skill(s) = arena.get(root_id) {
            for p in &s.params {
                if let Some(default) = &p.default {
                    if default.starts_with('"') && default.ends_with('"') && default.len() >= 2 {
                        let inner = &default[1..default.len() - 1];
                        consts_for_lookup
                            .entry(p.name.clone())
                            .or_insert_with(|| inner.to_string());
                    }
                }
            }
        }
    }
    // Codex review Finding (medium): block params with string defaults are
    // ALSO classified as PredicateConst by Analyze, but they are LOCAL to
    // the block they belong to. A flat file-level merge lets duplicate
    // names across blocks collide (first wins via `or_insert_with`); a
    // branch in the second block then renders the first block's prose.
    // Build a per-branch attribution map instead: for each block, walk
    // its `branch_steps` plus every reachable nested branch (through
    // then/elif/else bodies) and mark each Branch NodeId as owned by that
    // block. Predicate resolution below merges (skill_consts ∪
    // owning_block.string_default_params) per branch.
    let mut branch_block_params: HashMap<NodeId, BTreeMap<String, String>> = HashMap::new();
    for n in arena.nodes() {
        if let IrNode::Block(b) = n {
            if b.string_default_params.is_empty() || b.branch_steps.is_empty() {
                continue;
            }
            let params_for_block = &b.string_default_params;
            let mut to_visit: Vec<NodeId> = b.branch_steps.values().copied().collect();
            while let Some(nid) = to_visit.pop() {
                if branch_block_params.contains_key(&nid) {
                    continue;
                }
                if let IrNode::Branch(br) = arena.get(nid) {
                    branch_block_params.insert(nid, params_for_block.clone());
                    let mut push_branches = |body: &[NodeId]| {
                        for body_nid in body {
                            if matches!(arena.get(*body_nid), IrNode::Branch(_)) {
                                to_visit.push(*body_nid);
                            }
                        }
                    };
                    push_branches(&br.then_body);
                    for elif in &br.elif_branches {
                        push_branches(&elif.body);
                    }
                    if let Some(eb) = &br.else_body {
                        push_branches(eb);
                    }
                }
            }
        }
    }
    // Walk Branch nodes and populate resolved_predicates by consuming the
    // Analyze-attached `classification.tokens` instead of re-tokenizing.
    // Tokens with `is_comparison_operand == true` are skipped per
    // `design/data-flow.md` §327.
    let nodes = arena.nodes_mut();
    for i in 0..nodes.len() {
        if let IrNode::Branch(br) = &nodes[i] {
            let mut br_clone = br.clone();
            // Per-branch lookup: skill consts ∪ owning-block params (if any).
            // Skill consts retain precedence on collision, matching the
            // pre-existing `or_insert_with` semantics.
            let mut lookup = consts_for_lookup.clone();
            if let Some(block_params) = branch_block_params.get(&br_clone.node_id) {
                for (name, default) in block_params {
                    lookup
                        .entry(name.clone())
                        .or_insert_with(|| default.clone());
                }
            }
            populate_resolved_predicates(&mut br_clone, &lookup, &block_descriptions);
            if br_clone.resolved_predicates.is_some() {
                nodes[i] = IrNode::Branch(br_clone);
            }
        }
    }

    // Phase 2c: Flow-position-assignments §8.3 — populate `local_refs`.
    //
    // Walk the skill's flow tree in source order, building a producer table
    // (`bound_name → producing IrCall.node_id`) that respects the lexical
    // scope rules from `.flow-assign-spec.md` §6.1:
    //   * a binding becomes visible only after its declaring statement
    //     (handled by table mutation order),
    //   * branch-arm bindings stay scoped to the arm (handled by recursing
    //     with a *clone* of the table per arm — the clone is dropped on
    //     return, so the parent scope never sees the arm's additions),
    //   * outer bindings remain visible inside arms (handled by cloning the
    //     parent table into the recursion).
    //
    // For each `IrInlineInstruction.text` and each `IrCall.resolved_body`
    // encountered during the walk, run `slot::scan_slots` and classify:
    //   * `{name}` whose `name` is in the producer table → push
    //     `LocalRef { name, node_id }` into the node's `local_refs`.
    //     The literal `{name}` token STAYS in the IR text per §8.3 — IR is
    //     the source of truth for downstream Phase 6b checks; deterministic
    //     substitution into the rendered scaffold is Emit's job (§9.2).
    //   * Anything else (parameters, unknown names) is left untouched —
    //     existing analyze paths emit diagnostics for unknowns.
    //
    // Skill-only: block calls are a separate IR shape and never carry
    // flow-locals (§8.1 / Codex Round 3 High 2).
    if let Some(root_id) = arena.root_skill() {
        let step_ids: Vec<NodeId> = if let IrNode::Skill(s) = arena.get(root_id) {
            s.steps.clone()
        } else {
            Vec::new()
        };
        let mut producers: HashMap<String, NodeId> = HashMap::new();
        populate_local_refs_in_steps(&mut arena, &step_ids, &mut producers);
    }

    // Phase 3: Return folding (Phase 6 Step 1).
    // If the skill has a return_text, append it to the final step's text.
    //
    // Skipped when EITHER an output_contract OR a `-> Foo` return annotation
    // is in scope at the final step: the emit pass applies the locked §8.4
    // templates in those cases, and running the legacy fold alongside would
    // produce doubled return instructions like "Return the result of X.
    // Return a `Foo`." (`design/expand.md` §3.5; spec §8.4.)
    if let Some(root_id) = arena.root_skill() {
        let (return_text, skill_has_oc, skill_has_return_type) =
            if let IrNode::Skill(s) = arena.get(root_id) {
                (
                    s.return_text.clone(),
                    s.output_contract.is_some(),
                    s.return_type_text.is_some(),
                )
            } else {
                (None, false, false)
            };
        if let Some(ref ret) = return_text {
            let last_step_id = if let IrNode::Skill(s) = arena.get(root_id) {
                s.steps.last().copied()
            } else {
                None
            };
            if let Some(step_id) = last_step_id {
                // Read the callee's OC directly off the Call node. Populated
                // at lower time for same-file callees and at the cross-file
                // import fix-up step in `compile_source_with_resolved_imports`
                // for imported callees, so this gate behaves consistently
                // regardless of import boundary.
                let (callee_has_oc, callee_has_return_type) =
                    if let IrNode::Call(c) = arena.get(step_id) {
                        (
                            c.projection_tier == Some(1) && c.callee_output_contract.is_some(),
                            c.projection_tier == Some(1) && c.callee_return_type_text.is_some(),
                        )
                    } else {
                        (false, false)
                    };
                let nodes = arena.nodes_mut();
                match &mut nodes[step_id.0 as usize] {
                    IrNode::InlineInstruction(inst) if !skill_has_oc && !skill_has_return_type => {
                        inst.text = format!(
                            "{} Return the result of {}.",
                            inst.text.trim_end_matches('.').trim(),
                            ret
                        );
                    }
                    IrNode::Call(call)
                        if call.projection_tier == Some(1)
                            && !skill_has_oc
                            && !skill_has_return_type
                            && !callee_has_oc
                            && !callee_has_return_type =>
                    {
                        if let Some(body) = &mut call.resolved_body {
                            *body = format!(
                                "{} Return the result of {}.",
                                body.trim_end_matches('.').trim(),
                                ret
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    arena
}

/// Populate `branch.resolved_predicates` by walking the Analyze-attached
/// classification tokens on `branch` and each of its elif arms. Tokens marked
/// `is_comparison_operand` are skipped per `design/data-flow.md` §327 — they
/// are operands of an `==` comparison and must NOT enter resolved_predicates.
///
/// `consts` maps const-name → resolved body text (typically `IrArena.consts`).
/// `block_descriptions` maps block-name → its `description:` text (locals
/// override imports; the caller is responsible for that merge).
fn populate_resolved_predicates(
    branch: &mut crate::ir::IrBranch,
    consts: &BTreeMap<String, String>,
    block_descriptions: &HashMap<String, String>,
) {
    let mut rp: BTreeMap<String, String> = BTreeMap::new();

    let mut walk = |c: &crate::condition::ConditionClassification| {
        for tok in &c.tokens {
            if tok.is_comparison_operand {
                continue;
            }
            match tok.kind {
                ConditionTokenKind::PredicateApplies => {
                    let key = tok.text.trim_end_matches(".applies()").to_string();
                    if let Some(desc) = block_descriptions.get(&key) {
                        rp.insert(key, desc.clone());
                    }
                }
                ConditionTokenKind::PredicateConst => {
                    let key = tok.text.clone();
                    if let Some(body) = consts.get(&key) {
                        rp.insert(key, body.clone());
                    }
                }
                _ => {}
            }
        }
    };

    if let Some(c) = &branch.classification {
        walk(c);
    }
    for elif in &branch.elif_branches {
        if let Some(c) = &elif.classification {
            walk(c);
        }
    }

    branch.resolved_predicates = if rp.is_empty() { None } else { Some(rp) };
}

/// Resolve `{name}` slots in `text` against the producer table and return the
/// resulting `local_refs` vec. Each slot whose name is in `producers` yields
/// one `LocalRef` (deduplicated: repeat occurrences of the same name produce
/// one ref so emit's substitution remains O(n)). Slots not in the producer
/// table are ignored — analyze handles unknown-name diagnostics, and the
/// parameter slot machinery handles parameter slots.
fn collect_local_refs(text: &str, producers: &HashMap<String, NodeId>) -> Vec<LocalRef> {
    if producers.is_empty() {
        return Vec::new();
    }
    let mut refs: Vec<LocalRef> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for sm in slot::scan_slots(text) {
        if let Some(node_id) = producers.get(&sm.name) {
            if seen.insert(sm.name.clone()) {
                refs.push(LocalRef {
                    name: sm.name.clone(),
                    node_id: *node_id,
                });
            }
        }
    }
    refs
}

/// Walk a flat sequence of step `NodeId`s in source order, populating
/// `local_refs` on each `IrInlineInstruction` / `IrCall` whose text contains
/// `{name}` slots that match the live producer table. Mutates `producers` to
/// add a binding ONLY AFTER its declaring `IrCall` is processed (so a slot in
/// the declaring call's own body does not see the binding), then recurses
/// into branch arms with a clone (arm bindings don't leak).
fn populate_local_refs_in_steps(
    arena: &mut IrArena,
    step_ids: &[NodeId],
    producers: &mut HashMap<String, NodeId>,
) {
    for &nid in step_ids {
        // Snapshot fields we need from the immutable borrow before we mutate.
        let snapshot = match arena.get(nid) {
            IrNode::InlineInstruction(i) => StepSnapshot::Inline {
                text: i.text.clone(),
            },
            IrNode::Call(c) => StepSnapshot::Call {
                bound_name: c.bound_name.clone(),
                node_id: c.node_id,
                body_text: c.resolved_body.clone(),
            },
            IrNode::Branch(b) => StepSnapshot::Branch {
                then_body: b.then_body.clone(),
                elif_bodies: b.elif_branches.iter().map(|e| e.body.clone()).collect(),
                else_body: b.else_body.clone(),
            },
            _ => StepSnapshot::Other,
        };

        match snapshot {
            StepSnapshot::Inline { text } => {
                let refs = collect_local_refs(&text, producers);
                if !refs.is_empty() {
                    if let IrNode::InlineInstruction(i) = &mut arena.nodes_mut()[nid.0 as usize] {
                        i.local_refs = refs;
                    }
                }
            }
            StepSnapshot::Call {
                bound_name,
                node_id,
                body_text,
            } => {
                if let Some(text) = body_text {
                    let refs = collect_local_refs(&text, producers);
                    if !refs.is_empty() {
                        if let IrNode::Call(c) = &mut arena.nodes_mut()[nid.0 as usize] {
                            c.local_refs = refs;
                        }
                    }
                }
                // Producer table records the binding AFTER processing the
                // declaring statement — a `{ctx}` inside `inspect_repo`'s own
                // body resolved here is a parameter slot of the callee, not a
                // self-reference (the callee's body uses param slots like
                // `{scope}` populated from the caller's args, not its own
                // bound name).
                if let Some(name) = bound_name {
                    producers.insert(name, node_id);
                }
            }
            StepSnapshot::Branch {
                then_body,
                elif_bodies,
                else_body,
            } => {
                // Each arm gets a CLONE of the producer table so arm-local
                // bindings stay scoped to the arm; outer bindings remain
                // visible because the clone carries them in.
                let mut arm_producers = producers.clone();
                populate_local_refs_in_steps(arena, &then_body, &mut arm_producers);
                for body in &elif_bodies {
                    let mut elif_producers = producers.clone();
                    populate_local_refs_in_steps(arena, body, &mut elif_producers);
                }
                if let Some(eb) = else_body {
                    let mut else_producers = producers.clone();
                    populate_local_refs_in_steps(arena, &eb, &mut else_producers);
                }
            }
            StepSnapshot::Other => {}
        }
    }
}

/// Local snapshot of the per-node fields needed for `local_refs` resolution.
/// Decouples the read-only inspection from the mutable write-back, since the
/// arena owns its `IrNode` storage by index.
enum StepSnapshot {
    Inline {
        text: String,
    },
    Call {
        bound_name: Option<String>,
        node_id: NodeId,
        body_text: Option<String>,
    },
    Branch {
        then_body: Vec<NodeId>,
        elif_bodies: Vec<Vec<NodeId>>,
        else_body: Option<Vec<NodeId>>,
    },
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{emit, lower, parse};

    #[test]
    fn count_words_simple() {
        assert_eq!(count_words("Hello world"), 2);
        assert_eq!(count_words("One two three four five"), 5);
    }

    #[test]
    fn count_words_backtick_span() {
        assert_eq!(count_words("Run `cargo test` now"), 3);
        assert_eq!(count_words("`foo` and `bar`"), 3);
    }

    #[test]
    fn count_words_markdown_markers() {
        assert_eq!(count_words("- item one"), 2);
        assert_eq!(count_words("## heading text"), 2);
        assert_eq!(count_words("**bold** text"), 2);
    }

    #[test]
    fn count_words_empty() {
        assert_eq!(count_words(""), 0);
        assert_eq!(count_words("   "), 0);
    }

    fn compile_markdown(src: &str) -> String {
        let (file, _) = parse::parse(src, 0).expect("source should parse");
        let arena = lower::lower(&file).expect("source should lower");
        let arena = expand_step1(arena);
        emit::emit(&arena, false)
    }

    #[test]
    fn skill_output_contract_folds_to_natural_prose() {
        let md = compile_markdown(
            "\
skill current() -> BranchName
    description: \"Return the current branch.\"
    flow:
        return <current_branch>
",
        );
        // §8.4 row 3: `return <name>` + `-> Foo`, no `type Foo` decl.
        assert!(
            md.contains("Produce `current_branch` (`BranchName`)."),
            "compiled Markdown should use the §8.4 row-3 sentence:\n{md}"
        );
        assert!(
            !md.contains("<current_branch>"),
            "compiled Markdown must not leak literal output target token:\n{md}"
        );
    }

    #[test]
    fn block_output_contract_folds_before_inline_expansion() {
        // The helper is a return-only block. After Tier-1 inline expansion its
        // resolved_body is empty, so the last-step renders the §8.4 sentence
        // standalone instead of folding into an empty body.
        let md = compile_markdown(
            "\
block helper() -> BranchName
    flow:
        \"Probe the working tree.\"
        return <current_branch>

skill current()
    description: \"Return the current branch.\"
    flow:
        helper()
",
        );
        // §8.4 row 3 (callee's contract bubbles up via Tier-1).
        assert!(
            md.contains(". Produce `current_branch` (`BranchName`)."),
            "inlined block output contract should append the §8.4 row-3 sentence:\n{md}"
        );
        assert!(
            !md.contains("<current_branch>"),
            "compiled Markdown must not leak literal output target token:\n{md}"
        );
    }

    #[test]
    fn descriptive_output_contract_folds_into_prose() {
        let md = compile_markdown(
            "\
skill diagnose_issue() -> Diagnosis
    flow:
        \"Inspect the repository.\"
        return <\"root cause and affected files\">
",
        );
        // Constraint (a): no literal `<"…">` survives.
        assert!(
            !md.contains("<\"root cause and affected files\">"),
            "compiled Markdown leaked the descriptive token:\n{md}"
        );
        // §8.4 row 1: descriptive output target — `Produce: X.`.
        assert!(
            md.contains(". Produce: root cause and affected files."),
            "compiled Markdown must use the §8.4 row-1 sentence:\n{md}"
        );
    }

    #[test]
    fn empty_body_tier1_callee_uses_standalone_return() {
        // Return-only inline helper: flow body is just `return <X>`, so
        // `resolved_body` after Tier-1 inline expansion is empty. The last-step
        // path must emit the §8.4 sentence standalone rather than fold into
        // an empty body (which would render a leading-comma malformed line).
        let md = compile_markdown(
            "\
block helper() -> BranchName
    flow:
        return <current_branch>

skill main()
    description: \"Demo.\"
    flow:
        helper()
",
        );
        assert!(
            md.contains("1. Produce `current_branch` (`BranchName`)."),
            "return-only Tier-1 callee should produce the §8.4 row-3 sentence:\n{md}"
        );
        assert!(
            !md.contains("1. , and"),
            "must not emit a leading-comma malformed suffix:\n{md}"
        );
    }

    #[test]
    fn empty_body_tier1_callee_with_description_uses_standalone_return() {
        let md = compile_markdown(
            "\
block helper() -> Diagnosis
    flow:
        return <\"root cause and affected files\">

skill main()
    description: \"Demo.\"
    flow:
        helper()
",
        );
        // §8.4 row 1 wins regardless of the `-> Diagnosis` annotation.
        assert!(
            md.contains("1. Produce: root cause and affected files."),
            "return-only Tier-1 callee with descriptive contract should produce the §8.4 row-1 sentence:\n{md}"
        );
        assert!(
            !md.contains("1. , and"),
            "must not emit a leading-comma malformed suffix:\n{md}"
        );
    }

    #[test]
    fn descriptive_output_contract_in_block_folds_into_prose() {
        let md = compile_markdown(
            "\
block helper() -> BranchName
    flow:
        \"Probe the working tree.\"
        return <\"branch name as currently checked out\">

skill main()
    flow:
        helper()
",
        );
        assert!(!md.contains("<\"branch name as currently checked out\">"));
        // §8.4 row 1 (descriptive output target wins over the type annotation).
        assert!(
            md.contains(". Produce: branch name as currently checked out."),
            "block description return suffix should appear as §8.4 row-1 sentence:\n{md}"
        );
    }

    #[test]
    fn descriptive_output_contract_with_embedded_control_chars_normalizes_to_single_line() {
        // The tokenizer decodes `\n`/`\t` inside `<"…">` to literal control
        // characters before reaching the scaffold builder. The §8.4 sentence
        // must collapse runs of whitespace (incl. LF/CR/TAB) to single spaces
        // so the prose stays on one line.
        let md = compile_markdown(
            "\
skill diagnose_issue() -> Diagnosis
    flow:
        \"Inspect the repository.\"
        return <\"root cause\\nseverity\\tand affected files\">
",
        );
        // No raw control characters in the prose region.
        assert!(
            !md.contains("root cause\nseverity"),
            "compiled Markdown must not embed a literal LF inside the prose:\n{md:?}"
        );
        assert!(
            !md.contains("severity\tand"),
            "compiled Markdown must not embed a literal TAB inside the prose:\n{md:?}"
        );
        // Whitespace collapsed to single spaces.
        assert!(
            md.contains("root cause severity and affected files"),
            "expected whitespace-collapsed description in prose:\n{md}"
        );
        // Return sentence uses the §8.4 row-1 form.
        assert!(
            md.contains(". Produce: root cause severity and affected files."),
            "expected §8.4 row-1 sentence with whitespace-collapsed description:\n{md}"
        );
    }

    /// Precedence: when both the enclosing skill and an inlined callee
    /// declare an `output_contract`, the SKILL's contract wins. The skill's
    /// `return <…>` is the author's stated final return, so its §8.4 sentence
    /// must drive the suffix even though the body chunk comes from the callee.
    #[test]
    fn skill_output_contract_beats_callee_in_tier1() {
        let md = compile_markdown(
            "\
block helper() -> Diagnosis
    flow:
        \"Probe state.\"
        return <\"raw helper diagnosis\">

skill main() -> Diagnosis
    description: \"Wraps helper.\"
    flow:
        helper()
        return <\"final wrapped diagnosis\">
",
        );
        assert!(
            md.contains(". Produce: final wrapped diagnosis."),
            "skill's output_contract should drive the §8.4 row-1 sentence:\n{md}"
        );
        assert!(
            !md.contains("raw helper diagnosis"),
            "callee's contract description must not surface when skill's contract wins:\n{md}"
        );
    }

    /// Regression: a skill with a bare-name `return` whose final flow step is a
    /// Tier-1 call into a block carrying its own `output_contract` must NOT
    /// receive both the legacy "Return the result of …" fold and the §8.4
    /// sentence. The fold is silenced when the callee provides a contract.
    #[test]
    fn legacy_fold_skipped_when_callee_has_output_contract() {
        let md = compile_markdown(
            "\
block helper() -> BranchName
    flow:
        \"Inspect the working tree.\"
        return <current_branch>

skill current()
    description: \"Return the current branch.\"
    flow:
        helper()
        return current_branch
",
        );
        assert!(
            !md.contains("Return the result of current_branch"),
            "legacy return fold must not fire when the callee carries an output_contract:\n{md}"
        );
        assert!(
            md.contains(". Produce `current_branch` (`BranchName`)."),
            "expected the §8.4 row-3 sentence:\n{md}"
        );
    }

    /// Run parse → analyze_with_diagnostics → lower so that
    /// `IrBranch.classification` is populated by the live pipeline (Tasks 5/6).
    /// Post-Task-7, `expand_step1` consumes that classification directly,
    /// so any test that exercises `resolved_predicates` MUST go through
    /// the analyzed entry-point — `analyze::analyze` is a no-op stub.
    fn lower_analyzed(src: &str) -> crate::ir::IrArena {
        use crate::analyze::analyze_with_diagnostics;
        use crate::diagnostic::DiagBag;
        use crate::domain_registry::Registry;
        use crate::span::LineIndex;
        let (file, _) = parse::parse(src, 0).expect("source should parse");
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let mut registry = Registry::new();
        let analyzed =
            analyze_with_diagnostics(file, 0, "test", &line_index, &mut bag, &mut registry);
        lower::lower(&analyzed).expect("source should lower")
    }

    #[test]
    fn expand_step1_populates_resolved_predicates_for_const_form() {
        let src = r#"
const big = "the change is big"

skill foo()
    description: "test"
    flow:
        if big:
            "stop"
"#;
        let arena = lower_analyzed(src);
        let arena = expand_step1(arena);
        // Find the Branch node in the arena.
        let branch = arena
            .nodes()
            .iter()
            .find_map(|n| match n {
                crate::ir::IrNode::Branch(b) => Some(b),
                _ => None,
            })
            .expect("arena should contain a Branch node");
        let rp = branch
            .resolved_predicates
            .as_ref()
            .expect("resolved_predicates should be populated for PredicateConst");
        assert_eq!(rp.get("big"), Some(&"the change is big".to_string()));
    }

    #[test]
    fn expand_step1_populates_resolved_predicates_for_elif_const_form() {
        let src = r#"
const big = "the change is big"
const small = "the change is small"

skill foo()
    description: "test"
    flow:
        if big:
            "stop"
        elif small:
            "continue"
"#;
        let arena = lower_analyzed(src);
        let arena = expand_step1(arena);
        // Find the Branch node in the arena.
        let branch = arena
            .nodes()
            .iter()
            .find_map(|n| match n {
                crate::ir::IrNode::Branch(b) => Some(b),
                _ => None,
            })
            .expect("arena should contain a Branch node");
        let rp = branch
            .resolved_predicates
            .as_ref()
            .expect("resolved_predicates should be populated for PredicateConst");
        assert_eq!(rp.get("big"), Some(&"the change is big".to_string()));
        assert_eq!(rp.get("small"), Some(&"the change is small".to_string()));
    }

    /// Phase 4 Step 1 (`.flow-assign-spec.md` §8.3): walk inline-instruction
    /// text downstream of a flow-local binding, classify each `{name}` slot
    /// against the in-scope producer table, and push a `LocalRef` referencing
    /// the producing IrCall. The literal `{name}` token must STAY in
    /// `IrInlineInstruction.text` — IR is the source of truth for downstream
    /// Phase 6b checks; substitution is Emit's job (§9.2).
    #[test]
    fn step1_populates_local_refs() {
        let src = r#"
block inspect_repo(scope = ".") -> RepoContext
    "Inspect {scope}."

skill demo()
    description: "demo"
    flow:
        ctx = inspect_repo(".")
        "Use the result {ctx} to find issues"
        return ctx
"#;
        let arena = lower_analyzed(src);
        let arena = expand_step1(arena);
        // Find the producing IrCall.
        let producer = arena
            .nodes()
            .iter()
            .find_map(|n| match n {
                crate::ir::IrNode::Call(c) if c.bound_name.as_deref() == Some("ctx") => Some(c),
                _ => None,
            })
            .expect("arena should contain a producer IrCall with bound_name `ctx`");
        // Find the IrInlineInstruction whose text references `{ctx}`.
        let inline = arena
            .nodes()
            .iter()
            .find_map(|n| match n {
                crate::ir::IrNode::InlineInstruction(i) if i.text.contains("{ctx}") => Some(i),
                _ => None,
            })
            .expect("arena should contain an inline instruction referencing `{ctx}`");
        let lref = inline
            .local_refs
            .iter()
            .find(|l| l.name == "ctx")
            .expect("inline-instruction should carry a `ctx` local_ref");
        assert_eq!(lref.node_id, producer.node_id);
        // IR-level token stays literal per §8.3.
        assert!(inline.text.contains("{ctx}"));
    }
}
