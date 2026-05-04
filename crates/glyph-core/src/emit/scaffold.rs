//! Scaffold-with-spans intermediate representation. Pure data types + the
//! `build()` walker that turns a resolved `IrArena` into a `Scaffold`.
//! See `obsidian/plans/expand-emitter-design-2026-05-04.md`.

use crate::ir::NodeId;
use std::collections::BTreeMap;

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
