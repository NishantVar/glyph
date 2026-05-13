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

use crate::ir::{IrArena, OutputTargetForm, TypeRegistry};

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
/// - `## Steps` from flow strings
///
/// `output_form` carries the export block's `return <…>` contract when present.
/// Identifier and Description forms route through the locked templates in
/// `emit::templates` so a Tier-3 procedure retains its return contract on disk
/// (`design/compiled-output.md` §OutputContract Rendering).
/// One row in the Tier 3 procedure-file `## Parameters` bullet list. Mirrors
/// the four fields the skill `## Parameters` renderer reads from `IrParam` so
/// the two paths cannot drift on what they project (per `compiled-output.md`
/// §`## Parameters`).
pub struct ProcedureParam<'a> {
    pub name: &'a str,
    pub type_annotation: Option<&'a str>,
    pub description: Option<&'a str>,
    pub default: Option<&'a str>,
}

/// One freeform colon-keyword section (e.g. `quality:`, `risks:`) projected
/// into a Tier 3 procedure `.md` file at H2 depth. Mirrors what the skill
/// emitter reads from `IrFreeformSection` so the two paths cannot drift on
/// what they project (per `compiled-output.md` §Freeform sections).
///
/// Items carry fully-rendered text — the caller is responsible for
/// dereferencing `NameRef`s through their const-text table AND for running
/// `constraint::render` on `require`/`avoid`/`must`/`must avoid` clauses
/// before constructing each `ProcedureFreeformItem`. The IR-driven Tier 1 /
/// Tier 2 path performs the same rendering at lower time; emit must not
/// double-render.
///
/// Uses owned `String` rather than `&str` because the heading is rendered
/// from the source `name` field at call-time (e.g. `acceptance_criteria` →
/// `Acceptance Criteria`); the caller owns the rendered string.
pub struct ProcedureFreeformSection {
    pub heading: String,
    pub items: Vec<ProcedureFreeformItem>,
}

pub struct ProcedureFreeformItem {
    pub text: String,
}

pub fn emit_procedure(
    name: &str,
    description: &str,
    effects: &[String],
    params: &[ProcedureParam<'_>],
    flow_strings: &[String],
    output_form: Option<&OutputTargetForm>,
    return_type_text: Option<&str>,
    type_registry: &TypeRegistry,
    enable_effects: bool,
    freeform_sections: &[ProcedureFreeformSection],
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

    // Parameters — same bullet shape as the skill `## Parameters` emitter.
    // Picks per-param description first, falling back to the type-level
    // `type Foo = <"…">` lookup so Tier 3 procedure files mirror the skill
    // output (compiled-output.md §`## Parameters`).
    if !params.is_empty() {
        out.push_str("## Parameters\n\n");
        for p in params {
            let desc = templates::effective_param_description(
                p.description,
                p.type_annotation,
                type_registry,
            );
            out.push_str(&templates::render_param_bullet(
                p.name,
                p.type_annotation,
                desc.as_deref(),
                p.default,
            ));
        }
        out.push('\n');
    }

    // Steps
    let return_sentence =
        templates::compute_return_sentence(return_type_text, output_form, type_registry);
    let last_step_idx = flow_strings.len().checked_sub(1);
    if !flow_strings.is_empty() {
        out.push_str("## Steps\n\n");
        for (i, step) in flow_strings.iter().enumerate() {
            let body = if Some(i) == last_step_idx {
                match return_sentence.as_deref() {
                    Some(sent) => templates::append_return_sentence(step, sent),
                    None => step.clone(),
                }
            } else {
                step.clone()
            };
            out.push_str(&format!("{}. {}\n", i + 1, body));
        }
    } else if let Some(sent) = return_sentence.as_deref() {
        // No steps but the export block still yields a §8.4 sentence —
        // surface it as the sole step so the contract isn't silently dropped.
        out.push_str("## Steps\n\n");
        out.push_str(&format!("1. {}\n", sent));
    }

    // Freeform colon-keyword sections at peer-level H2 (depth 2) per design
    // §4.1.5 / D12: Tier 3 external file freeform sits at `##`, the top of
    // that document's body. Phase 3.C scope keeps the layout simple — these
    // trail the `## Steps` section. A future cluster can thread author source
    // lines through `emit_procedure` to feed the same D9 merge the skill path
    // uses; the current Tier 3 callers don't carry that metadata.
    if !freeform_sections.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    for section in freeform_sections {
        // H2 heading.
        out.push_str(&format!("## {}\n\n", section.heading));
        // Render items: §4.1.5 — one entry → paragraph, multiple → bulleted list.
        let rendered: Vec<String> = section
            .items
            .iter()
            .map(|item| render_procedure_freeform_item(item))
            .filter(|s| !s.is_empty())
            .collect();
        match rendered.len() {
            0 => {}
            1 => {
                out.push_str(&format!("{}\n\n", rendered[0]));
            }
            _ => {
                for body in &rendered {
                    out.push_str(&format!("- {}\n", body));
                }
                out.push('\n');
            }
        }
    }

    // Trim trailing blank lines for byte-stable output.
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

/// Render one Tier 3 freeform item to its body string. Items arrive
/// pre-rendered by the caller (see `ProcedureFreeformItem` docs) — emit
/// just passes the text through so the skill and procedure paths cannot
/// drift on what a freeform item projects.
fn render_procedure_freeform_item(item: &ProcedureFreeformItem) -> String {
    item.text.clone()
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
            local_refs: Vec::new(),
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
            return_type_text: None,
            return_local_ref: None,
            freeform_sections: Vec::new(),
            description_source_line: None,
            context_source_line: None,
            constraints_source_line: None,
            flow_source_line: None,
        }));
        arena.set_root_skill(skill_id);
        arena
    }

    #[test]
    fn emit_skips_effects_when_disabled() {
        let arena = arena_with_effects();
        let output = emit(&arena, false);
        assert!(
            !output.contains("effects:"),
            "effects line should be omitted when enable_effects is false"
        );
        assert!(
            output.contains("name: test_skill"),
            "name should still be present"
        );
    }

    #[test]
    fn emit_includes_effects_when_enabled() {
        let arena = arena_with_effects();
        let output = emit(&arena, true);
        assert!(
            output.contains("effects: [fs:write, net:http]"),
            "effects line should be present when enable_effects is true"
        );
    }

    #[test]
    fn emit_procedure_skips_effects_when_disabled() {
        let output = emit_procedure(
            "my_proc",
            "A procedure.",
            &["fs:read".to_string()],
            &[],
            &["Step one.".into()],
            None,
            None,
            &TypeRegistry::default(),
            false,
            &[],
        );
        assert!(
            !output.contains("effects:"),
            "effects line should be omitted when enable_effects is false"
        );
        assert!(
            output.contains("name: my-proc"),
            "name should still be present"
        );
    }

    #[test]
    fn emit_procedure_includes_effects_when_enabled() {
        let output = emit_procedure(
            "my_proc",
            "A procedure.",
            &["fs:read".to_string()],
            &[],
            &["Step one.".into()],
            None,
            None,
            &TypeRegistry::default(),
            true,
            &[],
        );
        assert!(
            output.contains("effects: [fs:read]"),
            "effects line should be present when enable_effects is true"
        );
    }

    #[test]
    fn emit_procedure_appends_identifier_sentence_to_last_step() {
        // §8.4 row 4: `return <name>` only, no `-> Foo`.
        let form = OutputTargetForm::Identifier("current_branch".into());
        let output = emit_procedure(
            "helper",
            "Returns the branch.",
            &[],
            &[],
            &["Examine the working tree.".into()],
            Some(&form),
            None,
            &TypeRegistry::default(),
            false,
            &[],
        );
        assert!(
            output.contains("1. Examine the working tree. Produce `current_branch`.\n"),
            "identifier-only output_form should append the §8.4 sentence to the final step:\n{output}"
        );
    }

    #[test]
    fn emit_procedure_appends_description_sentence_to_last_step() {
        // §8.4 row 1: `return <"X">` (descriptive output target).
        let form = OutputTargetForm::Description("the branch name".into());
        let output = emit_procedure(
            "helper",
            "Returns the branch.",
            &[],
            &[],
            &["Examine the working tree.".into()],
            Some(&form),
            None,
            &TypeRegistry::default(),
            false,
            &[],
        );
        assert!(
            output.contains("1. Examine the working tree. Produce: the branch name.\n"),
            "descriptive output_form should append the §8.4 sentence to the final step:\n{output}"
        );
    }

    #[test]
    fn emit_procedure_emits_standalone_sentence_when_no_steps() {
        let form = OutputTargetForm::Identifier("current_branch".into());
        let output = emit_procedure(
            "helper",
            "Returns the branch.",
            &[],
            &[],
            &[],
            Some(&form),
            None,
            &TypeRegistry::default(),
            false,
            &[],
        );
        assert!(
            output.contains("1. Produce `current_branch`.\n"),
            "with no steps, identifier-only output_form should produce a standalone §8.4 sentence:\n{output}"
        );
    }

    /// Phase 3.C / Task 3.9 — Tier 3 freeform sections render at H2 depth
    /// (`## <heading>`) with one-entry-paragraph / multi-entry-bullet shape
    /// per design §4.1.5.
    #[test]
    fn emit_procedure_emits_freeform_sections_at_depth_2() {
        let freeform = vec![ProcedureFreeformSection {
            heading: "Quality".to_string(),
            items: vec![
                ProcedureFreeformItem {
                    text: "Accuracy.".to_string(),
                },
                ProcedureFreeformItem {
                    text: "Completeness.".to_string(),
                },
            ],
        }];
        let output = emit_procedure(
            "helper",
            "Run the workflow.",
            &[],
            &[],
            &["Examine the working tree.".into()],
            None,
            None,
            &TypeRegistry::default(),
            false,
            &freeform,
        );
        assert!(
            output.contains("## Quality\n\n- Accuracy.\n- Completeness.\n"),
            "freeform section should render at H2 with bulleted list for multiple items:\n{output}"
        );
    }

    /// Phase 3.C / Task 3.9 — single-item freeform section renders as a
    /// paragraph under the heading (no bullet) per design §4.1.5.
    #[test]
    fn emit_procedure_single_freeform_item_renders_as_paragraph() {
        let freeform = vec![ProcedureFreeformSection {
            heading: "Quality".to_string(),
            items: vec![ProcedureFreeformItem {
                text: "Accuracy in every step.".to_string(),
            }],
        }];
        let output = emit_procedure(
            "helper",
            "Run the workflow.",
            &[],
            &[],
            &["Examine the working tree.".into()],
            None,
            None,
            &TypeRegistry::default(),
            false,
            &freeform,
        );
        assert!(
            output.contains("## Quality\n\nAccuracy in every step.\n"),
            "single-item freeform should render as paragraph (no bullet):\n{output}"
        );
        assert!(
            !output.contains("- Accuracy"),
            "single-item freeform must not be bulleted:\n{output}"
        );
    }
}
