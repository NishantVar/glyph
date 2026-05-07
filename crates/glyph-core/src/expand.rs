//! Phase 6 Step 1 (Expand, deterministic) — projection-tier assignment and Tier 1 inline.
//!
//! Per `design/pipeline.md` §Phase 6, Step 1 is deterministic:
//! - Computes `resolved_word_count` per block.
//! - Assigns projection tiers to call sites.
//! - Tier 1 (inline): callee body < 150 words → call keeps inline projection metadata.

use crate::condition::ConditionTokenKind;
use crate::emit::branch::extract_predicate_token;
use crate::ir::{IrArena, IrNode};
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
    let consts_for_lookup: BTreeMap<String, String> = arena.consts.clone();
    // Walk Branch nodes and populate resolved_predicates.
    let nodes = arena.nodes_mut();
    for i in 0..nodes.len() {
        if let IrNode::Branch(ref br) = nodes[i] {
            let mut descs: BTreeMap<String, String> = BTreeMap::new();
            // Check all conditions (if + elif) for .applies() patterns and
            // bare-identifier const references (PredicateConst).
            let mut conditions = vec![br.condition.clone()];
            for elif in &br.elif_branches {
                conditions.push(elif.condition.clone());
            }
            for cond in &conditions {
                // PredicateApplies: scan for all .applies() tokens.
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
                // PredicateConst: scan every whitespace-separated token for bare
                // identifier const references, including within mixed conditions.
                // (Previously only resolved pure single-token conditions; Task 4.5
                // extends this to mixed/compositional conditions.)
                // Strip trailing `:` (parser includes it in the condition string).
                // TODO: strip the trailing `:` once at IR construction time
                // (lower.rs / parse.rs) so consumers (analyze, expand, emit)
                // don't each have to redo this work.
                let cond_stripped = cond.trim_end_matches(':').trim();
                // `split_whitespace` mangles `"quoted literal"` tokens, but only `PredicateConst`
                // needs lookup here — `PredicateLiteral` is emitted directly by stub_fill (quotes
                // stripped at render time), so split-mangled literal fragments harmlessly fall
                // through `extract_predicate_token` and the `matches!` guard.
                for raw_tok in cond_stripped.split_whitespace() {
                    // Strip trailing punctuation so bare tokens like `big` are
                    // recognised even if adjacent to a comma or similar.
                    let tok = raw_tok.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_');
                    if let Some((token, ConditionTokenKind::PredicateConst)) =
                        extract_predicate_token(tok)
                    {
                        if let Some(body) = consts_for_lookup.get(&token) {
                            descs.insert(token, body.clone());
                        }
                    }
                }
            }
            if !descs.is_empty() {
                // We need to mutate the Branch — clone data and replace.
                let mut br_clone = br.clone();
                br_clone.resolved_predicates = Some(descs);
                nodes[i] = IrNode::Branch(br_clone);
            }
        }
    }

    // Phase 3: Return folding (Phase 6 Step 1).
    // If the skill has a return_text, append it to the final step's text.
    //
    // Skipped when an output_contract is in scope at the final step: the emit
    // pass applies the locked output-contract templates, and running the
    // legacy fold alongside would produce doubled return instructions like
    // "Return the result of X. ..., and return that as your result."
    // (`design/expand.md` §3.5; `design/compiled-output.md` §OutputContract
    // Rendering.)
    if let Some(root_id) = arena.root_skill() {
        let (return_text, skill_has_oc) = if let IrNode::Skill(s) = arena.get(root_id) {
            (s.return_text.clone(), s.output_contract.is_some())
        } else {
            (None, false)
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
                let callee_has_oc = if let IrNode::Call(c) = arena.get(step_id) {
                    c.projection_tier == Some(1) && c.callee_output_contract.is_some()
                } else {
                    false
                };
                let nodes = arena.nodes_mut();
                match &mut nodes[step_id.0 as usize] {
                    IrNode::InlineInstruction(inst) if !skill_has_oc => {
                        inst.text = format!(
                            "{} Return the result of {}.",
                            inst.text.trim_end_matches('.').trim(),
                            ret
                        );
                    }
                    IrNode::Call(call)
                        if call.projection_tier == Some(1) && !skill_has_oc && !callee_has_oc =>
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
            md.contains("Return current branch as your result."),
            "compiled Markdown should use the standalone Identifier return form:\n{md}"
        );
        assert!(
            !md.contains("<current_branch>"),
            "compiled Markdown must not leak literal output target token:\n{md}"
        );
    }

    #[test]
    fn block_output_contract_folds_before_inline_expansion() {
        // The helper is a return-only block. After Tier-1 inline expansion its
        // resolved_body is empty, so the last-step renders via the standalone
        // return template instead of suffixing an empty body.
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
        assert!(
            md.contains(", and return that as your result."),
            "inlined block output contract should use the locked Identifier return suffix:\n{md}"
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
        // Constraint (b): description text appears in the return suffix
        // (with a leading determiner so the wrapper reads grammatically).
        assert!(
            md.contains(", and return the root cause and affected files as your result."),
            "compiled Markdown must incorporate the description text in the return suffix:\n{md}"
        );
    }

    #[test]
    fn empty_body_tier1_callee_uses_standalone_return() {
        // Return-only inline helper: flow body is just `return <X>`, so
        // `resolved_body` after Tier-1 inline expansion is empty. The last-step
        // path must route to the standalone return template instead of
        // suffixing the empty string (which would render
        // `1. , and return that as your result.`).
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
            md.contains("1. Return current branch as your result."),
            "return-only Tier-1 callee should produce a standalone return step:\n{md}"
        );
        assert!(
            !md.contains("1. , and return"),
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
        assert!(
            md.contains("1. Return the root cause and affected files as your result."),
            "return-only Tier-1 callee with descriptive contract should produce a standalone return step:\n{md}"
        );
        assert!(
            !md.contains("1. , and return"),
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
        assert!(
            md.contains(", and return the branch name as currently checked out as your result."),
            "block description return suffix should appear in compiled markdown:\n{md}"
        );
    }

    #[test]
    fn descriptive_output_contract_with_embedded_control_chars_normalizes_to_single_line() {
        // The tokenizer decodes `\n`/`\t` inside `<"…">` to literal control
        // characters before reaching the scaffold builder. Inserting them verbatim
        // breaks the single-sentence ", and return X as your result." contract — a
        // newline in `desc` splits the prose across two Markdown lines. The scaffold
        // builder collapses runs of whitespace (incl. LF/CR/TAB) to a single space.
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
        // Return suffix uses the new locked form.
        assert!(
            md.contains(", and return the root cause severity and affected files as your result."),
            "expected single-line return suffix with whitespace-collapsed description:\n{md}"
        );
    }

    /// Precedence: when both the enclosing skill and an inlined callee
    /// declare an `output_contract`, the SKILL's contract wins. The skill's
    /// `return <…>` is the author's stated final return, so its template
    /// must drive the locked suffix even though the body chunk happens to
    /// come from the callee.
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
            md.contains(", and return the final wrapped diagnosis as your result."),
            "skill's output_contract should drive the suffix in compiled markdown:\n{md}"
        );
        assert!(
            !md.contains("raw helper diagnosis"),
            "callee's contract description must not surface when skill's contract wins:\n{md}"
        );
    }

    /// Regression: a skill with a bare-name `return` whose final flow step is a
    /// Tier-1 call into a block carrying its own `output_contract` must NOT
    /// receive both the legacy "Return the result of …" fold and the locked
    /// emit-pass suffix. The legacy fold is the deterministic emitter's job to
    /// own when the new contract pipeline is active; this gate prevents the
    /// doubled "Return the result of X. ..., and return that as your result."
    /// shape Codex flagged.
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
            md.contains(", and return that as your result."),
            "expected the locked Identifier return suffix:\n{md}"
        );
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
        let (file, _) = parse::parse(src, 0).expect("source should parse");
        let arena = lower::lower(&file).expect("source should lower");
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
        let (file, _) = parse::parse(src, 0).expect("source should parse");
        let arena = lower::lower(&file).expect("source should lower");
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
}
