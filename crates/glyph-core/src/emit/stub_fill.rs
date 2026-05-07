//! Deterministic fill for every `SpanKind`. Today this is the only filler;
//! when the LLM Expand pass lands, per-kind overrides will replace some
//! arms (see `obsidian/plans/expand-emitter-design-2026-05-04.md`).

use super::scaffold::{Chunk, Scaffold, SpanId, SpanKind};
use std::collections::{BTreeMap, HashMap};

pub fn fill(scaffold: &Scaffold) -> HashMap<SpanId, String> {
    let mut out = HashMap::new();
    for chunk in &scaffold.chunks {
        if let Chunk::Span(span) = chunk {
            let s = match span.kind {
                SpanKind::ParamDescription => String::new(),
                SpanKind::BranchCondition => {
                    let raw = span
                        .payload
                        .condition_expression
                        .clone()
                        .unwrap_or_default();
                    if let Some(rp) = &span.payload.resolved_predicates {
                        substitute_predicate_tokens(&raw, rp, span.payload.classification.as_ref())
                    } else {
                        raw
                    }
                }
                SpanKind::CallBodyShape => span.payload.resolved_body.clone().unwrap_or_default(),
            };
            out.insert(span.id, s);
        }
    }
    out
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
