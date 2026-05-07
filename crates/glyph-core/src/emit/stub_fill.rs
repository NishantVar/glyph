//! Deterministic fill for every `SpanKind`. Today this is the only filler;
//! when the LLM Expand pass lands, per-kind overrides will replace some
//! arms (see `obsidian/plans/expand-emitter-design-2026-05-04.md`).

use super::scaffold::{Chunk, Scaffold, SpanId, SpanKind};
use super::templates::{
    ensure_determiner, DESCRIPTION_RETURN_SUFFIX_PREFIX, DESCRIPTION_RETURN_SUFFIX_TAIL,
};
use std::collections::HashMap;

pub fn fill(scaffold: &Scaffold) -> HashMap<SpanId, String> {
    let mut out = HashMap::new();
    for chunk in &scaffold.chunks {
        if let Chunk::Span(span) = chunk {
            let s = match span.kind {
                SpanKind::ParamDescription => String::new(),
                SpanKind::DescriptionReturnFold => {
                    let desc = span
                        .payload
                        .description_text
                        .clone()
                        .unwrap_or_default();
                    let phrase = ensure_determiner(&desc);
                    format!("{DESCRIPTION_RETURN_SUFFIX_PREFIX}{phrase}{DESCRIPTION_RETURN_SUFFIX_TAIL}\n")
                }
                SpanKind::BranchCondition => span
                    .payload
                    .condition_expression
                    .clone()
                    .unwrap_or_default(),
                SpanKind::CallBodyShape => span
                    .payload
                    .resolved_body
                    .clone()
                    .unwrap_or_default(),
            };
            out.insert(span.id, s);
        }
    }
    out
}
