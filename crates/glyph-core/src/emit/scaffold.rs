//! Scaffold-with-spans intermediate representation. Pure data types + the
//! `build()` walker that turns a resolved `IrArena` into a `Scaffold`.
//! See `obsidian/plans/expand-emitter-design-2026-05-04.md`.

use super::templates;
use crate::ir::{
    BranchPredicateShape, IrArena, IrBlock, IrCall, IrFreeformSection, IrNode, IrSkill, LocalRef,
    NodeId, OutputTargetForm,
};
use crate::slot;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;

/// D9 merge-algorithm unit (§4.1.5). One body unit to render, distinguishing
/// explicit (author-positioned, anchored to `source_line`) sections from
/// synthetic (catalogue-only) ones (anchored to `canonical_slot`). Freeform
/// sections are always explicit and never participate in synthetic insertion.
#[derive(Debug, Clone)]
pub(crate) struct RenderUnit {
    pub kind: RenderKind,
    /// Source line of the section's header when the author wrote a colon-keyword
    /// for this section. `None` for synthetic emissions (e.g. body-level
    /// `require x` markers that produce constraints without a `constraints:`
    /// header).
    pub source_line: Option<u32>,
    /// Canonical slot per spec §4.1.5: Goal=1, Parameters=2, Constraints=3,
    /// Context=4, Flow=5. `None` for freeform sections — they never carry a
    /// canonical slot and are skipped during synthetic-insertion lookup.
    pub canonical_slot: Option<u32>,
}

#[derive(Debug, Clone)]
pub(crate) enum RenderKind {
    /// `## Parameters` — always synthetic in Phase 3.C (no `parameters:` colon
    /// keyword exists yet). Slot 2.
    Parameters,
    /// `## Constraints` — explicit when authored as `constraints:`, synthetic
    /// when body-level `require x` markers populate `IrSkill.constraints`.
    /// Slot 3.
    Constraints,
    /// `## Context` — explicit when authored as `context:`, synthetic when
    /// body-level `context x` markers populate `IrSkill.context`. Slot 4.
    ContextSection,
    /// `## Steps` — always explicit when present (the `flow:` keyword is
    /// authored). Slot 5.
    Flow,
    /// `## <Heading>` — freeform colon-keyword section. Anchored to its
    /// source line; never inserted by synthetic-position lookup. Catalogue
    /// entries with a `canonical_slot` (today: `[goal]` at slot 1) also
    /// flow through this variant — their ordering is driven by the
    /// `canonical_slot` field on `RenderUnit`, not a dedicated kind.
    Freeform(NodeId),
}

/// Flow-position-assignments §9.1 — naming sentence for an `IrCall` whose
/// `bound_name` is `Some(n)`. Returns `None` when the call carries no binding.
///
/// - Agent shape (`is_agent == true`): *"Refer to this agent as '<n>.'"* —
///   single quotes around `n`, matching GLYPH_LANGUAGE_GUIDE §18.4 verbatim.
/// - Value shape: *"Refer to this result as <n>."* — bare `n`, no quotes.
pub(super) fn naming_sentence_for_call(c: &IrCall) -> Option<String> {
    let n = c.bound_name.as_deref()?;
    if c.is_agent {
        Some(format!("Refer to this agent as '{}.'", n))
    } else {
        Some(format!("Refer to this result as {}.", n))
    }
}

/// Does this Call need LLM-grade body shaping? When this returns true the
/// emit site must push a `CallBodyShape` span; the stub filler hard-fails
/// (see `stub_fill.rs`) producing `G::expand::llm-required-for-call`.
///
/// Per spec §3.3: a non-empty `site_modifier` (the `with "…"` clause) or
/// a non-empty `local_refs` (LLM-grade cross-references like
/// "the diagnosis from your earlier analysis", which the deterministic
/// `substitute_local_refs_in` bare-substitution cannot produce).
pub(crate) fn call_needs_llm_fill(c: &crate::ir::IrCall) -> bool {
    c.site_modifier.is_some() || !c.local_refs.is_empty()
}

/// Map `IrCall.projection_tier` + `bound_name` into the payload-side
/// `ProjectionMode`. Mirrors the actual emit-site match order: a Call
/// carrying both a `projection_tier` and a `bound_name` routes through
/// its tier path, not the stdlib anchor. `StdlibBound` is reached only
/// when no tier applies.
pub(crate) fn projection_mode_from(c: &crate::ir::IrCall) -> Option<ProjectionMode> {
    match c.projection_tier {
        Some(1) => Some(ProjectionMode::Inline),
        Some(2) => Some(ProjectionMode::SameFileProcedure),
        Some(3) => Some(ProjectionMode::ExternalFile),
        _ if c.bound_name.is_some() => Some(ProjectionMode::StdlibBound),
        _ => None,
    }
}

/// Flow-position-assignments §9.2 — substitute `{name}` slots in `text` whose
/// `name` is in `local_refs` with the bare `name`. Slots whose `name` is not
/// in `local_refs` (parameters, unknown-but-non-flow-local — though analyze
/// rejects those) pass through verbatim.
pub(super) fn substitute_local_refs_in(text: &str, local_refs: &[LocalRef]) -> String {
    if local_refs.is_empty() {
        return text.to_string();
    }
    slot::substitute_local_refs(text, |name| local_refs.iter().any(|l| l.name == name))
}

/// Tier-1 last-step folding context for [`push_call_body`].
///
/// Set by top-level scaffold callers; in-arm callers always pass `None`.
pub(crate) struct Tier1FoldCtx {
    /// True only for the final flow step at the top level.
    pub is_last: bool,
    /// The §8.4 return sentence to fold into / over the call body when present.
    pub return_sentence: Option<String>,
}

/// Shared Call-rendering helper used by every CallBodyShape emit site.
///
/// Owns: [`call_needs_llm_fill`] dispatch, [`projection_mode_from`] lookup,
/// CallBodyShape `SpanPayload` construction, raw-body-vs-anchor selection,
/// Tier-1 last-step return-sentence folding (`post_merge_return_sentence`
/// for spans; in-line fold via [`templates::append_return_sentence`] for
/// literals), and the trailing §9.1 naming sentence + line break.
///
/// Callers handle their own step-numbering prefix (e.g. `"1. "` or
/// `"   a. "`) and any site-specific bookkeeping (e.g. Tier-2
/// `procedure_seen` tracking).
///
/// `anchor_or_body` is the *raw* call body for Tier-1 (slots intact when
/// LLM-fill is needed) or the pre-formatted anchor sentence for
/// Tier-2/3/Stdlib. `tier1` carries last-step folding context and is
/// `None` for non-Tier-1 callers.
fn indent_continuation_lines(body: &str, indent: &str) -> String {
    if !body.contains('\n') {
        return body.to_string();
    }
    let trimmed = body.trim_end_matches('\n');
    let mut out = String::with_capacity(trimmed.len() + 16);
    for (i, line) in trimmed.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
            if !line.is_empty() {
                out.push_str(indent);
            }
        }
        out.push_str(line);
    }
    out
}

pub(crate) fn push_call_body(
    s: &mut Scaffold,
    c: &crate::ir::IrCall,
    anchor_or_body: &str,
    tier1: Option<Tier1FoldCtx>,
    next_span_id: &mut u32,
    continuation_indent: &str,
) {
    let needs_fill = call_needs_llm_fill(c);
    let projection = projection_mode_from(c);
    match tier1 {
        Some(t1) => {
            let body_is_empty = anchor_or_body.trim().is_empty();
            if needs_fill {
                let id = SpanId(*next_span_id);
                *next_span_id += 1;
                let resolved = if t1.is_last && body_is_empty && t1.return_sentence.is_some() {
                    String::new()
                } else {
                    anchor_or_body.to_string()
                };
                s.push_span(SpanRef {
                    id,
                    kind: SpanKind::CallBodyShape,
                    ir_node: c.node_id,
                    payload: SpanPayload {
                        target_name: Some(c.target.clone()),
                        projection_mode: projection,
                        site_modifier: c.site_modifier.clone(),
                        resolved_body: Some(resolved),
                        local_refs: c.local_refs.clone(),
                        post_merge_return_sentence: t1.return_sentence,
                        ..SpanPayload::default()
                    },
                });
            } else {
                let body_owned = substitute_local_refs_in(anchor_or_body, &c.local_refs);
                let body_owned = indent_continuation_lines(&body_owned, continuation_indent);
                let body = body_owned.as_str();
                let rendered = if t1.is_last {
                    match (t1.return_sentence.as_deref(), body_is_empty) {
                        (Some(sent), true) => sent.to_string(),
                        (Some(sent), false) => templates::append_return_sentence(body, sent),
                        (None, _) => body.to_string(),
                    }
                } else {
                    body.to_string()
                };
                s.push_literal(rendered);
            }
        }
        None => {
            if needs_fill {
                let id = SpanId(*next_span_id);
                *next_span_id += 1;
                s.push_span(SpanRef {
                    id,
                    kind: SpanKind::CallBodyShape,
                    ir_node: c.node_id,
                    payload: SpanPayload {
                        target_name: Some(c.target.clone()),
                        projection_mode: projection,
                        site_modifier: c.site_modifier.clone(),
                        resolved_body: Some(anchor_or_body.to_string()),
                        local_refs: c.local_refs.clone(),
                        ..SpanPayload::default()
                    },
                });
            } else {
                s.push_literal(indent_continuation_lines(
                    anchor_or_body,
                    continuation_indent,
                ));
            }
        }
    }
    if let Some(naming) = naming_sentence_for_call(c) {
        s.push_literal(format!(" {}", naming));
    }
    s.push_literal("\n");
}

/// Flow-position-assignments §9.3 noun-phrase priority chain. Given a producer
/// `IrCall`, derive a noun phrase for the return-prose template:
///
/// 1. `callee_output_contract`:
///    - `Description(text)` → `"the <text>"` (e.g. *"the root cause and …"*).
///    - `Identifier(name)`  → `"the <name>"`.
/// 2. else `callee_return_type_text` → `"the <humanized type>"`.
/// 3. else `"the result"`.
///
/// The humanizer breaks CamelCase / snake_case into lowercase space-separated
/// words (e.g. `RepoContext` → `"repo context"`). No registry lookup is
/// performed — the canonical descriptive path is the `Description` form.
fn noun_phrase_for_producer(c: &IrCall) -> String {
    if let Some(form) = &c.callee_output_contract {
        match form {
            OutputTargetForm::Description(text) => {
                let cleaned = text.split_whitespace().collect::<Vec<_>>().join(" ");
                return format!("the {}", cleaned);
            }
            OutputTargetForm::Identifier(name) => {
                return format!("the {}", name);
            }
        }
    }
    if let Some(t) = &c.callee_return_type_text {
        return format!("the {}", humanize_type_text(t));
    }
    "the result".to_string()
}

/// Convert a type-text source spelling (CamelCase, snake_case, or mixed) into
/// a lowercase space-separated phrase. Used by §9.3 fallback noun-phrase.
///
/// Examples:
/// - `RepoContext`  → `"repo context"`
/// - `repo_context` → `"repo context"`
/// - `URL`          → `"url"`
/// - `Diagnosis`    → `"diagnosis"`
fn humanize_type_text(t: &str) -> String {
    let mut out = String::with_capacity(t.len() + 4);
    let chars: Vec<char> = t.chars().collect();
    let mut prev_was_lower_or_digit = false;
    for (i, &ch) in chars.iter().enumerate() {
        if ch == '_' {
            // Underscore → word break (collapse runs).
            if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
            prev_was_lower_or_digit = false;
            continue;
        }
        if ch.is_uppercase() {
            // CamelCase boundary: insert space before an uppercase letter
            // when the previous char was lowercase/digit (e.g. `RepoContext`
            // → `Repo Context`), or when it starts a new word in an acronym
            // followed by lowercase (e.g. `URLPath` → `URL Path`).
            let next_is_lower = chars.get(i + 1).is_some_and(|c| c.is_lowercase());
            let previous_was_upper = chars
                .get(i.wrapping_sub(1))
                .is_some_and(|c| c.is_uppercase());
            if !out.is_empty()
                && (prev_was_lower_or_digit || (previous_was_upper && next_is_lower))
                && !out.ends_with(' ')
            {
                out.push(' ');
            }
            out.extend(ch.to_lowercase());
            prev_was_lower_or_digit = false;
        } else {
            out.push(ch);
            prev_was_lower_or_digit = ch.is_alphanumeric();
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Look up the `OutputTargetForm` for a block by name, returning an owned clone.
fn block_output_form_owned(arena: &IrArena, block_name: &str) -> Option<OutputTargetForm> {
    for node in arena.nodes() {
        if let IrNode::Block(b) = node {
            if b.name == block_name {
                if let Some(oc_id) = b.output_contract {
                    if let IrNode::OutputContract(oc) = arena.get(oc_id) {
                        return Some(oc.form.clone());
                    }
                }
                return None;
            }
        }
    }
    None
}

/// Look up the source-text `-> Foo` annotation for a block by name. Mirrors
/// `block_output_form_owned`; needed by §8.4 templates because the canonical
/// `return_type` loses casing.
fn block_return_type_text_owned(arena: &IrArena, block_name: &str) -> Option<String> {
    for node in arena.nodes() {
        if let IrNode::Block(b) = node {
            if b.name == block_name {
                return b.return_type_text.clone();
            }
        }
    }
    None
}

/// Render one `### Context` entry as a column-0 Markdown bullet whose body
/// is line-wise indented so the bullet contains the full body as nested
/// content. Blank lines stay empty (no `  ` whitespace-only lines, per
/// `compiled-output.md` "no trailing whitespace"). When `name` is `Some`,
/// the bullet leads with a bold kebab-case label, then a blank line, then
/// the indented body — same shape used by `### Procedure: <name>`.
fn render_context_entry(text: &str, name: Option<&str>) -> String {
    /// Indent every non-empty line by two spaces. Blank lines stay empty.
    fn indent_continuation(body: &str) -> String {
        body.lines()
            .map(|line| {
                if line.is_empty() {
                    String::new()
                } else {
                    format!("  {}", line)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    let trimmed = text.trim_matches('\n');
    match name {
        Some(n) => {
            let label = templates::kebab_case(n);
            let indented = indent_continuation(trimmed);
            format!("- **{}**\n\n{}\n\n", label, indented)
        }
        None => {
            // First line follows `- ` directly; rest indented under it.
            let mut lines = trimmed.lines();
            let first = lines.next().unwrap_or("");
            let rest: Vec<&str> = lines.collect();
            if rest.is_empty() {
                format!("- {}\n\n", first)
            } else {
                let rest_indented = rest
                    .iter()
                    .map(|line| {
                        if line.is_empty() {
                            String::new()
                        } else {
                            format!("  {}", line)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("- {}\n{}\n\n", first, rest_indented)
            }
        }
    }
}

/// Look up the `OutputTargetForm` for the skill's output_contract, returning an owned clone.
fn skill_output_form_owned(arena: &IrArena) -> Option<OutputTargetForm> {
    let root_id = arena.root_skill()?;
    if let IrNode::Skill(s) = arena.get(root_id) {
        if let Some(oc_id) = s.output_contract {
            if let IrNode::OutputContract(oc) = arena.get(oc_id) {
                return Some(oc.form.clone());
            }
        }
    }
    None
}

/// Look up the source-text `-> Foo` annotation on the root skill.
fn skill_return_type_text_owned(arena: &IrArena) -> Option<String> {
    let root_id = arena.root_skill()?;
    if let IrNode::Skill(s) = arena.get(root_id) {
        return s.return_type_text.clone();
    }
    None
}

/// ADR 0026: render the body of an `Output:` step for an `IrReturn` node.
/// Description form → `Output: <description>.`
/// Identifier  form → `Output: <name> from step <M>.` when `<name>` is a
/// flow-local binding produced by a numbered step in `skill.steps`;
/// otherwise → `Output: <name>.`
fn render_return_step(
    _arena: &crate::ir::IrArena,
    skill: &crate::ir::IrSkill,
    ret: &crate::ir::IrReturn,
) -> String {
    match &ret.form {
        crate::ir::OutputTargetForm::Description(text) => {
            // ADR 0026 + control-char normalization: collapse all runs of
            // ASCII whitespace (including LF/CR/TAB) to single spaces so the
            // emitted line stays on a single Markdown row.
            let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let trimmed = collapsed.trim_end_matches('.').trim();
            format!("Output: {}.", trimmed)
        }
        crate::ir::OutputTargetForm::Identifier(name) => {
            // ADR 0026 + reviewer follow-up: consume the explicit producer
            // provenance captured at lower time. Fallback to the bare form
            // when the binding doesn't resolve to a flow-local producer.
            if let Some(step_num) = ret
                .producer_node_id
                .and_then(|pid| producer_step_index(skill, pid))
            {
                format!("Output: {} from step {}.", name, step_num)
            } else {
                format!("Output: {}.", name)
            }
        }
    }
}

/// ADR 0026 + reviewer follow-up: locate the 1-based step number of the
/// producer flow node identified by `NodeId`. Replaces the previous
/// name-resolving `producer_step_number` walker; emit consumes the field.
fn producer_step_index(skill: &crate::ir::IrSkill, producer: crate::ir::NodeId) -> Option<usize> {
    skill
        .steps
        .iter()
        .position(|step_id| *step_id == producer)
        .map(|idx| idx + 1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SpanId(pub u32);

#[derive(Clone, Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "boxing Span would change every Chunk construction/match site; out of scope for lint cleanup"
)]
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
    BranchCondition,
    CallBodyShape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionMode {
    Inline,
    SameFileProcedure,
    ExternalFile,
    StdlibBound,
}

#[derive(Clone, Debug, Default)]
pub struct SpanPayload {
    pub site_modifier: Option<String>,
    #[allow(
        dead_code,
        reason = "read only under cfg(test); retained as emission metadata on the non-test read path"
    )]
    pub resolved_body: Option<String>,
    pub condition_expression: Option<String>,
    pub resolved_predicates: Option<BTreeMap<String, String>>,
    pub classification: Option<crate::condition::ConditionClassification>,
    #[expect(
        dead_code,
        reason = "populated from branch lowering; retained as emission metadata, not yet consumed on the read path"
    )]
    pub predicate_shape: BranchPredicateShape,
    pub param_name: Option<String>,
    pub param_type: Option<String>,
    pub param_default: Option<String>,
    // New for CallBodyShape (see docs/superpowers/specs/2026-05-18-callbodyshape-span-emission-design.md §3.5):
    pub target_name: Option<String>,
    #[allow(
        dead_code,
        reason = "CallBodyShape span metadata; read only under cfg(test) on the non-test read path"
    )]
    pub projection_mode: Option<ProjectionMode>,
    pub local_refs: Vec<crate::ir::LocalRef>,
    pub post_merge_return_sentence: Option<String>,
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

/// Walk a slice of `NodeId`s and record any Tier-2 `IrCall.target` reached by
/// recursing through `Branch.then_body` / `elif_branches` / `else_body`. Used
/// both as the seed (over `skill.steps`) and as the per-procedure expansion
/// step (over a discovered block's `branch_steps`) of the worklist BFS in
/// `build()`.
fn collect_tier2_targets(
    nodes: &[NodeId],
    arena: &IrArena,
    seen: &mut HashSet<String>,
    order: &mut Vec<String>,
    queue: &mut VecDeque<String>,
) {
    for nid in nodes {
        match arena.get(*nid) {
            IrNode::Call(c) if c.projection_tier == Some(2) => {
                record(&c.target, seen, order, queue);
            }
            IrNode::Branch(b) => {
                collect_tier2_targets(&b.then_body, arena, seen, order, queue);
                for elif in &b.elif_branches {
                    collect_tier2_targets(&elif.body, arena, seen, order, queue);
                }
                if let Some(else_body) = &b.else_body {
                    collect_tier2_targets(else_body, arena, seen, order, queue);
                }
            }
            _ => {}
        }
    }
}

/// BFS bookkeeping: register a newly-discovered Tier-2 procedure name into
/// `seen` (for cycle safety), `order` (for parent-before-child render order),
/// and `queue` (for transitive expansion). Idempotent on already-seen names.
fn record(
    name: &str,
    seen: &mut HashSet<String>,
    order: &mut Vec<String>,
    queue: &mut VecDeque<String>,
) {
    if seen.insert(name.to_string()) {
        order.push(name.to_string());
        queue.push_back(name.to_string());
    }
}

/// Emit-time approximation for block-only outgoing edges, matching the expand
/// criteria available from `IrBlock` metadata. Block top-level calls do not
/// become `IrCall` nodes (they live as `outgoing_calls` strings + the
/// `"call <name>"` placeholder in `flow_statements`), so `target_to_tier`
/// gives a false negative for any block reached only via top-level outgoing
/// edges. The structural legs of expand's Tier-2 rule
/// (`stmt_count >= 4 || has_branches || wc >= 150`) are derivable from
/// `IrBlock` metadata and are checked here. The `freq >= 2` leg is
/// intentionally omitted: `freq` counts `IrCall` nodes, so if it would have
/// fired, `target_to_tier` already carries the entry.
fn classifies_as_tier2(
    name: &str,
    target_to_tier: &HashMap<String, u8>,
    blocks_by_name: &HashMap<&str, &IrBlock>,
) -> bool {
    if target_to_tier.get(name) == Some(&2) {
        return true;
    }
    let Some(b) = blocks_by_name.get(name) else {
        return false;
    };
    let stmt_count = b.flow_items.len();
    let has_branches = !b.branch_steps.is_empty();
    let wc = b.resolved_word_count.unwrap_or(0) as usize;
    let has_body_constraints = !b.constraints.is_empty();
    let has_body_context = !b.context.is_empty();
    stmt_count >= 4 || has_branches || wc >= 150 || has_body_constraints || has_body_context
}

/// D9 merge — gather phase. Build the unordered list of `RenderUnit`s for the
/// skill's body sections. Each built-in section emits one unit when it has
/// content; freeform sections emit one unit each. The `source_line` slot is
/// `Some(_)` when the author authored an explicit `<name>:` header in source
/// (anchoring the section in author order) and `None` when the section's
/// content was produced synthetically (e.g. body-level `require x` markers
/// emit a `## Constraints` section without a `constraints:` header).
///
/// Procedures (`### Procedure: <name>`) are NOT in this list — they always
/// trail the merged body and are emitted separately by `build()`.
pub(crate) fn gather_render_units(arena: &IrArena, skill: &IrSkill) -> Vec<RenderUnit> {
    let mut units: Vec<RenderUnit> = Vec::new();

    // Parameters — always synthetic in Phase 3.C (no `parameters:` colon
    // keyword exists). Emit only when the skill has params.
    if !skill.params.is_empty() {
        units.push(RenderUnit {
            kind: RenderKind::Parameters,
            source_line: None,
            canonical_slot: Some(2),
        });
    }

    // Constraints — explicit position if authored as `constraints:`; synthetic
    // otherwise. The catalog rule per §4.1.5: emit only when there's content
    // (top-level constraints exist).
    if !skill.constraints.is_empty() {
        units.push(RenderUnit {
            kind: RenderKind::Constraints,
            source_line: skill.constraints_source_line,
            canonical_slot: Some(3),
        });
    }

    // Context — explicit when authored as `context:`; synthetic when produced
    // by body-level markers.
    if !skill.context.is_empty() {
        units.push(RenderUnit {
            kind: RenderKind::ContextSection,
            source_line: skill.context_source_line,
            canonical_slot: Some(4),
        });
    }

    // Flow — always explicit when authored (the `flow:` keyword is required
    // to declare steps). Emit when there are steps OR when the return-prose
    // template will synthesize one (preserves existing return-only-skill
    // behaviour).
    let has_steps = !skill.steps.is_empty();
    let has_return_sentence = {
        let oc_form = skill_output_form_owned(arena);
        let rt_text = skill_return_type_text_owned(arena);
        templates::compute_return_sentence(
            rt_text.as_deref(),
            oc_form.as_ref(),
            &arena.type_registry,
        )
        .is_some()
    };
    if has_steps || has_return_sentence {
        units.push(RenderUnit {
            kind: RenderKind::Flow,
            source_line: skill.flow_source_line,
            canonical_slot: Some(5),
        });
    }

    // Freeform sections — always explicit, anchored to source line. A
    // catalogue entry with a `canonical_slot` (e.g. `[goal]`, slot 1)
    // additionally lets the section participate in synthetic-insertion
    // ordering under D9 (`order_render_units`): synthetic units consult an
    // explicit unit's `canonical_slot` when deciding where to splice in.
    // Sections with no catalogue entry stay `canonical_slot: None` and are
    // skipped during that lookup, preserving the freeform default.
    let catalogue = crate::sections::SectionCatalogue::load();
    for ff_id in &skill.freeform_sections {
        let ff = match arena.get(*ff_id) {
            IrNode::FreeformSection(s) => s,
            _ => continue, // defensive: lower ensures the id points at a FreeformSection
        };
        let canonical_slot = catalogue.get(&ff.name).and_then(|e| e.canonical_slot);
        units.push(RenderUnit {
            kind: RenderKind::Freeform(*ff_id),
            source_line: Some(ff.source_line),
            canonical_slot,
        });
    }

    // Goal (Phase 6) — emitted via the catalogued-freeform path above.
    // `[goal]` (canonical_slot = 1) is a freeform colon-keyword section
    // whose catalogue entry routes ordering through `canonical_slot`. No
    // dedicated `RenderKind::Goal` variant exists today.

    units
}

/// D9 merge — order phase. Per spec §4.1.5:
/// 1. Partition into explicit (`source_line.is_some()`) and synthetic.
/// 2. Sort explicit by `source_line` (ascending).
/// 3. Sort synthetic by `canonical_slot` (ascending).
/// 4. Insert each synthetic unit S into the explicit list BEFORE the first
///    explicit unit E whose `canonical_slot > S.canonical_slot`. Freeform
///    units (canonical_slot = None) are skipped during this comparison.
///
/// The result is the final body emission order.
pub(crate) fn order_render_units(units: Vec<RenderUnit>) -> Vec<RenderUnit> {
    let (mut explicit, mut synthetic): (Vec<RenderUnit>, Vec<RenderUnit>) =
        units.into_iter().partition(|u| u.source_line.is_some());

    explicit.sort_by_key(|u| u.source_line.unwrap_or(u32::MAX));
    synthetic.sort_by_key(|u| u.canonical_slot.unwrap_or(u32::MAX));

    let mut out: Vec<RenderUnit> = explicit;
    for syn in synthetic {
        let syn_slot = match syn.canonical_slot {
            Some(slot) => slot,
            None => {
                // Synthetic without a canonical slot is meaningless under §4.1.5,
                // but defensively append rather than crash. Freeform entries
                // (which lack a canonical_slot) are always explicit and therefore
                // never land in this branch.
                out.push(syn);
                continue;
            }
        };
        let insert_at = out
            .iter()
            .position(|e| match e.canonical_slot {
                Some(es) => es > syn_slot,
                None => false, // freeform entries are skipped during lookup
            })
            .unwrap_or(out.len());
        out.insert(insert_at, syn);
    }
    out
}

pub fn build(arena: &IrArena, enable_effects: bool) -> Scaffold {
    let root_id = arena
        .root_skill()
        .expect("validate guarantees a root skill before emit");
    let skill = match arena.get(root_id) {
        IrNode::Skill(s) => s,
        _ => unreachable!("root skill ID must point to a Skill node"),
    };

    let mut s = Scaffold::default();
    let mut next_span_id: u32 = 0;

    // Frontmatter
    s.push_literal("---\n");
    s.push_literal(format!("name: {}\n", skill.name));
    s.push_literal(format!(
        "description: '{}'\n",
        skill.description.replace('\'', "''")
    ));
    if enable_effects && !skill.effects.is_empty() {
        let mut sorted_effects = skill.effects.clone();
        sorted_effects.sort();
        s.push_literal(format!("effects: [{}]\n", sorted_effects.join(", ")));
    }
    s.push_literal("---\n\n");

    // Procedure discovery (Tier 2) — transitive closure. A procedure
    // reachable only by walking through another procedure's body must still
    // get its `### Procedure: <name>` section emitted, otherwise the call-site
    // `Follow the <X> procedure.` reference dangles. We seed from `skill.steps`
    // and then drain a queue, opening each discovered procedure's
    // `branch_steps` (structural branches) and `outgoing_calls` (top-level
    // call edges) to find further Tier-2 callees. Cycle-safe via `seen`.
    // See specs/nested-procedure-discovery-2026-05-10.md.
    //
    // Phase 3.C: the BFS now runs before the D9 loop so the Flow renderer can
    // consume the already-populated `procedure_seen` / `procedure_order` regardless
    // of where in the merged body order the `## Steps` section lands.
    let mut procedure_order: Vec<String> = Vec::new();
    let mut procedure_seen: HashSet<String> = HashSet::new();
    let mut procedure_queue: VecDeque<String> = VecDeque::new();
    let mut target_to_tier: HashMap<String, u8> = HashMap::new();
    let mut blocks_by_name: HashMap<&str, &IrBlock> = HashMap::new();
    for node in arena.nodes() {
        match node {
            IrNode::Call(c) => {
                if let Some(tier) = c.projection_tier {
                    let entry = target_to_tier.entry(c.target.clone()).or_insert(tier);
                    if tier == 2 {
                        *entry = 2;
                    }
                }
            }
            IrNode::Block(b) => {
                blocks_by_name.insert(b.name.as_str(), b);
            }
            _ => {}
        }
    }
    collect_tier2_targets(
        &skill.steps,
        arena,
        &mut procedure_seen,
        &mut procedure_order,
        &mut procedure_queue,
    );
    while let Some(name) = procedure_queue.pop_front() {
        let Some(block) = blocks_by_name.get(name.as_str()).copied() else {
            continue;
        };
        let mut indexed: Vec<(usize, NodeId)> =
            block.branch_steps.iter().map(|(k, v)| (*k, *v)).collect();
        indexed.sort_by_key(|(idx, _)| *idx);
        let sorted_branch_ids: Vec<NodeId> = indexed.into_iter().map(|(_, v)| v).collect();
        collect_tier2_targets(
            &sorted_branch_ids,
            arena,
            &mut procedure_seen,
            &mut procedure_order,
            &mut procedure_queue,
        );
        for callee in &block.outgoing_calls {
            if classifies_as_tier2(callee, &target_to_tier, &blocks_by_name) {
                record(
                    callee,
                    &mut procedure_seen,
                    &mut procedure_order,
                    &mut procedure_queue,
                );
            }
        }
    }

    // D9 merge — gather and order this skill's body sections per spec §4.1.5,
    // then dispatch each unit to its renderer. Procedures (`### Procedure: …`)
    // trail the merged body and are emitted afterwards.
    let units = order_render_units(gather_render_units(arena, skill));
    for unit in units {
        match unit.kind {
            RenderKind::Parameters => {
                emit_parameters_section(&mut s, arena, skill, &mut next_span_id);
            }
            RenderKind::ContextSection => {
                emit_context_section(&mut s, arena, skill);
            }
            RenderKind::Flow => {
                emit_flow_section(
                    &mut s,
                    arena,
                    skill,
                    &mut next_span_id,
                    &mut procedure_seen,
                    &mut procedure_order,
                );
            }
            RenderKind::Constraints => {
                emit_constraints_section(&mut s, arena, skill);
            }
            RenderKind::Freeform(ff_id) => {
                emit_freeform_section(&mut s, arena, ff_id, 2);
            }
        }
    }

    // ### Procedure: <name> sections
    for target_name in &procedure_order {
        let kebab_name = target_name.replace('_', "-");
        // Collect the block's flow_items + contract metadata before emitting.
        // Also collect freeform_sections so we can emit them as `####` children
        // of the procedure heading per design §4.1.5 / D12.
        let (flow_items, proc_oc_form, proc_rt_text, proc_freeform, proc_constraints, proc_context) = {
            let mut items: Option<Vec<crate::ir::IrBlockFlowItem>> = None;
            let mut oc: Option<OutputTargetForm> = None;
            let mut rt: Option<String> = None;
            let mut ff: Vec<NodeId> = Vec::new();
            let mut cs: Vec<NodeId> = Vec::new();
            let mut cx: Vec<NodeId> = Vec::new();
            for node in arena.nodes() {
                if let IrNode::Block(b) = node {
                    if b.name == *target_name {
                        items = Some(b.flow_items.clone());
                        oc = block_output_form_owned(arena, target_name);
                        rt = block_return_type_text_owned(arena, target_name);
                        ff = b.freeform_sections.clone();
                        cs = b.constraints.clone();
                        cx = b.context.clone();
                        break;
                    }
                }
            }
            (items, oc, rt, ff, cs, cx)
        };
        if let Some(items) = flow_items {
            s.push_literal(format!("### Procedure: {}\n\n", kebab_name));
            // #168: Tier 2 procedure preamble — body-level constraints and context
            // declared on the block render as prose paragraphs (bold-prefix lines,
            // NOT bullets and NOT numbered) between the heading and the steps.
            // Matches the four-form constraint template (`emit::constraint::render`)
            // and the implementer-chosen context format: `**<kebab>:** <text>` when
            // `IrContext.name` is Some, else `**Context:** <text>`.
            let mut had_preamble = false;
            for c_id in &proc_constraints {
                if let IrNode::Constraint(c) = arena.get(*c_id) {
                    let line = crate::sections::hooks::dispatch_constraints_expand(
                        c.strength, c.polarity, &c.text,
                    );
                    s.push_literal(format!("{}\n\n", line));
                    had_preamble = true;
                }
            }
            for c_id in &proc_context {
                if let IrNode::Context(c) = arena.get(*c_id) {
                    let label = match c.name.as_deref() {
                        Some(n) => n.replace('_', "-"),
                        None => "Context".to_string(),
                    };
                    let body = c.text.trim();
                    let needs_period =
                        !matches!(body.chars().last(), Some('.') | Some('!') | Some('?'));
                    let suffix = if needs_period { "." } else { "" };
                    s.push_literal(format!("**{}:** {}{}\n\n", label, body, suffix));
                    had_preamble = true;
                }
            }
            // Each preamble line already ends with `\n\n`; the final `\n\n`
            // supplies the blank line separator between preamble and steps.
            let _ = had_preamble;
            // §3.10: drive procedure-body emission off the structured
            // `flow_items` (tagged enum) instead of the legacy lossy
            // `flow_statements: Vec<String>`. Each Call variant carries its
            // pre-allocated IrCall NodeId, so the shared `push_call_body`
            // helper handles tier-1/2/3/stdlib rendering uniformly. Branch
            // variants carry their NodeId directly — no side-channel
            // `branch_steps` map needed.
            let visible_count = items
                .iter()
                .filter(|it| !matches!(it, crate::ir::IrBlockFlowItem::Return))
                .count();
            let proc_sentence = templates::compute_return_sentence(
                proc_rt_text.as_deref(),
                proc_oc_form.as_ref(),
                &arena.type_registry,
            );

            if let Some(sentence) = proc_sentence.as_deref().filter(|_| visible_count == 0) {
                // Return-only block: emit the §8.4 sentence as a standalone step.
                s.push_literal(format!("1. {sentence}\n"));
            } else {
                let mut visible_idx: usize = 0;
                for item in &items {
                    if matches!(item, crate::ir::IrBlockFlowItem::Return) {
                        continue;
                    }
                    visible_idx += 1;
                    let step_num = visible_idx;
                    let is_last = visible_idx == visible_count;
                    match item {
                        crate::ir::IrBlockFlowItem::Branch { node_id } => {
                            if let IrNode::Branch(br) = arena.get(*node_id) {
                                super::branch::emit_to_scaffold(
                                    &mut s,
                                    arena,
                                    br,
                                    step_num,
                                    &mut next_span_id,
                                );
                                // Trailing §8.4 sentence when the final visible
                                // step is a branch (mirrors the in-skill flow
                                // emitter's matching path).
                                if is_last {
                                    if let Some(sent) = proc_sentence.as_deref() {
                                        s.push_literal(format!("{}. {}\n", step_num + 1, sent));
                                    }
                                }
                            }
                        }
                        crate::ir::IrBlockFlowItem::Call { node_id } => {
                            if let IrNode::Call(c) = arena.get(*node_id) {
                                s.push_literal(format!("{}. ", step_num));
                                match c.projection_tier {
                                    Some(1) => {
                                        let raw_body =
                                            c.resolved_body.as_deref().unwrap_or_default();
                                        let return_sentence =
                                            if is_last { proc_sentence.clone() } else { None };
                                        push_call_body(
                                            &mut s,
                                            c,
                                            raw_body,
                                            Some(Tier1FoldCtx {
                                                is_last,
                                                return_sentence,
                                            }),
                                            &mut next_span_id,
                                            &" ".repeat(format!("{}. ", step_num).len()),
                                        );
                                    }
                                    Some(2) => {
                                        let callee_kebab = c.target.replace('_', "-");
                                        let anchor =
                                            format!("Follow the {callee_kebab} procedure below.");
                                        push_call_body(
                                            &mut s,
                                            c,
                                            &anchor,
                                            None,
                                            &mut next_span_id,
                                            &" ".repeat(format!("{}. ", step_num).len()),
                                        );
                                    }
                                    Some(3) => {
                                        let proc_path =
                                            c.procedure_path.as_deref().unwrap_or("unknown");
                                        let anchor = templates::external_file_step(proc_path);
                                        push_call_body(
                                            &mut s,
                                            c,
                                            &anchor,
                                            None,
                                            &mut next_span_id,
                                            &" ".repeat(format!("{}. ", step_num).len()),
                                        );
                                    }
                                    _ if c.bound_name.is_some() => {
                                        let anchor = format!("Call `{}`.", c.target);
                                        push_call_body(
                                            &mut s,
                                            c,
                                            &anchor,
                                            None,
                                            &mut next_span_id,
                                            &" ".repeat(format!("{}. ", step_num).len()),
                                        );
                                    }
                                    _ => {
                                        panic!(
                                            "IrCall to `{}` survived past expand without tier assignment",
                                            c.target
                                        );
                                    }
                                }
                            }
                        }
                        crate::ir::IrBlockFlowItem::Inline { text } => {
                            if is_last {
                                match proc_sentence.as_deref() {
                                    Some(sent) => {
                                        let body = templates::append_return_sentence(text, sent);
                                        s.push_literal(format!("{}. {}\n", step_num, body));
                                    }
                                    None => {
                                        s.push_literal(format!("{}. {}\n", step_num, text));
                                    }
                                }
                            } else {
                                s.push_literal(format!("{}. {}\n", step_num, text));
                            }
                        }
                        crate::ir::IrBlockFlowItem::Constraint { rendered }
                        | crate::ir::IrBlockFlowItem::Context { rendered } => {
                            s.push_literal(format!("{}. {}\n", step_num, rendered));
                        }
                        crate::ir::IrBlockFlowItem::BareName { name } => {
                            s.push_literal(format!("{}. {}\n", step_num, name));
                        }
                        crate::ir::IrBlockFlowItem::Return => unreachable!(),
                    }
                }
            }
            s.push_literal("\n");

            // Freeform colon-keyword sections at depth 4 (children of the
            // `### Procedure: <name>` heading), per design §4.1.5 / D12.
            for ff_id in &proc_freeform {
                emit_freeform_section(&mut s, arena, *ff_id, 4);
            }
        }
    }

    // Trim trailing blank line — pop chunks/chars until output doesn't end with "\n\n".
    trim_trailing_blank_line(&mut s);

    s
}

/// D9 renderer for `## Parameters`. Mechanical extraction of the prior inline
/// block in `build()`; output is byte-stable for a given input. A
/// `SpanKind::ParamDescription` span is pushed only for params with no
/// effective description (no inline `<"…">` and no type-registry entry); the
/// LLM expand pass fills that span, and `emit::stub_fill` hard-fails when it
/// is not filled.
fn emit_parameters_section(
    s: &mut Scaffold,
    arena: &IrArena,
    skill: &IrSkill,
    next_span_id: &mut u32,
) {
    s.push_literal("## Parameters\n\n");
    for p in &skill.params {
        // Build the type annotation suffix if present.
        let type_suffix = match &p.type_annotation {
            Some(t) => format!(" ({})", t),
            None => String::new(),
        };
        // Metadata tail: "Default: X." or "Required."
        let meta_tail = match &p.default {
            Some(v) => format!("Default: {}.", v),
            None => "Required.".to_string(),
        };

        // Emit bullet header and span, then description+metadata.
        // Effective description: per-param wins, else type-level (from registry), else none.
        let effective_desc: Option<String> = p.description.clone().or_else(|| {
            p.type_annotation
                .as_ref()
                .and_then(|t| arena.type_registry.get(t).cloned())
        });
        let has_desc = effective_desc.is_some();
        let desc_text = effective_desc.as_deref().unwrap_or("");
        let is_multiline = has_desc && (desc_text.contains('\n') || desc_text.len() > 120);

        if is_multiline {
            // Multi-line form:
            //   - **<name>**[ (<Type>)]:
            //     <description lines>
            //     Default: X. / Required.
            // Author prose lands via push_literal below; no ParamDescription
            // span needed (Task 4 will hard-fail that stub-fill arm).
            s.push_literal(format!("- **{}**{}:\n", p.name, type_suffix));
            for line in desc_text.lines() {
                s.push_literal(format!("  {}\n", line));
            }
            s.push_literal(format!("  {}\n", meta_tail));
        } else if has_desc {
            // Single-line description form:
            //   - **<name>**[ (<Type>)]: <description>. Default: X. / Required.
            // Author prose lands via push_literal below; no ParamDescription
            // span needed (Task 4 will hard-fail that stub-fill arm).
            let trimmed = desc_text.trim_end_matches('.').trim_end();
            s.push_literal(format!("- **{}**{}: ", p.name, type_suffix));
            s.push_literal(format!("{}. {}\n", trimmed, meta_tail));
        } else {
            // No description form:
            //   - **<name>**[ (<Type>)]. Default: X. / Required.
            // Span pushed only here so stub_fill (Task 4) can hard-fail with
            // a remediation diagnostic.
            s.push_literal(format!("- **{}**{}. ", p.name, type_suffix));
            let id = SpanId(*next_span_id);
            *next_span_id += 1;
            s.push_span(SpanRef {
                id,
                kind: SpanKind::ParamDescription,
                ir_node: skill.node_id,
                payload: SpanPayload {
                    param_name: Some(p.name.clone()),
                    param_type: p.type_annotation.clone(),
                    param_default: p.default.clone(),
                    ..SpanPayload::default()
                },
            });
            s.push_literal(format!("{}\n", meta_tail));
        }
    }
    s.push_literal("\n");
}

/// D9 renderer for `## Context`. Mechanical extraction of the prior inline
/// block in `build()`. Each `IrContext` entry projects via
/// `render_context_entry`.
fn emit_context_section(s: &mut Scaffold, arena: &IrArena, skill: &IrSkill) {
    s.push_literal("## Context\n\n");
    for ctx_id in &skill.context {
        let (text, name) = match arena.get(*ctx_id) {
            IrNode::Context(c) => (c.text.clone(), c.name.clone()),
            _ => panic!("Context node was not a Context"),
        };
        s.push_literal(render_context_entry(&text, name.as_deref()));
    }
}

/// D9 renderer for `## Steps`. Mechanical extraction of the prior inline
/// block in `build()`. Mutates `procedure_seen` / `procedure_order` when a
/// step is a Tier 2 call so the trailing `### Procedure: <name>` sections
/// pick up call-site discoveries (kept in addition to the BFS-discovered
/// transitive closure).
fn emit_flow_section(
    s: &mut Scaffold,
    arena: &IrArena,
    skill: &IrSkill,
    next_span_id: &mut u32,
    procedure_seen: &mut HashSet<String>,
    procedure_order: &mut Vec<String>,
) {
    // Pre-compute skill output_contract form once (owned), for use in the
    // last-step suffix logic below.
    let skill_oc_form = skill_output_form_owned(arena);
    let skill_rt_text = skill_return_type_text_owned(arena);
    let skill_step_count = skill.steps.len();
    let skill_has_return_sentence = templates::compute_return_sentence(
        skill_rt_text.as_deref(),
        skill_oc_form.as_ref(),
        &arena.type_registry,
    )
    .is_some();

    s.push_literal("## Steps\n\n");

    if skill_step_count == 0 {
        // Return-only skill: no flow steps but has a contract that yields a
        // §8.4 sentence. Emit it as the sole step.
        if skill_has_return_sentence {
            let sentence = templates::compute_return_sentence(
                skill_rt_text.as_deref(),
                skill_oc_form.as_ref(),
                &arena.type_registry,
            )
            .expect("guarded by skill_has_return_sentence");
            s.push_literal(format!("1. {}\n", sentence));
        }
    } else {
        for (idx, step_id) in skill.steps.iter().enumerate() {
            let is_last = idx + 1 == skill_step_count;
            match arena.get(*step_id) {
                IrNode::InlineInstruction(i) => {
                    // Flow-position-assignments §9.2: rewrite `{name}` →
                    // bare `name` for any slot whose name resolves to a
                    // flow-local in scope here.
                    let text = substitute_local_refs_in(&i.text, &i.local_refs);
                    if is_last {
                        // Codex M1 (round 2): when the §9.3 return-prose
                        // step is about to be emitted (the skill returns
                        // a flow-local binding), suppress the §8.4 generic
                        // "Return a `<T>`." suffix on this last inline
                        // step too. The §9.3 step that follows already
                        // states the return; appending the suffix here
                        // duplicates the return prose. Mirrors the same
                        // gate on the tier-1 Call last-step path below.
                        let suppress_return_suffix = skill.return_local_ref.is_some();
                        let sentence = if suppress_return_suffix {
                            None
                        } else {
                            templates::compute_return_sentence(
                                skill_rt_text.as_deref(),
                                skill_oc_form.as_ref(),
                                &arena.type_registry,
                            )
                        };
                        match sentence {
                            Some(sent) => {
                                let body = templates::append_return_sentence(&text, &sent);
                                s.push_literal(format!("{}. {}\n", idx + 1, body));
                            }
                            None => {
                                s.push_literal(format!("{}. {}\n", idx + 1, text));
                            }
                        }
                    } else {
                        s.push_literal(format!("{}. {}\n", idx + 1, text));
                    }
                }
                IrNode::Branch(br) => {
                    super::branch::emit_to_scaffold(s, arena, br, idx + 1, next_span_id);
                }
                IrNode::Call(c) if c.projection_tier == Some(1) => {
                    s.push_literal(format!("{}. ", idx + 1));
                    let is_returned_producer = skill
                        .return_local_ref
                        .as_ref()
                        .is_some_and(|lr| lr.node_id == c.node_id);
                    let (effective_form, effective_rt) = match skill_oc_form.as_ref() {
                        Some(form) => (Some(form), skill_rt_text.as_deref()),
                        None => (
                            c.callee_output_contract.as_ref(),
                            c.callee_return_type_text.as_deref(),
                        ),
                    };
                    let return_sentence = if is_last && !is_returned_producer {
                        templates::compute_return_sentence(
                            effective_rt,
                            effective_form,
                            &arena.type_registry,
                        )
                    } else {
                        None
                    };

                    let raw_body = c.resolved_body.as_deref().unwrap_or_default();

                    // §9.1 producer naming sentence — Post-span Literal chunk when emitted.

                    push_call_body(
                        s,
                        c,
                        raw_body,
                        Some(Tier1FoldCtx {
                            is_last,
                            return_sentence,
                        }),
                        next_span_id,
                        &" ".repeat(format!("{}. ", idx + 1).len()),
                    );
                }
                IrNode::Call(c) if c.projection_tier == Some(2) => {
                    s.push_literal(format!("{}. ", idx + 1));
                    let kebab_name = c.target.replace('_', "-");
                    let anchor = format!("Follow the {kebab_name} procedure below.");
                    push_call_body(
                        s,
                        c,
                        &anchor,
                        None,
                        next_span_id,
                        &" ".repeat(format!("{}. ", idx + 1).len()),
                    );

                    if procedure_seen.insert(c.target.clone()) {
                        procedure_order.push(c.target.clone());
                    }
                }
                IrNode::Call(c) if c.projection_tier == Some(3) => {
                    s.push_literal(format!("{}. ", idx + 1));
                    let proc_path = c.procedure_path.as_deref().unwrap_or("unknown");
                    let anchor = templates::external_file_step(proc_path);
                    push_call_body(
                        s,
                        c,
                        &anchor,
                        None,
                        next_span_id,
                        &" ".repeat(format!("{}. ", idx + 1).len()),
                    );
                }
                IrNode::Call(c) if c.bound_name.is_some() => {
                    // Flow-position-assignments §9.1: a stdlib or otherwise
                    // unresolved producer (no `resolved_body`, no
                    // projection_tier) — most commonly `subagent(...)` —
                    // still needs an action sentence so the §9.1 naming
                    // sentence has somewhere to attach. Synthesize a
                    // generic `Call <target>.` action; Step 2 (LLM) is
                    // free to weave it more fluently when a `with`
                    // modifier is present.
                    s.push_literal(format!("{}. ", idx + 1));
                    let anchor = format!("Call `{}`.", c.target);
                    push_call_body(
                        s,
                        c,
                        &anchor,
                        None,
                        next_span_id,
                        &" ".repeat(format!("{}. ", idx + 1).len()),
                    );
                }
                IrNode::Call(c) => {
                    panic!(
                        "IrNode::Call to `{}` survived past expand without tier assignment",
                        c.target
                    );
                }
                IrNode::Return(r) => {
                    let body = render_return_step(arena, skill, r);
                    s.push_literal(format!("{}. {}\n", idx + 1, body));
                }
                _ => panic!("Step node was not an InlineInstruction, Branch, Call, or Return"),
            };
        }
    }
    // Flow-position-assignments §9.3: when the skill's `return <ident>`
    // resolved to a flow-local producer, append the return-prose template
    // as an extra step paragraph after the regular flow steps.
    if let Some(lref) = skill.return_local_ref.as_ref() {
        // Look up the producing IrCall to derive the noun phrase.
        let producer = arena.nodes().iter().find_map(|n| match n {
            IrNode::Call(c) if c.node_id == lref.node_id => Some(c),
            _ => None,
        });
        let noun = producer
            .map(noun_phrase_for_producer)
            .unwrap_or_else(|| "the result".to_string());
        let next_step_num = skill.steps.len() + 1;
        s.push_literal(format!(
            "{}. Your result is {} ({} produced above).\n",
            next_step_num, lref.name, noun
        ));
    }
    s.push_literal("\n");
}

/// D9 renderer for `## Constraints`. Mechanical extraction of the prior
/// inline block in `build()`. Each entry projects via the locked
/// `(strength × polarity)` four-form templates in `emit::constraint`,
/// routed through the catalogue's `[constraints].expand_hook` (Phase 5)
/// so a future re-skin is a one-line catalogue edit.
fn emit_constraints_section(s: &mut Scaffold, arena: &IrArena, skill: &IrSkill) {
    s.push_literal("## Constraints\n\n");
    for c_id in &skill.constraints {
        let c = match arena.get(*c_id) {
            IrNode::Constraint(c) => c,
            _ => panic!("Constraint node was not a Constraint"),
        };
        let line =
            crate::sections::hooks::dispatch_constraints_expand(c.strength, c.polarity, &c.text);
        s.push_literal(format!("- {}\n", line));
    }
    s.push_literal("\n");
}

/// D9 renderer for a freeform colon-keyword section (e.g. `quality:`,
/// `risks:`). Heading is `<depth-#-of-`#`> <title-case heading>`.
/// Per design D2/§4.1.5: one item → paragraph; multiple items → bulleted list.
/// Reserved-marker items render via the constraint four-form template;
/// `context`-marked items use the bare body; plain strings render as-is.
///
/// `depth` is the H-level for the section heading (e.g. 2 for skill-top
/// freeform, 4 for Tier-2-procedure-nested freeform). Phase 3.C wires
/// depth=2 only; Phase 3.9 will extend to depth=4 / depth=2 (Tier 3 file).
fn emit_freeform_section(s: &mut Scaffold, arena: &IrArena, ff_id: NodeId, depth: usize) {
    let ff: &IrFreeformSection = match arena.get(ff_id) {
        IrNode::FreeformSection(f) => f,
        _ => panic!("Freeform section node was not a FreeformSection"),
    };

    let rendered: Vec<String> = ff
        .items
        .iter()
        .filter_map(|item_id| {
            let item = match arena.get(*item_id) {
                IrNode::FreeformContent(c) => c,
                _ => return None,
            };
            let body = render_freeform_item_body(item);
            if body.is_empty() {
                None
            } else {
                Some(body)
            }
        })
        .collect();

    if rendered.is_empty() {
        // Pathological: a freeform section with no rendered content.
        // An empty heading is never useful output — suppress both.
        return;
    }

    let hashes = "#".repeat(depth);
    s.push_literal(format!("{} {}\n\n", hashes, ff.heading));

    if rendered.len() == 1 {
        // Single item → paragraph form.
        s.push_literal(format!("{}\n\n", rendered[0]));
    } else {
        for body in &rendered {
            s.push_literal(format!("- {}\n", body));
        }
        s.push_literal("\n");
    }
}

/// Render one `IrFreeformContent` body. The text field already holds the
/// fully-rendered prose (`lower::lower_freeform_item` runs `constraint::render`
/// for `require`/`avoid`/`must`/`must avoid` clauses at lower time and
/// dereferences `NameRef` items through the `texts` map). Emit projects the
/// stored text verbatim so the two paths cannot drift on what a freeform item
/// renders to.
fn render_freeform_item_body(item: &crate::ir::IrFreeformContent) -> String {
    item.text.clone()
}

fn trim_trailing_blank_line(s: &mut Scaffold) {
    // The last chunk (if any) is a Literal in the patterns above. If it ends with
    // a redundant trailing newline, trim. The cheapest correct implementation is to
    // walk the tail of `chunks` and pop newlines.
    while let Some(Chunk::Literal(text)) = s.chunks.last_mut() {
        while text.ends_with("\n\n") {
            text.pop();
        }
        if text.is_empty() {
            s.chunks.pop();
            continue;
        }
        break;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IrArena, IrNode, IrParam, IrSkill, NodeId};

    #[test]
    fn render_context_entry_named_emits_kebab_label_and_indented_body() {
        let body = "First paragraph.\n\n- nested bullet\n- another nested";
        let out = render_context_entry(body, Some("glyph_overview"));
        assert_eq!(
            out,
            "- **glyph-overview**\n\n  First paragraph.\n\n  - nested bullet\n  - another nested\n\n"
        );
    }

    #[test]
    fn render_context_entry_unnamed_single_line_keeps_simple_form() {
        let out = render_context_entry("This codebase follows a monorepo layout.", None);
        assert_eq!(out, "- This codebase follows a monorepo layout.\n\n");
    }

    #[test]
    fn render_context_entry_unnamed_multiline_indents_continuation_lines() {
        let body = "First line.\n\nSecond paragraph.\n- nested bullet";
        let out = render_context_entry(body, None);
        assert_eq!(
            out,
            "- First line.\n\n  Second paragraph.\n  - nested bullet\n\n"
        );
    }

    #[test]
    fn render_context_entry_blank_lines_stay_empty_no_trailing_whitespace() {
        let body = "Para A.\n\nPara B.";
        let out = render_context_entry(body, Some("alpha"));
        // Critical: no `  ` whitespace-only blank line between paragraphs.
        assert!(
            !out.contains("\n  \n"),
            "blank line must not carry indent whitespace: {:?}",
            out
        );
    }

    #[test]
    fn build_parameters_section_emits_one_span_per_param() {
        let mut arena = IrArena::new();
        let s_id = arena.push(IrNode::Skill(IrSkill {
            node_id: NodeId(0),
            name: "demo".into(),
            description: "Demo.".into(),
            effects: vec![],
            params: vec![IrParam {
                name: "branch".into(),
                default: None,
                description: None,
                type_annotation: None,
            }],
            steps: vec![],
            context: vec![],
            constraints: vec![],
            return_text: None,
            return_type: None,
            output_contract: None,
            return_type_text: None,
            return_local_ref: None,
            freeform_sections: Vec::new(),
            description_source_line: None,
            context_source_line: None,
            constraints_source_line: None,
            flow_source_line: None,
        }));
        arena.set_root_skill(s_id);

        let scaffold = build(&arena, false);
        let span_count = scaffold
            .chunks
            .iter()
            .filter(|c| matches!(c, Chunk::Span(sp) if sp.kind == SpanKind::ParamDescription))
            .count();
        assert_eq!(span_count, 1, "one ParamDescription span per param");
    }

    /// Task 2 (ParamDescription hard-fail plan): when a param carries an
    /// author-provided description, the scaffold must NOT push a
    /// ParamDescription span (the author's prose lands via push_literal).
    /// This is a precondition for Task 4 flipping the stub-fill arm to
    /// hard-fail — existing skills-with-described-params must not regress.
    #[test]
    fn param_description_span_elided_when_description_present() {
        let mut arena = IrArena::new();
        let s_id = arena.push(IrNode::Skill(IrSkill {
            node_id: NodeId(0),
            name: "demo".into(),
            description: "Demo.".into(),
            effects: vec![],
            params: vec![IrParam {
                name: "branch".into(),
                default: None,
                description: Some("the branch to inspect".into()),
                type_annotation: None,
            }],
            steps: vec![],
            context: vec![],
            constraints: vec![],
            return_text: None,
            return_type: None,
            output_contract: None,
            return_type_text: None,
            return_local_ref: None,
            freeform_sections: Vec::new(),
            description_source_line: None,
            context_source_line: None,
            constraints_source_line: None,
            flow_source_line: None,
        }));
        arena.set_root_skill(s_id);

        let scaffold = build(&arena, false);
        let span_count = scaffold
            .chunks
            .iter()
            .filter(|c| matches!(c, Chunk::Span(sp) if sp.kind == SpanKind::ParamDescription))
            .count();
        assert_eq!(
            span_count, 0,
            "described params must not emit a ParamDescription span"
        );
    }

    /// Task 2 (ParamDescription hard-fail plan): an undescribed, untyped
    /// param must still emit exactly one ParamDescription span so the
    /// stub-fill pass (Task 4) can hard-fail with a remediation diagnostic.
    #[test]
    fn param_description_span_pushed_when_description_absent() {
        let mut arena = IrArena::new();
        let s_id = arena.push(IrNode::Skill(IrSkill {
            node_id: NodeId(0),
            name: "demo".into(),
            description: "Demo.".into(),
            effects: vec![],
            params: vec![IrParam {
                name: "branch".into(),
                default: None,
                description: None,
                type_annotation: None,
            }],
            steps: vec![],
            context: vec![],
            constraints: vec![],
            return_text: None,
            return_type: None,
            output_contract: None,
            return_type_text: None,
            return_local_ref: None,
            freeform_sections: Vec::new(),
            description_source_line: None,
            context_source_line: None,
            constraints_source_line: None,
            flow_source_line: None,
        }));
        arena.set_root_skill(s_id);

        let scaffold = build(&arena, false);
        let span_count = scaffold
            .chunks
            .iter()
            .filter(|c| matches!(c, Chunk::Span(sp) if sp.kind == SpanKind::ParamDescription))
            .count();
        assert_eq!(
            span_count, 1,
            "undescribed param must emit exactly one ParamDescription span"
        );
    }

    /// Phase 4 Emit prose tests (`.flow-assign-spec.md` §9).
    ///
    /// We build via parse → analyze → lower → expand → emit directly so the
    /// pipeline ignores Repairable diagnostics that don't affect lower (e.g.,
    /// `missing-effects`, `stdlib-missing-import` — the latter does not
    /// suppress lower's stdlib-aware lookup, so the IR carries `is_agent`
    /// regardless). Mirrors the test rig in `lower::flow_assign_lower_tests`.
    fn compile_to_md(src: &str) -> String {
        use crate::analyze::analyze_with_diagnostics;
        use crate::diagnostic::DiagBag;
        use crate::domain_registry::Registry;
        use crate::span::LineIndex;
        let (file, _) = crate::parse::parse(src, 0).expect("source should parse");
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let mut registry = Registry::new();
        let analyzed =
            analyze_with_diagnostics(file, 0, "test.glyph", &line_index, &mut bag, &mut registry);
        let arena = crate::lower::lower(&analyzed).expect("source should lower");
        let arena = crate::expand::expand_step1(arena);
        crate::emit::emit(&arena, false).expect("trivial fixture must compile")
    }

    /// §9.1 (value shape): producer step appends `Refer to this result as <n>.`
    /// (bare `n`, no quotes).
    #[test]
    fn emit_value_binding_naming_sentence() {
        let src = r#"
block inspect_repo(scope = ".") -> RepoContext
    "Inspect {scope}."

skill demo() -> RepoContext
    description: "demo"
    flow:
        ctx = inspect_repo(".")
        return ctx
"#;
        let md = compile_to_md(src);
        assert!(
            md.contains("Refer to this result as ctx."),
            "missing value-binding naming sentence:\n{md}"
        );
    }

    /// §9.1 (agent shape): `subagent` callee → `Refer to this agent as 'n.'`
    /// (single quotes around `n`, matching GLYPH_LANGUAGE_GUIDE §18.4).
    #[test]
    fn emit_agent_binding_naming_sentence() {
        let src = r#"
import "@glyph/std" { subagent }

skill demo()
    description: "demo"
    flow:
        researcher = subagent("investigate this area")
        return researcher
"#;
        let md = compile_to_md(src);
        assert!(
            md.contains("Refer to this agent as 'researcher.'"),
            "missing agent-binding naming sentence:\n{md}"
        );
    }

    /// §9.2: deterministic local-ref substitution turns `{ctx}` into bare `ctx`
    /// in inline-instruction text BEFORE `push_literal`. Parameter slots
    /// (different name) must pass through untouched — covered indirectly by
    /// the producer's resolved body containing `{scope}`.
    #[test]
    fn emit_substitutes_local_refs_in_inline_text() {
        let src = r#"
block inspect_repo(scope = ".") -> RepoContext
    "Inspect {scope}."

skill demo()
    description: "demo"
    flow:
        ctx = inspect_repo(".")
        "Use the result {ctx} to find issues"
        return ctx
"#;
        let md = compile_to_md(src);
        assert!(
            md.contains("Use the result ctx to find issues"),
            "expected `{{ctx}}` substituted to bare `ctx`:\n{md}"
        );
        assert!(
            !md.contains("{ctx}"),
            "literal `{{ctx}}` leaked into output:\n{md}"
        );
    }

    /// §9.2: substitution must apply to inline-instruction text emitted inside
    /// a branch arm body, not just at the top level. A `{ctx}` slot whose name
    /// is a flow-local in scope at the arm site must become bare `ctx`.
    #[test]
    fn emit_substitutes_local_refs_in_arm_body() {
        let src = r#"
const big_change = "the change is big"

block inspect_repo(scope = ".") -> RepoContext
    "Inspect {scope}."

skill demo() -> RepoContext
    description: "demo"
    flow:
        ctx = inspect_repo(".")
        if big_change:
            "Use the result {ctx} inside this arm"
        return ctx
"#;
        let md = compile_to_md(src);
        assert!(
            md.contains("Use the result ctx inside this arm"),
            "expected `{{ctx}}` substituted to bare `ctx` inside arm body:\n{md}"
        );
        assert!(
            !md.contains("{ctx}"),
            "literal `{{ctx}}` leaked into arm body:\n{md}"
        );
    }

    /// §9.3: return prose uses noun phrase derived from
    /// `callee_output_contract` → `callee_return_type_text` → "the result".
    /// Here the callee block declares `-> RepoContext` (no descriptive output
    /// contract), so the noun phrase falls back to the type-text path.
    #[test]
    fn emit_return_prose_uses_noun_phrase() {
        let src = r#"
block inspect_repo(scope = ".") -> RepoContext
    "Inspect {scope}."

skill demo() -> RepoContext
    description: "demo"
    flow:
        ctx = inspect_repo(".")
        return ctx
"#;
        let md = compile_to_md(src);
        assert!(
            md.contains("Your result is ctx"),
            "expected return prose `Your result is ctx`:\n{md}"
        );
        assert!(
            md.contains("produced above"),
            "expected `produced above` parenthetical in return prose:\n{md}"
        );
    }

    #[test]
    fn span_payload_default_carries_new_call_body_shape_fields() {
        let p = SpanPayload::default();
        assert!(p.target_name.is_none());
        assert!(p.projection_mode.is_none());
        assert!(p.local_refs.is_empty());
        assert!(p.post_merge_return_sentence.is_none());
    }

    #[test]
    fn projection_mode_variants_exist() {
        let modes = [
            ProjectionMode::Inline,
            ProjectionMode::SameFileProcedure,
            ProjectionMode::ExternalFile,
            ProjectionMode::StdlibBound,
        ];
        assert_eq!(modes.len(), 4);
    }

    #[test]
    fn call_needs_llm_fill_recognises_modifier_and_local_refs() {
        use crate::ir::{IrCall, LocalRef};
        let mut c = IrCall {
            node_id: NodeId(0),
            target: "x".into(),
            args: Vec::new(),
            resolved_body: None,
            site_modifier: None,
            projection_tier: Some(1),
            procedure_path: None,
            return_type: None,
            callee_output_contract: None,
            callee_return_type_text: None,
            bound_name: None,
            local_refs: Vec::new(),
            is_agent: false,
        };
        assert!(
            !call_needs_llm_fill(&c),
            "trivial Call must not need LLM fill"
        );
        c.site_modifier = Some("focus on lint".into());
        assert!(call_needs_llm_fill(&c), "with-modifier triggers LLM fill");
        c.site_modifier = None;
        c.local_refs.push(LocalRef {
            name: "ctx".into(),
            node_id: NodeId(7),
        });
        assert!(
            call_needs_llm_fill(&c),
            "non-empty local_refs triggers LLM fill"
        );
    }

    #[test]
    fn projection_mode_from_maps_tier_and_bound_name() {
        use crate::ir::IrCall;
        let mk = |tier: Option<u8>, bound: Option<&str>| IrCall {
            node_id: NodeId(0),
            target: "x".into(),
            args: Vec::new(),
            resolved_body: None,
            site_modifier: None,
            projection_tier: tier,
            procedure_path: None,
            return_type: None,
            callee_output_contract: None,
            callee_return_type_text: None,
            bound_name: bound.map(str::to_string),
            local_refs: Vec::new(),
            is_agent: false,
        };
        assert_eq!(
            projection_mode_from(&mk(Some(1), None)),
            Some(ProjectionMode::Inline)
        );
        assert_eq!(
            projection_mode_from(&mk(Some(2), None)),
            Some(ProjectionMode::SameFileProcedure)
        );
        assert_eq!(
            projection_mode_from(&mk(Some(3), None)),
            Some(ProjectionMode::ExternalFile)
        );
        assert_eq!(
            projection_mode_from(&mk(None, Some("subagent"))),
            Some(ProjectionMode::StdlibBound)
        );
        assert_eq!(
            projection_mode_from(&mk(Some(1), Some("subagent"))),
            Some(ProjectionMode::Inline)
        );
        assert_eq!(projection_mode_from(&mk(None, None)), None);
    }

    #[test]
    fn top_level_tier2_call_with_modifier_emits_span() {
        use crate::ir::{IrCall, IrNode, IrSkill};
        let mut arena = IrArena::new();
        let call_id = arena.push(IrNode::Call(IrCall {
            node_id: NodeId(0),
            target: "do_steps".into(),
            args: Vec::new(),
            resolved_body: None,
            site_modifier: Some("focus on errors".into()),
            projection_tier: Some(2),
            procedure_path: None,
            return_type: None,
            callee_output_contract: None,
            callee_return_type_text: None,
            bound_name: None,
            local_refs: Vec::new(),
            is_agent: false,
        }));
        let skill_id = arena.push(IrNode::Skill(IrSkill {
            node_id: NodeId(1),
            name: "demo".into(),
            description: "Demo.".into(),
            effects: vec![],
            params: vec![],
            steps: vec![call_id],
            context: vec![],
            constraints: vec![],
            return_text: None,
            return_type: None,
            output_contract: None,
            return_type_text: None,
            return_local_ref: None,
            freeform_sections: Vec::new(),
            description_source_line: None,
            context_source_line: None,
            constraints_source_line: None,
            flow_source_line: None,
        }));
        arena.set_root_skill(skill_id);
        let scaffold = build(&arena, false);
        let spans: Vec<_> = scaffold
            .chunks
            .iter()
            .filter_map(|c| match c {
                Chunk::Span(sp) if sp.kind == SpanKind::CallBodyShape => Some(sp),
                _ => None,
            })
            .collect();
        assert_eq!(
            spans.len(),
            1,
            "tier-2 top-level Call with modifier must emit a CallBodyShape span"
        );
        assert_eq!(
            spans[0].payload.projection_mode,
            Some(ProjectionMode::SameFileProcedure),
            "tier-2 projection_mode should be SameFileProcedure"
        );
    }

    #[test]
    fn top_level_tier2_call_without_modifier_stays_literal() {
        use crate::ir::{IrCall, IrNode, IrSkill};
        let mut arena = IrArena::new();
        let call_id = arena.push(IrNode::Call(IrCall {
            node_id: NodeId(0),
            target: "do_steps".into(),
            args: Vec::new(),
            resolved_body: None,
            site_modifier: None,
            projection_tier: Some(2),
            procedure_path: None,
            return_type: None,
            callee_output_contract: None,
            callee_return_type_text: None,
            bound_name: None,
            local_refs: Vec::new(),
            is_agent: false,
        }));
        let skill_id = arena.push(IrNode::Skill(IrSkill {
            node_id: NodeId(1),
            name: "demo".into(),
            description: "Demo.".into(),
            effects: vec![],
            params: vec![],
            steps: vec![call_id],
            context: vec![],
            constraints: vec![],
            return_text: None,
            return_type: None,
            output_contract: None,
            return_type_text: None,
            return_local_ref: None,
            freeform_sections: Vec::new(),
            description_source_line: None,
            context_source_line: None,
            constraints_source_line: None,
            flow_source_line: None,
        }));
        arena.set_root_skill(skill_id);
        let scaffold = build(&arena, false);
        let span_count = scaffold
            .chunks
            .iter()
            .filter(|c| matches!(c, Chunk::Span(sp) if sp.kind == SpanKind::CallBodyShape))
            .count();
        assert_eq!(span_count, 0, "trivial tier-2 Call must NOT emit a span");
    }

    #[test]
    fn top_level_stdlib_bound_with_modifier_emits_span() {
        use crate::ir::{IrCall, IrNode, IrSkill};
        let mut arena = IrArena::new();
        let call_id = arena.push(IrNode::Call(IrCall {
            node_id: NodeId(0),
            target: "subagent".into(),
            args: Vec::new(),
            resolved_body: None,
            site_modifier: Some("brief response".into()),
            projection_tier: None,
            procedure_path: None,
            return_type: None,
            callee_output_contract: None,
            callee_return_type_text: None,
            bound_name: Some("foo".into()),
            local_refs: Vec::new(),
            is_agent: false,
        }));
        let skill_id = arena.push(IrNode::Skill(IrSkill {
            node_id: NodeId(1),
            name: "demo".into(),
            description: "Demo.".into(),
            effects: vec![],
            params: vec![],
            steps: vec![call_id],
            context: vec![],
            constraints: vec![],
            return_text: None,
            return_type: None,
            output_contract: None,
            return_type_text: None,
            return_local_ref: None,
            freeform_sections: Vec::new(),
            description_source_line: None,
            context_source_line: None,
            constraints_source_line: None,
            flow_source_line: None,
        }));
        arena.set_root_skill(skill_id);
        let scaffold = build(&arena, false);
        let spans: Vec<_> = scaffold
            .chunks
            .iter()
            .filter_map(|c| match c {
                Chunk::Span(sp) if sp.kind == SpanKind::CallBodyShape => Some(sp),
                _ => None,
            })
            .collect();
        assert_eq!(spans.len(), 1);
        assert_eq!(
            spans[0].payload.projection_mode,
            Some(ProjectionMode::StdlibBound)
        );
    }

    #[test]
    fn top_level_tier1_call_with_modifier_emits_span_with_raw_resolved_body() {
        use crate::ir::{IrCall, IrNode, IrSkill, LocalRef};
        let mut arena = IrArena::new();
        let call_id = arena.push(IrNode::Call(IrCall {
            node_id: NodeId(0),
            target: "inspect".into(),
            args: Vec::new(),
            resolved_body: Some("Look at {ctx}.".into()),
            site_modifier: Some("focus on lint".into()),
            projection_tier: Some(1),
            procedure_path: None,
            return_type: None,
            callee_output_contract: None,
            callee_return_type_text: None,
            bound_name: None,
            local_refs: vec![LocalRef {
                name: "ctx".into(),
                node_id: NodeId(99),
            }],
            is_agent: false,
        }));
        let skill_id = arena.push(IrNode::Skill(IrSkill {
            node_id: NodeId(1),
            name: "demo".into(),
            description: "Demo.".into(),
            effects: vec![],
            params: vec![],
            steps: vec![call_id],
            context: vec![],
            constraints: vec![],
            return_text: None,
            return_type: None,
            output_contract: None,
            return_type_text: None,
            return_local_ref: None,
            freeform_sections: Vec::new(),
            description_source_line: None,
            context_source_line: None,
            constraints_source_line: None,
            flow_source_line: None,
        }));
        arena.set_root_skill(skill_id);
        let scaffold = build(&arena, false);
        let spans: Vec<_> = scaffold
            .chunks
            .iter()
            .filter_map(|c| match c {
                Chunk::Span(sp) if sp.kind == SpanKind::CallBodyShape => Some(sp),
                _ => None,
            })
            .collect();
        assert_eq!(
            spans.len(),
            1,
            "tier-1 top-level Call with modifier+local_refs must emit a span"
        );
        assert_eq!(
                spans[0].payload.resolved_body.as_deref(),
                Some("Look at {ctx}."),
                "tier-1 non-trivial path must keep the raw {{name}} slot intact (no substitute_local_refs_in)"
            );
    }

    #[test]
    fn top_level_tier1_final_call_with_modifier_carries_post_merge_return_sentence() {
        use crate::ir::{
            IrCall, IrNode, IrOutputContract, IrSkill, OutputSource, OutputTargetForm,
        };
        let mut arena = IrArena::new();
        let call_id = arena.push(IrNode::Call(IrCall {
            node_id: NodeId(0),
            target: "produce".into(),
            args: Vec::new(),
            resolved_body: Some("Inspect the working tree.".into()),
            site_modifier: Some("focus on lint".into()),
            projection_tier: Some(1),
            procedure_path: None,
            return_type: None,
            callee_output_contract: None,
            callee_return_type_text: None,
            bound_name: None,
            local_refs: Vec::new(),
            is_agent: false,
        }));
        let oc_id = arena.push(IrNode::OutputContract(IrOutputContract {
            node_id: NodeId(0),
            form: OutputTargetForm::Identifier("current_branch".into()),
            ty: None,
            source: OutputSource::SynthesizedByAgent,
        }));
        let skill_id = arena.push(IrNode::Skill(IrSkill {
            node_id: NodeId(1),
            name: "demo".into(),
            description: "Demo.".into(),
            effects: vec![],
            params: vec![],
            steps: vec![call_id],
            context: vec![],
            constraints: vec![],
            return_text: None,
            return_type: None,
            output_contract: Some(oc_id),
            return_type_text: None,
            return_local_ref: None,
            freeform_sections: Vec::new(),
            description_source_line: None,
            context_source_line: None,
            constraints_source_line: None,
            flow_source_line: None,
        }));
        arena.set_root_skill(skill_id);
        let scaffold = build(&arena, false);
        let spans: Vec<_> = scaffold
            .chunks
            .iter()
            .filter_map(|c| match c {
                Chunk::Span(sp) if sp.kind == SpanKind::CallBodyShape => Some(sp.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(spans.len(), 1);
        assert_eq!(
                    spans[0].payload.post_merge_return_sentence.as_deref(),
                    Some("Produce `current_branch`."),
                    "final-step tier-1 Call with output_contract must carry the §8.4 return sentence on the payload"
                );
    }

    /// Task 10 / spec §6.7: when a tier-1 Call carries a `bound_name`, the
    /// scaffold must emit the `naming_sentence_for_call` text as a Literal
    /// chunk immediately AFTER the CallBodyShape span (so the merger renders
    /// it after the span body verbatim, with no LLM rewrite of the naming
    /// sentence itself).
    #[test]
    fn naming_sentence_emitted_as_post_span_literal_chunk() {
        use crate::ir::{IrCall, IrNode, IrSkill};
        let mut arena = IrArena::new();
        let call_id = arena.push(IrNode::Call(IrCall {
            node_id: NodeId(0),
            target: "inspect".into(),
            args: Vec::new(),
            resolved_body: Some("Inspect.".into()),
            site_modifier: Some("focus".into()),
            projection_tier: Some(1),
            procedure_path: None,
            return_type: None,
            callee_output_contract: None,
            callee_return_type_text: None,
            bound_name: Some("foo".into()),
            local_refs: Vec::new(),
            is_agent: false,
        }));
        let skill_id = arena.push(IrNode::Skill(IrSkill {
            node_id: NodeId(1),
            name: "demo".into(),
            description: "Demo.".into(),
            effects: vec![],
            params: vec![],
            steps: vec![call_id],
            context: vec![],
            constraints: vec![],
            return_text: None,
            return_type: None,
            output_contract: None,
            return_type_text: None,
            return_local_ref: None,
            freeform_sections: Vec::new(),
            description_source_line: None,
            context_source_line: None,
            constraints_source_line: None,
            flow_source_line: None,
        }));
        arena.set_root_skill(skill_id);
        let scaffold = build(&arena, false);
        let mut iter = scaffold.chunks.iter().peekable();
        let mut found = false;
        while let Some(chunk) = iter.next() {
            if let Chunk::Span(sp) = chunk {
                if sp.kind == SpanKind::CallBodyShape {
                    let next = iter.next().expect("expected literal after span");
                    match next {
                        Chunk::Literal(l) => {
                            assert!(
                                l.contains("Refer to this") && l.contains("foo"),
                                "expected naming sentence as post-span literal; got: {l:?}"
                            );
                        }
                        _ => panic!("expected Literal after CallBodyShape span"),
                    }
                    found = true;
                    break;
                }
            }
        }
        assert!(found, "expected a CallBodyShape span in chunk stream");
    }

    /// Task 10 / spec §6.7: the §8.4 return-fold sentence rides on the span
    /// `payload.post_merge_return_sentence` (merged into the LLM-filled body
    /// at merge time), NOT as a separate Literal chunk between the span and
    /// the trailing newline.
    #[test]
    fn return_fold_is_carrier_not_literal_chunk_between_span_and_newline() {
        use crate::ir::{
            IrCall, IrNode, IrOutputContract, IrSkill, OutputSource, OutputTargetForm,
        };
        let mut arena = IrArena::new();
        let call_id = arena.push(IrNode::Call(IrCall {
            node_id: NodeId(0),
            target: "inspect".into(),
            args: Vec::new(),
            resolved_body: Some("Inspect.".into()),
            site_modifier: Some("focus".into()),
            projection_tier: Some(1),
            procedure_path: None,
            return_type: None,
            callee_output_contract: None,
            callee_return_type_text: None,
            bound_name: None,
            local_refs: Vec::new(),
            is_agent: false,
        }));
        let oc_id = arena.push(IrNode::OutputContract(IrOutputContract {
            node_id: NodeId(0),
            form: OutputTargetForm::Identifier("id".into()),
            ty: None,
            source: OutputSource::SynthesizedByAgent,
        }));
        let skill_id = arena.push(IrNode::Skill(IrSkill {
            node_id: NodeId(1),
            name: "demo".into(),
            description: "Demo.".into(),
            effects: vec![],
            params: vec![],
            steps: vec![call_id],
            context: vec![],
            constraints: vec![],
            return_text: None,
            return_type: None,
            output_contract: Some(oc_id),
            return_type_text: None,
            return_local_ref: None,
            freeform_sections: Vec::new(),
            description_source_line: None,
            context_source_line: None,
            constraints_source_line: None,
            flow_source_line: None,
        }));
        arena.set_root_skill(skill_id);
        let scaffold = build(&arena, false);
        let chunks = &scaffold.chunks;
        let mut found = false;
        for (i, chunk) in chunks.iter().enumerate() {
            if let Chunk::Span(sp) = chunk {
                if sp.kind == SpanKind::CallBodyShape {
                    assert_eq!(
                        sp.payload.post_merge_return_sentence.as_deref(),
                        Some("Produce `id`."),
                        "payload must carry the §8.4 return sentence (not a separate Literal chunk)"
                    );
                    let next = chunks.get(i + 1).expect("expected a chunk after the span");
                    match next {
                        Chunk::Literal(l) => assert_eq!(
                            l, "\n",
                            "expected newline literal immediately after span; got: {l:?}"
                        ),
                        _ => panic!(
                            "expected Literal('\\n') after span; no return-fold literal between them"
                        ),
                    }
                    found = true;
                    break;
                }
            }
        }
        assert!(found, "expected a CallBodyShape span in the chunk stream");
    }

    /// Task 10 / spec §6.7 — regression for the modifier-drop bug surfaced
    /// 2026-05-18: an if/else where both arms call the same procedure with
    /// only the then-arm carrying a `site_modifier`. Assert that the then-arm
    /// emits exactly one CallBodyShape span (so the expand pass weaves the
    /// modifier into prose), while the otherwise-arm stays as the
    /// deterministic `Follow the {kebab} procedure.` literal (no span).
    #[test]
    fn if_arms_with_same_target_only_modifier_arm_emits_call_body_shape_span() {
        use crate::ir::{BranchPredicateShape, IrBranch, IrCall, IrNode, IrSkill};
        let mut arena = IrArena::new();
        let then_call_id = arena.push(IrNode::Call(IrCall {
            node_id: NodeId(0),
            target: "build_walkthrough".into(),
            args: Vec::new(),
            resolved_body: None,
            site_modifier: Some("name each construct".into()),
            projection_tier: Some(2),
            procedure_path: None,
            return_type: None,
            callee_output_contract: None,
            callee_return_type_text: None,
            bound_name: None,
            local_refs: Vec::new(),
            is_agent: false,
        }));
        let else_call_id = arena.push(IrNode::Call(IrCall {
            node_id: NodeId(1),
            target: "build_walkthrough".into(),
            args: Vec::new(),
            resolved_body: None,
            site_modifier: None,
            projection_tier: Some(2),
            procedure_path: None,
            return_type: None,
            callee_output_contract: None,
            callee_return_type_text: None,
            bound_name: None,
            local_refs: Vec::new(),
            is_agent: false,
        }));
        let if_id = arena.push(IrNode::Branch(IrBranch {
            node_id: NodeId(2),
            condition: "x == 1".into(),
            then_body: vec![then_call_id],
            elif_branches: vec![],
            else_body: Some(vec![else_call_id]),
            resolved_predicates: None,
            predicate_shape: BranchPredicateShape::default(),
            classification: None,
        }));
        let skill_id = arena.push(IrNode::Skill(IrSkill {
            node_id: NodeId(3),
            name: "demo".into(),
            description: "Demo.".into(),
            effects: vec![],
            params: vec![],
            steps: vec![if_id],
            context: vec![],
            constraints: vec![],
            return_text: None,
            return_type: None,
            output_contract: None,
            return_type_text: None,
            return_local_ref: None,
            freeform_sections: Vec::new(),
            description_source_line: None,
            context_source_line: None,
            constraints_source_line: None,
            flow_source_line: None,
        }));
        arena.set_root_skill(skill_id);
        let scaffold = build(&arena, false);

        let span_count = scaffold
            .chunks
            .iter()
            .filter(|c| matches!(c, Chunk::Span(sp) if sp.kind == SpanKind::CallBodyShape))
            .count();
        assert_eq!(
            span_count, 1,
            "exactly one CallBodyShape span expected (the modifier-bearing then-arm); got {span_count}"
        );
        let modifier_span = scaffold
            .chunks
            .iter()
            .find_map(|c| match c {
                Chunk::Span(sp) if sp.kind == SpanKind::CallBodyShape => Some(sp),
                _ => None,
            })
            .expect("modifier-bearing CallBodyShape span must be present");
        assert_eq!(
            modifier_span.ir_node, then_call_id,
            "CallBodyShape span must reference the then-arm Call, not the otherwise-arm"
        );
        assert!(
            modifier_span.payload.site_modifier.is_some(),
            "CallBodyShape span must carry the with-modifier in its payload"
        );

        let any_literal_has_kebab = scaffold.chunks.iter().any(|c| {
            matches!(c, Chunk::Literal(l) if l.contains("Follow the build-walkthrough procedure."))
        });
        assert!(
            any_literal_has_kebab,
            "expected otherwise-arm to stay as deterministic 'Follow the build-walkthrough procedure.' literal"
        );
    }
}
