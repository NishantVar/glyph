//! Phase 6 Step 1 (Expand, deterministic) — projection-tier assignment and Tier 1 inline.
//!
//! Per `design/pipeline.md` §Phase 6, Step 1 is deterministic:
//! - Computes `resolved_word_count` per block.
//! - Assigns projection tiers to call sites.
//! - Tier 1 (inline): callee body < 150 words → call keeps inline projection metadata.

use crate::ir::{IrArena, IrInlineInstruction, IrNode, NodeId, Role};
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
    fold_block_output_contracts(&mut arena);

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
    let mut block_flow_counts: HashMap<String, usize> = HashMap::new();
    for n in arena.nodes() {
        if let IrNode::Block(b) = n {
            block_flow_counts.insert(b.name.clone(), b.flow_statements.len());
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

            // Tier 2 conditions: >= 4 flow statements, or called 2+ times.
            let is_tier2 = stmt_count >= 4 || freq >= 2;

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

    // Phase 2b: Populate applies_descriptions on Branch nodes.
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
    // Walk Branch nodes and populate applies_descriptions.
    let nodes = arena.nodes_mut();
    for i in 0..nodes.len() {
        if let IrNode::Branch(ref br) = nodes[i] {
            let mut descs: BTreeMap<String, String> = BTreeMap::new();
            // Check all conditions (if + elif) for .applies() patterns.
            let mut conditions = vec![br.condition.clone()];
            for elif in &br.elif_branches {
                conditions.push(elif.condition.clone());
            }
            for cond in &conditions {
                let applies_suffix = ".applies()";
                let mut search_from = 0;
                while let Some(pos) = cond[search_from..].find(applies_suffix) {
                    let abs_pos = search_from + pos;
                    let receiver = &cond[..abs_pos];
                    let block_name = receiver
                        .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
                        .next()
                        .unwrap_or("");
                    if !block_name.is_empty() {
                        if let Some(desc) = block_descriptions.get(block_name) {
                            descs.insert(block_name.to_string(), desc.clone());
                        }
                    }
                    search_from = abs_pos + applies_suffix.len();
                }
            }
            if !descs.is_empty() {
                // We need to mutate the Branch — clone data and replace.
                let mut br_clone = br.clone();
                br_clone.applies_descriptions = Some(descs);
                nodes[i] = IrNode::Branch(br_clone);
            }
        }
    }

    // Phase 3: Return folding (Phase 6 Step 1).
    // If the skill has a return_text, append it to the final step's text.
    if let Some(root_id) = arena.root_skill() {
        let return_text = if let IrNode::Skill(s) = arena.get(root_id) {
            s.return_text.clone()
        } else {
            None
        };
        if let Some(ref ret) = return_text {
            let last_step_id = if let IrNode::Skill(s) = arena.get(root_id) {
                s.steps.last().copied()
            } else {
                None
            };
            if let Some(step_id) = last_step_id {
                let nodes = arena.nodes_mut();
                match &mut nodes[step_id.0 as usize] {
                    IrNode::InlineInstruction(inst) => {
                        inst.text = format!(
                            "{} Return the result of {}.",
                            inst.text.trim_end_matches('.').trim(),
                            ret
                        );
                    }
                    IrNode::Call(call) if call.projection_tier == Some(1) => {
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

    fold_skill_output_contract(&mut arena);

    arena
}

fn output_contract_target(arena: &IrArena, slot: Option<NodeId>) -> Option<String> {
    slot.and_then(|id| match arena.get(id) {
        IrNode::OutputContract(oc) => Some(oc.target_name.clone()),
        _ => None,
    })
}

fn output_target_sentence(target_name: &str) -> String {
    let display_name = target_name.replace('_', " ");
    format!("Produce {display_name} as the final output.")
}

fn append_sentence(existing: &str, sentence: &str) -> String {
    let trimmed = existing.trim();
    if trimmed.is_empty() {
        sentence.to_string()
    } else {
        format!("{}. {}", trimmed.trim_end_matches('.'), sentence)
    }
}

fn fold_block_output_contracts(arena: &mut IrArena) {
    let mut block_targets: HashMap<String, String> = HashMap::new();
    for node in arena.nodes() {
        if let IrNode::Block(block) = node {
            if let Some(target) = output_contract_target(arena, block.output_contract) {
                block_targets.insert(block.name.clone(), target);
            }
        }
    }

    if block_targets.is_empty() {
        return;
    }

    let mut updated_block_bodies: HashMap<String, String> = HashMap::new();
    for node in arena.nodes_mut() {
        if let IrNode::Block(block) = node {
            let Some(target) = block_targets.get(&block.name) else {
                continue;
            };
            let sentence = output_target_sentence(target);
            block.body_text = append_sentence(&block.body_text, &sentence);
            if let Some(pos) = block.flow_statements.iter().rposition(|s| s == "return") {
                block.flow_statements[pos] = sentence;
            } else {
                block.flow_statements.push(sentence);
            }
            updated_block_bodies.insert(block.name.clone(), block.body_text.clone());
        }
    }

    for node in arena.nodes_mut() {
        if let IrNode::Call(call) = node {
            if call.resolved_body.is_some() {
                if let Some(body) = updated_block_bodies.get(&call.target) {
                    call.resolved_body = Some(body.clone());
                }
            }
        }
    }
}

fn fold_skill_output_contract(arena: &mut IrArena) {
    let Some(root_id) = arena.root_skill() else {
        return;
    };
    let (target, last_step_id) = match arena.get(root_id) {
        IrNode::Skill(skill) => (
            output_contract_target(arena, skill.output_contract),
            skill.steps.last().copied(),
        ),
        _ => (None, None),
    };
    let Some(target) = target else {
        return;
    };
    let sentence = output_target_sentence(&target);

    if let Some(step_id) = last_step_id {
        let nodes = arena.nodes_mut();
        match &mut nodes[step_id.0 as usize] {
            IrNode::InlineInstruction(inst) => {
                inst.text = append_sentence(&inst.text, &sentence);
                return;
            }
            IrNode::Call(call) if call.projection_tier == Some(1) => {
                if let Some(body) = &mut call.resolved_body {
                    *body = append_sentence(body, &sentence);
                    return;
                }
            }
            _ => {}
        }
    }

    let next = NodeId(arena.len() as u32);
    let step_id = arena.push(IrNode::InlineInstruction(IrInlineInstruction {
        node_id: next,
        text: sentence,
        role: Role::Step,
    }));
    let nodes = arena.nodes_mut();
    if let IrNode::Skill(skill) = &mut nodes[root_id.0 as usize] {
        skill.steps.push(step_id);
    }
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
        assert!(
            md.contains("Produce current branch as the final output."),
            "compiled Markdown should name the synthesized target naturally:\n{md}"
        );
        assert!(
            !md.contains("<current_branch>"),
            "compiled Markdown must not leak literal output target token:\n{md}"
        );
    }

    #[test]
    fn block_output_contract_folds_before_inline_expansion() {
        let md = compile_markdown(
            "\
block helper() -> BranchName
    flow:
        return <current_branch>

skill current()
    description: \"Return the current branch.\"
    flow:
        helper()
",
        );
        assert!(
            md.contains("Produce current branch as the final output."),
            "inlined block output contract should survive as natural prose:\n{md}"
        );
        assert!(
            !md.contains("<current_branch>"),
            "compiled Markdown must not leak literal output target token:\n{md}"
        );
    }
}
