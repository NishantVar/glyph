//! Branch projection: pure-predicate sub-cases + mixed-condition
//! fallback. See `design/expand.md` §3.3.

use crate::condition::ConditionTokenKind;
use crate::emit::scaffold::{Scaffold, SpanId, SpanKind, SpanPayload, SpanRef};
use crate::ir::{IrArena, IrBranch, IrNode, NodeId};

pub const SINGLE_ARM_OPENER_PREFIX: &str = "Decide whether ";
pub const SINGLE_ARM_OPENER_TAIL: &str = " applies and, if so:";
pub const MULTI_ARM_OPENER: &str =
    "Decide which of the following applies and follow only that path:";

pub fn is_pure_predicate(br: &IrBranch) -> bool {
    br.predicate_shape.is_pure_predicate()
        && br
            .elif_branches
            .iter()
            .all(|e| e.predicate_shape.is_pure_predicate())
}

pub fn extract_predicate_token(condition: &str) -> Option<(String, ConditionTokenKind)> {
    let trimmed = condition.trim();

    // Form 1: .applies() — "name.applies()"
    if trimmed.ends_with(".applies()") {
        let stem = &trimmed[..trimmed.len() - ".applies()".len()];
        if !stem.is_empty() && is_bare_identifier(stem) {
            return Some((trimmed.to_string(), ConditionTokenKind::PredicateApplies));
        }
        return None;
    }

    // Form 2: literal — "\"text inside quotes\""
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        let inner = &trimmed[1..trimmed.len() - 1];
        return Some((inner.to_string(), ConditionTokenKind::PredicateLiteral));
    }

    // Form 3: bare identifier const ref
    if is_bare_identifier(trimmed) {
        return Some((trimmed.to_string(), ConditionTokenKind::PredicateConst));
    }

    None
}

fn is_bare_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
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
    // Internal helpers still named `*_applies` — renamed/extended in Task 4.4.
    if is_pure_predicate(br) {
        emit_pure_applies(s, arena, br, step_num);
    } else {
        emit_mixed_condition(s, arena, br, step_num, next_span_id);
    }
}

fn emit_pure_applies(s: &mut Scaffold, arena: &IrArena, br: &IrBranch, step_num: usize) {
    let single_arm = br.elif_branches.is_empty() && br.else_body.is_none();
    if single_arm {
        let (token, _kind) = extract_predicate_token(&br.condition)
            .unwrap_or_else(|| (br.condition.trim().to_string(), ConditionTokenKind::PredicateConst));
        // Lookup key: strip `.applies()` suffix for the applies form (Task 4.4 generalises).
        let lookup_key = token.strip_suffix(".applies()").unwrap_or(&token).to_string();
        let desc = br
            .resolved_predicates
            .as_ref()
            .and_then(|m| m.get(&lookup_key))
            .cloned()
            .unwrap_or_else(|| lookup_key.clone());
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
    let (token, _kind) = extract_predicate_token(condition)
        .unwrap_or_else(|| (condition.trim().to_string(), ConditionTokenKind::PredicateConst));
    // Lookup key: strip `.applies()` suffix for the applies form (Task 4.4 generalises).
    let lookup_key = token.strip_suffix(".applies()").unwrap_or(&token).to_string();
    let desc = br
        .resolved_predicates
        .as_ref()
        .and_then(|m| m.get(&lookup_key))
        .cloned()
        .unwrap_or_else(|| lookup_key.clone());
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
            resolved_predicates: br.resolved_predicates.clone(),
            predicate_shape: br.predicate_shape.clone(),
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
                resolved_predicates: br.resolved_predicates.clone(),
                predicate_shape: elif.predicate_shape.clone(),
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
    use crate::ir::{BranchPredicateShape, IrBranch, IrElifBranch, NodeId};
    use std::collections::BTreeMap;

    #[test]
    fn pure_predicate_single_arm_applies_form() {
        let br = IrBranch {
            node_id: NodeId(0),
            condition: "needs_review.applies()".into(),
            then_body: vec![],
            elif_branches: vec![],
            else_body: None,
            resolved_predicates: Some({
                let mut m = BTreeMap::new();
                m.insert("needs_review".into(), "the change needs review".into());
                m
            }),
            predicate_shape: BranchPredicateShape {
                has_boolean_token: false,
                has_predicate_token: true,
                has_compositional_operator: false,
            },
        };
        assert!(is_pure_predicate(&br));
        assert!(br.elif_branches.is_empty());
        assert!(br.else_body.is_none());
    }

    #[test]
    fn pure_predicate_multi_arm_applies_form() {
        let br = IrBranch {
            node_id: NodeId(0),
            condition: "a.applies()".into(),
            then_body: vec![],
            elif_branches: vec![IrElifBranch {
                condition: "b.applies()".into(),
                body: vec![],
                predicate_shape: BranchPredicateShape {
                    has_boolean_token: false,
                    has_predicate_token: true,
                    has_compositional_operator: false,
                },
            }],
            else_body: None,
            resolved_predicates: None,
            predicate_shape: BranchPredicateShape {
                has_boolean_token: false,
                has_predicate_token: true,
                has_compositional_operator: false,
            },
        };
        assert!(is_pure_predicate(&br));
    }

    #[test]
    fn mixed_condition_is_not_pure_predicate() {
        let br = IrBranch {
            node_id: NodeId(0),
            condition: "x == 1".into(),
            then_body: vec![],
            elif_branches: vec![],
            else_body: None,
            resolved_predicates: None,
            predicate_shape: BranchPredicateShape::default(),
        };
        assert!(!is_pure_predicate(&br));
    }

    #[test]
    fn extract_predicate_token_handles_all_three_forms() {
        let (tok, kind) = extract_predicate_token("my_block.applies()").unwrap();
        assert_eq!(tok, "my_block.applies()");
        assert_eq!(kind, ConditionTokenKind::PredicateApplies);

        let (tok, kind) = extract_predicate_token("complex_change").unwrap();
        assert_eq!(tok, "complex_change");
        assert_eq!(kind, ConditionTokenKind::PredicateConst);

        let (tok, kind) = extract_predicate_token("\"the user opted in\"").unwrap();
        assert_eq!(tok, "the user opted in");
        assert_eq!(kind, ConditionTokenKind::PredicateLiteral);
    }

    #[test]
    fn extract_predicate_token_rejects_compound_condition() {
        assert_eq!(extract_predicate_token("x == 1"), None);
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

    #[test]
    fn pure_predicate_recognises_string_const() {
        let br = IrBranch {
            node_id: NodeId(0),
            condition: "big".into(),
            then_body: vec![],
            elif_branches: vec![],
            else_body: None,
            resolved_predicates: Some({
                let mut m = BTreeMap::new();
                m.insert("big".into(), "the change is big".into());
                m
            }),
            predicate_shape: BranchPredicateShape {
                has_boolean_token: false,
                has_predicate_token: true,
                has_compositional_operator: false,
            },
        };
        assert!(is_pure_predicate(&br));
    }

    #[test]
    fn pure_predicate_recognises_inline_literal() {
        let br = IrBranch {
            node_id: NodeId(0),
            condition: "\"the user opted in\"".into(),
            then_body: vec![],
            elif_branches: vec![],
            else_body: None,
            resolved_predicates: None,
            predicate_shape: BranchPredicateShape {
                has_boolean_token: false,
                has_predicate_token: true,
                has_compositional_operator: false,
            },
        };
        assert!(is_pure_predicate(&br));
    }

    #[test]
    fn pure_predicate_rejects_const_with_not() {
        let br = IrBranch {
            node_id: NodeId(0),
            condition: "not big".into(),
            then_body: vec![],
            elif_branches: vec![],
            else_body: None,
            resolved_predicates: None,
            predicate_shape: BranchPredicateShape {
                has_boolean_token: false,
                has_predicate_token: true,
                has_compositional_operator: true,
            },
        };
        assert!(!is_pure_predicate(&br));
    }
}
