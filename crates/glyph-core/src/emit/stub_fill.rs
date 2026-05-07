//! Deterministic fill for every `SpanKind`. Today this is the only filler;
//! when the LLM Expand pass lands, per-kind overrides will replace some
//! arms (see `obsidian/plans/expand-emitter-design-2026-05-04.md`).

use super::branch::{extract_predicate_token, lookup_key_for_token};
use super::scaffold::{Chunk, Scaffold, SpanId, SpanKind};
use super::templates::{
    ensure_determiner, DESCRIPTION_RETURN_SUFFIX_PREFIX, DESCRIPTION_RETURN_SUFFIX_TAIL,
};
use crate::condition::{tokenize_condition, ConditionTokenKind};
use std::collections::{BTreeMap, HashMap};

pub fn fill(scaffold: &Scaffold) -> HashMap<SpanId, String> {
    let mut out = HashMap::new();
    for chunk in &scaffold.chunks {
        if let Chunk::Span(span) = chunk {
            let s = match span.kind {
                SpanKind::ParamDescription => String::new(),
                SpanKind::DescriptionReturnFold => {
                    let desc = span.payload.description_text.clone().unwrap_or_default();
                    let phrase = ensure_determiner(&desc);
                    format!("{DESCRIPTION_RETURN_SUFFIX_PREFIX}{phrase}{DESCRIPTION_RETURN_SUFFIX_TAIL}\n")
                }
                SpanKind::BranchCondition => {
                    let raw = span
                        .payload
                        .condition_expression
                        .clone()
                        .unwrap_or_default();
                    if let Some(rp) = &span.payload.resolved_predicates {
                        substitute_predicate_tokens(&raw, rp)
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
/// Three substitution rules (applied per token):
///   1. Bare identifier AND present in `rp` → emit `rp[token]` (PredicateConst).
///   2. Quoted literal `"..."` → strip surrounding quotes, emit the inner text.
///   3. Everything else (operators, booleans, unknown idents) → pass through unchanged.
///
/// The trailing `:` inserted by the parser is stripped here so the scaffold's
/// own `":\n"` suffix (emitted by `emit_mixed_condition` in branch.rs) is not
/// doubled. See the existing TODO in expand.rs:187 and branch.rs:23-25 — this
/// strips only the span content; authoritative strip should move to IR construction.
fn substitute_predicate_tokens(raw: &str, rp: &BTreeMap<String, String>) -> String {
    // Strip trailing `:` before tokenising (see module-level note above).
    let s = raw.trim().trim_end_matches(':').trim();

    let tokens = tokenize_condition(s);
    let parts: Vec<String> = tokens
        .into_iter()
        .map(|tok| {
            match extract_predicate_token(&tok) {
                Some((inner, ConditionTokenKind::PredicateLiteral)) => {
                    // Quoted literal: inner text already has quotes stripped by
                    // extract_predicate_token.
                    inner
                }
                Some((key, ConditionTokenKind::PredicateConst)) => {
                    rp.get(&key).cloned().unwrap_or(key)
                }
                Some((stem, ConditionTokenKind::PredicateApplies)) => {
                    let lookup = lookup_key_for_token(&stem, ConditionTokenKind::PredicateApplies);
                    rp.get(lookup)
                        .cloned()
                        .unwrap_or_else(|| lookup.to_string())
                }
                _ => tok,
            }
        })
        .collect();
    parts.join(" ")
}
