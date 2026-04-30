//! Phase 7 (Emit) — deterministic Markdown projection.
//!
//! Walking-skeleton scope per `design/mvp-acceptance.md` §1: parameterless skill,
//! inline strings as Steps, constraint markers as bulleted Constraints. The output
//! shape is fixed by `design/compiled-output.md`.

use crate::ir::{IrArena, IrBranch, IrNode, NodeId, Polarity};
use std::collections::HashSet;

pub fn emit(arena: &IrArena) -> String {
    let root_id = arena
        .root_skill()
        .expect("validate guarantees a root skill before emit");
    let skill = match arena.get(root_id) {
        IrNode::Skill(s) => s,
        _ => unreachable!("root skill ID must point to a Skill node"),
    };

    let mut out = String::new();

    // ----- Frontmatter -----
    out.push_str("---\n");
    out.push_str(&format!("name: {}\n", skill.name));
    out.push_str(&format!("description: {}\n", skill.description));
    if !skill.effects.is_empty() {
        let mut sorted_effects = skill.effects.clone();
        sorted_effects.sort();
        out.push_str(&format!("effects: [{}]\n", sorted_effects.join(", ")));
    }
    out.push_str("---\n\n");

    // ----- ## Parameters (conditional) -----
    // Per `design/compiled-output.md` §`## Parameters`, the section is emitted
    // only when the skill declares one or more parameters. Each entry renders
    // as a bulleted item with either `(default: <value>)` or `(required)`. The
    // walking-skeleton emitter does not generate descriptions yet (Step 2 LLM
    // work in a later slice), so we omit the description fragment.
    if !skill.params.is_empty() {
        out.push_str("## Parameters\n\n");
        for p in &skill.params {
            match &p.default {
                Some(v) => {
                    out.push_str(&format!("- **{}** (default: {})\n", p.name, v));
                }
                None => {
                    out.push_str(&format!("- **{}** (required)\n", p.name));
                }
            }
        }
        out.push('\n');
    }

    // ----- ## Instructions -----
    out.push_str("## Instructions\n\n");

    // ### Context (bulleted list, before Steps).
    if !skill.context.is_empty() {
        out.push_str("### Context\n\n");
        for ctx_id in &skill.context {
            let text = match arena.get(*ctx_id) {
                IrNode::Context(c) => c.text.clone(),
                _ => panic!("Context node was not a Context"),
            };
            out.push_str(&format!("- {}\n", text));
        }
        out.push('\n');
    }

    // ### Steps (numbered list).
    // Track Tier 2 procedure references in first-reference order.
    let mut procedure_order: Vec<String> = Vec::new();
    let mut procedure_seen: HashSet<String> = HashSet::new();

    if !skill.steps.is_empty() {
        out.push_str("### Steps\n\n");
        for (idx, step_id) in skill.steps.iter().enumerate() {
            match arena.get(*step_id) {
                IrNode::InlineInstruction(i) => {
                    out.push_str(&format!("{}. {}\n", idx + 1, i.text));
                }
                IrNode::Branch(br) => {
                    emit_branch(&mut out, arena, br, idx + 1);
                }
                IrNode::Call(c) if c.projection_tier == Some(2) => {
                    let kebab_name = c.target.replace('_', "-");
                    out.push_str(&format!(
                        "{}. Follow the {} procedure below.\n",
                        idx + 1,
                        kebab_name
                    ));
                    if procedure_seen.insert(c.target.clone()) {
                        procedure_order.push(c.target.clone());
                    }
                }
                IrNode::Call(c) => {
                    panic!(
                        "IrNode::Call to `{}` survived past expand without tier assignment",
                        c.target
                    );
                }
                _ => panic!("Step node was not an InlineInstruction, Branch, or Call"),
            };
        }
        out.push('\n');
    }

    // ### Constraints (bulleted list).
    if !skill.constraints.is_empty() {
        out.push_str("### Constraints\n\n");
        for c_id in &skill.constraints {
            let c = match arena.get(*c_id) {
                IrNode::Constraint(c) => c,
                _ => panic!("Constraint node was not a Constraint"),
            };
            let line = render_constraint(&c.text, c.polarity);
            out.push_str(&format!("- {}\n", line));
        }
        out.push('\n');
    }

    // ### Procedure: <name> sections (Tier 2, in first-reference order).
    for target_name in &procedure_order {
        let kebab_name = target_name.replace('_', "-");
        // Find the Block node for this target.
        let block = arena.nodes().iter().find_map(|n| {
            if let IrNode::Block(b) = n {
                if b.name == *target_name {
                    return Some(b);
                }
            }
            None
        });
        if let Some(block) = block {
            out.push_str(&format!("### Procedure: {}\n\n", kebab_name));
            for (i, stmt) in block.flow_statements.iter().enumerate() {
                out.push_str(&format!("{}. {}\n", i + 1, stmt));
            }
            out.push('\n');
        }
    }

    // Trim trailing blank line for byte-stable output.
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

/// Emit a branch as a single numbered step with lettered sub-steps per arm.
///
/// Per `compiled-output.md` §Constraint Rendering:
/// - Each arm is introduced by a condition header.
/// - Step-projecting nodes become lettered sub-steps (`a.`, `b.`, `c.`).
/// - Letters reset per arm.
/// - Pure-applies branches use "Decide which of the following applies" form.
fn emit_branch(out: &mut String, arena: &IrArena, br: &IrBranch, step_num: usize) {
    // Check if this is a pure-applies branch (all conditions are .applies() calls).
    let is_pure_applies = is_pure_applies_branch(br);

    if is_pure_applies {
        out.push_str(&format!(
            "{}. Decide which of the following applies and follow only that path:\n",
            step_num
        ));
        // Emit the "if" arm using its description.
        emit_applies_arm(out, arena, br, &br.condition, &br.then_body);
        // Emit elif arms.
        for elif in &br.elif_branches {
            emit_applies_arm(out, arena, br, &elif.condition, &elif.body);
        }
        // Emit else arm.
        if let Some(else_body) = &br.else_body {
            out.push_str("   Otherwise:\n");
            emit_lettered_substeps(out, arena, else_body);
        }
    } else {
        // Standard conditional form.
        out.push_str(&format!("{}. If {}:\n", step_num, br.condition));
        emit_lettered_substeps(out, arena, &br.then_body);
        for elif in &br.elif_branches {
            out.push_str(&format!("   If {}:\n", elif.condition));
            emit_lettered_substeps(out, arena, &elif.body);
        }
        if let Some(else_body) = &br.else_body {
            out.push_str("   Otherwise:\n");
            emit_lettered_substeps(out, arena, else_body);
        }
    }
}

/// Emit an applies-arm using the resolved description from applies_descriptions.
fn emit_applies_arm(
    out: &mut String,
    arena: &IrArena,
    br: &IrBranch,
    condition: &str,
    body: &[NodeId],
) {
    // Extract block name from condition like "block_name.applies()"
    let block_name = extract_applies_block_name(condition);
    let description = block_name
        .as_deref()
        .and_then(|name| {
            br.applies_descriptions
                .as_ref()
                .and_then(|map| map.get(name))
        });
    if let Some(desc) = description {
        out.push_str(&format!("   When {}:\n", desc));
    } else {
        out.push_str(&format!("   If {}:\n", condition));
    }
    emit_lettered_substeps(out, arena, body);
}

/// Emit lettered sub-steps for a branch arm body.
fn emit_lettered_substeps(out: &mut String, arena: &IrArena, body: &[NodeId]) {
    let mut letter = b'a';
    for node_id in body {
        let text = match arena.get(*node_id) {
            IrNode::InlineInstruction(i) => i.text.clone(),
            IrNode::Call(c) if c.projection_tier == Some(2) => {
                let kebab_name = c.target.replace('_', "-");
                format!("Follow the {} procedure.", kebab_name)
            }
            IrNode::Call(c) => {
                panic!(
                    "IrNode::Call to `{}` survived past expand in branch body",
                    c.target
                );
            }
            IrNode::Branch(_) => {
                // Nested branches flatten into prose (one level of structure only).
                "(nested branch)".to_string()
            }
            _ => panic!("Unexpected node type in branch body"),
        };
        out.push_str(&format!("   {}. {}\n", letter as char, text));
        letter += 1;
    }
}

/// Check if a branch is a pure-applies branch (all conditions are single .applies() calls).
fn is_pure_applies_branch(br: &IrBranch) -> bool {
    if !is_applies_condition(&br.condition) {
        return false;
    }
    for elif in &br.elif_branches {
        if !is_applies_condition(&elif.condition) {
            return false;
        }
    }
    true
}

/// Check if a condition string is a single `BLOCKNAME.applies()` call.
fn is_applies_condition(condition: &str) -> bool {
    condition.trim().ends_with(".applies()")
}

/// Extract the block name from a `BLOCKNAME.applies()` condition.
fn extract_applies_block_name(condition: &str) -> Option<String> {
    let trimmed = condition.trim();
    trimmed.strip_suffix(".applies()").map(|s| s.to_string())
}

/// Deterministic constraint phrasing per `design/compiled-output.md` §Constraint Rendering.
///
/// The walking skeleton does not invoke an LLM, so this is a small, table-driven
/// projection of polarity onto the resolved text. `require`-polarity text is emitted
/// verbatim. `avoid`-polarity text is reshaped into "Do not <base-form> ...".
fn render_constraint(text: &str, polarity: Polarity) -> String {
    match polarity {
        Polarity::Require => text.to_string(),
        Polarity::Avoid => avoid_phrasing(text),
    }
}

/// Convert the `avoid`-polarity resolved text into a "Do not ..." prohibition.
///
/// Rule: split off the first whitespace-delimited word, gerund-strip it (drop trailing
/// `ing`, optionally re-adding `e` for verbs whose base ends in a single consonant), then
/// emit `Do not <base> <rest>`. If the first word is not a recognisable gerund the text
/// is returned unchanged with a leading "Do not ".
///
/// Walking-skeleton coverage is scoped to the corpus in `mvp-acceptance.md` §1 — namely
/// "Leaving references to removed or renamed symbols." The general rule is intentionally
/// minimal; broader natural-language reshaping is Step 2 (LLM) work in later slices.
fn avoid_phrasing(text: &str) -> String {
    let mut parts = text.splitn(2, char::is_whitespace);
    let first = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("");
    let base = gerund_to_base(first);
    if rest.is_empty() {
        format!("Do not {}", base)
    } else {
        format!("Do not {} {}", base, rest)
    }
}

/// Convert a gerund (e.g., "Leaving") to its base verb form (e.g., "leave").
fn gerund_to_base(word: &str) -> String {
    let lower = word.to_lowercase();
    if let Some(stem) = lower.strip_suffix("ing") {
        // Heuristic: if the stem ends in a single consonant after a single vowel, append "e"
        // (consume → consum + e). For "Leaving" → "Leav" → "Leave".
        let s_bytes = stem.as_bytes();
        let needs_e = match s_bytes.len() {
            0 => false,
            n => {
                let last = s_bytes[n - 1];
                let prev = if n >= 2 { Some(s_bytes[n - 2]) } else { None };
                let is_vowel = |b: u8| matches!(b, b'a' | b'e' | b'i' | b'o' | b'u');
                let is_consonant = |b: u8| b.is_ascii_alphabetic() && !is_vowel(b);
                is_consonant(last) && prev.map(is_vowel).unwrap_or(false)
            }
        };
        if needs_e {
            format!("{}e", stem)
        } else {
            stem.to_string()
        }
    } else {
        // Not a gerund — return lowercased original.
        lower
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gerund_to_base_known() {
        assert_eq!(gerund_to_base("Leaving"), "leave");
        assert_eq!(gerund_to_base("Making"), "make");
        assert_eq!(gerund_to_base("Adding"), "add");
    }

    #[test]
    fn avoid_phrasing_walking_skeleton() {
        let s = avoid_phrasing("Leaving references to removed or renamed symbols.");
        assert_eq!(s, "Do not leave references to removed or renamed symbols.");
    }
}
