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
            Chunk::Span(span) => match fills.get(&span.id) {
                Some(s) => out.push_str(s),
                None => return Err(MergeError::MissingSpan(span.id)),
            },
        }
    }
    Ok(out)
}
