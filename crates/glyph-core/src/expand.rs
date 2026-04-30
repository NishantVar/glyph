//! Phase 6 Step 1 (Expand, deterministic) — projection-tier assignment and Tier 1 inline.
//!
//! Per `design/pipeline.md` §Phase 6, Step 1 is deterministic:
//! - Computes `resolved_word_count` per block.
//! - Assigns projection tiers to call sites.
//! - Tier 1 (inline): callee body < 150 words → inlined as InlineInstruction.

use crate::ir::{IrArena, IrInlineInstruction, IrNode, Role};
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

pub fn expand_step1(mut arena: IrArena) -> IrArena {
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

    // Phase 2: Tier assignment and Tier 1 inline expansion.
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
            let wc = block_word_counts.get(target).copied().unwrap_or_else(|| {
                count_words(c.resolved_body.as_ref().unwrap())
            });
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
                // Tier 1: inline.
                let node_id = c.node_id;
                let body = c.resolved_body.clone().unwrap();
                nodes[i] = IrNode::InlineInstruction(IrInlineInstruction {
                    node_id,
                    text: body,
                    role: Role::Step,
                });
            }
        }
    }

    // Phase 2b: Populate applies_descriptions on Branch nodes.
    // Collect block descriptions into a lookup map.
    let mut block_descriptions: HashMap<String, String> = HashMap::new();
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
                if let IrNode::InlineInstruction(inst) = &mut nodes[step_id.0 as usize] {
                    inst.text = format!(
                        "{} Return the result of {}.",
                        inst.text.trim_end_matches('.').trim(),
                        ret
                    );
                }
            }
        }
    }

    arena
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
