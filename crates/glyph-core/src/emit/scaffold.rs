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

/// Append a sentence to a Step body, separated by `". "` and stripping any
/// trailing period from the body so the transition reads naturally. Mirrors
/// `templates::append_return_sentence` but exposed locally for the
/// flow-assignment naming sentence.
pub(super) fn append_sentence(body: &str, sentence: &str) -> String {
    let trimmed = body.trim_end().trim_end_matches('.').trim_end();
    if trimmed.is_empty() {
        sentence.to_string()
    } else {
        format!("{trimmed}. {sentence}")
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
    BranchCondition,
    CallBodyShape,
}

#[derive(Clone, Debug, Default)]
pub struct SpanPayload {
    pub site_modifier: Option<String>,
    pub resolved_body: Option<String>,
    pub condition_expression: Option<String>,
    pub resolved_predicates: Option<BTreeMap<String, String>>,
    pub classification: Option<crate::condition::ConditionClassification>,
    pub predicate_shape: BranchPredicateShape,
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
    let stmt_count = b.flow_statements.len();
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
        // Collect the block's flow_statements + contract metadata before emitting.
        // Also collect freeform_sections so we can emit them as `####` children
        // of the procedure heading per design §4.1.5 / D12.
        let (flow_stmts, proc_oc_form, proc_rt_text, proc_freeform, proc_constraints, proc_context) = {
            let mut stmts: Option<Vec<String>> = None;
            let mut oc: Option<OutputTargetForm> = None;
            let mut rt: Option<String> = None;
            let mut ff: Vec<NodeId> = Vec::new();
            let mut cs: Vec<NodeId> = Vec::new();
            let mut cx: Vec<NodeId> = Vec::new();
            for node in arena.nodes() {
                if let IrNode::Block(b) = node {
                    if b.name == *target_name {
                        stmts = Some(b.flow_statements.clone());
                        oc = block_output_form_owned(arena, target_name);
                        rt = block_return_type_text_owned(arena, target_name);
                        ff = b.freeform_sections.clone();
                        cs = b.constraints.clone();
                        cx = b.context.clone();
                        break;
                    }
                }
            }
            (stmts, oc, rt, ff, cs, cx)
        };
        if let Some(stmts) = flow_stmts {
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
            // Codex review Finding 2: Tier 2 procedures must project block-level
            // `if`/elif/else through the same `branch::emit_to_scaffold` path the
            // skill flow uses. Pre-fix, `flow_statements` carried `if {condition}`
            // verbatim and the body was dropped entirely. The block's
            // `branch_steps` map (idx -> IrBranch NodeId) lets us swap the raw
            // string in for the structured node at the matching original index.
            let branch_steps: std::collections::HashMap<usize, NodeId> = arena
                .nodes()
                .iter()
                .find_map(|node| {
                    if let IrNode::Block(b) = node {
                        if b.name == *target_name {
                            return Some(b.branch_steps.clone());
                        }
                    }
                    None
                })
                .unwrap_or_default();
            // Filter out raw "return" markers; they are replaced by the §8.4 sentence.
            let visible_count = stmts.iter().filter(|st| st.as_str() != "return").count();
            let proc_sentence = templates::compute_return_sentence(
                proc_rt_text.as_deref(),
                proc_oc_form.as_ref(),
                &arena.type_registry,
            );

            if visible_count == 0 && proc_sentence.is_some() {
                // Return-only block: emit the §8.4 sentence as a standalone step.
                s.push_literal(format!("1. {}\n", proc_sentence.unwrap()));
            } else {
                let mut visible_idx: usize = 0;
                for (orig_idx, stmt) in stmts.iter().enumerate() {
                    if stmt == "return" {
                        continue;
                    }
                    visible_idx += 1;
                    let step_num = visible_idx;
                    let is_last = visible_idx == visible_count;
                    if let Some(branch_id) = branch_steps.get(&orig_idx) {
                        if let IrNode::Branch(br) = arena.get(*branch_id) {
                            super::branch::emit_to_scaffold(
                                &mut s,
                                arena,
                                br,
                                step_num,
                                &mut next_span_id,
                            );
                            // Codex review Finding (medium): when the
                            // last visible step is a branch, the
                            // §8.4 sentence still has to render. The
                            // branch emitter has no place to fold the
                            // sentence in (its arms are sub-steps),
                            // so we emit it as a trailing standalone
                            // step — same shape as the return-only
                            // procedure path above.
                            if is_last {
                                if let Some(sent) = proc_sentence.as_deref() {
                                    s.push_literal(format!("{}. {}\n", step_num + 1, sent));
                                }
                            }
                            continue;
                        }
                    }
                    if is_last {
                        match proc_sentence.as_deref() {
                            Some(sent) => {
                                let body = templates::append_return_sentence(stmt, sent);
                                s.push_literal(format!("{}. {}\n", step_num, body));
                            }
                            None => {
                                s.push_literal(format!("{}. {}\n", step_num, stmt));
                            }
                        }
                    } else {
                        s.push_literal(format!("{}. {}\n", step_num, stmt));
                    }
                }
            }
            s.push_literal("\n");

            // Freeform colon-keyword sections at depth 4 (children of the
            // `### Procedure: <name>` heading), per design §4.1.5 / D12.
            // Phase 3.C scope: a procedure block's `freeform_sections` is the
            // only authoring channel for sub-headings — there is no D9 merge
            // here because procedures do not project built-in body sections.
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
/// block in `build()`; output is byte-stable for a given input. Emits one
/// `SpanKind::ParamDescription` span per param (parameter description bodies
/// land via `emit::stub_fill`).
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
            s.push_literal(format!("- **{}**{}:\n", p.name, type_suffix));
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
            for line in desc_text.lines() {
                s.push_literal(format!("  {}\n", line));
            }
            s.push_literal(format!("  {}\n", meta_tail));
        } else if has_desc {
            // Single-line description form:
            //   - **<name>**[ (<Type>)]: <description>. Default: X. / Required.
            let trimmed = desc_text.trim_end_matches('.').trim_end();
            s.push_literal(format!("- **{}**{}: ", p.name, type_suffix));
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
            s.push_literal(format!("{}. {}\n", trimmed, meta_tail));
        } else {
            // No description form:
            //   - **<name>**[ (<Type>)]. Default: X. / Required.
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
                    // §9.2: substitute `{n}` → bare `n` for flow-locals in
                    // the inlined body. Parameter slots pass through and
                    // are filled by the existing stub-fill machinery.
                    let raw_body = c.resolved_body.as_deref().unwrap_or_default();
                    let body_owned = substitute_local_refs_in(raw_body, &c.local_refs);
                    let body = body_owned.as_str();
                    if is_last {
                        // Codex M4: when this final call IS the producer
                        // whose result the skill returns (`skill.return_local_ref`
                        // points at this `c.node_id`), the §9.3 return-prose
                        // step ("Your result is <name> …") will be emitted
                        // immediately below. Suppress the §8.4 generic
                        // "Return a `<T>`." suffix here so the two prose
                        // forms don't both render and duplicate the return
                        // statement.
                        let is_returned_producer = skill
                            .return_local_ref
                            .as_ref()
                            .is_some_and(|lr| lr.node_id == c.node_id);
                        // For tier-1 calls, the enclosing skill's output_contract
                        // wins when both exist: the skill's `return <…>` is the
                        // author's stated final return, so its template must take
                        // precedence over the inlined callee's contract.
                        // (`design/expand.md` §3.5;
                        // `design/compiled-output.md` §OutputContract Rendering.)
                        // The callee's OC is read directly off the Call node —
                        // populated at lower time for same-file callees and at
                        // the cross-file import fix-up for imported callees.
                        let (effective_form, effective_rt) = match skill_oc_form.as_ref() {
                            Some(form) => (Some(form), skill_rt_text.as_deref()),
                            None => (
                                c.callee_output_contract.as_ref(),
                                c.callee_return_type_text.as_deref(),
                            ),
                        };
                        let sentence = if is_returned_producer {
                            None
                        } else {
                            templates::compute_return_sentence(
                                effective_rt,
                                effective_form,
                                &arena.type_registry,
                            )
                        };
                        // A return-only callee (e.g. `block helper: do { return <x> }`)
                        // inlines with an empty resolved_body. Suffixing onto an
                        // empty body would yield a malformed leading-comma line;
                        // emit the §8.4 sentence as a standalone step instead.
                        let body_is_empty = body.trim().is_empty();
                        // Pre-fold the §8.4 sentence (if any) onto the
                        // body first; the §9.1 naming sentence — when
                        // applicable — then trails the whole thing so the
                        // step renders `<body>. <return-sentence>. Refer
                        // to this … as <n>.`
                        let mut step_text = match (sentence, body_is_empty) {
                            (Some(sent), true) => sent,
                            (Some(sent), false) => templates::append_return_sentence(body, &sent),
                            (None, _) => body.to_string(),
                        };
                        if let Some(naming) = naming_sentence_for_call(c) {
                            step_text = append_sentence(&step_text, &naming);
                        }
                        s.push_literal(format!("{}. {}\n", idx + 1, step_text));
                    } else {
                        // Producer step in a non-last position. Append the
                        // §9.1 naming sentence directly to the inlined
                        // body — this is the "action sentence + naming
                        // sentence in the same Step" rule from §9.1.
                        let mut step_text = body.to_string();
                        if let Some(naming) = naming_sentence_for_call(c) {
                            step_text = append_sentence(&step_text, &naming);
                        }
                        s.push_literal(format!("{}. {}\n", idx + 1, step_text));
                    }
                }
                IrNode::Call(c) if c.projection_tier == Some(2) => {
                    let kebab_name = c.target.replace('_', "-");
                    let mut step_text = format!("Follow the {} procedure below.", kebab_name);
                    if let Some(naming) = naming_sentence_for_call(c) {
                        step_text = append_sentence(&step_text, &naming);
                    }
                    s.push_literal(format!("{}. {}\n", idx + 1, step_text));
                    if procedure_seen.insert(c.target.clone()) {
                        procedure_order.push(c.target.clone());
                    }
                }
                IrNode::Call(c) if c.projection_tier == Some(3) => {
                    let proc_path = c.procedure_path.as_deref().unwrap_or("unknown");
                    let mut step_text = templates::external_file_step(proc_path);
                    if let Some(naming) = naming_sentence_for_call(c) {
                        step_text = append_sentence(&step_text, &naming);
                    }
                    s.push_literal(format!("{}. {}\n", idx + 1, step_text));
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
                    let mut step_text = format!("Call `{}`.", c.target);
                    if let Some(naming) = naming_sentence_for_call(c) {
                        step_text = append_sentence(&step_text, &naming);
                    }
                    s.push_literal(format!("{}. {}\n", idx + 1, step_text));
                }
                IrNode::Call(c) => {
                    panic!(
                        "IrNode::Call to `{}` survived past expand without tier assignment",
                        c.target
                    );
                }
                _ => panic!("Step node was not an InlineInstruction, Branch, or Call"),
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
    loop {
        match s.chunks.last_mut() {
            Some(Chunk::Literal(text)) => {
                while text.ends_with("\n\n") {
                    text.pop();
                }
                if text.is_empty() {
                    s.chunks.pop();
                    continue;
                }
                break;
            }
            _ => break,
        }
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
        crate::emit::emit(&arena, false)
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
}
