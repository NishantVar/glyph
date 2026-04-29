//! Phase 6 Step 1 (Expand, deterministic) — projection-tier assignment and Tier 1 inline.
//!
//! Per `design/pipeline.md` §Phase 6, Step 1 is deterministic:
//! - Computes `resolved_word_count` per block.
//! - Assigns projection tiers to call sites.
//! - Tier 1 (inline): callee body < 150 words → inlined as InlineInstruction.

use crate::ir::{IrArena, IrInlineInstruction, IrNode, Role};
use std::collections::HashMap;

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

    // Phase 2: Tier 1 inline expansion.
    // For each Call node whose resolved_body is Some and whose callee's word
    // count is < 150, replace the Call with an InlineInstruction.
    let nodes = arena.nodes_mut();
    for i in 0..nodes.len() {
        let should_inline = if let IrNode::Call(c) = &nodes[i] {
            if let Some(ref body) = c.resolved_body {
                let wc = block_word_counts.get(&c.target).copied().unwrap_or_else(|| count_words(body));
                wc < 150
            } else {
                false
            }
        } else {
            false
        };

        if should_inline {
            let (node_id, body) = if let IrNode::Call(c) = &nodes[i] {
                (c.node_id, c.resolved_body.clone().unwrap())
            } else {
                unreachable!()
            };
            nodes[i] = IrNode::InlineInstruction(IrInlineInstruction {
                node_id,
                text: body,
                role: Role::Step,
            });
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
