//! Scaffold-with-spans intermediate representation. Pure data types + the
//! `build()` walker that turns a resolved `IrArena` into a `Scaffold`.
//! See `obsidian/plans/expand-emitter-design-2026-05-04.md`.

use crate::ir::{IrArena, IrNode, NodeId};
use std::collections::BTreeMap;
use std::collections::HashSet;

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
    DescriptionReturnFold,
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

    // ## Parameters — one ParamDescription span per param
    if !skill.params.is_empty() {
        s.push_literal("## Parameters\n\n");
        for p in &skill.params {
            s.push_literal(format!("- **{}**", p.name));
            // Span for the (currently empty) description.
            let id = SpanId(next_span_id);
            next_span_id += 1;
            s.push_span(SpanRef {
                id,
                kind: SpanKind::ParamDescription,
                ir_node: skill.node_id,
                payload: SpanPayload {
                    param_name: Some(p.name.clone()),
                    param_default: p.default.clone(),
                    ..SpanPayload::default()
                },
            });
            match &p.default {
                Some(v) => s.push_literal(format!(" (default: {})\n", v)),
                None => s.push_literal(" (required)\n"),
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

    if !skill.steps.is_empty() {
        s.push_literal("### Steps\n\n");
        for (idx, step_id) in skill.steps.iter().enumerate() {
            match arena.get(*step_id) {
                IrNode::InlineInstruction(i) => {
                    s.push_literal(format!("{}. {}\n", idx + 1, i.text));
                }
                IrNode::Branch(br) => {
                    // Use a temporary String buffer with the existing emit_branch helper.
                    let mut buf = String::new();
                    super::emit_branch(&mut buf, arena, br, idx + 1);
                    s.push_literal(buf);
                }
                IrNode::Call(c) if c.projection_tier == Some(1) => {
                    s.push_literal(format!(
                        "{}. {}\n",
                        idx + 1,
                        c.resolved_body.as_deref().unwrap_or_default()
                    ));
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
                        "{}. Load and follow the procedure in `{}`.\n",
                        idx + 1,
                        proc_path
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
        let block = arena.nodes().iter().find_map(|n| {
            if let IrNode::Block(b) = n {
                if b.name == *target_name {
                    return Some(b);
                }
            }
            None
        });
        if let Some(block) = block {
            s.push_literal(format!("### Procedure: {}\n\n", kebab_name));
            for (i, stmt) in block.flow_statements.iter().enumerate() {
                s.push_literal(format!("{}. {}\n", i + 1, stmt));
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
            }],
            steps: vec![],
            context: vec![],
            constraints: vec![],
            return_text: None,
            return_type: None,
            output_contract: None,
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
