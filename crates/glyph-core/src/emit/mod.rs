//! Phase 7 (Emit) — deterministic Markdown projection.
//!
//! Walking-skeleton scope per `design/mvp-acceptance.md` §1: parameterless skill,
//! inline strings as Steps, constraint markers as bulleted Constraints. The output
//! shape is fixed by `design/compiled-output.md`.

pub(crate) mod scaffold;
pub(crate) mod merger;
pub(crate) mod stub_fill;

use crate::ir::{IrArena, IrBranch, IrNode, NodeId, Polarity};

pub fn emit(arena: &IrArena, enable_effects: bool) -> String {
    let scaffold = scaffold::build(arena, enable_effects);
    let fills = stub_fill::fill(&scaffold);
    merger::merge(scaffold, fills).expect("scaffold/fill mismatch is a bug")
}

/// Emit a branch as a single numbered step with lettered sub-steps per arm.
///
/// Per `compiled-output.md` §Constraint Rendering:
/// - Each arm is introduced by a condition header.
/// - Step-projecting nodes become lettered sub-steps (`a.`, `b.`, `c.`).
/// - Letters reset per arm.
/// - Pure-applies branches use "Decide which of the following applies" form.
pub(crate) fn emit_branch(out: &mut String, arena: &IrArena, br: &IrBranch, step_num: usize) {
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
pub(crate) fn emit_applies_arm(
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
pub(crate) fn emit_lettered_substeps(out: &mut String, arena: &IrArena, body: &[NodeId]) {
    let mut letter = b'a';
    for node_id in body {
        let text = match arena.get(*node_id) {
            IrNode::InlineInstruction(i) => i.text.clone(),
            IrNode::Call(c) if c.projection_tier == Some(1) => {
                c.resolved_body.clone().unwrap_or_default()
            }
            IrNode::Call(c) if c.projection_tier == Some(2) => {
                let kebab_name = c.target.replace('_', "-");
                format!("Follow the {} procedure.", kebab_name)
            }
            IrNode::Call(c) if c.projection_tier == Some(3) => {
                let proc_path = c.procedure_path.as_deref().unwrap_or("unknown");
                format!("Load and follow the procedure in `{}`.", proc_path)
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
pub(crate) fn is_pure_applies_branch(br: &IrBranch) -> bool {
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
pub(crate) fn is_applies_condition(condition: &str) -> bool {
    condition.trim().ends_with(".applies()")
}

/// Extract the block name from a `BLOCKNAME.applies()` condition.
pub(crate) fn extract_applies_block_name(condition: &str) -> Option<String> {
    let trimmed = condition.trim();
    trimmed.strip_suffix(".applies()").map(|s| s.to_string())
}

/// Deterministic constraint phrasing per `design/compiled-output.md` §Constraint Rendering.
///
/// The walking skeleton does not invoke an LLM, so this is a small, table-driven
/// projection of polarity onto the resolved text. `require`-polarity text is emitted
/// verbatim. `avoid`-polarity text is reshaped into "Do not <base-form> ...".
pub(crate) fn render_constraint(text: &str, polarity: Polarity) -> String {
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
pub(crate) fn avoid_phrasing(text: &str) -> String {
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
pub(crate) fn gerund_to_base(word: &str) -> String {
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

/// Emit a standalone procedure `.md` file for a Tier 3 external-file export block.
///
/// Per `compiled-output.md` §External Procedure Files, the format is:
/// - YAML frontmatter with `name`, `kind: procedure`, `description`, optional `effects`
/// - `## Parameters` (if any)
/// - `## Instructions` with `### Steps` from flow strings
pub fn emit_procedure(
    name: &str,
    description: &str,
    effects: &[String],
    params: &[(String, Option<String>)],
    flow_strings: &[String],
    enable_effects: bool,
) -> String {
    let kebab_name = name.replace('_', "-");
    let mut out = String::new();

    // Frontmatter
    out.push_str("---\n");
    out.push_str(&format!("name: {}\n", kebab_name));
    out.push_str("kind: procedure\n");
    out.push_str(&format!("description: {}\n", description));
    if enable_effects && !effects.is_empty() {
        let mut sorted = effects.to_vec();
        sorted.sort();
        out.push_str(&format!("effects: [{}]\n", sorted.join(", ")));
    }
    out.push_str("---\n\n");

    // Parameters
    if !params.is_empty() {
        out.push_str("## Parameters\n\n");
        for (pname, default) in params {
            match default {
                Some(v) => out.push_str(&format!("- **{}** (default: {})\n", pname, v)),
                None => out.push_str(&format!("- **{}** (required)\n", pname)),
            }
        }
        out.push('\n');
    }

    // Instructions / Steps
    out.push_str("## Instructions\n\n");
    if !flow_strings.is_empty() {
        out.push_str("### Steps\n\n");
        for (i, step) in flow_strings.iter().enumerate() {
            out.push_str(&format!("{}. {}\n", i + 1, step));
        }
    }

    // Trim trailing blank lines for byte-stable output.
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
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

    use crate::ir::{IrArena, IrNode, IrSkill, NodeId};

    /// Build a minimal arena with a skill that has effects.
    fn arena_with_effects() -> IrArena {
        let mut arena = IrArena::new();
        let step_id = arena.push(IrNode::InlineInstruction(crate::ir::IrInlineInstruction {
            node_id: NodeId(0),
            text: "Do something.".into(),
            role: crate::ir::Role::Step,
        }));
        let skill_id = arena.push(IrNode::Skill(IrSkill {
            node_id: NodeId(1),
            name: "test_skill".into(),
            description: "A test skill.".into(),
            effects: vec!["fs:write".into(), "net:http".into()],
            params: vec![],
            steps: vec![step_id],
            context: vec![],
            constraints: vec![],
            return_text: None,
            return_type: None,
            output_contract: None,
        }));
        arena.set_root_skill(skill_id);
        arena
    }

    #[test]
    fn emit_skips_effects_when_disabled() {
        let arena = arena_with_effects();
        let output = emit(&arena, false);
        assert!(!output.contains("effects:"), "effects line should be omitted when enable_effects is false");
        assert!(output.contains("name: test_skill"), "name should still be present");
    }

    #[test]
    fn emit_includes_effects_when_enabled() {
        let arena = arena_with_effects();
        let output = emit(&arena, true);
        assert!(output.contains("effects: [fs:write, net:http]"), "effects line should be present when enable_effects is true");
    }

    #[test]
    fn emit_procedure_skips_effects_when_disabled() {
        let output = emit_procedure("my_proc", "A procedure.", &["fs:read".to_string()], &[], &["Step one.".into()], false);
        assert!(!output.contains("effects:"), "effects line should be omitted when enable_effects is false");
        assert!(output.contains("name: my-proc"), "name should still be present");
    }

    #[test]
    fn emit_procedure_includes_effects_when_enabled() {
        let output = emit_procedure("my_proc", "A procedure.", &["fs:read".to_string()], &[], &["Step one.".into()], true);
        assert!(output.contains("effects: [fs:read]"), "effects line should be present when enable_effects is true");
    }
}
