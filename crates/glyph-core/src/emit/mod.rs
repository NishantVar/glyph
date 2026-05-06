//! Phase 7 (Emit) — deterministic Markdown projection.
//!
//! Walking-skeleton scope per `design/mvp-acceptance.md` §1: parameterless skill,
//! inline strings as Steps, constraint markers as bulleted Constraints. The output
//! shape is fixed by `design/compiled-output.md`.

pub(crate) mod branch;
pub(crate) mod constraint;
pub(crate) mod merger;
pub(crate) mod scaffold;
pub(crate) mod stub_fill;
pub(crate) mod templates;

use crate::ir::{IrArena, OutputTargetForm};

pub fn emit(arena: &IrArena, enable_effects: bool) -> String {
    let scaffold = scaffold::build(arena, enable_effects);
    let fills = stub_fill::fill(&scaffold);
    merger::merge(scaffold, fills).expect("scaffold/fill mismatch is a bug")
}

/// Emit a standalone procedure `.md` file for a Tier 3 external-file export block.
///
/// Per `compiled-output.md` §External Procedure Files, the format is:
/// - YAML frontmatter with `name`, `kind: procedure`, `description`, optional `effects`
/// - `## Parameters` (if any)
/// - `## Instructions` with `### Steps` from flow strings
///
/// `output_form` carries the export block's `return <…>` contract when present.
/// Identifier and Description forms route through the locked templates in
/// `emit::templates` so a Tier-3 procedure retains its return contract on disk
/// (`design/compiled-output.md` §OutputContract Rendering).
pub fn emit_procedure(
    name: &str,
    description: &str,
    effects: &[String],
    params: &[(String, Option<String>)],
    flow_strings: &[String],
    output_form: Option<&OutputTargetForm>,
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
    let last_step_idx = flow_strings.len().checked_sub(1);
    if !flow_strings.is_empty() {
        out.push_str("### Steps\n\n");
        for (i, step) in flow_strings.iter().enumerate() {
            let body = if Some(i) == last_step_idx {
                match output_form {
                    Some(OutputTargetForm::Identifier(_)) => {
                        templates::append_identifier_suffix(step)
                    }
                    Some(OutputTargetForm::Description(desc)) => {
                        let normalized = desc.split_whitespace().collect::<Vec<_>>().join(" ");
                        templates::append_description_suffix(step, &normalized)
                    }
                    None => step.clone(),
                }
            } else {
                step.clone()
            };
            out.push_str(&format!("{}. {}\n", i + 1, body));
        }
    } else if let Some(form) = output_form {
        // No steps but the export block still declares a return contract —
        // surface it as a single standalone-return step so the contract isn't
        // silently dropped from the procedure file.
        out.push_str("### Steps\n\n");
        let line = match form {
            OutputTargetForm::Identifier(name) => templates::standalone_return_identifier(name),
            OutputTargetForm::Description(desc) => {
                let normalized = desc.split_whitespace().collect::<Vec<_>>().join(" ");
                templates::standalone_return_description(&normalized)
            }
        };
        out.push_str(&format!("1. {}\n", line));
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
        let output = emit_procedure("my_proc", "A procedure.", &["fs:read".to_string()], &[], &["Step one.".into()], None, false);
        assert!(!output.contains("effects:"), "effects line should be omitted when enable_effects is false");
        assert!(output.contains("name: my-proc"), "name should still be present");
    }

    #[test]
    fn emit_procedure_includes_effects_when_enabled() {
        let output = emit_procedure("my_proc", "A procedure.", &["fs:read".to_string()], &[], &["Step one.".into()], None, true);
        assert!(output.contains("effects: [fs:read]"), "effects line should be present when enable_effects is true");
    }

    #[test]
    fn emit_procedure_appends_identifier_suffix_to_last_step() {
        let form = OutputTargetForm::Identifier("current_branch".into());
        let output = emit_procedure(
            "helper",
            "Returns the branch.",
            &[],
            &[],
            &["Examine the working tree.".into()],
            Some(&form),
            false,
        );
        assert!(
            output.contains("1. Examine the working tree, and return that as your result.\n"),
            "identifier output_form should append the locked suffix to the final step:\n{output}"
        );
    }

    #[test]
    fn emit_procedure_appends_description_suffix_to_last_step() {
        let form = OutputTargetForm::Description("the branch name".into());
        let output = emit_procedure(
            "helper",
            "Returns the branch.",
            &[],
            &[],
            &["Examine the working tree.".into()],
            Some(&form),
            false,
        );
        assert!(
            output.contains(
                "1. Examine the working tree, and return the branch name as your result.\n"
            ),
            "description output_form should append the locked suffix with the text:\n{output}"
        );
    }

    #[test]
    fn emit_procedure_emits_standalone_return_when_no_steps() {
        let form = OutputTargetForm::Identifier("current_branch".into());
        let output = emit_procedure(
            "helper",
            "Returns the branch.",
            &[],
            &[],
            &[],
            Some(&form),
            false,
        );
        assert!(
            output.contains("1. Return current branch as your result.\n"),
            "with no steps, output_form should produce a standalone return line:\n{output}"
        );
    }
}
