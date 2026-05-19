//! Merge a `Scaffold` and a fill map into the final compiled Markdown string.

use super::scaffold::{Chunk, Scaffold, SpanId};
use std::collections::HashMap;

#[derive(Debug)]
pub enum MergeError {
    MissingSpan(SpanId),
    UnknownSpan(SpanId),
}

pub fn merge(scaffold: Scaffold, fills: HashMap<SpanId, String>) -> Result<String, MergeError> {
    use std::collections::HashSet;
    let emitted_ids: HashSet<SpanId> = scaffold
        .chunks
        .iter()
        .filter_map(|c| match c {
            Chunk::Span(s) => Some(s.id),
            _ => None,
        })
        .collect();
    for fill_id in fills.keys() {
        if !emitted_ids.contains(fill_id) {
            return Err(MergeError::UnknownSpan(*fill_id));
        }
    }
    let mut out = String::new();
    for chunk in scaffold.chunks {
        match chunk {
            Chunk::Literal(s) => out.push_str(&s),
            Chunk::Span(span) => {
                let filled = match fills.get(&span.id) {
                    Some(s) => s.clone(),
                    None => return Err(MergeError::MissingSpan(span.id)),
                };
                let body = match span.payload.post_merge_return_sentence.as_deref() {
                    Some(sent) => crate::emit::templates::append_return_sentence(&filled, sent),
                    None => filled,
                };
                out.push_str(&body);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emit::scaffold::{Scaffold, SpanId, SpanKind, SpanPayload, SpanRef};
    use crate::ir::NodeId;

    fn span(id: u32, kind: SpanKind) -> SpanRef {
        SpanRef {
            id: SpanId(id),
            kind,
            ir_node: NodeId(0),
            payload: SpanPayload::default(),
        }
    }

    #[test]
    fn merge_literal_only() {
        let mut s = Scaffold::default();
        s.push_literal("hello\n");
        let fills = HashMap::new();
        assert_eq!(merge(s, fills).unwrap(), "hello\n");
    }

    #[test]
    fn merge_with_span_fill() {
        let mut s = Scaffold::default();
        s.push_literal("- **name**");
        s.push_span(span(0, SpanKind::ParamDescription));
        s.push_literal(" (required)\n");
        let mut fills = HashMap::new();
        fills.insert(SpanId(0), ": the description".into());
        assert_eq!(
            merge(s, fills).unwrap(),
            "- **name**: the description (required)\n"
        );
    }

    #[test]
    fn merge_missing_fill_errors() {
        let mut s = Scaffold::default();
        s.push_span(span(7, SpanKind::CallBodyShape));
        let result = merge(s, HashMap::new());
        match result {
            Err(MergeError::MissingSpan(SpanId(7))) => {}
            other => panic!("expected MissingSpan(7), got {other:?}"),
        }
    }

    #[test]
    fn merge_unknown_fill_errors() {
        let mut s = Scaffold::default();
        s.push_literal("ok");
        let mut fills = HashMap::new();
        fills.insert(SpanId(99), "unexpected".into());
        match merge(s, fills) {
            Err(MergeError::UnknownSpan(SpanId(99))) => {}
            other => panic!("expected UnknownSpan(99), got {other:?}"),
        }
    }

    /// Task 14 reviewer nit: when a Span's payload carries
    /// `post_merge_return_sentence`, the merger must fold the §8.4 sentence
    /// onto the filled body via `append_return_sentence`. The fold is the
    /// last-step return-folding mechanism for the shared `push_call_body`
    /// helper — it lets the helper stash the sentence on the span at
    /// scaffold time and have it land after stub_fill/merger completes.
    #[test]
    fn merge_applies_post_merge_return_sentence() {
        let mut s = Scaffold::default();
        let mut span = span(0, SpanKind::CallBodyShape);
        span.payload.post_merge_return_sentence = Some("Return a `Report`.".to_string());
        s.push_span(span);
        let mut fills = HashMap::new();
        fills.insert(SpanId(0), "Examine the working tree.".to_string());
        let out = merge(s, fills).expect("merge with post-merge sentence succeeds");
        assert_eq!(
            out, "Examine the working tree. Return a `Report`.",
            "post_merge_return_sentence must be folded onto the filled body via append_return_sentence"
        );
    }
}
