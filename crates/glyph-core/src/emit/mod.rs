//! Phase 7 (Emit) — deterministic Markdown projection.
//!
//! Walking-skeleton scope per `docs/reference/mvp-acceptance.md` §1: parameterless skill,
//! inline strings as Steps, constraint markers as bulleted Constraints. The output
//! shape is fixed by `design/compiled-output.md`.

pub(crate) mod branch;
pub(crate) mod constraint;
pub(crate) mod merger;
pub(crate) mod scaffold;
pub(crate) mod stub_fill;
pub(crate) mod templates;

use crate::ir::{IrArena, OutputTargetForm, TypeRegistry};

pub use stub_fill::StubFillError;

pub fn emit(arena: &IrArena, enable_effects: bool) -> Result<String, Vec<StubFillError>> {
    let scaffold = scaffold::build(arena, enable_effects);
    let fills = stub_fill::fill(&scaffold)?;
    Ok(merger::merge(scaffold, fills).expect("scaffold/fill mismatch is a bug"))
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

/// #168: Tier 3 procedure preamble — body-level constraint marker
/// resolved to its (strength, polarity, text) triple. Mirrors `IrConstraint`
/// but flat (no `NodeId`), so it can be threaded through the AST-driven
/// `emit_library_procedures` caller without exposing IR types here.
pub struct ProcedureConstraint<'a> {
    pub strength: crate::ir::Strength,
    pub polarity: crate::ir::Polarity,
    pub text: &'a str,
}

/// #168: Tier 3 procedure preamble — body-level context entry resolved
/// to its `(name, text)` pair. `name` is the source identifier (used to
/// render the bold `**<kebab>:**` label); `None` means the entry was an
/// inline string and renders under the generic `**Context:**` label.
pub struct ProcedureContext<'a> {
    pub name: Option<&'a str>,
    pub text: &'a str,
}

pub fn emit_procedure(
    name: &str,
    description: &str,
    effects: &[String],
    params: &[ProcedureParam<'_>],
    flow_items: &[crate::ir::IrBlockFlowItem],
    arena: &IrArena,
    output_form: Option<&OutputTargetForm>,
    return_type_text: Option<&str>,
    type_registry: &TypeRegistry,
    enable_effects: bool,
    freeform_sections: &[ProcedureFreeformSection],
    constraints: &[ProcedureConstraint<'_>],
    context: &[ProcedureContext<'_>],
) -> Result<String, Vec<StubFillError>> {
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

    // #168: Tier 3 procedure preamble — body-level constraints and context
    // declared on the export block render between `## Parameters` and `## Steps`,
    // mirroring the Tier 2 (same-file) layout. Format matches the Tier 2 path
    // emitter in `scaffold.rs::build`: four-form constraint template +
    // `**<kebab>:** <text>.` for named context, `**Context:** <text>.` otherwise.
    let mut had_preamble = false;
    for c in constraints {
        let line =
            crate::sections::hooks::dispatch_constraints_expand(c.strength, c.polarity, c.text);
        out.push_str(&format!("{}\n\n", line));
        had_preamble = true;
    }
    for c in context {
        let label = match c.name {
            Some(n) => n.replace('_', "-"),
            None => "Context".to_string(),
        };
        let body = c.text.trim();
        let needs_period = !matches!(body.chars().last(), Some('.') | Some('!') | Some('?'));
        let suffix = if needs_period { "." } else { "" };
        out.push_str(&format!("**{}:** {}{}\n\n", label, body, suffix));
        had_preamble = true;
    }
    // Each preamble line already ends with `\n\n`; the final `\n\n`
    // supplies the blank line separator before the `## Steps` heading.
    let _ = had_preamble;

    // Steps — drive emission through the same shared Scaffold/Span/
    // stub_fill/merger pipeline the in-skill emitter uses, so procedure-
    // body Calls route through the `push_call_body` helper alongside
    // the seven other emit sites (Task 14: 8th call-emit surface).
    let return_sentence =
        templates::compute_return_sentence(return_type_text, output_form, type_registry);
    let visible_count = flow_items
        .iter()
        .filter(|it| !matches!(it, crate::ir::IrBlockFlowItem::Return))
        .count();

    if visible_count > 0 {
        let mut scaffold = scaffold::Scaffold::default();
        let mut next_span_id: u32 = 0;
        scaffold.push_literal("## Steps\n\n".to_string());

        let mut visible_idx: usize = 0;
        for item in flow_items {
            if matches!(item, crate::ir::IrBlockFlowItem::Return) {
                continue;
            }
            visible_idx += 1;
            let step_num = visible_idx;
            let is_last = visible_idx == visible_count;
            match item {
                crate::ir::IrBlockFlowItem::Inline { text } => {
                    let body = if is_last {
                        match return_sentence.as_deref() {
                            Some(sent) => templates::append_return_sentence(text, sent),
                            None => text.clone(),
                        }
                    } else {
                        text.clone()
                    };
                    scaffold.push_literal(format!("{}. {}\n", step_num, body));
                }
                crate::ir::IrBlockFlowItem::Call { node_id } => {
                    if let crate::ir::IrNode::Call(c) = arena.get(*node_id) {
                        scaffold.push_literal(format!("{}. ", step_num));
                        match c.projection_tier {
                            Some(1) => {
                                let raw_body = c.resolved_body.as_deref().unwrap_or_default();
                                let rs = if is_last {
                                    return_sentence.clone()
                                } else {
                                    None
                                };
                                scaffold::push_call_body(
                                    &mut scaffold,
                                    c,
                                    raw_body,
                                    Some(scaffold::Tier1FoldCtx {
                                        is_last,
                                        return_sentence: rs,
                                    }),
                                    &mut next_span_id,
                                );
                            }
                            Some(2) => {
                                let callee_kebab = c.target.replace('_', "-");
                                let anchor = format!("Follow the {callee_kebab} procedure below.");
                                scaffold::push_call_body(
                                    &mut scaffold,
                                    c,
                                    &anchor,
                                    None,
                                    &mut next_span_id,
                                );
                            }
                            Some(3) => {
                                let proc_path = c.procedure_path.as_deref().unwrap_or("unknown");
                                let anchor = templates::external_file_step(proc_path);
                                scaffold::push_call_body(
                                    &mut scaffold,
                                    c,
                                    &anchor,
                                    None,
                                    &mut next_span_id,
                                );
                            }
                            _ if c.bound_name.is_some() => {
                                let anchor = format!("Call `{}`.", c.target);
                                scaffold::push_call_body(
                                    &mut scaffold,
                                    c,
                                    &anchor,
                                    None,
                                    &mut next_span_id,
                                );
                            }
                            _ => {
                                panic!(
                                    "IrCall to `{}` survived past expand without tier assignment",
                                    c.target
                                );
                            }
                        }
                    }
                }
                crate::ir::IrBlockFlowItem::Branch { node_id } => {
                    if let crate::ir::IrNode::Branch(br) = arena.get(*node_id) {
                        branch::emit_to_scaffold(
                            &mut scaffold,
                            arena,
                            br,
                            step_num,
                            &mut next_span_id,
                        );
                    }
                }
                crate::ir::IrBlockFlowItem::Constraint { rendered }
                | crate::ir::IrBlockFlowItem::Context { rendered } => {
                    scaffold.push_literal(format!("{}. {}\n", step_num, rendered));
                }
                crate::ir::IrBlockFlowItem::BareName { name } => {
                    scaffold.push_literal(format!("{}. {}\n", step_num, name));
                }
                crate::ir::IrBlockFlowItem::Return => unreachable!(),
            }
        }

        // Local-pipeline: stub_fill + merger. CallBodyShape spans here
        // surface as `Vec<StubFillError>` returned to the caller, who
        // maps them into `G::expand::llm-required-for-call` diagnostics.
        let fills = stub_fill::fill(&scaffold)?;
        let steps_md = merger::merge(scaffold, fills)
            .expect("local pipeline scaffold has no unknown/missing spans");
        out.push_str(&steps_md);
    } else if let Some(sent) = return_sentence.as_deref() {
        // No steps but the export block still yields a §8.4 sentence —
        // surface it as the sole step so the contract isn't silently dropped.
        out.push_str("## Steps\n\n");
        out.push_str(&format!("1. {}\n", sent));
    }

    // Freeform colon-keyword sections at peer-level H2 (depth 2) per design
    // §4.1.5 / D12.
    if !freeform_sections.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    for section in freeform_sections {
        out.push_str(&format!("## {}\n\n", section.heading));
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
    Ok(out)
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

    /// Task 14 — local helper: turn a slice of `&str` into the structured
    /// `IrBlockFlowItem::Inline` vector the new `emit_procedure` signature
    /// expects. All test fixtures use plain inline text.
    fn flow_items_from_strs(items: &[&str]) -> Vec<crate::ir::IrBlockFlowItem> {
        items
            .iter()
            .map(|s| crate::ir::IrBlockFlowItem::Inline {
                text: (*s).to_string(),
            })
            .collect()
    }

    /// Task 14 — local helper: empty arena stub for tests that pass only
    /// `IrBlockFlowItem::Inline` items (no Call/Branch references).
    fn empty_arena() -> IrArena {
        IrArena::new()
    }

    #[test]
    fn emit_skips_effects_when_disabled() {
        let arena = arena_with_effects();
        let output = emit(&arena, false).expect("trivial skill must compile");
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
        let output = emit(&arena, true).expect("trivial skill must compile");
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
            &flow_items_from_strs(&["Step one."]),
            &empty_arena(),
            None,
            None,
            &TypeRegistry::default(),
            false,
            &[],
            &[],
            &[],
        )
        .expect("test fixture: emit_procedure should not surface CallBodyShape errors");
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
            &flow_items_from_strs(&["Step one."]),
            &empty_arena(),
            None,
            None,
            &TypeRegistry::default(),
            true,
            &[],
            &[],
            &[],
        )
        .expect("test fixture: emit_procedure should not surface CallBodyShape errors");
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
            &flow_items_from_strs(&["Examine the working tree."]),
            &empty_arena(),
            Some(&form),
            None,
            &TypeRegistry::default(),
            false,
            &[],
            &[],
            &[],
        )
        .expect("test fixture: emit_procedure should not surface CallBodyShape errors");
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
            &flow_items_from_strs(&["Examine the working tree."]),
            &empty_arena(),
            Some(&form),
            None,
            &TypeRegistry::default(),
            false,
            &[],
            &[],
            &[],
        )
        .expect("test fixture: emit_procedure should not surface CallBodyShape errors");
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
            &flow_items_from_strs(&[]),
            &empty_arena(),
            Some(&form),
            None,
            &TypeRegistry::default(),
            false,
            &[],
            &[],
            &[],
        )
        .expect("test fixture: emit_procedure should not surface CallBodyShape errors");
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
            &flow_items_from_strs(&["Examine the working tree."]),
            &empty_arena(),
            None,
            None,
            &TypeRegistry::default(),
            false,
            &freeform,
            &[],
            &[],
        )
        .expect("test fixture: emit_procedure should not surface CallBodyShape errors");
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
            &flow_items_from_strs(&["Examine the working tree."]),
            &empty_arena(),
            None,
            None,
            &TypeRegistry::default(),
            false,
            &freeform,
            &[],
            &[],
        )
        .expect("test fixture: emit_procedure should not surface CallBodyShape errors");
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
