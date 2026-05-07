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
    // Strip trailing `:` — the parser includes it in the condition string.
    // TODO: strip the trailing `:` once at IR construction time
    // (lower.rs / parse.rs) so consumers (analyze, expand, emit)
    // don't each have to redo this work. See expand.rs near line 187 for the same TODO.
    let trimmed = condition.trim().trim_end_matches(':').trim();

    // Form 1: .applies() — "name.applies()"
    if trimmed.ends_with(".applies()") {
        let stem = &trimmed[..trimmed.len() - ".applies()".len()];
        if !stem.is_empty() && is_ident(stem) {
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
    if is_ident(trimmed) {
        return Some((trimmed.to_string(), ConditionTokenKind::PredicateConst));
    }

    None
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// For PredicateApplies form, `resolved_predicates` is keyed by the bare
// block name (without `.applies()`). For other forms, key is the token text.
pub(crate) fn lookup_key_for_token(token: &str, kind: ConditionTokenKind) -> &str {
    match kind {
        ConditionTokenKind::PredicateApplies => token.strip_suffix(".applies()").unwrap_or(token),
        _ => token,
    }
}

pub fn strip_trailing_period(s: &str) -> &str {
    s.trim_end().trim_end_matches('.')
}

/// Render a condition by walking its classified tokens and substituting
/// predicate tokens with their resolved values from `resolved_predicates`.
/// Operand tokens, operators, parens, and unknown tokens pass through verbatim.
///
/// For `PredicateApplies`: lookup key is the receiver name (`name.applies()` →
/// `name`), matching `branch.rs::lookup_key_for_token`.
///
/// For `PredicateConst`: lookup key is the bare token text.
///
/// For `PredicateLiteral`: emit the literal text with surrounding quotes
/// stripped (matches the existing `extract_predicate_token` contract for
/// quoted literals).
pub(crate) fn render_substituted_condition(
    classification: &crate::condition::ConditionClassification,
    resolved_predicates: &std::collections::BTreeMap<String, String>,
) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(classification.tokens.len());
    for tok in &classification.tokens {
        if tok.is_comparison_operand {
            parts.push(tok.text.clone());
            continue;
        }
        match tok.kind {
            ConditionTokenKind::PredicateApplies => {
                let key = tok.text.trim_end_matches(".applies()");
                parts.push(
                    resolved_predicates
                        .get(key)
                        .cloned()
                        .unwrap_or_else(|| tok.text.clone()),
                );
            }
            ConditionTokenKind::PredicateConst => {
                parts.push(
                    resolved_predicates
                        .get(&tok.text)
                        .cloned()
                        .unwrap_or_else(|| tok.text.clone()),
                );
            }
            ConditionTokenKind::PredicateLiteral => {
                let inner = tok.text.trim_start_matches('"').trim_end_matches('"');
                parts.push(inner.to_string());
            }
            _ => parts.push(tok.text.clone()),
        }
    }
    parts.join(" ")
}

pub fn emit_to_scaffold(
    s: &mut Scaffold,
    arena: &IrArena,
    br: &IrBranch,
    step_num: usize,
    next_span_id: &mut u32,
) {
    if is_pure_predicate(br) {
        emit_pure_predicate(s, arena, br, step_num);
    } else {
        emit_mixed_condition(s, arena, br, step_num, next_span_id);
    }
}

fn emit_pure_predicate(s: &mut Scaffold, arena: &IrArena, br: &IrBranch, step_num: usize) {
    let single_arm = br.elif_branches.is_empty() && br.else_body.is_none();
    if single_arm {
        let desc_owned = render_condition_for_arm(
            &br.condition,
            br.classification.as_ref(),
            br.resolved_predicates.as_ref(),
        );
        let desc = strip_trailing_period(&desc_owned);
        s.push_literal(format!(
            "{step_num}. {SINGLE_ARM_OPENER_PREFIX}{desc}{SINGLE_ARM_OPENER_TAIL}\n"
        ));
        emit_lettered_substeps(s, arena, &br.then_body);
    } else {
        s.push_literal(format!("{step_num}. {MULTI_ARM_OPENER}\n"));
        emit_predicate_arm_header_and_body(
            s,
            arena,
            br,
            &br.condition,
            br.classification.as_ref(),
            &br.then_body,
        );
        for elif in &br.elif_branches {
            emit_predicate_arm_header_and_body(
                s,
                arena,
                br,
                &elif.condition,
                elif.classification.as_ref(),
                &elif.body,
            );
        }
        if let Some(else_body) = &br.else_body {
            s.push_literal("   Otherwise:\n");
            emit_lettered_substeps(s, arena, else_body);
        }
    }
}

/// Render the natural-language description of a condition for use inside an
/// arm header. When classification + resolved_predicates are present, walks
/// every token and substitutes predicate tokens (Task 8). Otherwise falls
/// back to the legacy single-token extraction path — preserved so emit still
/// works on JSON-loaded IR (where `classification` is `#[serde(skip)]`).
fn render_condition_for_arm(
    condition: &str,
    classification: Option<&crate::condition::ConditionClassification>,
    resolved_predicates: Option<&std::collections::BTreeMap<String, String>>,
) -> String {
    if let (Some(c), Some(rp)) = (classification, resolved_predicates) {
        return render_substituted_condition(c, rp);
    }
    let (token, kind) = extract_predicate_token(condition).unwrap_or_else(|| {
        (
            condition.trim().to_string(),
            ConditionTokenKind::PredicateConst,
        )
    });
    resolve_predicate_prose_legacy(&token, kind, resolved_predicates)
}

/// Legacy variant of the predicate prose resolver that doesn't require a full
/// `IrBranch` — only `resolved_predicates`. Used by the JSON-loaded fallback
/// path (where `IrBranch.classification` is `#[serde(skip)]` and absent).
fn resolve_predicate_prose_legacy(
    token: &str,
    kind: ConditionTokenKind,
    resolved_predicates: Option<&std::collections::BTreeMap<String, String>>,
) -> String {
    match kind {
        ConditionTokenKind::PredicateLiteral => token.to_string(),
        ConditionTokenKind::Boolean
        | ConditionTokenKind::Numeric
        | ConditionTokenKind::Operator => {
            unreachable!("non-predicate token reached resolve_predicate_prose_legacy")
        }
        ConditionTokenKind::PredicateApplies | ConditionTokenKind::PredicateConst => {
            let lookup_key = lookup_key_for_token(token, kind);
            resolved_predicates
                .and_then(|m| m.get(lookup_key))
                .cloned()
                .unwrap_or_else(|| lookup_key.to_string())
        }
    }
}

fn emit_predicate_arm_header_and_body(
    s: &mut Scaffold,
    arena: &IrArena,
    br: &IrBranch,
    condition: &str,
    classification: Option<&crate::condition::ConditionClassification>,
    body: &[NodeId],
) {
    let desc_owned =
        render_condition_for_arm(condition, classification, br.resolved_predicates.as_ref());
    let desc = strip_trailing_period(&desc_owned);
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
            classification: br.classification.clone(),
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
                classification: elif.classification.clone(),
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
            classification: None,
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
                classification: None,
            }],
            else_body: None,
            resolved_predicates: None,
            predicate_shape: BranchPredicateShape {
                has_boolean_token: false,
                has_predicate_token: true,
                has_compositional_operator: false,
            },
            classification: None,
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
            classification: None,
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
        // operator-joined conditions
        assert!(extract_predicate_token("x == 1").is_none());
        assert!(extract_predicate_token("a.applies() or b.applies()").is_none());

        // compound .applies() stem (stem must be a single identifier)
        assert!(extract_predicate_token("x y.applies()").is_none());

        // empty .applies() stem
        assert!(extract_predicate_token(".applies()").is_none());

        // empty input
        assert!(extract_predicate_token("").is_none());

        // single dangling quote — not a closed literal
        assert!(extract_predicate_token("\"").is_none());
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
            classification: None,
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
            classification: None,
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
            classification: None,
        };
        assert!(!is_pure_predicate(&br));
    }
}
