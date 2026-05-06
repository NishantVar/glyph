//! Branch projection: three pure-`applies()` sub-cases + mixed-condition
//! fallback. See `design/expand.md` §3.3.

use crate::emit::scaffold::{Scaffold, SpanId, SpanKind, SpanPayload, SpanRef};
use crate::ir::{IrArena, IrBranch, IrNode, NodeId};

pub const SINGLE_ARM_OPENER_PREFIX: &str = "Decide whether ";
pub const SINGLE_ARM_OPENER_TAIL: &str = " applies and, if so:";
pub const MULTI_ARM_OPENER: &str =
    "Decide which of the following applies and follow only that path:";

pub fn is_pure_applies(br: &IrBranch) -> bool {
    is_applies_only(&br.condition)
        && br.elif_branches.iter().all(|e| is_applies_only(&e.condition))
}

fn is_applies_only(c: &str) -> bool {
    c.trim().ends_with(".applies()")
}

pub fn extract_block_name(condition: &str) -> Option<String> {
    condition
        .trim()
        .strip_suffix(".applies()")
        .map(str::to_string)
}

pub fn strip_trailing_period(s: &str) -> &str {
    s.trim_end().trim_end_matches('.')
}

pub fn emit_to_scaffold(
    s: &mut Scaffold,
    arena: &IrArena,
    br: &IrBranch,
    step_num: usize,
    next_span_id: &mut u32,
) {
    if is_pure_applies(br) {
        emit_pure_applies(s, arena, br, step_num);
    } else {
        emit_mixed_condition(s, arena, br, step_num, next_span_id);
    }
}

fn emit_pure_applies(s: &mut Scaffold, arena: &IrArena, br: &IrBranch, step_num: usize) {
    let single_arm = br.elif_branches.is_empty() && br.else_body.is_none();
    if single_arm {
        let block_name = extract_block_name(&br.condition).unwrap_or_default();
        let desc = br
            .applies_descriptions
            .as_ref()
            .and_then(|m| m.get(&block_name))
            .cloned()
            .unwrap_or_else(|| block_name.clone());
        let desc = strip_trailing_period(&desc);
        s.push_literal(format!(
            "{step_num}. {SINGLE_ARM_OPENER_PREFIX}{desc}{SINGLE_ARM_OPENER_TAIL}\n"
        ));
        emit_lettered_substeps(s, arena, &br.then_body);
    } else {
        s.push_literal(format!("{step_num}. {MULTI_ARM_OPENER}\n"));
        emit_applies_arm_header_and_body(s, arena, br, &br.condition, &br.then_body);
        for elif in &br.elif_branches {
            emit_applies_arm_header_and_body(s, arena, br, &elif.condition, &elif.body);
        }
        if let Some(else_body) = &br.else_body {
            s.push_literal("   Otherwise:\n");
            emit_lettered_substeps(s, arena, else_body);
        }
    }
}

fn emit_applies_arm_header_and_body(
    s: &mut Scaffold,
    arena: &IrArena,
    br: &IrBranch,
    condition: &str,
    body: &[NodeId],
) {
    let block_name = extract_block_name(condition).unwrap_or_default();
    let desc = br
        .applies_descriptions
        .as_ref()
        .and_then(|m| m.get(&block_name))
        .cloned()
        .unwrap_or_else(|| block_name.clone());
    let desc = strip_trailing_period(&desc);
    s.push_literal(format!("   If {desc}:\n"));
    emit_lettered_substeps(s, arena, body);
}

fn emit_mixed_condition(
    s: &mut Scaffold,
    arena: &IrArena,
    br: &IrBranch,
    step_num: usize,
    next_span_id: &mut u32,
) {
    s.push_literal(format!("{step_num}. If "));
    let id = SpanId(*next_span_id);
    *next_span_id += 1;
    s.push_span(SpanRef {
        id,
        kind: SpanKind::BranchCondition,
        ir_node: br.node_id,
        payload: SpanPayload {
            condition_expression: Some(br.condition.clone()),
            applies_descriptions: br.applies_descriptions.clone(),
            ..SpanPayload::default()
        },
    });
    s.push_literal(":\n");
    emit_lettered_substeps(s, arena, &br.then_body);
    for elif in &br.elif_branches {
        s.push_literal("   If ");
        let id = SpanId(*next_span_id);
        *next_span_id += 1;
        s.push_span(SpanRef {
            id,
            kind: SpanKind::BranchCondition,
            ir_node: br.node_id,
            payload: SpanPayload {
                condition_expression: Some(elif.condition.clone()),
                applies_descriptions: br.applies_descriptions.clone(),
                ..SpanPayload::default()
            },
        });
        s.push_literal(":\n");
        emit_lettered_substeps(s, arena, &elif.body);
    }
    if let Some(else_body) = &br.else_body {
        s.push_literal("   Otherwise:\n");
        emit_lettered_substeps(s, arena, else_body);
    }
}

fn emit_lettered_substeps(s: &mut Scaffold, arena: &IrArena, body: &[NodeId]) {
    let mut letter = b'a';
    for node_id in body {
        let text = match arena.get(*node_id) {
            IrNode::InlineInstruction(i) => i.text.clone(),
            IrNode::Call(c) if c.projection_tier == Some(1) => {
                c.resolved_body.clone().unwrap_or_default()
            }
            IrNode::Call(c) if c.projection_tier == Some(2) => {
                let kebab = crate::emit::templates::kebab_case(&c.target);
                format!("Follow the {kebab} procedure.")
            }
            IrNode::Call(c) if c.projection_tier == Some(3) => {
                let path = c.procedure_path.as_deref().unwrap_or("unknown");
                crate::emit::templates::external_file_step(path)
            }
            IrNode::Call(c) => panic!("Call to `{}` survived past expand", c.target),
            IrNode::Branch(_) => "(nested branch)".into(),
            _ => panic!("Unexpected node type in branch body"),
        };
        s.push_literal(format!("   {}. {}\n", letter as char, text));
        letter += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IrBranch, IrElifBranch, NodeId};
    use std::collections::BTreeMap;

    #[test]
    fn pure_applies_single_arm() {
        let br = IrBranch {
            node_id: NodeId(0),
            condition: "needs_review.applies()".into(),
            then_body: vec![],
            elif_branches: vec![],
            else_body: None,
            applies_descriptions: Some({
                let mut m = BTreeMap::new();
                m.insert("needs_review".into(), "the change needs review".into());
                m
            }),
        };
        assert!(is_pure_applies(&br));
        assert!(br.elif_branches.is_empty());
        assert!(br.else_body.is_none());
    }

    #[test]
    fn pure_applies_multi_arm() {
        let br = IrBranch {
            node_id: NodeId(0),
            condition: "a.applies()".into(),
            then_body: vec![],
            elif_branches: vec![IrElifBranch {
                condition: "b.applies()".into(),
                body: vec![],
            }],
            else_body: None,
            applies_descriptions: None,
        };
        assert!(is_pure_applies(&br));
    }

    #[test]
    fn mixed_condition_is_not_pure_applies() {
        let br = IrBranch {
            node_id: NodeId(0),
            condition: "x == 1".into(),
            then_body: vec![],
            elif_branches: vec![],
            else_body: None,
            applies_descriptions: None,
        };
        assert!(!is_pure_applies(&br));
    }

    #[test]
    fn extract_block_name_basic() {
        assert_eq!(
            extract_block_name("needs_review.applies()"),
            Some("needs_review".to_string())
        );
        assert_eq!(extract_block_name("x == 1"), None);
    }

    #[test]
    fn period_strip_in_arm_header() {
        assert_eq!(
            strip_trailing_period("the change is risky."),
            "the change is risky"
        );
        assert_eq!(
            strip_trailing_period("the change is risky"),
            "the change is risky"
        );
    }
}
