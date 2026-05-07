//! Scaffold-with-spans intermediate representation. Pure data types + the
//! `build()` walker that turns a resolved `IrArena` into a `Scaffold`.
//! See `obsidian/plans/expand-emitter-design-2026-05-04.md`.

use crate::ir::{IrArena, IrNode, NodeId, OutputTargetForm};
use super::templates;
use std::collections::BTreeMap;
use std::collections::HashSet;

/// Look up the `OutputTargetForm` for a block by name, returning an owned clone.
fn block_output_form_owned(arena: &IrArena, block_name: &str) -> Option<OutputTargetForm> {
    for node in arena.nodes() {
        if let IrNode::Block(b) = node {
            if b.name == block_name {
                if let Some(oc_id) = b.output_contract {
                    if let IrNode::OutputContract(oc) = arena.get(oc_id) {
                        return Some(oc.form.clone());
                    }
                }
                return None;
            }
        }
    }
    None
}

/// Look up the source-text `-> Foo` annotation for a block by name. Mirrors
/// `block_output_form_owned`; needed by §8.4 templates because the canonical
/// `return_type` loses casing.
fn block_return_type_text_owned(arena: &IrArena, block_name: &str) -> Option<String> {
    for node in arena.nodes() {
        if let IrNode::Block(b) = node {
            if b.name == block_name {
                return b.return_type_text.clone();
            }
        }
    }
    None
}

/// Look up the `OutputTargetForm` for the skill's output_contract, returning an owned clone.
fn skill_output_form_owned(arena: &IrArena) -> Option<OutputTargetForm> {
    let root_id = arena.root_skill()?;
    if let IrNode::Skill(s) = arena.get(root_id) {
        if let Some(oc_id) = s.output_contract {
            if let IrNode::OutputContract(oc) = arena.get(oc_id) {
                return Some(oc.form.clone());
            }
        }
    }
    None
}

/// Look up the source-text `-> Foo` annotation on the root skill.
fn skill_return_type_text_owned(arena: &IrArena) -> Option<String> {
    let root_id = arena.root_skill()?;
    if let IrNode::Skill(s) = arena.get(root_id) {
        return s.return_type_text.clone();
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SpanId(pub u32);

#[derive(Clone, Debug)]
pub enum Chunk {
    Literal(String),
    Span(SpanRef),
}

#[derive(Clone, Debug)]
pub struct SpanRef {
    pub id: SpanId,
    pub kind: SpanKind,
    pub ir_node: NodeId,
    pub payload: SpanPayload,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanKind {
    ParamDescription,
    BranchCondition,
    CallBodyShape,
}

#[derive(Clone, Debug, Default)]
pub struct SpanPayload {
    pub site_modifier: Option<String>,
    pub resolved_body: Option<String>,
    pub description_text: Option<String>,
    pub condition_expression: Option<String>,
    pub applies_descriptions: Option<BTreeMap<String, String>>,
    pub param_name: Option<String>,
    pub param_type: Option<String>,
    pub param_default: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Scaffold {
    pub chunks: Vec<Chunk>,
}

impl Scaffold {
    pub fn push_literal(&mut self, s: impl Into<String>) {
        self.chunks.push(Chunk::Literal(s.into()));
    }
    pub fn push_span(&mut self, span: SpanRef) {
        self.chunks.push(Chunk::Span(span));
    }
}

pub fn build(arena: &IrArena, enable_effects: bool) -> Scaffold {
    let root_id = arena
        .root_skill()
        .expect("validate guarantees a root skill before emit");
    let skill = match arena.get(root_id) {
        IrNode::Skill(s) => s,
        _ => unreachable!("root skill ID must point to a Skill node"),
    };

    let mut s = Scaffold::default();
    let mut next_span_id: u32 = 0;

    // Frontmatter
    s.push_literal("---\n");
    s.push_literal(format!("name: {}\n", skill.name));
    s.push_literal(format!("description: {}\n", skill.description));
    if enable_effects && !skill.effects.is_empty() {
        let mut sorted_effects = skill.effects.clone();
        sorted_effects.sort();
        s.push_literal(format!("effects: [{}]\n", sorted_effects.join(", ")));
    }
    s.push_literal("---\n\n");

    // ## Parameters — one ParamDescription span per param (sentence-style)
    if !skill.params.is_empty() {
        s.push_literal("## Parameters\n\n");
        for p in &skill.params {
            // Build the type annotation suffix if present.
            let type_suffix = match &p.type_annotation {
                Some(t) => format!(" ({})", t),
                None => String::new(),
            };
            // Metadata tail: "Default: X." or "Required."
            let meta_tail = match &p.default {
                Some(v) => format!("Default: {}.", v),
                None => "Required.".to_string(),
            };

            // Emit bullet header and span, then description+metadata.
            // Effective description: per-param wins, else type-level (from registry), else none.
            let effective_desc: Option<String> = p.description.clone().or_else(|| {
                p.type_annotation
                    .as_ref()
                    .and_then(|t| arena.type_registry.descriptions.get(t).cloned())
            });
            let has_desc = effective_desc.is_some();
            let desc_text = effective_desc.as_deref().unwrap_or("");
            let is_multiline = has_desc && (desc_text.contains('\n') || desc_text.len() > 120);

            if is_multiline {
                // Multi-line form:
                //   - **<name>**[ (<Type>)]:
                //     <description lines>
                //     Default: X. / Required.
                s.push_literal(format!("- **{}**{}:\n", p.name, type_suffix));
                let id = SpanId(next_span_id);
                next_span_id += 1;
                s.push_span(SpanRef {
                    id,
                    kind: SpanKind::ParamDescription,
                    ir_node: skill.node_id,
                    payload: SpanPayload {
                        param_name: Some(p.name.clone()),
                        param_type: p.type_annotation.clone(),
                        param_default: p.default.clone(),
                        description_text: effective_desc.clone(),
                        ..SpanPayload::default()
                    },
                });
                for line in desc_text.lines() {
                    s.push_literal(format!("  {}\n", line));
                }
                s.push_literal(format!("  {}\n", meta_tail));
            } else if has_desc {
                // Single-line description form:
                //   - **<name>**[ (<Type>)]: <description>. Default: X. / Required.
                let trimmed = desc_text.trim_end_matches('.').trim_end();
                s.push_literal(format!("- **{}**{}: ", p.name, type_suffix));
                let id = SpanId(next_span_id);
                next_span_id += 1;
                s.push_span(SpanRef {
                    id,
                    kind: SpanKind::ParamDescription,
                    ir_node: skill.node_id,
                    payload: SpanPayload {
                        param_name: Some(p.name.clone()),
                        param_type: p.type_annotation.clone(),
                        param_default: p.default.clone(),
                        description_text: effective_desc.clone(),
                        ..SpanPayload::default()
                    },
                });
                s.push_literal(format!("{}. {}\n", trimmed, meta_tail));
            } else {
                // No description form:
                //   - **<name>**[ (<Type>)]. Default: X. / Required.
                s.push_literal(format!("- **{}**{}. ", p.name, type_suffix));
                let id = SpanId(next_span_id);
                next_span_id += 1;
                s.push_span(SpanRef {
                    id,
                    kind: SpanKind::ParamDescription,
                    ir_node: skill.node_id,
                    payload: SpanPayload {
                        param_name: Some(p.name.clone()),
                        param_type: p.type_annotation.clone(),
                        param_default: p.default.clone(),
                        description_text: effective_desc.clone(),
                        ..SpanPayload::default()
                    },
                });
                s.push_literal(format!("{}\n", meta_tail));
            }
        }
        s.push_literal("\n");
    }

    // ## Instructions
    s.push_literal("## Instructions\n\n");

    // ### Context
    if !skill.context.is_empty() {
        s.push_literal("### Context\n\n");
        for ctx_id in &skill.context {
            let text = match arena.get(*ctx_id) {
                IrNode::Context(c) => c.text.clone(),
                _ => panic!("Context node was not a Context"),
            };
            s.push_literal(format!("- {}\n", text));
        }
        s.push_literal("\n");
    }

    // ### Steps
    let mut procedure_order: Vec<String> = Vec::new();
    let mut procedure_seen: HashSet<String> = HashSet::new();

    // Pre-compute skill output_contract form once (owned), for use in the
    // last-step suffix logic below.
    let skill_oc_form = skill_output_form_owned(arena);
    let skill_rt_text = skill_return_type_text_owned(arena);
    let skill_step_count = skill.steps.len();
    let skill_has_return_sentence = templates::compute_return_sentence(
        skill_rt_text.as_deref(),
        skill_oc_form.as_ref(),
        &arena.type_registry,
    )
    .is_some();

    if skill_step_count > 0 || skill_has_return_sentence {
        s.push_literal("### Steps\n\n");

        if skill_step_count == 0 {
            // Return-only skill: no flow steps but has a contract that yields a
            // §8.4 sentence. Emit it as the sole step.
            let sentence = templates::compute_return_sentence(
                skill_rt_text.as_deref(),
                skill_oc_form.as_ref(),
                &arena.type_registry,
            )
            .expect("guarded by skill_has_return_sentence");
            s.push_literal(format!("1. {}\n", sentence));
        } else {
            for (idx, step_id) in skill.steps.iter().enumerate() {
                let is_last = idx + 1 == skill_step_count;
                match arena.get(*step_id) {
                    IrNode::InlineInstruction(i) => {
                        if is_last {
                            let sentence = templates::compute_return_sentence(
                                skill_rt_text.as_deref(),
                                skill_oc_form.as_ref(),
                                &arena.type_registry,
                            );
                            match sentence {
                                Some(sent) => {
                                    let body = templates::append_return_sentence(&i.text, &sent);
                                    s.push_literal(format!("{}. {}\n", idx + 1, body));
                                }
                                None => {
                                    s.push_literal(format!("{}. {}\n", idx + 1, i.text));
                                }
                            }
                        } else {
                            s.push_literal(format!("{}. {}\n", idx + 1, i.text));
                        }
                    }
                    IrNode::Branch(br) => {
                        super::branch::emit_to_scaffold(&mut s, arena, br, idx + 1, &mut next_span_id);
                    }
                    IrNode::Call(c) if c.projection_tier == Some(1) => {
                        let body = c.resolved_body.as_deref().unwrap_or_default();
                        if is_last {
                            // For tier-1 calls, the enclosing skill's output_contract
                            // wins when both exist: the skill's `return <…>` is the
                            // author's stated final return, so its template must take
                            // precedence over the inlined callee's contract.
                            // (`design/expand.md` §3.5;
                            // `design/compiled-output.md` §OutputContract Rendering.)
                            // The callee's OC is read directly off the Call node —
                            // populated at lower time for same-file callees and at
                            // the cross-file import fix-up for imported callees.
                            let (effective_form, effective_rt) = match skill_oc_form.as_ref() {
                                Some(form) => (Some(form), skill_rt_text.as_deref()),
                                None => (
                                    c.callee_output_contract.as_ref(),
                                    c.callee_return_type_text.as_deref(),
                                ),
                            };
                            let sentence = templates::compute_return_sentence(
                                effective_rt,
                                effective_form,
                                &arena.type_registry,
                            );
                            // A return-only callee (e.g. `block helper: do { return <x> }`)
                            // inlines with an empty resolved_body. Suffixing onto an
                            // empty body would yield a malformed leading-comma line;
                            // emit the §8.4 sentence as a standalone step instead.
                            let body_is_empty = body.trim().is_empty();
                            match (sentence, body_is_empty) {
                                (Some(sent), true) => {
                                    s.push_literal(format!("{}. {}\n", idx + 1, sent));
                                }
                                (Some(sent), false) => {
                                    let folded = templates::append_return_sentence(body, &sent);
                                    s.push_literal(format!("{}. {}\n", idx + 1, folded));
                                }
                                (None, _) => {
                                    s.push_literal(format!("{}. {}\n", idx + 1, body));
                                }
                            }
                        } else {
                            s.push_literal(format!("{}. {}\n", idx + 1, body));
                        }
                    }
                    IrNode::Call(c) if c.projection_tier == Some(2) => {
                        let kebab_name = c.target.replace('_', "-");
                        s.push_literal(format!(
                            "{}. Follow the {} procedure below.\n",
                            idx + 1,
                            kebab_name
                        ));
                        if procedure_seen.insert(c.target.clone()) {
                            procedure_order.push(c.target.clone());
                        }
                    }
                    IrNode::Call(c) if c.projection_tier == Some(3) => {
                        let proc_path = c.procedure_path.as_deref().unwrap_or("unknown");
                        s.push_literal(format!(
                            "{}. {}\n",
                            idx + 1,
                            templates::external_file_step(proc_path)
                        ));
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
        }
        s.push_literal("\n");
    }

    // ### Constraints
    if !skill.constraints.is_empty() {
        s.push_literal("### Constraints\n\n");
        for c_id in &skill.constraints {
            let c = match arena.get(*c_id) {
                IrNode::Constraint(c) => c,
                _ => panic!("Constraint node was not a Constraint"),
            };
            let line = crate::emit::constraint::render(c.strength, c.polarity, &c.text);
            s.push_literal(format!("- {}\n", line));
        }
        s.push_literal("\n");
    }

    // ### Procedure: <name> sections
    for target_name in &procedure_order {
        let kebab_name = target_name.replace('_', "-");
        // Collect the block's flow_statements + contract metadata before emitting.
        let (flow_stmts, proc_oc_form, proc_rt_text) = {
            let mut stmts: Option<Vec<String>> = None;
            let mut oc: Option<OutputTargetForm> = None;
            let mut rt: Option<String> = None;
            for node in arena.nodes() {
                if let IrNode::Block(b) = node {
                    if b.name == *target_name {
                        stmts = Some(b.flow_statements.clone());
                        oc = block_output_form_owned(arena, target_name);
                        rt = block_return_type_text_owned(arena, target_name);
                        break;
                    }
                }
            }
            (stmts, oc, rt)
        };
        if let Some(stmts) = flow_stmts {
            s.push_literal(format!("### Procedure: {}\n\n", kebab_name));
            // Filter out raw "return" markers; they are replaced by the §8.4 sentence.
            let visible_stmts: Vec<&String> = stmts.iter().filter(|st| st.as_str() != "return").collect();
            let visible_count = visible_stmts.len();
            let proc_sentence = templates::compute_return_sentence(
                proc_rt_text.as_deref(),
                proc_oc_form.as_ref(),
                &arena.type_registry,
            );

            if visible_count == 0 && proc_sentence.is_some() {
                // Return-only block: emit the §8.4 sentence as a standalone step.
                s.push_literal(format!("1. {}\n", proc_sentence.unwrap()));
            } else {
                for (i, stmt) in visible_stmts.iter().enumerate() {
                    let is_last = i + 1 == visible_count;
                    if is_last {
                        match proc_sentence.as_deref() {
                            Some(sent) => {
                                let body = templates::append_return_sentence(stmt, sent);
                                s.push_literal(format!("{}. {}\n", i + 1, body));
                            }
                            None => {
                                s.push_literal(format!("{}. {}\n", i + 1, stmt));
                            }
                        }
                    } else {
                        s.push_literal(format!("{}. {}\n", i + 1, stmt));
                    }
                }
            }
            s.push_literal("\n");
        }
    }

    // Trim trailing blank line — pop chunks/chars until output doesn't end with "\n\n".
    trim_trailing_blank_line(&mut s);

    s
}

fn trim_trailing_blank_line(s: &mut Scaffold) {
    // The last chunk (if any) is a Literal in the patterns above. If it ends with
    // a redundant trailing newline, trim. The cheapest correct implementation is to
    // walk the tail of `chunks` and pop newlines.
    loop {
        match s.chunks.last_mut() {
            Some(Chunk::Literal(text)) => {
                while text.ends_with("\n\n") {
                    text.pop();
                }
                if text.is_empty() {
                    s.chunks.pop();
                    continue;
                }
                break;
            }
            _ => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IrArena, IrNode, IrParam, IrSkill, NodeId};

    #[test]
    fn build_parameters_section_emits_one_span_per_param() {
        let mut arena = IrArena::new();
        let s_id = arena.push(IrNode::Skill(IrSkill {
            node_id: NodeId(0),
            name: "demo".into(),
            description: "Demo.".into(),
            effects: vec![],
            params: vec![IrParam {
                name: "branch".into(),
                default: None,
                description: None,
                type_annotation: None,
            }],
            steps: vec![],
            context: vec![],
            constraints: vec![],
            return_text: None,
            return_type: None,
            output_contract: None,
            return_type_text: None,
        }));
        arena.set_root_skill(s_id);

        let scaffold = build(&arena, false);
        let span_count = scaffold
            .chunks
            .iter()
            .filter(|c| matches!(c, Chunk::Span(sp) if sp.kind == SpanKind::ParamDescription))
            .count();
        assert_eq!(span_count, 1, "one ParamDescription span per param");
    }
}
