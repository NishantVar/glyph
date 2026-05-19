//! Deterministic fill for every `SpanKind`. Today this is the only filler;
//! when the LLM Expand pass lands, per-kind overrides will replace some
//! arms (see `obsidian/plans/expand-emitter-design-2026-05-04.md`).

use super::scaffold::{Chunk, Scaffold, SpanId, SpanKind};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StubFillError {
    CallBody {
        ir_node: crate::ir::NodeId,
        target_name: Option<String>,
        has_modifier: bool,
        has_local_refs: bool,
    },
    ParamDescription {
        origin: ParamDescriptionOrigin,
        param_name: Option<String>,
        param_type: Option<String>,
        param_default: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParamDescriptionOrigin {
    /// Skill-path: scaffold pushed a `ParamDescription` span on the
    /// `## Parameters` block of a skill. Sorts ascending by `ir_node.0`.
    Skill { ir_node: crate::ir::NodeId },
    /// Procedure-path: `emit_procedure` walked `params` and found one with
    /// no `effective_param_description`. No IR-node attached — the caller
    /// drives the AST with a stub arena. Stable-sort retains insertion order.
    Procedure,
}

impl StubFillError {
    pub(crate) fn sort_key(&self) -> (u8, u64) {
        match self {
            StubFillError::CallBody { ir_node, .. } => (0, ir_node.0 as u64),
            StubFillError::ParamDescription {
                origin: ParamDescriptionOrigin::Skill { ir_node },
                ..
            } => (0, ir_node.0 as u64),
            StubFillError::ParamDescription {
                origin: ParamDescriptionOrigin::Procedure,
                ..
            } => (1, 0),
        }
    }
}

pub fn fill(scaffold: &Scaffold) -> Result<HashMap<SpanId, String>, Vec<StubFillError>> {
    let mut out = HashMap::new();
    let mut errors: Vec<StubFillError> = Vec::new();
    for chunk in &scaffold.chunks {
        if let Chunk::Span(span) = chunk {
            match span.kind {
                SpanKind::ParamDescription => {
                    errors.push(StubFillError::ParamDescription {
                        origin: ParamDescriptionOrigin::Skill {
                            ir_node: span.ir_node,
                        },
                        param_name: span.payload.param_name.clone(),
                        param_type: span.payload.param_type.clone(),
                        param_default: span.payload.param_default.clone(),
                    });
                }
                SpanKind::BranchCondition => {
                    let raw = span
                        .payload
                        .condition_expression
                        .clone()
                        .unwrap_or_default();
                    let empty = BTreeMap::new();
                    let rp = span.payload.resolved_predicates.as_ref().unwrap_or(&empty);
                    let s =
                        substitute_predicate_tokens(&raw, rp, span.payload.classification.as_ref());
                    out.insert(span.id, s);
                }
                SpanKind::CallBodyShape => {
                    errors.push(StubFillError::CallBody {
                        ir_node: span.ir_node,
                        target_name: span.payload.target_name.clone(),
                        has_modifier: span.payload.site_modifier.is_some(),
                        has_local_refs: !span.payload.local_refs.is_empty(),
                    });
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

/// Substitute recognised predicate tokens in a mixed condition string.
///
/// When `classification` is present (compile-time path), defer to
/// `branch::render_substituted_condition` which walks the classified token
/// stream — this path preserves operand quotes and handles `==` operands
/// uniformly.
///
/// When `classification` is absent (validate-output / JSON-loaded IR — see
/// `IrBranch.classification` `#[serde(skip)]`), fall back to a position-aware
/// re-tokenize + balanced-paren walk to mark `==` operand spans, then
/// substitute non-operand tokens against `rp`.
fn substitute_predicate_tokens(
    raw: &str,
    rp: &BTreeMap<String, String>,
    classification: Option<&crate::condition::ConditionClassification>,
) -> String {
    if let Some(c) = classification {
        return crate::emit::branch::render_substituted_condition(c, rp);
    }
    render_substitution_from_retokenize(raw, rp)
}

/// JSON-loaded fallback. Re-tokenizes the condition string and marks `==`
/// operands using the same balanced-paren approach as classify_condition,
/// then substitutes any token that matches a key in `rp`. Operand-side
/// tokens always pass through verbatim (preserves quotes for string operands).
fn render_substitution_from_retokenize(raw: &str, rp: &BTreeMap<String, String>) -> String {
    use crate::condition::{match_paren_left, match_paren_right, tokenize_condition};

    let trimmed = raw.trim().trim_end_matches(':').trim();
    let tokens = tokenize_condition(trimmed);

    let mut is_operand = vec![false; tokens.len()];
    for (i, tok) in tokens.iter().enumerate() {
        if tok == "==" {
            if i > 0 {
                let lhs_end = i - 1;
                let lhs_start = match_paren_left(&tokens, lhs_end);
                for j in lhs_start..=lhs_end {
                    is_operand[j] = true;
                }
            }
            if i + 1 < tokens.len() {
                let rhs_start = i + 1;
                let rhs_end = match_paren_right(&tokens, rhs_start);
                for j in rhs_start..=rhs_end {
                    is_operand[j] = true;
                }
            }
        }
    }

    let mut parts: Vec<String> = Vec::with_capacity(tokens.len());
    for (i, tok) in tokens.into_iter().enumerate() {
        if is_operand[i] {
            parts.push(tok);
            continue;
        }
        if let Some(v) = rp.get(&tok) {
            parts.push(v.clone());
            continue;
        }
        let stripped = tok.trim_end_matches(".applies()");
        if stripped != tok {
            if let Some(v) = rp.get(stripped) {
                parts.push(v.clone());
                continue;
            }
        }
        if tok.starts_with('"') && tok.ends_with('"') && tok.len() >= 2 {
            parts.push(tok[1..tok.len() - 1].to_string());
            continue;
        }
        parts.push(tok);
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emit::scaffold::{Scaffold, SpanId, SpanKind, SpanPayload, SpanRef};
    use crate::ir::NodeId;

    fn span(id: u32, kind: SpanKind, payload: SpanPayload) -> SpanRef {
        SpanRef {
            id: SpanId(id),
            kind,
            ir_node: NodeId(3),
            payload,
        }
    }

    #[test]
    fn fill_returns_ok_when_no_call_body_shape_spans() {
        let s = Scaffold::default();
        let r = fill(&s);
        assert!(r.is_ok());
        assert!(r.unwrap().is_empty());
    }

    #[test]
    fn fill_hard_fails_on_call_body_shape_with_modifier() {
        let mut s = Scaffold::default();
        s.push_span(span(
            0,
            SpanKind::CallBodyShape,
            SpanPayload {
                target_name: Some("inspect_failure".into()),
                site_modifier: Some("focus on lint".into()),
                ..SpanPayload::default()
            },
        ));
        let r = fill(&s);
        let errors = r.expect_err("CallBodyShape span must hard-fail in stub filler");
        assert_eq!(errors.len(), 1);
        match &errors[0] {
            StubFillError::CallBody {
                ir_node,
                target_name,
                has_modifier,
                has_local_refs,
            } => {
                assert_eq!(target_name.as_deref(), Some("inspect_failure"));
                assert!(*has_modifier);
                assert!(!*has_local_refs);
                assert_eq!(*ir_node, NodeId(3));
            }
            _ => panic!("expected CallBody variant"),
        }
    }

    #[test]
    fn fill_collects_multiple_errors_in_chunk_order() {
        let mut s = Scaffold::default();
        s.push_span(SpanRef {
            id: SpanId(0),
            kind: SpanKind::CallBodyShape,
            ir_node: NodeId(5),
            payload: SpanPayload {
                target_name: Some("a".into()),
                site_modifier: Some("m".into()),
                ..Default::default()
            },
        });
        s.push_literal("between\n");
        s.push_span(SpanRef {
            id: SpanId(1),
            kind: SpanKind::CallBodyShape,
            ir_node: NodeId(2),
            payload: SpanPayload {
                target_name: Some("b".into()),
                local_refs: vec![crate::ir::LocalRef {
                    name: "x".into(),
                    node_id: NodeId(99),
                }],
                ..Default::default()
            },
        });
        let errs = fill(&s).expect_err("two CallBodyShape spans must yield two errors");
        assert_eq!(errs.len(), 2);
        // Order at this layer is chunk-stream order, not sorted; the lib-level
        // helper sorts before pushing into the bag.
        match &errs[0] {
            StubFillError::CallBody { ir_node, .. } => assert_eq!(*ir_node, NodeId(5)),
            _ => panic!("expected CallBody variant"),
        }
        match &errs[1] {
            StubFillError::CallBody {
                ir_node,
                has_local_refs,
                ..
            } => {
                assert_eq!(*ir_node, NodeId(2));
                assert!(*has_local_refs);
            }
            _ => panic!("expected CallBody variant"),
        }
    }
}
