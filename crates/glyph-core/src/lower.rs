//! Phase 4 (Lower) — converts the loose AST into the typed IR arena.
//!
//! Walking-skeleton scope: handles only the constructs in `update_docs.glyph`.
//! Per `docs/adr/` §A4, IDs are allocated in pre-order source
//! traversal starting at `n0`.

use crate::ast::{
    BlockDecl, ConstValue, ConstraintMarkerKind, ContextEntry, Decl, ExportBlockDecl, FlowStmt,
    FreeformItem, FreeformSection, Param, ReservedMarker, ReturnExpr, Skill, SourceFile,
};
use crate::domain_registry::canonicalize_identifier;
use crate::ir::{
    BranchPredicateShape, IrArena, IrBlock, IrBlockFlowItem, IrBranch, IrCall, IrConstraint,
    IrContext, IrElifBranch, IrFreeformContent, IrFreeformSection, IrInlineInstruction, IrNode,
    IrOutputContract, IrParam, IrReturn, IrSkill, NodeId, OutputSource, OutputTargetForm, Polarity,
    Role, Strength,
};
use crate::kind_infer::{infer_primitive, Literal as KindLiteral, TypeTag};
use crate::output_target::OutputTargetExpr;
use std::collections::BTreeMap;

/// Map an AST `ConditionClassification` (optional) into an IR `BranchPredicateShape`.
/// Returns an all-false shape when no classification is present (e.g. in
/// nodes produced before Analyze runs, or in test fixtures).
fn predicate_shape_from(
    c: Option<&crate::condition::ConditionClassification>,
) -> BranchPredicateShape {
    let Some(c) = c else {
        return BranchPredicateShape::default();
    };
    BranchPredicateShape {
        has_boolean_token: c.has_boolean_token,
        has_predicate_token: c.has_predicate_token,
        has_compositional_operator: c.has_compositional_operator,
    }
}

/// Construct an `IrCall` with the field shape required by the spec invariant
/// (§3.10: all three lowering paths must emit structurally-identical Call IR).
/// All three `lower::*` Call sites route through this helper so a structural
/// drift on one site is impossible.
#[allow(clippy::too_many_arguments)]
fn build_call_ir_node(
    next: NodeId,
    target_node: String,
    args: Vec<String>,
    resolved_body: Option<String>,
    site_modifier: Option<String>,
    return_type: Option<TypeTag>,
    callee_output_contract: Option<OutputTargetForm>,
    callee_return_type_text: Option<String>,
    bound_name: Option<String>,
    is_agent: bool,
) -> IrCall {
    IrCall {
        node_id: next,
        target: target_node,
        args,
        resolved_body,
        site_modifier,
        projection_tier: None,
        procedure_path: None,
        return_type,
        callee_output_contract,
        callee_return_type_text,
        bound_name,
        local_refs: Vec::new(),
        is_agent,
    }
}

/// Map an identifier in type-position (the `<DomainType>` half of a
/// `-> <DomainType>` annotation) to its `TypeTag`. Six built-in names match
/// case-insensitive ASCII per `design/values-and-names.md` §Case
/// Normalization; everything else lowers to
/// `DomainType(canonicalize_identifier(name))` per D6.
///
/// **Banned-generic names like `List`/`Dict` are NOT special-cased here.** They
/// lower to `DomainType(canonical_form)`, e.g. `name_to_typetag("List")` →
/// `DomainType("list")`. The `G::analyze::generic-type-name` diagnostic
/// (chunk 2 banned-list, issue #83 AC3) surfaces them upstream. This module
/// records authorial intent in canonical form; analyze owns warnings.
fn name_to_typetag(name: &str) -> TypeTag {
    // Issue #84 codex pass 3 — F1 [P2]: classify by canonical form per D6,
    // not raw ASCII case. `values-and-names.md §Case Normalization` treats
    // underscores as insignificant alongside ASCII case, so `A_g_e_n_t`
    // (canonical `agent`) is the same name as `Agent` and must lower to
    // the same `TypeTag::Agent`. Pre-fix, `eq_ignore_ascii_case` missed
    // the underscore axis and underscore-perturbed built-ins fell through
    // to the `DomainType` fallback — wrong IR JSON.
    let canonical = canonicalize_identifier(name);
    match canonical.as_str() {
        "string" => TypeTag::String,
        "int" => TypeTag::Int,
        "float" => TypeTag::Float,
        "bool" => TypeTag::Bool,
        "none" => TypeTag::None,
        "agent" => TypeTag::Agent,
        // Everything else: lower to a DomainType keyed by the canonical
        // form already computed. Banned generics (e.g. `List`, `Dict`)
        // land here too — see module doc for the analyze-owns-warnings
        // rationale.
        _ => TypeTag::DomainType(canonical),
    }
}

/// Adapt a `ConstValue` into the `kind_infer::Literal` shape for the inferer.
/// Adapter is one-to-one — variants carry the same source-text rendering.
fn const_value_to_kind_literal(value: &ConstValue) -> KindLiteral {
    match value {
        ConstValue::String(s) => KindLiteral::String(s.clone()),
        // Both Int and Float carry numeric source text; the inferer
        // disambiguates by `'.'` presence per `values-and-names.md`
        // §Numeric Coercion.
        ConstValue::Int(s) | ConstValue::Float(s) => KindLiteral::Number(s.clone()),
        ConstValue::Bool(s) => KindLiteral::Bool(s.clone()),
    }
}

/// Build the const-binding map for a source file: name → (rendered source
/// text, inferred `TypeTag`). Runs the primitive-kind inferer on every
/// `Decl::Const` so chunk 1's module is exercised by the pipeline.
///
/// Bool values are lowercase-normalized at this lowering boundary per
/// `design/values-and-names.md` §Booleans (`true`/`false` accept
/// case-insensitive input but normalize to lowercase in IR). The AST keeps
/// the authored casing on `ConstValue::Bool`; this function is the single
/// site that applies the normalization on the way into the IR-facing
/// resolution map.
///
/// Exposed at `pub(crate)` so unit tests in this module (and the integrated
/// pipeline tests in `lib.rs`) can assert which TypeTag the inferer assigned
/// to each const without round-tripping through the full IR.
pub(crate) fn collect_consts(file: &SourceFile) -> BTreeMap<String, (String, TypeTag)> {
    let mut out: BTreeMap<String, (String, TypeTag)> = BTreeMap::new();
    for d in &file.decls {
        if let Decl::Const(c) = d {
            let lit = const_value_to_kind_literal(&c.node.value);
            let tag = infer_primitive(&lit);
            let rendered = match &c.node.value {
                ConstValue::Bool(s) => s.to_ascii_lowercase(),
                other => other.rendered().to_string(),
            };
            out.insert(c.node.name.clone(), (rendered, tag));
        }
    }
    out
}

#[derive(Debug)]
pub enum LowerError {
    NoSkill,
    UndefinedConstraintRef(String),
    UndefinedContextRef(String),
}

/// Resolve a block's flow body into a single text string for Tier 1 inline expansion.
/// Concatenates all inline instruction strings with spaces.
fn resolve_block_body_text(
    block: &BlockDecl,
    _texts: &BTreeMap<String, String>,
) -> Result<String, LowerError> {
    let mut parts: Vec<String> = Vec::new();
    for stmt in &block.flow {
        // Other flow stmt types are not handled for Tier 1 inline in this slice.
        if let FlowStmt::InlineString(s) = stmt {
            parts.push(s.clone());
        }
    }
    Ok(parts.join(" "))
}

/// PRD #103 / Slice 2 (#105): same-file export-block bodies must lower into
/// `IrCall.resolved_body` when called from a sibling skill or block, otherwise
/// the Validate pass fires `UnresolvedCallee`. ExportBlockDecl already keeps
/// its inline string payload in `flow_strings`; mirror the private-block
/// joiner above so call sites can reuse the same `BTreeMap` lookup pattern.
fn resolve_export_block_body_text(block: &ExportBlockDecl) -> String {
    block.flow_strings.join(" ")
}

fn resolve_context_entry(
    entry: &ContextEntry,
    texts: &BTreeMap<String, String>,
) -> Result<String, LowerError> {
    match entry {
        ContextEntry::InlineString(s) => Ok(s.clone()),
        ContextEntry::NameRef(name) => texts
            .get(&name.node)
            .cloned()
            .ok_or_else(|| LowerError::UndefinedContextRef(name.node.clone())),
    }
}

/// Map a reserved-marker AST variant to the `(Strength, Polarity, marker_word)`
/// triple stored on `IrFreeformContent`. `context` carries no strength /
/// polarity (returns `(_, _, "context")` shape — caller drops the
/// strength/polarity slots).
///
/// Shared by the IR-driven Tier 1 / Tier 2 path (via `lower_freeform_item`) and
/// the AST-driven Tier 3 path (via `lib::resolve_freeform_item`) so both arrive
/// at the same `(strength, polarity, word)` mapping.
pub(crate) fn marker_metadata(
    marker: ReservedMarker,
) -> (Option<Strength>, Option<Polarity>, &'static str) {
    match marker {
        ReservedMarker::Require => (Some(Strength::Soft), Some(Polarity::Require), "require"),
        ReservedMarker::Avoid => (Some(Strength::Soft), Some(Polarity::Avoid), "avoid"),
        ReservedMarker::Must => (Some(Strength::Hard), Some(Polarity::Require), "must"),
        ReservedMarker::MustAvoid => (Some(Strength::Hard), Some(Polarity::Avoid), "must avoid"),
        ReservedMarker::Context => (None, None, "context"),
    }
}

/// Convert a section name (`acceptance_criteria`, `quality`) into the Title
/// Case heading used in compiled output (`Acceptance Criteria`, `Quality`).
/// Splits on `_` and capitalises each word's first ASCII letter; unicode
/// segments pass through unchanged.
///
/// Exposed at `pub(crate)` so the Tier 3 caller (`lib.rs`) can use the same
/// canonical mapping as the IR-driven Tier 1 / Tier 2 path; the two callers
/// must not drift on how a `<name>:` colon-keyword becomes an `## <heading>`.
pub(crate) fn derive_heading(name: &str) -> String {
    name.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => {
                    let mut out = String::new();
                    for uc in c.to_uppercase() {
                        out.push(uc);
                    }
                    out.push_str(chars.as_str());
                    out
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Lower one `FreeformItem` to an `IrNode::FreeformContent` arena entry and
/// return its `NodeId`. Marker clauses are rendered into prose via the locked
/// constraint template (`emit::constraint::render`) for `require`/`avoid`/
/// `must`/`must avoid`; `context` clauses keep the raw text. Name refs
/// resolve through `texts`; unresolved refs surface as
/// `LowerError::UndefinedContextRef` (the same shape used by the canonical
/// context-section lower path).
fn lower_freeform_item(
    item: &FreeformItem,
    texts: &BTreeMap<String, String>,
    arena: &mut IrArena,
) -> Result<NodeId, LowerError> {
    let (text, marker_word, strength, polarity, name) = match item {
        FreeformItem::StringLiteral(s) => (s.node.clone(), None, None, None, None),
        FreeformItem::NameRef(name) => {
            let resolved = texts
                .get(&name.node)
                .cloned()
                .ok_or_else(|| LowerError::UndefinedContextRef(name.node.clone()))?;
            (resolved, None, None, None, Some(name.node.clone()))
        }
        FreeformItem::MarkerClause { marker, text } => {
            let (strength, polarity, word) = marker_metadata(*marker);
            // Resolve operand: bare-name operands look up via `texts`;
            // string-literal operands pass through. The AST shape stores
            // both under `text: Spanned<String>`, so we treat the operand as
            // a name iff `texts` has an entry for it. This mirrors the
            // canonical constraint-marker lower path (lines 304, 845) which
            // requires the operand to resolve via `texts`.
            let raw = text.node.clone();
            let resolved = match (strength, polarity) {
                (Some(s), Some(p)) => {
                    // `require`/`avoid`/`must`/`must avoid` — render via the
                    // locked four-form constraint template. Operand text is
                    // resolved through `texts` when it names a const; else
                    // treated as inline. Routed through the catalogue's
                    // `[constraints].expand_hook` (Phase 5) so a future
                    // re-skin is a one-line catalogue edit.
                    let body = texts.get(&raw).cloned().unwrap_or_else(|| raw.clone());
                    crate::sections::hooks::dispatch_constraints_expand(s, p, &body)
                }
                _ => {
                    // `context` — keep raw text, resolving name refs via
                    // `texts` (mirrors `resolve_context_entry`).
                    texts.get(&raw).cloned().unwrap_or_else(|| raw.clone())
                }
            };
            (resolved, Some(word.to_string()), strength, polarity, None)
        }
    };
    let next = arena.next_id();
    let id = arena.push(IrNode::FreeformContent(IrFreeformContent {
        node_id: next,
        text,
        marker_word,
        strength,
        polarity,
        name,
    }));
    Ok(id)
}

/// Resolve the heading for a freeform-lowered section. The catalogue's
/// explicit `heading` override wins; otherwise we fall back to
/// `derive_heading(name)` (Title Case of the snake_case identifier).
pub(crate) fn resolve_freeform_heading(
    catalogue: &crate::sections::SectionCatalogue,
    name: &str,
) -> String {
    catalogue
        .get(name)
        .and_then(|entry| entry.heading.clone())
        .unwrap_or_else(|| derive_heading(name))
}

/// Lower a single AST `FreeformSection` to an `IrFreeformSection` plus its
/// child `IrFreeformContent` entries. Returns the `NodeId` of the container.
/// Each child is pushed first so it owns a stable id; the container is then
/// pushed with the child id list.
///
/// The IR heading is taken from the catalogue when the section name is
/// catalogued and the entry declares an explicit `heading`; otherwise it
/// falls back to `derive_heading(name)` (Title Case of the snake_case
/// identifier).
fn lower_freeform_section(
    section: &FreeformSection,
    texts: &BTreeMap<String, String>,
    arena: &mut IrArena,
) -> Result<NodeId, LowerError> {
    let mut item_ids: Vec<NodeId> = Vec::with_capacity(section.items.len());
    for item in &section.items {
        item_ids.push(lower_freeform_item(item, texts, arena)?);
    }
    let catalogue = crate::sections::SectionCatalogue::load();
    let heading = resolve_freeform_heading(&catalogue, &section.name);
    let next = arena.next_id();
    let id = arena.push(IrNode::FreeformSection(IrFreeformSection {
        node_id: next,
        name: section.name.clone(),
        heading,
        source_line: section.span.line,
        items: item_ids,
    }));
    Ok(id)
}

/// Lower every freeform section in a slice and return the container ids in
/// source order. Convenience wrapper used by the skill and private-block
/// lower paths. Empty input produces an empty `Vec` (no arena allocations).
fn lower_freeform_sections(
    sections: &[FreeformSection],
    texts: &BTreeMap<String, String>,
    arena: &mut IrArena,
) -> Result<Vec<NodeId>, LowerError> {
    let mut ids = Vec::with_capacity(sections.len());
    for section in sections {
        ids.push(lower_freeform_section(section, texts, arena)?);
    }
    Ok(ids)
}

/// Extract the source name from a context entry if it was a NameRef.
/// Returns `None` for inline strings.
fn context_entry_name(entry: &ContextEntry) -> Option<String> {
    match entry {
        ContextEntry::NameRef(name) => Some(name.node.clone()),
        ContextEntry::InlineString(_) => None,
    }
}

/// Extract the `OutputTargetForm` from a `ReturnExpr::OutputTarget`,
/// returning `None` for any other return shape. Used to populate
/// `IrCall::callee_output_contract` so expand- and emit-time gates can read
/// the callee's OC without an arena lookup keyed by block name.
fn output_form_from_return_expr(expr: &ReturnExpr) -> Option<OutputTargetForm> {
    match expr {
        ReturnExpr::OutputTarget(OutputTargetExpr::Identifier(id)) => {
            Some(OutputTargetForm::Identifier(id.name.clone()))
        }
        ReturnExpr::OutputTarget(OutputTargetExpr::Description(d)) => {
            Some(OutputTargetForm::Description(d.content.clone()))
        }
        _ => None,
    }
}

/// Walk a private block's flow looking for the terminal
/// `Return(OutputTarget(...))` and, if found, return its lowered form. Mirrors
/// the scan in `lower_output_contract_for_flow` but produces the form
/// directly instead of pushing an `IrOutputContract` node.
fn block_callee_output_form(block: &BlockDecl) -> Option<OutputTargetForm> {
    for stmt in &block.flow {
        if let FlowStmt::Return(expr) = stmt {
            if let Some(form) = output_form_from_return_expr(expr) {
                return Some(form);
            }
        }
    }
    None
}

/// Like `block_callee_output_form` but for `ExportBlockDecl`, which carries
/// the terminal return separately from `flow_strings`.
fn export_block_callee_output_form(eb: &ExportBlockDecl) -> Option<OutputTargetForm> {
    eb.terminal_return
        .as_ref()
        .and_then(output_form_from_return_expr)
}

/// Issue #85: scan a decl's flow for a top-level
/// `FlowStmt::Return(ReturnExpr::OutputTarget(...))` and, if found, push the
/// matching `IrOutputContract` node into the arena. Returns its `NodeId` so
/// the caller can wire it into the enclosing decl's `output_contract` slot.
///
/// `enclosing_return_type` is the lowered `-> DomainType` annotation on the
/// enclosing decl (`Skill`/`Block`). The diagnostic for a missing annotation
/// is chunk 8/9's job — chunk 4 simply forwards `None` and proceeds.
fn lower_output_contract_for_flow(
    flow: &[FlowStmt],
    arena: &mut IrArena,
    enclosing_return_type: Option<TypeTag>,
) -> Option<NodeId> {
    for stmt in flow {
        let form = match stmt {
            FlowStmt::Return(ReturnExpr::OutputTarget(OutputTargetExpr::Identifier(id))) => {
                OutputTargetForm::Identifier(id.name.clone())
            }
            FlowStmt::Return(ReturnExpr::OutputTarget(OutputTargetExpr::Description(d))) => {
                OutputTargetForm::Description(d.content.clone())
            }
            _ => continue,
        };
        let next = NodeId(arena.len() as u32);
        let oc_id = arena.push(IrNode::OutputContract(IrOutputContract {
            node_id: next,
            form,
            ty: enclosing_return_type,
            source: OutputSource::SynthesizedByAgent,
        }));
        return Some(oc_id);
    }
    None
}

/// Flow-position-assignments §9.1 agent-shape rule. Determine whether a flow
/// call's callee returns an agent-shape value (`TypeTag::Agent`), so emit can
/// pick between "Refer to this agent as 'n.'" vs "Refer to this result as n.".
///
/// Resolution mirrors analyze's `resolve_callee_return_for_assign`:
///   1. Same-file blocks / export-blocks — case-insensitive "Agent" on the
///      raw `-> Type` text.
///   2. Stdlib — `crate::stdlib_sig(name).is_agent` (covers `subagent`).
///
/// Anything else (unresolved callee, plain `-> DomainType`, no annotation)
/// is treated as not-agent.
fn callee_is_agent(
    target: &str,
    blocks: &BTreeMap<String, &BlockDecl>,
    export_blocks: &BTreeMap<String, &ExportBlockDecl>,
) -> bool {
    if let Some(b) = blocks.get(target) {
        if let Some(rt) = b.return_type.as_ref() {
            return rt.node.eq_ignore_ascii_case("Agent");
        }
    }
    if let Some(eb) = export_blocks.get(target) {
        if let Some(rt) = eb.return_type.as_ref() {
            return rt.node.eq_ignore_ascii_case("Agent");
        }
    }
    crate::stdlib_sig(target)
        .map(|s| s.is_agent)
        .unwrap_or(false)
}

/// Lower a list of flow statements into IR nodes, returning node IDs.
/// Used for branch body lowering. Constraint/context markers inside branch
/// bodies stay inline (not hoisted) per pipeline.md §Phase 4.
fn lower_flow_body(
    stmts: &[FlowStmt],
    arena: &mut IrArena,
    texts: &BTreeMap<String, String>,
    blocks: &BTreeMap<String, &BlockDecl>,
    export_blocks: &BTreeMap<String, &ExportBlockDecl>,
) -> Result<Vec<NodeId>, LowerError> {
    let mut ids = Vec::new();
    for stmt in stmts {
        match stmt {
            FlowStmt::InlineString(text) => {
                let next = NodeId(arena.len() as u32);
                let id = arena.push(IrNode::InlineInstruction(IrInlineInstruction {
                    node_id: next,
                    text: text.clone(),
                    role: Role::Step,
                    local_refs: Vec::new(),
                }));
                ids.push(id);
            }
            FlowStmt::ConstraintMarker(marker) => {
                // Inside a branch body: stays inline, rendered as part of
                // conditional Step prose. Create an InlineInstruction with
                // the constraint text so it can be emitted as a sub-step.
                let resolved = texts
                    .get(&marker.name.node)
                    .cloned()
                    .ok_or_else(|| LowerError::UndefinedConstraintRef(marker.name.node.clone()))?;
                let prefix = match marker.marker {
                    ConstraintMarkerKind::Require => "",
                    ConstraintMarkerKind::Avoid => "Do not: ",
                    ConstraintMarkerKind::Must => "MUST: ",
                    ConstraintMarkerKind::MustAvoid => "MUST NOT: ",
                };
                let next = NodeId(arena.len() as u32);
                let id = arena.push(IrNode::InlineInstruction(IrInlineInstruction {
                    node_id: next,
                    text: format!("{}{}", prefix, resolved),
                    role: Role::Constraint,
                    local_refs: Vec::new(),
                }));
                ids.push(id);
            }
            FlowStmt::ContextMarker(entry) => {
                // Inside a branch body: stays inline per spec.
                let resolved = resolve_context_entry(entry, texts)?;
                let next = NodeId(arena.len() as u32);
                let id = arena.push(IrNode::InlineInstruction(IrInlineInstruction {
                    node_id: next,
                    text: format!("Note: {}", resolved),
                    role: Role::Context,
                    local_refs: Vec::new(),
                }));
                ids.push(id);
            }
            FlowStmt::Call {
                target,
                args,
                site_modifier,
                bound_name,
            } => {
                let resolved_body = if let Some(block) = blocks.get(target.node.as_str()) {
                    let body_text = resolve_block_body_text(block, texts)?;
                    Some(body_text)
                } else {
                    export_blocks
                        .get(target.node.as_str())
                        .map(|eb| resolve_export_block_body_text(eb))
                };
                // Issue #84 chunk 6: same-file callee return-type lookup. Reads
                // `BlockDecl::return_type` from the same `blocks` map used for
                // body-text resolution; stdlib calls (no map entry) → None.
                // Cross-file resolution is deferred to D17.
                let callee_rt_spanned = blocks
                    .get(target.node.as_str())
                    .and_then(|b| b.return_type.as_ref())
                    .or_else(|| {
                        export_blocks
                            .get(target.node.as_str())
                            .and_then(|b| b.return_type.as_ref())
                    });
                let return_type = callee_rt_spanned.map(|s| name_to_typetag(s.node.as_str()));
                let callee_return_type_text = callee_rt_spanned.map(|s| s.node.clone());
                let callee_output_contract = blocks
                    .get(target.node.as_str())
                    .and_then(|b| block_callee_output_form(b))
                    .or_else(|| {
                        export_blocks
                            .get(target.node.as_str())
                            .and_then(|eb| export_block_callee_output_form(eb))
                    });
                // Flow-position-assignments §8.1: copy the AST-side bound name
                // verbatim. §9.1: pre-compute the agent-shape flag here so
                // emit doesn't have to re-resolve the callee.
                let bound_name_lowered = bound_name.as_ref().map(|s| s.node.clone());
                let is_agent = bound_name_lowered.is_some()
                    && callee_is_agent(target.node.as_str(), blocks, export_blocks);
                let next = NodeId(arena.len() as u32);
                let id = arena.push(IrNode::Call(build_call_ir_node(
                    next,
                    target.node.clone(),
                    args.clone(),
                    resolved_body,
                    site_modifier.clone(),
                    return_type,
                    callee_output_contract,
                    callee_return_type_text,
                    bound_name_lowered,
                    is_agent,
                )));
                ids.push(id);
            }
            FlowStmt::Branch {
                condition,
                then_body,
                elif_branches,
                else_body,
                condition_classification,
                condition_refs: _,
            } => {
                let branch_id = NodeId(arena.len() as u32);
                // Reserve a slot for the Branch node.
                arena.push(IrNode::InlineInstruction(IrInlineInstruction {
                    node_id: branch_id,
                    text: String::new(),
                    role: Role::Step,
                    local_refs: Vec::new(),
                }));
                let then_ids = lower_flow_body(then_body, arena, texts, blocks, export_blocks)?;
                let mut ir_elifs = Vec::new();
                for elif in elif_branches {
                    let elif_ids =
                        lower_flow_body(&elif.body, arena, texts, blocks, export_blocks)?;
                    ir_elifs.push(IrElifBranch {
                        condition: elif.condition.clone(),
                        body: elif_ids,
                        predicate_shape: predicate_shape_from(
                            elif.condition_classification.as_ref(),
                        ),
                        classification: elif.condition_classification.clone(),
                    });
                }
                let ir_else = if let Some(eb) = else_body {
                    Some(lower_flow_body(eb, arena, texts, blocks, export_blocks)?)
                } else {
                    None
                };
                // Replace the placeholder with the actual Branch node.
                let nodes = arena.nodes_mut();
                nodes[branch_id.0 as usize] = IrNode::Branch(IrBranch {
                    node_id: branch_id,
                    condition: condition.clone(),
                    then_body: then_ids,
                    elif_branches: ir_elifs,
                    else_body: ir_else,
                    resolved_predicates: None,
                    predicate_shape: predicate_shape_from(condition_classification.as_ref()),
                    classification: condition_classification.clone(),
                });
                ids.push(branch_id);
            }
            FlowStmt::Return(_) | FlowStmt::BareName(_) => {
                // Return in branch body is caught by check_return_rules.
                // BareName is caught by Analyze.
            }
        }
    }
    Ok(ids)
}

pub fn lower(file: &SourceFile) -> Result<IrArena, LowerError> {
    lower_with_imports(file, &BTreeMap::new(), &BTreeMap::new(), &BTreeMap::new())
}

/// Resolve a parameter's `default` field for emission. Literal defaults
/// pass through; name_ref defaults (`p.default_is_name_ref == true`) are
/// substituted with the referenced const's rendered value. For string-typed
/// consts the substituted value is wrapped in surrounding quotes to match
/// the storage shape of literal-string defaults (parser pre-renders string
/// literals as `"\"value\""`), so the downstream `Default: X.` template
/// produces consistent output regardless of authoring form.
///
/// `same_file_consts` carries `(rendered, TypeTag)` per local `const`;
/// `imported_const_types` carries the inferred `TypeTag` for each imported
/// const value present in `imported_texts`. Name_refs that resolve via
/// `imported_texts` but lack a TypeTag entry fall through with the
/// imported rendered value as-is (best-effort; analyze should reject
/// unresolved refs).
pub(crate) fn resolve_param_default(
    p: &Param,
    same_file_consts: &BTreeMap<String, (String, TypeTag)>,
    imported_const_types: &BTreeMap<String, TypeTag>,
    imported_texts: &BTreeMap<String, String>,
) -> Option<String> {
    let raw = p.default.as_ref()?;
    if !p.default_is_name_ref {
        return Some(raw.clone());
    }
    if let Some((rendered, tag)) = same_file_consts.get(raw) {
        return Some(rerender_for_default(rendered, tag));
    }
    if let Some(rendered) = imported_texts.get(raw) {
        let tag = imported_const_types
            .get(raw)
            .cloned()
            .unwrap_or(TypeTag::None);
        return Some(rerender_for_default(rendered, &tag));
    }
    Some(raw.clone())
}

/// Re-render a const's rendered text for use in a parameter `Default:` slot.
/// Wraps `TypeTag::String` values in quotes; passes other primitive renderings
/// through verbatim.
fn rerender_for_default(rendered: &str, tag: &TypeTag) -> String {
    match tag {
        TypeTag::String => format!("\"{}\"", rendered),
        _ => rendered.to_string(),
    }
}

/// Lower with additional imported text values and type descriptions available
/// for constraint/context resolution and TypeRegistry folding. Same-file `type`
/// decls take precedence on name collision (mirrors the
/// `local-const-overwrites-imported-text` rule for cross-file `const`
/// shadowing earlier in this function).
///
/// `imported_const_types` complements `imported_texts` with each imported
/// const's inferred `TypeTag` so name_ref parameter defaults can be re-rendered
/// with the correct quoting (string consts wrap in `"…"`; numeric/bool/none
/// pass through verbatim).
pub fn lower_with_imports(
    file: &SourceFile,
    imported_texts: &BTreeMap<String, String>,
    imported_const_types: &BTreeMap<String, TypeTag>,
    imported_type_descriptions: &BTreeMap<String, String>,
) -> Result<IrArena, LowerError> {
    // Collect imported text values into a name → value map. Local bindings
    // are merged in below from `collect_consts` (the sole local source of
    // value-bindings post-issue-#81).
    let mut texts: BTreeMap<String, String> = imported_texts.clone();

    // Collect const declarations. For each const, run the primitive-kind
    // inferer (chunk 1) to compute its `TypeTag`; then merge the rendered
    // source-text form into the `texts` resolution map so reference sites
    // pick up the inlined value uniformly with text decls (Option C — kind
    // doesn't change observable output for #81 chunk 2).
    //
    // The full consts map (name → (rendered_text, TypeTag)) is built here for
    // future kind-aware lowering and exposed via a test helper below; chunk 2
    // doesn't yet have a reference site that needs the TypeTag, but the
    // inferer must be exercised on every const decl so chunk 1's module is
    // wired into the pipeline.
    let consts: BTreeMap<String, (String, TypeTag)> = collect_consts(file);
    for (name, (rendered, _tag)) in &consts {
        // Local consts overwrite imports of the same name (matches prior
        // `text` semantics from baseline `8a7d8dd`). Per
        // `design/values-and-names.md` §No Shadowing this should ultimately
        // be a `G::analyze::name-collision` hard error — tracked as a
        // follow-up; out of scope for #81.
        texts.insert(name.clone(), rendered.clone());
    }

    // Collect block declarations into a name → BlockDecl map.
    let mut blocks: BTreeMap<String, &BlockDecl> = BTreeMap::new();
    for d in &file.decls {
        if let Decl::Block(b) = d {
            blocks.insert(b.node.name.clone(), &b.node);
        }
    }
    // PRD #103 / Slice 2 (#105): same-file export-block decls so calls in the
    // skill's flow lower with `IrCall.resolved_body` populated. Without this
    // map the Validate pass would fire `UnresolvedCallee` against the
    // sibling export-block call.
    let mut export_blocks: BTreeMap<String, &ExportBlockDecl> = BTreeMap::new();
    for d in &file.decls {
        if let Decl::ExportBlock(b) = d {
            export_blocks.insert(b.node.name.clone(), &b.node);
        }
    }

    // Build the TypeRegistry from same-file `type` decls, then fold in
    // imported `export type` decls. Same-file decls take precedence on name
    // collision (matches the `local-const-overwrites-imported-text` rule
    // above for cross-file `const` shadowing).
    let mut type_registry = crate::ir::TypeRegistry::default();
    for d in &file.decls {
        if let Decl::TypeDecl(t) = d {
            type_registry.insert(&t.node.name, t.node.description.node.clone());
        }
    }
    for (name, desc) in imported_type_descriptions {
        // Imported types lose to same-file decls on collision (D6-canonical
        // form): if the consumer's `type` decl already populated this
        // canonical key, leave it.
        if type_registry.get(name).is_none() {
            type_registry.insert(name, desc.clone());
        }
    }

    // Find the skill declaration (exactly one in walking skeleton).
    let skill: &Skill = file
        .decls
        .iter()
        .find_map(|d| match d {
            Decl::Skill(s) => Some(&s.node),
            _ => None,
        })
        .ok_or(LowerError::NoSkill)?;
    // Issue #109 chunk 3 — Lower-side defensive guard. Analyze rejects any
    // AST whose `Skill`/`BlockDecl`/`ExportBlockDecl` carries non-empty
    // `extra_subsections` with `G::analyze::unmerged-duplicate-subsection`,
    // and the pipeline gate (`bag.has_error()`) stops Lower being called.
    // This `debug_assert!` is belt-and-suspenders: it trips if Lower is ever
    // invoked directly on an unrepaired AST, instead of silently dropping
    // the duplicate body's content.
    debug_assert!(
        skill.extra_subsections.is_empty(),
        "lower invariant violated: skill `{}` carries non-empty extra_subsections — \
         Analyze should have rejected it with G::analyze::unmerged-duplicate-subsection",
        skill.name
    );

    let mut arena = IrArena::new();

    // Reserve n0 for the skill (pre-order: container before children).
    let params: Vec<IrParam> = skill
        .params
        .iter()
        .map(|p| IrParam {
            name: p.name.clone(),
            default: resolve_param_default(p, &consts, imported_const_types, imported_texts),
            description: p.description.as_ref().map(|s| s.node.clone()),
            type_annotation: p.type_annotation.as_ref().map(|s| s.node.clone()),
        })
        .collect();
    let skill_return_type: Option<TypeTag> = skill
        .return_type
        .as_ref()
        .map(|s| name_to_typetag(s.node.as_str()));
    let skill_return_type_text: Option<String> = skill.return_type.as_ref().map(|s| s.node.clone());
    let skill_id = arena.push(IrNode::Skill(IrSkill {
        node_id: NodeId(0),
        name: skill.name.clone(),
        description: skill.description.clone().unwrap_or_default(),
        effects: skill
            .effects
            .iter()
            .filter(|e| e.as_str() != "none")
            .cloned()
            .collect(),
        params,
        steps: Vec::new(),
        context: Vec::new(),
        constraints: Vec::new(),
        return_text: None,
        return_type: skill_return_type.clone(),
        output_contract: None,
        return_type_text: skill_return_type_text,
        return_local_ref: None,
        freeform_sections: Vec::new(),
        description_source_line: skill.description_span.map(|s| s.line),
        context_source_line: skill.context_section_span.map(|s| s.line),
        constraints_source_line: skill.constraints_section_span.map(|s| s.line),
        flow_source_line: skill.flow_span.map(|s| s.line),
    }));

    // Lower block declarations to IrBlock nodes.
    for d in &file.decls {
        if let Decl::Block(b) = d {
            let block = &b.node;
            // Issue #109 chunk 3 — same Lower-side defensive guard as on
            // Skill above. See its comment for context.
            debug_assert!(
                block.extra_subsections.is_empty(),
                "lower invariant violated: block `{}` carries non-empty extra_subsections — \
                 Analyze should have rejected it with G::analyze::unmerged-duplicate-subsection",
                block.name
            );
            let body_text = resolve_block_body_text(block, &texts)?;
            // Collect outgoing call targets from the block's flow. Issue #84
            // codex pass 2 — F2: `return foo()` is also an outgoing call
            // edge; without this arm a self-recursive `return recurse()`
            // bypassed validate's DFS cycle check. Mirrors the symmetric
            // chunk-7a fix in `analyze::track_flow_usage` (`Return(Call)`
            // counts as a use of an imported block).
            let outgoing_calls: Vec<String> = block
                .flow
                .iter()
                .filter_map(|stmt| match stmt {
                    FlowStmt::Call { target, .. } => Some(target.node.clone()),
                    FlowStmt::Return(ReturnExpr::Call { target, .. }) => Some(target.node.clone()),
                    _ => None,
                })
                .collect();
            // Collect individual flow statement strings for Tier 2 procedure emission.
            // §3.10 procedure-body invariant: build `flow_items` (structured) by
            // walking the block's flow in source order. Call arms allocate
            // `IrNode::Call` arena entries that mirror skill-flow Call lowering
            // (preserving `site_modifier`, `bound_name`, `resolved_body`,
            // return-type, callee_output_contract, agent shape) so emit can
            // route procedure-body Calls through the same scaffold/span pipeline
            // as skill-flow Calls. Branch arms are linked by NodeId (allocated
            // in the subsequent branch_steps loop). Replaces the lossy
            // stringifier that dropped every payload (`call <target>`,
            // `if <cond>`, `constraint <name>`, ...).
            let mut flow_items: Vec<IrBlockFlowItem> = Vec::with_capacity(block.flow.len());
            for stmt in &block.flow {
                let item = match stmt {
                    FlowStmt::InlineString(s) => IrBlockFlowItem::Inline { text: s.clone() },
                    FlowStmt::Call {
                        target,
                        args,
                        site_modifier,
                        bound_name,
                    } => {
                        let resolved_body = if let Some(callee) = blocks.get(target.node.as_str()) {
                            Some(resolve_block_body_text(callee, &texts)?)
                        } else {
                            export_blocks
                                .get(target.node.as_str())
                                .map(|eb| resolve_export_block_body_text(eb))
                        };
                        let callee_rt_spanned = blocks
                            .get(target.node.as_str())
                            .and_then(|b| b.return_type.as_ref())
                            .or_else(|| {
                                export_blocks
                                    .get(target.node.as_str())
                                    .and_then(|b| b.return_type.as_ref())
                            });
                        let return_type =
                            callee_rt_spanned.map(|s| name_to_typetag(s.node.as_str()));
                        let callee_return_type_text = callee_rt_spanned.map(|s| s.node.clone());
                        let callee_output_contract = blocks
                            .get(target.node.as_str())
                            .and_then(|b| block_callee_output_form(b))
                            .or_else(|| {
                                export_blocks
                                    .get(target.node.as_str())
                                    .and_then(|eb| export_block_callee_output_form(eb))
                            });
                        let bound_name_lowered = bound_name.as_ref().map(|s| s.node.clone());
                        let is_agent = bound_name_lowered.is_some()
                            && callee_is_agent(target.node.as_str(), &blocks, &export_blocks);
                        let next = NodeId(arena.len() as u32);
                        let call_id = arena.push(IrNode::Call(build_call_ir_node(
                            next,
                            target.node.clone(),
                            args.clone(),
                            resolved_body,
                            site_modifier.clone(),
                            return_type,
                            callee_output_contract,
                            callee_return_type_text,
                            bound_name_lowered,
                            is_agent,
                        )));
                        IrBlockFlowItem::Call { node_id: call_id }
                    }
                    FlowStmt::Branch { condition, .. } => {
                        // Placeholder NodeId(0) — patched in the branch_steps loop
                        // below once the IrBranch is allocated. Keeping
                        // `flow_items` indices aligned with `block.flow` indices.
                        let _ = condition;
                        IrBlockFlowItem::Branch { node_id: NodeId(0) }
                    }
                    FlowStmt::ConstraintMarker(m) => IrBlockFlowItem::Constraint {
                        rendered: format!("constraint {}", m.name.node),
                    },
                    FlowStmt::ContextMarker(_) => IrBlockFlowItem::Context {
                        rendered: "context".to_string(),
                    },
                    FlowStmt::Return(_) => IrBlockFlowItem::Return,
                    FlowStmt::BareName(n) => IrBlockFlowItem::BareName {
                        name: n.node.clone(),
                    },
                };
                flow_items.push(item);
            }
            let block_return_type: Option<TypeTag> = block
                .return_type
                .as_ref()
                .map(|s| name_to_typetag(s.node.as_str()));
            let block_return_type_text: Option<String> =
                block.return_type.as_ref().map(|s| s.node.clone());
            // Issue #85: scan for a top-level `return <IDENT>` in this
            // block's flow. If present, push an `IrOutputContract` node now
            // (so its id < the block's id is fine — the block holds an
            // optional reference, not a strict pre-order requirement) and
            // store the id in `IrBlock.output_contract`.
            let block_output_contract: Option<NodeId> =
                lower_output_contract_for_flow(&block.flow, &mut arena, block_return_type.clone());
            // Codex review Finding 2: lower every `FlowStmt::Branch` in the
            // block's flow into a structured `IrBranch` node so the Tier 2
            // procedure emitter can dispatch to `branch::emit_to_scaffold`
            // instead of printing the raw `if {condition}` placeholder
            // produced for `flow_items`. Indexed by the position in
            // `flow_items` so emit can override per-step. The bodies
            // re-use `lower_flow_body` so nested `if`/elif/else arms get the
            // same InlineInstruction/Call/Branch treatment as skill arms.
            let mut branch_steps: std::collections::HashMap<usize, NodeId> =
                std::collections::HashMap::new();
            for (idx, stmt) in block.flow.iter().enumerate() {
                if let FlowStmt::Branch {
                    condition,
                    then_body,
                    elif_branches,
                    else_body,
                    condition_classification,
                    condition_refs: _,
                } = stmt
                {
                    let branch_id = NodeId(arena.len() as u32);
                    // Reserve a slot so recursively-lowered children get
                    // node IDs strictly greater than `branch_id`.
                    arena.push(IrNode::InlineInstruction(IrInlineInstruction {
                        node_id: branch_id,
                        text: String::new(),
                        role: Role::Step,
                        local_refs: Vec::new(),
                    }));
                    let then_ids =
                        lower_flow_body(then_body, &mut arena, &texts, &blocks, &export_blocks)?;
                    let mut ir_elifs = Vec::new();
                    for elif in elif_branches {
                        let elif_ids = lower_flow_body(
                            &elif.body,
                            &mut arena,
                            &texts,
                            &blocks,
                            &export_blocks,
                        )?;
                        ir_elifs.push(IrElifBranch {
                            condition: elif.condition.clone(),
                            body: elif_ids,
                            predicate_shape: predicate_shape_from(
                                elif.condition_classification.as_ref(),
                            ),
                            classification: elif.condition_classification.clone(),
                        });
                    }
                    let ir_else = if let Some(eb) = else_body {
                        Some(lower_flow_body(
                            eb,
                            &mut arena,
                            &texts,
                            &blocks,
                            &export_blocks,
                        )?)
                    } else {
                        None
                    };
                    let nodes = arena.nodes_mut();
                    nodes[branch_id.0 as usize] = IrNode::Branch(IrBranch {
                        node_id: branch_id,
                        condition: condition.clone(),
                        then_body: then_ids,
                        elif_branches: ir_elifs,
                        else_body: ir_else,
                        resolved_predicates: None,
                        predicate_shape: predicate_shape_from(condition_classification.as_ref()),
                        classification: condition_classification.clone(),
                    });
                    branch_steps.insert(idx, branch_id);
                    // §3.10: patch the corresponding `IrBlockFlowItem::Branch`
                    // placeholder (allocated above with NodeId(0)) to point at
                    // the freshly-allocated IrBranch arena entry.
                    if let Some(item) = flow_items.get_mut(idx) {
                        if matches!(item, IrBlockFlowItem::Branch { .. }) {
                            *item = IrBlockFlowItem::Branch { node_id: branch_id };
                        }
                    }
                }
            }
            // Codex review Finding (medium): collect string-default params
            // (with the quoted form unwrapped) so Expand can merge them into
            // `consts_for_lookup` for branch-predicate resolution. Mirrors
            // the type-annotation guard in Analyze's classifier
            // (`analyze.rs:3433-3438`): an explicit Bool/Int/Float annotation
            // means the param is not a PredicateConst and must NOT be merged.
            let mut block_string_default_params: BTreeMap<String, String> = BTreeMap::new();
            for p in &block.params {
                if p.default_is_name_ref {
                    continue;
                }
                if let Some(ta) = &p.type_annotation {
                    let name_lc = ta.node.to_ascii_lowercase();
                    if matches!(name_lc.as_str(), "bool" | "int" | "float") {
                        continue;
                    }
                }
                if let Some(default) = &p.default {
                    if default.starts_with('"') && default.ends_with('"') && default.len() >= 2 {
                        let inner = &default[1..default.len() - 1];
                        block_string_default_params.insert(p.name.clone(), inner.to_string());
                    }
                }
            }
            // Phase 3.B (Task 3.7): lower freeform sections owned by this
            // block before pushing the container so the `freeform_sections`
            // child ids are populated at construction. Empty for blocks
            // without any freeform sections (the common case) — no arena
            // allocations occur.
            let block_freeform_ids =
                lower_freeform_sections(&block.freeform_sections, &texts, &mut arena)?;
            // #167/#168: lower per-block `body_constraints` and `body_context`
            // markers into IrConstraint / IrContext arena nodes, mirroring the
            // skill-side declaration-level lowering below. The resulting
            // NodeIds populate `IrBlock.constraints` / `IrBlock.context` so
            // emit can render them on each procedure section.
            let mut block_constraint_ids: Vec<NodeId> = Vec::new();
            for marker in &block.body_constraints {
                let resolved = texts
                    .get(&marker.name.node)
                    .cloned()
                    .ok_or_else(|| LowerError::UndefinedConstraintRef(marker.name.node.clone()))?;
                let (strength, polarity) = match marker.marker {
                    ConstraintMarkerKind::Require => (Strength::Soft, Polarity::Require),
                    ConstraintMarkerKind::Avoid => (Strength::Soft, Polarity::Avoid),
                    ConstraintMarkerKind::Must => (Strength::Hard, Polarity::Require),
                    ConstraintMarkerKind::MustAvoid => (Strength::Hard, Polarity::Avoid),
                };
                let next = NodeId(arena.len() as u32);
                let id = arena.push(IrNode::Constraint(IrConstraint {
                    node_id: next,
                    text: resolved,
                    strength,
                    polarity,
                }));
                block_constraint_ids.push(id);
            }
            let mut block_context_ids: Vec<NodeId> = Vec::new();
            for entry in &block.body_context {
                let resolved = resolve_context_entry(entry, &texts)?;
                let name = context_entry_name(entry);
                let next = NodeId(arena.len() as u32);
                let id = arena.push(IrNode::Context(IrContext {
                    node_id: next,
                    text: resolved,
                    name,
                }));
                block_context_ids.push(id);
            }
            let next = NodeId(arena.len() as u32);
            arena.push(IrNode::Block(IrBlock {
                node_id: next,
                name: block.name.clone(),
                description: block.description.clone(),
                body_text,
                flow_items,
                resolved_word_count: None,
                outgoing_calls,
                return_type: block_return_type,
                output_contract: block_output_contract,
                return_type_text: block_return_type_text,
                branch_steps,
                string_default_params: block_string_default_params,
                freeform_sections: block_freeform_ids,
                description_source_line: block.description_span.map(|s| s.line),
                context_source_line: block.context_section_span.map(|s| s.line),
                constraints_source_line: block.constraints_section_span.map(|s| s.line),
                flow_source_line: block.flow_span.map(|s| s.line),
                context: block_context_ids,
                constraints: block_constraint_ids,
            }));
        }
    }

    // Lower flow → Step nodes. Constraint/context markers at flow top-level
    // are hoisted into the declaration's constraint/context lists (Phase 4 Lower
    // per pipeline.md). BareName flow statements are skipped (they are caught
    // by Analyze as G::analyze::text-in-flow before reaching Lower).
    let mut step_ids: Vec<NodeId> = Vec::new();
    let mut flow_hoisted_constraint_ids: Vec<NodeId> = Vec::new();
    let mut flow_hoisted_context_ids: Vec<NodeId> = Vec::new();
    let mut return_text: Option<String> = None;
    let mut skill_output_contract: Option<NodeId> = None;
    // Flow-position-assignments §8.2 producer table: bound_name → producing
    // IrCall.node_id. Populated only by top-level skill-flow calls; branch-arm
    // bindings do not leak (§6.1 lexical scoping). Consumed below to wire
    // `return_local_ref` when the skill returns a flow-local name.
    let mut skill_producers: BTreeMap<String, NodeId> = BTreeMap::new();
    let mut skill_return_local_ref: Option<crate::ir::LocalRef> = None;
    for stmt in &skill.flow {
        match stmt {
            FlowStmt::InlineString(text) => {
                let next = NodeId(arena.len() as u32);
                let id = arena.push(IrNode::InlineInstruction(IrInlineInstruction {
                    node_id: next,
                    text: text.clone(),
                    role: Role::Step,
                    local_refs: Vec::new(),
                }));
                step_ids.push(id);
            }
            FlowStmt::ConstraintMarker(marker) => {
                // Flow-top-level constraint → hoist to declaration's constraints list.
                let resolved = texts
                    .get(&marker.name.node)
                    .cloned()
                    .ok_or_else(|| LowerError::UndefinedConstraintRef(marker.name.node.clone()))?;
                let (strength, polarity) = match marker.marker {
                    ConstraintMarkerKind::Require => (Strength::Soft, Polarity::Require),
                    ConstraintMarkerKind::Avoid => (Strength::Soft, Polarity::Avoid),
                    ConstraintMarkerKind::Must => (Strength::Hard, Polarity::Require),
                    ConstraintMarkerKind::MustAvoid => (Strength::Hard, Polarity::Avoid),
                };
                let next = NodeId(arena.len() as u32);
                let id = arena.push(IrNode::Constraint(IrConstraint {
                    node_id: next,
                    text: resolved,
                    strength,
                    polarity,
                }));
                flow_hoisted_constraint_ids.push(id);
            }
            FlowStmt::ContextMarker(entry) => {
                // Flow-top-level context → hoist to declaration's context list.
                let resolved = resolve_context_entry(entry, &texts)?;
                let name = context_entry_name(entry);
                let next = NodeId(arena.len() as u32);
                let id = arena.push(IrNode::Context(IrContext {
                    node_id: next,
                    text: resolved,
                    name,
                }));
                flow_hoisted_context_ids.push(id);
            }
            FlowStmt::Call {
                target,
                args,
                site_modifier,
                bound_name,
            } => {
                // Create an IrCall node. Resolve callee body if block exists.
                let resolved_body = if let Some(block) = blocks.get(target.node.as_str()) {
                    let body_text = resolve_block_body_text(block, &texts)?;
                    Some(body_text)
                } else {
                    // Falls back to None for an undefined call; Analyze already flagged it.
                    export_blocks
                        .get(target.node.as_str())
                        .map(|eb| resolve_export_block_body_text(eb))
                };
                // Issue #84 chunk 6: same-file callee return-type lookup —
                // see the matching site in `lower_flow_body` above for the
                // shared rationale.
                let callee_rt_spanned = blocks
                    .get(target.node.as_str())
                    .and_then(|b| b.return_type.as_ref())
                    .or_else(|| {
                        export_blocks
                            .get(target.node.as_str())
                            .and_then(|b| b.return_type.as_ref())
                    });
                let return_type = callee_rt_spanned.map(|s| name_to_typetag(s.node.as_str()));
                let callee_return_type_text = callee_rt_spanned.map(|s| s.node.clone());
                let callee_output_contract = blocks
                    .get(target.node.as_str())
                    .and_then(|b| block_callee_output_form(b))
                    .or_else(|| {
                        export_blocks
                            .get(target.node.as_str())
                            .and_then(|eb| export_block_callee_output_form(eb))
                    });
                // Flow-position-assignments §8.1/§9.1: copy bound_name verbatim
                // and pre-compute the agent-shape flag for emit.
                let bound_name_lowered = bound_name.as_ref().map(|s| s.node.clone());
                let is_agent = bound_name_lowered.is_some()
                    && callee_is_agent(target.node.as_str(), &blocks, &export_blocks);
                let next = NodeId(arena.len() as u32);
                let id = arena.push(IrNode::Call(build_call_ir_node(
                    next,
                    target.node.clone(),
                    args.clone(),
                    resolved_body,
                    site_modifier.clone(),
                    return_type,
                    callee_output_contract,
                    callee_return_type_text,
                    bound_name_lowered.clone(),
                    is_agent,
                )));
                // §8.2 producer table for return_local_ref resolution. Top-level
                // skill-flow calls are visible to a top-level `return <name>`.
                // Branch-arm bindings do NOT leak (§6.1 lexical scoping mirror)
                // so they are intentionally not registered here. The producer
                // table is consumed below where `return_text`/
                // `return_local_ref` get computed.
                if let Some(n) = bound_name_lowered {
                    skill_producers.insert(n, id);
                }
                step_ids.push(id);
            }
            FlowStmt::Return(expr) => {
                // Flow-position-assignments §8.2: when `return <name>` resolves
                // to a flow-local producer, lift the binding into
                // `IrSkill.return_local_ref` and clear `return_text` so
                // Expand's legacy return-folding does not double-emit. Bare
                // `return`, inline-string returns, and `return <param>` (no
                // producer-table hit) keep the legacy `return_text` path.
                let mut bound_match: Option<crate::ir::LocalRef> = None;
                if let ReturnExpr::Name(name) = expr {
                    if let Some(producer_id) = skill_producers.get(&name.node) {
                        bound_match = Some(crate::ir::LocalRef {
                            name: name.node.clone(),
                            node_id: *producer_id,
                        });
                    }
                }

                // Capture the return expression text for return folding in Expand.
                let text = match expr {
                    ReturnExpr::None => None,
                    ReturnExpr::Call { target, args } => {
                        if args.is_empty() {
                            Some(format!("{}()", target.node))
                        } else {
                            Some(format!("{}({})", target.node, args.join(", ")))
                        }
                    }
                    ReturnExpr::Name(name) => Some(name.node.clone()),
                    ReturnExpr::Inline(s) => Some(s.clone()),
                    // Issue #85: handled out-of-band below — push an
                    // `IrOutputContract` instead of folding into return text.
                    ReturnExpr::OutputTarget(_) => None,
                };
                if let Some(local_ref) = bound_match {
                    // Single source of truth: when return_local_ref is Some,
                    // emit owns the return prose. Force return_text = None.
                    skill_return_local_ref = Some(local_ref);
                    return_text = None;
                } else {
                    return_text = text;
                }
                let form = match expr {
                    ReturnExpr::OutputTarget(OutputTargetExpr::Identifier(id)) => {
                        Some(OutputTargetForm::Identifier(id.name.clone()))
                    }
                    ReturnExpr::OutputTarget(OutputTargetExpr::Description(d)) => {
                        Some(OutputTargetForm::Description(d.content.clone()))
                    }
                    _ => None,
                };
                if let Some(form) = form {
                    // ADR 0026: keep IrOutputContract as a metadata view for
                    // type-checking, and additionally push an IrReturn flow
                    // node so the renderable position lives in skill.steps.
                    let oc_next = NodeId(arena.len() as u32);
                    let oc_id = arena.push(IrNode::OutputContract(IrOutputContract {
                        node_id: oc_next,
                        form: form.clone(),
                        ty: skill_return_type.clone(),
                        source: OutputSource::SynthesizedByAgent,
                    }));
                    skill_output_contract = Some(oc_id);
                    let ret_next = NodeId(arena.len() as u32);
                    // ADR 0026 + reviewer follow-up: resolve the binding to its producing
                    // flow node at lower time when the return is in identifier form. This
                    // captures provenance explicitly on the IR node instead of having emit
                    // re-walk the flow with name resolution.
                    let producer_node_id = match &form {
                        OutputTargetForm::Identifier(n) => skill_producers.get(n).copied(),
                        OutputTargetForm::Description(_) => None,
                    };
                    let ret_id = arena.push(IrNode::Return(IrReturn {
                        node_id: ret_next,
                        form,
                        ty: skill_return_type.clone(),
                        producer_node_id,
                    }));
                    step_ids.push(ret_id);
                }
            }
            FlowStmt::BareName(_) => {
                // BareName in flow is caught by Analyze before Lower runs.
                // If we somehow reach here, skip silently — the diagnostic
                // was already emitted.
            }
            FlowStmt::Branch {
                condition,
                then_body,
                elif_branches,
                else_body,
                condition_classification,
                condition_refs: _,
            } => {
                let branch_id = NodeId(arena.len() as u32);
                // Reserve a placeholder slot.
                arena.push(IrNode::InlineInstruction(IrInlineInstruction {
                    node_id: branch_id,
                    text: String::new(),
                    role: Role::Step,
                    local_refs: Vec::new(),
                }));
                let then_ids =
                    lower_flow_body(then_body, &mut arena, &texts, &blocks, &export_blocks)?;
                let mut ir_elifs = Vec::new();
                for elif in elif_branches {
                    let elif_ids =
                        lower_flow_body(&elif.body, &mut arena, &texts, &blocks, &export_blocks)?;
                    ir_elifs.push(IrElifBranch {
                        condition: elif.condition.clone(),
                        body: elif_ids,
                        predicate_shape: predicate_shape_from(
                            elif.condition_classification.as_ref(),
                        ),
                        classification: elif.condition_classification.clone(),
                    });
                }
                let ir_else = if let Some(eb) = else_body {
                    Some(lower_flow_body(
                        eb,
                        &mut arena,
                        &texts,
                        &blocks,
                        &export_blocks,
                    )?)
                } else {
                    None
                };
                // Replace placeholder with actual Branch.
                let nodes = arena.nodes_mut();
                nodes[branch_id.0 as usize] = IrNode::Branch(IrBranch {
                    node_id: branch_id,
                    condition: condition.clone(),
                    then_body: then_ids,
                    elif_branches: ir_elifs,
                    else_body: ir_else,
                    resolved_predicates: None,
                    predicate_shape: predicate_shape_from(condition_classification.as_ref()),
                    classification: condition_classification.clone(),
                });
                step_ids.push(branch_id);
            }
        }
    }

    // Lower body-level constraint markers → Constraint nodes.
    let mut constraint_ids: Vec<NodeId> = Vec::new();
    for marker in &skill.body_constraints {
        let resolved = texts
            .get(&marker.name.node)
            .cloned()
            .ok_or_else(|| LowerError::UndefinedConstraintRef(marker.name.node.clone()))?;
        let (strength, polarity) = match marker.marker {
            ConstraintMarkerKind::Require => (Strength::Soft, Polarity::Require),
            ConstraintMarkerKind::Avoid => (Strength::Soft, Polarity::Avoid),
            ConstraintMarkerKind::Must => (Strength::Hard, Polarity::Require),
            ConstraintMarkerKind::MustAvoid => (Strength::Hard, Polarity::Avoid),
        };
        let next = NodeId(arena.len() as u32);
        let id = arena.push(IrNode::Constraint(IrConstraint {
            node_id: next,
            text: resolved,
            strength,
            polarity,
        }));
        constraint_ids.push(id);
    }

    // Append flow-hoisted constraints (deduped by canonical text + strength + polarity).
    for id in flow_hoisted_constraint_ids {
        if let IrNode::Constraint(c) = arena.get(id) {
            let dominated = constraint_ids.iter().any(|existing_id| {
                if let IrNode::Constraint(e) = arena.get(*existing_id) {
                    e.text == c.text && e.strength == c.strength && e.polarity == c.polarity
                } else {
                    false
                }
            });
            if !dominated {
                constraint_ids.push(id);
            }
        }
    }

    // Lower context entries (from context: section + body-level markers).
    let mut context_ids: Vec<NodeId> = Vec::new();
    let mut seen_context_texts: Vec<String> = Vec::new();

    let all_context_entries = skill
        .context_section
        .iter()
        .chain(skill.body_context.iter());
    for entry in all_context_entries {
        let resolved = resolve_context_entry(entry, &texts)?;
        if !seen_context_texts.contains(&resolved) {
            seen_context_texts.push(resolved.clone());
            let name = context_entry_name(entry);
            let next = NodeId(arena.len() as u32);
            let id = arena.push(IrNode::Context(IrContext {
                node_id: next,
                text: resolved,
                name,
            }));
            context_ids.push(id);
        }
    }

    // Append flow-hoisted context (deduped by canonical text).
    for id in flow_hoisted_context_ids {
        if let IrNode::Context(c) = arena.get(id) {
            if !seen_context_texts.contains(&c.text) {
                seen_context_texts.push(c.text.clone());
                context_ids.push(id);
            }
        }
    }

    // Phase 3.B (Task 3.7): lower the skill's freeform sections. Done here
    // (after all flow-driven lowering) so the freeform-content arena ids
    // sit after the canonical step / context / constraint ids without
    // changing the pre-existing pre-order traversal of the skill body.
    let skill_freeform_ids = lower_freeform_sections(&skill.freeform_sections, &texts, &mut arena)?;

    // Patch the skill node now that step/context/constraint IDs are known.
    {
        let nodes = arena.nodes_mut();
        if let IrNode::Skill(s) = &mut nodes[skill_id.0 as usize] {
            s.steps = step_ids;
            s.context = context_ids;
            s.constraints = constraint_ids;
            s.return_text = return_text;
            s.output_contract = skill_output_contract;
            s.return_local_ref = skill_return_local_ref;
            s.freeform_sections = skill_freeform_ids;
        }
    }
    arena.set_root_skill(skill_id);
    // Persist all const declarations (name → rendered body) for use by
    // downstream passes (Expand resolves bare-identifier predicate tokens
    // against this map). TypeTag is dropped — Expand only needs body text.
    let mut merged: BTreeMap<String, String> = consts
        .into_iter()
        .map(|(name, (rendered, _tag))| (name, rendered))
        .collect();
    // Closes the previously-noted "imported consts not merged" TODO.
    // Imported consts join same-file consts; same-file wins on collision
    // (defensive — analyze rejects collisions).
    for (name, rendered) in imported_texts.iter() {
        merged
            .entry(name.clone())
            .or_insert_with(|| rendered.clone());
    }
    arena.consts = merged;
    arena.type_registry = type_registry;

    Ok(arena)
}

#[cfg(test)]
mod freeform_heading_resolution_tests {
    //! Verify `lower_freeform_section` honors a catalogue entry's explicit
    //! `heading` override (D9/§4.2 of `design/glyph-freeform-sections-design`)
    //! rather than mechanically deriving from the section name.

    use super::*;
    use crate::sections::{CatalogueEntry, SectionCatalogue};

    #[test]
    fn resolve_uses_catalogue_heading_when_set() {
        let entry = CatalogueEntry {
            heading: Some("Steps".to_string()),
            ..Default::default()
        };
        let catalogue = SectionCatalogue::from_entries(vec![("flow".to_string(), entry)]);
        // derive_heading("flow") would return "Flow" — the catalogue override
        // wins.
        assert_eq!(resolve_freeform_heading(&catalogue, "flow"), "Steps");
    }

    #[test]
    fn resolve_falls_back_to_derive_when_catalogue_missing() {
        let catalogue = SectionCatalogue::from_entries(vec![]);
        assert_eq!(resolve_freeform_heading(&catalogue, "quality"), "Quality");
        assert_eq!(
            resolve_freeform_heading(&catalogue, "acceptance_criteria"),
            "Acceptance Criteria"
        );
    }

    #[test]
    fn resolve_falls_back_to_derive_when_catalogue_entry_has_no_heading() {
        // Entry exists but without an explicit heading — fall back to
        // derive_heading.
        let entry = CatalogueEntry {
            heading: None,
            ..Default::default()
        };
        let catalogue = SectionCatalogue::from_entries(vec![("quality".to_string(), entry)]);
        assert_eq!(resolve_freeform_heading(&catalogue, "quality"), "Quality");
    }
}

#[cfg(test)]
mod name_to_typetag_tests {
    //! Issue #84 chunk 6 — IR `return_type` propagation. Unit tests for the
    //! lower-time identifier→`TypeTag` mapping. Built-in names (case-insensitive
    //! ASCII, six total) lower to their corresponding TypeTag variant; everything
    //! else lowers to `DomainType(canonicalize_identifier(name))` per
    //! `design/values-and-names.md` §Case Normalization (D6).
    //!
    //! Per planner h.2: banned-generic identifiers (e.g. `List`, `Dict`) are NOT
    //! special-cased here. They lower to `DomainType("list")` etc.; the
    //! `G::analyze::generic-type-name` warning surfaces them upstream. This
    //! module records authorial intent in canonical form; analyze owns warnings.
    use super::*;

    // f.4: built-in `String` is case-insensitive ASCII per §Case Normalization
    // and the existing `eq_ignore_ascii_case` convention used by the chunk-2
    // banned-generic check.
    #[test]
    fn name_to_typetag_string_is_case_insensitive() {
        assert_eq!(name_to_typetag("String"), TypeTag::String);
        assert_eq!(name_to_typetag("string"), TypeTag::String);
        assert_eq!(name_to_typetag("STRING"), TypeTag::String);
        assert_eq!(name_to_typetag("StRiNg"), TypeTag::String);
    }

    // f.5: domain-type names canonicalize per D6 — ASCII-lowercase + strip `_`.
    // Cross-spelling identifiers (CamelCase + snake_case) must land on the
    // same canonical form so chunk-1's registry treats them as the same name.
    #[test]
    fn name_to_typetag_domain_canonicalizes_per_d6() {
        // CamelCase, snake_case, SHOUTY_SNAKE — all `repocontext`.
        for variant in ["RepoContext", "repo_context", "REPO_CONTEXT", "repocontext"] {
            assert_eq!(
                name_to_typetag(variant),
                TypeTag::DomainType("repocontext".into()),
                "variant `{}` should canonicalize to `repocontext`",
                variant
            );
        }
    }

    // f.6: `Agent` is a built-in `TypeTag` variant (legitimate IR-internal
    // type for stdlib `subagent()` per #83 AC3). It must lower to
    // `TypeTag::Agent`, *not* `DomainType("agent")`. Case-insensitive ASCII
    // matches the convention for the other built-ins.
    #[test]
    fn name_to_typetag_agent_is_builtin_not_domain() {
        assert_eq!(name_to_typetag("Agent"), TypeTag::Agent);
        assert_eq!(name_to_typetag("agent"), TypeTag::Agent);
        assert_eq!(name_to_typetag("AGENT"), TypeTag::Agent);
    }

    // Codex pass 3 — F1 [P2] (lower side). Built-in classification per
    // `values-and-names.md` §Case Normalization (D6) treats underscores as
    // insignificant alongside ASCII case. `name_to_typetag` formerly used
    // `eq_ignore_ascii_case` only, so an underscore-perturbed spelling like
    // `A_g_e_n_t` (canonicalizes to `agent`) misclassified as
    // `DomainType("agent")` instead of `TypeTag::Agent`. Wrong IR JSON, and
    // a regression off the spec because D6 is the canonical-name rule the
    // module doc-comment already cites.
    //
    // Post-fix: canonicalize the input first, then compare against the
    // canonical built-in set (`agent`, `string`, etc.).
    #[test]
    fn name_to_typetag_strips_underscores_per_d6_for_agent() {
        for variant in ["A_g_e_n_t", "_agent_", "AGENT__", "ag_ent"] {
            assert_eq!(
                name_to_typetag(variant),
                TypeTag::Agent,
                "underscore-perturbed `{}` must classify as built-in TypeTag::Agent",
                variant
            );
        }
    }

    // Codex pass 3 — F1 [P2] generic application. Underscore-strip is
    // per-D6 not Agent-specific; pin a second built-in (`String`) so a fix
    // that narrowly special-cases Agent still trips a test.
    #[test]
    fn name_to_typetag_strips_underscores_per_d6_for_string() {
        for variant in ["S_t_r_i_n_g", "_String_", "STR_ING"] {
            assert_eq!(
                name_to_typetag(variant),
                TypeTag::String,
                "underscore-perturbed `{}` must classify as built-in TypeTag::String",
                variant
            );
        }
    }
}

#[cfg(test)]
mod return_type_lower_tests {
    //! Issue #84 chunk 6 — `IrSkill`/`IrBlock`/`IrCall.return_type` propagation.
    //! Parse a small source, lower it, then assert the new `return_type`
    //! fields land in the arena. Cross-spelling tests use matching declarer/
    //! caller spellings: per planner's micro-flag, `lower::blocks` (L249-254)
    //! is keyed by raw block name, so cross-spelling at the *Call* level is
    //! out-of-scope for this chunk (covered at *analyze* time in chunk 4 via
    //! `G::analyze::nominal-mismatch`).
    use super::*;
    use crate::ir::IrNode;
    use crate::parse;

    fn parse_file(src: &str) -> SourceFile {
        let (file, _) = parse::parse(src, 0).expect("source should parse");
        file
    }

    fn lower_skill(src: &str) -> IrArena {
        let file = parse_file(src);
        lower(&file).expect("source should lower")
    }

    fn root_skill(arena: &IrArena) -> &IrSkill {
        let root = arena.root_skill().expect("arena should have a root skill");
        match arena.get(root) {
            IrNode::Skill(s) => s,
            other => panic!("root_skill node was not Skill: {:?}", other),
        }
    }

    // f.7a: a skill declared with `-> Report` must surface
    // `IrSkill.return_type == Some(TypeTag::DomainType("report"))` after
    // lowering. Pins both the IR field exists AND the lower-time wiring
    // through `name_to_typetag` (`Report` is not built-in → DomainType
    // canonical form).
    #[test]
    fn skill_return_type_lowers_to_domain_type_some() {
        let src = "\
skill make_report() -> Report
    flow:
        \"do work\"
";
        let arena = lower_skill(src);
        let skill = root_skill(&arena);
        assert_eq!(
            skill.return_type,
            Some(TypeTag::DomainType("report".into()))
        );
    }

    fn find_block<'a>(arena: &'a IrArena, name: &str) -> &'a crate::ir::IrBlock {
        arena
            .nodes()
            .iter()
            .find_map(|n| match n {
                IrNode::Block(b) if b.name == name => Some(b),
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected IrBlock named `{}` in arena", name))
    }

    fn find_call<'a>(arena: &'a IrArena, target: &str) -> &'a IrCall {
        arena
            .nodes()
            .iter()
            .find_map(|n| match n {
                IrNode::Call(c) if c.target == target => Some(c),
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected IrCall to `{}` in arena", target))
    }

    // f.8: a private block declared with `-> Plan` must surface
    // `IrBlock.return_type == Some(DomainType("plan"))` after lowering. Per
    // planner h.1 decision, Block return_type is stored on IR but NOT emitted
    // as a top-level Block JSON kind (the JSON-visible promise rides via the
    // caller's `IrCall.return_type` lookup; cycle 9 covers that).
    #[test]
    fn block_return_type_lowers_to_domain_type_some() {
        let src = "\
skill drive()
    flow:
        \"go\"

block plan_work() -> Plan
    flow:
        \"compute the plan\"
";
        let arena = lower_skill(src);
        let block = find_block(&arena, "plan_work");
        assert_eq!(block.return_type, Some(TypeTag::DomainType("plan".into())));
    }

    // f.9a: same-file `Call.return_type` propagation — when a skill calls a
    // block whose header declares `-> Plan`, the `IrCall` node for that call
    // site must carry `Some(DomainType("plan"))`. Per planner's micro-flag,
    // use matching spellings at declarer + caller so `lower::blocks` (raw-key
    // BTreeMap) hits.
    #[test]
    fn call_return_type_lowers_from_same_file_block_decl() {
        let src = "\
skill drive()
    flow:
        make_plan()

block make_plan() -> Plan
    flow:
        \"compute the plan\"
";
        let arena = lower_skill(src);
        let call = find_call(&arena, "make_plan");
        assert_eq!(call.return_type, Some(TypeTag::DomainType("plan".into())));
    }

    // f.9b: the Call node's IR-JSON `return_type` slot must round-trip the
    // populated `Some(DomainType("plan"))` to `{"domain_type": "plan"}`.
    #[test]
    fn call_return_type_round_trips_through_ir_json() {
        let src = "\
skill drive()
    flow:
        make_plan()

block make_plan() -> Plan
    flow:
        \"compute the plan\"
";
        let arena = lower_skill(src);
        let json_str = crate::emit_ir::serialize_ir_json(&arena, "drive.glyph", false)
            .expect("arena should serialize");
        let v: serde_json::Value = serde_json::from_str(&json_str).expect("parse");
        // Find the call inside skill.flow[*] with target == "make_plan".
        let flow = v["skill"]["flow"]
            .as_array()
            .expect("skill.flow should be array");
        let call_json = flow
            .iter()
            .find(|n| n["kind"] == "call" && n["target"] == "make_plan")
            .unwrap_or_else(|| {
                panic!(
                    "expected a call to make_plan in skill.flow; got {}",
                    json_str
                )
            });
        assert_eq!(
            call_json["return_type"],
            serde_json::json!({ "domain_type": "plan" }),
            "call.return_type slot wrong; full JSON:\n{}",
            json_str
        );
    }

    // f.7b: round-trip through `serialize_ir_json` — the IR-JSON output for a
    // skill declared with `-> Report` must carry
    // `"return_type": {"domain_type": "report"}` on the skill object. Pins the
    // emit-side wiring (chunk-6 (d) plan).
    #[test]
    fn skill_return_type_round_trips_through_ir_json() {
        let src = "\
skill make_report() -> Report
    flow:
        \"do work\"
";
        let arena = lower_skill(src);
        let json_str = crate::emit_ir::serialize_ir_json(&arena, "make_report.glyph", false)
            .expect("arena with root skill should serialize");
        let v: serde_json::Value =
            serde_json::from_str(&json_str).expect("emitter output should parse as JSON");
        assert_eq!(
            v["skill"]["return_type"],
            serde_json::json!({ "domain_type": "report" }),
            "skill.return_type slot missing or wrong shape; full JSON:\n{}",
            json_str
        );
    }

    // f.10: built-in surfacing — a skill declared with `-> String` (a
    // banned-generic per #83 AC3, but ALSO the canonical name for the
    // built-in `TypeTag::String` variant) lowers to `Some(TypeTag::String)`
    // and serializes as the lowercase JSON string `"string"`. Confirms the
    // built-in arm of `name_to_typetag` flows end-to-end through to
    // `typetag_to_json`'s lowercase-string output.
    //
    // Note: the analyze layer fires `G::analyze::generic-type-name` for this
    // source independently — that's tested elsewhere; this test pins the
    // *lower → emit* path under the assumption that the warning has fired
    // upstream and the user shipped anyway.
    #[test]
    fn skill_return_type_string_round_trips_as_lowercase_string() {
        let src = "\
skill greet() -> String
    flow:
        \"hi\"
";
        let arena = lower_skill(src);
        let skill = root_skill(&arena);
        assert_eq!(skill.return_type, Some(TypeTag::String));

        let json_str = crate::emit_ir::serialize_ir_json(&arena, "greet.glyph", false)
            .expect("arena should serialize");
        let v: serde_json::Value = serde_json::from_str(&json_str).expect("parse");
        assert_eq!(v["skill"]["return_type"], serde_json::json!("string"));
    }

    // N1: a skill with no `-> DomainType` annotation lowers with
    // `return_type == None`, and the IR-JSON Skill object surfaces the slot
    // as JSON `null` (matches the slot's pre-chunk-6 default).
    #[test]
    fn skill_without_annotation_lowers_to_none_and_emits_null() {
        let src = "\
skill drive()
    flow:
        \"go\"
";
        let arena = lower_skill(src);
        assert!(root_skill(&arena).return_type.is_none());

        let json_str = crate::emit_ir::serialize_ir_json(&arena, "drive.glyph", false)
            .expect("arena should serialize");
        let v: serde_json::Value = serde_json::from_str(&json_str).expect("parse");
        assert_eq!(v["skill"]["return_type"], serde_json::Value::Null);
    }

    // N2: a call to a target that isn't a same-file block (e.g. stdlib or
    // unresolved call) lowers `IrCall.return_type` to `None` and emits JSON
    // null. Lower doesn't reject unresolved targets — analyze handles those —
    // so we use an unresolved name to exercise the "no entry in `blocks`"
    // path of the chunk-6 lookup.
    #[test]
    fn call_to_non_block_target_lowers_to_none_and_emits_null() {
        let src = "\
skill drive()
    flow:
        unresolved_target()
";
        let arena = lower_skill(src);
        let call = find_call(&arena, "unresolved_target");
        assert!(call.return_type.is_none());

        let json_str = crate::emit_ir::serialize_ir_json(&arena, "drive.glyph", false)
            .expect("arena should serialize");
        let v: serde_json::Value = serde_json::from_str(&json_str).expect("parse");
        let flow = v["skill"]["flow"].as_array().expect("flow array");
        let call_json = flow
            .iter()
            .find(|n| n["target"] == "unresolved_target")
            .expect("call node present");
        assert_eq!(call_json["return_type"], serde_json::Value::Null);
    }

    // N3: cross-file call regression pin for D17. A skill that imports a
    // typed export-block and calls it must today lower the `IrCall` with
    // `return_type == None`. `lower::blocks` (L249-254) collects only local
    // `Decl::Block`; `Decl::Import` is not consulted, and there is no
    // imported-block-types map threaded through `lower_with_imports`. When
    // D17 lands, this test will need an updated expectation — that's the
    // point of pinning the *current* behavior.
    #[test]
    fn cross_file_call_lowers_to_none_today_d17_regression_pin() {
        let src = "\
import \"./lib.glyph\" { do_thing }

skill main()
    flow:
        do_thing()
";
        let arena = lower_skill(src);
        let call = find_call(&arena, "do_thing");
        assert!(
            call.return_type.is_none(),
            "cross-file call resolution is deferred to D17; today the callee \
             return_type must be None at the lower layer"
        );

        let json_str = crate::emit_ir::serialize_ir_json(&arena, "main.glyph", false)
            .expect("arena should serialize");
        let v: serde_json::Value = serde_json::from_str(&json_str).expect("parse");
        let flow = v["skill"]["flow"].as_array().expect("flow array");
        let call_json = flow
            .iter()
            .find(|n| n["target"] == "do_thing")
            .expect("call node present");
        assert_eq!(call_json["return_type"], serde_json::Value::Null);
    }

    // Issue #84 codex pass 2 — F2: `return foo()` is a call edge for cycle
    // detection. `lower::FlowStmt::Call` populates `outgoing_calls`, but the
    // matching `FlowStmt::Return(ReturnExpr::Call { target, .. })` arm did
    // not — so a self-recursive `return recurse()` slipped past validate's
    // DFS cycle check (which reads only `IrBlock.outgoing_calls`). Pinning
    // through the public pipeline (parse → lower → validate) so a future
    // refactor that moves the edge-emission logic stays correct.
    #[test]
    fn return_call_in_block_flow_emits_outgoing_call_edge_for_cycle_check() {
        let src = "\
skill drive()
    description: \"drive.\"
    flow:
        recurse()

block recurse() -> Plan
    description: \"recurse.\"
    flow:
        return recurse()
";
        let arena = lower_skill(src);
        let block = find_block(&arena, "recurse");
        assert_eq!(
            block.outgoing_calls,
            vec!["recurse".to_string()],
            "`return recurse()` must register as an outgoing call edge so \
             validate's DFS cycle check sees the self-loop; got: {:?}",
            block.outgoing_calls
        );

        // Behavior pin via the public validate surface: the cycle DFS must
        // now reject the self-recursion it formerly missed.
        let err = crate::validate::validate(&arena).unwrap_err();
        assert!(
            matches!(&err, crate::validate::ValidateError::RecursiveCall(name) if name == "recurse"),
            "expected RecursiveCall(\"recurse\"); got: {:?}",
            err
        );
    }

    // Issue #84 codex pass 2 — F2 transitive coverage. `block a -> return b()`,
    // `block b -> return a()` exercises a path the direct-self-recursion
    // case does not: cycle detection must see edges contributed by *both*
    // blocks' Return(Call) statements. Pre-fix, both `outgoing_calls` lists
    // were empty, the DFS adjacency was empty, and validate returned Ok.
    #[test]
    fn return_call_transitive_cycle_is_rejected_by_validate() {
        let src = "\
skill drive()
    description: \"drive.\"
    flow:
        a()

block a() -> Plan
    description: \"a.\"
    flow:
        return b()

block b() -> Plan
    description: \"b.\"
    flow:
        return a()
";
        let arena = lower_skill(src);
        let a_block = find_block(&arena, "a");
        let b_block = find_block(&arena, "b");
        assert_eq!(a_block.outgoing_calls, vec!["b".to_string()]);
        assert_eq!(b_block.outgoing_calls, vec!["a".to_string()]);

        let err = crate::validate::validate(&arena).unwrap_err();
        match &err {
            crate::validate::ValidateError::RecursiveCall(name) => {
                assert!(
                    name == "a" || name == "b",
                    "expected cycle participant `a` or `b`, got: {}",
                    name
                );
            }
            other => panic!("expected RecursiveCall, got: {:?}", other),
        }
    }

    // N4: a banned-generic identifier in type position (e.g. `List`, `Dict`)
    // is NOT special-cased by lower (per planner h.2 decision). It lowers to
    // `DomainType(canonicalize_identifier("List")) = DomainType("list")`.
    // The `G::analyze::generic-type-name` warning surfaces it upstream; lower
    // stays decoupled from analyze's decisions and records authorial intent
    // in canonical form.
    #[test]
    fn banned_generic_in_type_position_lowers_to_canonical_domain_type() {
        // `List` is on the chunk-2 banned list; it is NOT a `TypeTag` built-in
        // variant, so the fallback arm of `name_to_typetag` lowers it to
        // `DomainType("list")`.
        let src = "\
skill drive() -> List
    flow:
        \"go\"
";
        let arena = lower_skill(src);
        let skill = root_skill(&arena);
        assert_eq!(
            skill.return_type,
            Some(TypeTag::DomainType("list".into())),
            "banned-generic `List` must lower to DomainType(\"list\"); analyze's \
             generic-type-name warning is the user-facing channel, not lower."
        );

        let json_str = crate::emit_ir::serialize_ir_json(&arena, "drive.glyph", false)
            .expect("arena should serialize");
        let v: serde_json::Value = serde_json::from_str(&json_str).expect("parse");
        assert_eq!(
            v["skill"]["return_type"],
            serde_json::json!({ "domain_type": "list" })
        );
    }
}

#[cfg(test)]
mod const_lower_tests {
    //! Issue #81 chunk 2 — verifies the primitive-kind inferer is invoked on
    //! every `Decl::Const` during lower, and that the const map produced by
    //! `collect_consts` carries the rendered text + correct TypeTag.
    //!
    //! Reference-resolution test confirms a `const x = "hello"` lowers
    //! through the `texts` map and inlines into a constraint marker — i.e.
    //! the Text-equivalent path works end-to-end at indent-1 reference sites.
    //!
    //! Per planner Option-C resolution, kind doesn't change observable IR
    //! output for #81 chunk 2; so these tests assert the inferer's TypeTag
    //! via `collect_consts` rather than by round-tripping through IR.

    use super::*;
    use crate::parse;

    fn parse_file(src: &str) -> SourceFile {
        let (file, _) = parse::parse(src, 0).expect("source should parse");
        file
    }

    #[test]
    fn collect_consts_runs_inferer_for_string_const() {
        let file = parse_file("const greeting = \"hello\"\n");
        let consts = collect_consts(&file);
        let (rendered, tag) = consts.get("greeting").expect("entry present");
        assert_eq!(rendered, "hello");
        assert_eq!(*tag, TypeTag::String);
    }

    #[test]
    fn collect_consts_runs_inferer_for_int_const() {
        let file = parse_file("const max = 3\n");
        let consts = collect_consts(&file);
        let (rendered, tag) = consts.get("max").expect("entry present");
        assert_eq!(rendered, "3");
        assert_eq!(*tag, TypeTag::Int);
    }

    #[test]
    fn collect_consts_runs_inferer_for_float_const() {
        let file = parse_file("const ratio = 3.14\n");
        let consts = collect_consts(&file);
        let (_, tag) = consts.get("ratio").expect("entry present");
        assert_eq!(*tag, TypeTag::Float);
    }

    #[test]
    fn collect_consts_runs_inferer_for_bool_const() {
        let file = parse_file("const flag = true\n");
        let consts = collect_consts(&file);
        let (_, tag) = consts.get("flag").expect("entry present");
        assert_eq!(*tag, TypeTag::Bool);
    }

    #[test]
    fn const_string_resolves_via_texts_map_into_skill_constraint() {
        // Verify reference-site resolution: a `const x = "hello"` referenced
        // from a skill's body-level `require` marker should inline as the
        // `"hello"` text in the resulting Constraint IR node — same path
        // that text decls take.
        let src = "\
const policy_text = \"be careful\"
skill demo()
    require policy_text
    flow:
        \"do work\"
";
        let file = parse_file(src);
        let arena = lower(&file).expect("should lower");
        // Find the Constraint node and confirm its text is the inlined const.
        let mut found = false;
        for n in arena.nodes() {
            if let crate::ir::IrNode::Constraint(c) = n {
                if c.text == "be careful" {
                    found = true;
                    break;
                }
            }
        }
        assert!(found, "expected Constraint node with const-resolved text");
    }

    #[test]
    fn export_const_appears_in_consts_map_and_is_marked_exported() {
        // Verify export classification at the AST level. `extract_exports`
        // (lib.rs) classifies exported consts into the `texts` namespace;
        // here we just confirm the AST flag is set so analyze.rs picks it up.
        let file = parse_file("export const greet = \"hi\"\n");
        let consts = collect_consts(&file);
        assert!(consts.contains_key("greet"));
        // Walk decls to assert the AST flag.
        match &file.decls[0] {
            Decl::Const(c) => assert!(c.node.exported, "should be exported"),
            other => panic!("expected Decl::Const, got {:?}", other),
        }
    }

    #[test]
    fn local_const_overwrites_imported_text_value() {
        // Regression: chunk 2 introduced `texts.entry().or_insert_with(...)`
        // for the local-const merge, which silently let an imported const
        // shadow a same-named local one. Prior `text` semantics (baseline
        // `8a7d8dd`) used `texts.insert(...)` — local OVERWRITES imported.
        // This test pins that restored behavior.
        let src = "\
const greeting = \"local\"
skill demo()
    require greeting
    flow:
        \"do work\"
";
        let file = parse_file(src);
        let mut imported = BTreeMap::new();
        imported.insert("greeting".to_string(), "imported".to_string());
        let arena = lower_with_imports(&file, &imported, &BTreeMap::new(), &BTreeMap::new())
            .expect("should lower");
        let mut found_local = false;
        let mut found_imported = false;
        for n in arena.nodes() {
            if let crate::ir::IrNode::Constraint(c) = n {
                if c.text == "local" {
                    found_local = true;
                }
                if c.text == "imported" {
                    found_imported = true;
                }
            }
        }
        assert!(found_local, "expected Constraint with local const value");
        assert!(
            !found_imported,
            "imported value should not shadow local const"
        );
    }

    #[test]
    fn generated_const_parses_lowers_and_runs_inferer() {
        let src = "\
generated const auto_summary = \"auto-generated\"
skill demo()
    flow:
        \"do work\"
";
        let file = parse_file(src);
        // collect_consts runs the inferer on every const, including generated ones.
        let consts = collect_consts(&file);
        let (rendered, tag) = consts.get("auto_summary").expect("entry present");
        assert_eq!(rendered, "auto-generated");
        assert_eq!(*tag, TypeTag::String);
        // Lower must succeed: a generated const is lower-equivalent to a
        // private text decl as far as #81 chunk 2 is concerned.
        let _arena = lower(&file).expect("should lower with a generated const");
    }

    // Helper: pull `IrParam` slice off the first `IrSkill` in the arena.
    fn skill_params(arena: &IrArena) -> &[IrParam] {
        let root = arena.root_skill().expect("arena should have a root skill");
        match arena.get(root) {
            IrNode::Skill(s) => &s.params,
            other => panic!("root_skill node was not Skill: {:?}", other),
        }
    }

    #[test]
    fn name_ref_default_resolves_same_file_string_const_with_quotes() {
        // `risk = default_risk` is a name_ref default; lower must substitute
        // the const's rendered value, wrapping it in quotes to match the
        // literal-string default storage shape (`"\"low\""`).
        let src = "\
const default_risk = \"low\"
skill demo(risk = default_risk)
    flow:
        \"do work\"
";
        let arena = lower(&parse_file(src)).expect("should lower");
        let params = skill_params(&arena);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "risk");
        assert_eq!(params[0].default.as_deref(), Some("\"low\""));
    }

    #[test]
    fn name_ref_default_resolves_same_file_int_const_without_quotes() {
        // Numeric consts pass through verbatim (no quote-wrapping).
        let src = "\
const max_size = 5
skill demo(size = max_size)
    flow:
        \"do work\"
";
        let arena = lower(&parse_file(src)).expect("should lower");
        let params = skill_params(&arena);
        assert_eq!(params[0].default.as_deref(), Some("5"));
    }

    #[test]
    fn name_ref_default_resolves_same_file_float_const_without_quotes() {
        let src = "\
const ratio = 3.14
skill demo(r = ratio)
    flow:
        \"do work\"
";
        let arena = lower(&parse_file(src)).expect("should lower");
        let params = skill_params(&arena);
        assert_eq!(params[0].default.as_deref(), Some("3.14"));
    }

    #[test]
    fn name_ref_default_resolves_same_file_bool_const_without_quotes() {
        let src = "\
const default_flag = true
skill demo(flag = default_flag)
    flow:
        \"do work\"
";
        let arena = lower(&parse_file(src)).expect("should lower");
        let params = skill_params(&arena);
        assert_eq!(params[0].default.as_deref(), Some("true"));
    }

    #[test]
    fn bool_literal_default_is_not_resolved_via_texts_map() {
        // Regression guard: the parser sets `default_is_name_ref=false` for
        // bool/`none` literals so the resolver must NOT look them up in
        // `imported_texts` / `consts`. We seed `imported_texts` with a bogus
        // `"true"` key — if the resolver skips the name_ref guard, that
        // bogus value would leak into the IR.
        let src = "\
skill demo(flag = true)
    flow:
        \"do work\"
";
        let file = parse_file(src);
        let mut imported = BTreeMap::new();
        imported.insert("true".to_string(), "SHOULD_NOT_APPEAR".to_string());
        let arena = lower_with_imports(&file, &imported, &BTreeMap::new(), &BTreeMap::new())
            .expect("should lower");
        let params = skill_params(&arena);
        assert_eq!(params[0].default.as_deref(), Some("true"));
    }

    #[test]
    fn none_literal_default_is_not_resolved_via_texts_map() {
        let src = "\
skill demo(x = none)
    flow:
        \"do work\"
";
        let file = parse_file(src);
        let mut imported = BTreeMap::new();
        imported.insert("none".to_string(), "SHOULD_NOT_APPEAR".to_string());
        let arena = lower_with_imports(&file, &imported, &BTreeMap::new(), &BTreeMap::new())
            .expect("should lower");
        let params = skill_params(&arena);
        assert_eq!(params[0].default.as_deref(), Some("none"));
    }

    #[test]
    fn name_ref_default_resolves_imported_string_const_with_quotes() {
        // Imported const with a known `TypeTag::String` must be wrapped in
        // surrounding quotes when substituted into a name_ref default.
        let src = "\
skill demo(g = greeting)
    flow:
        \"do work\"
";
        let file = parse_file(src);
        let mut imported_texts = BTreeMap::new();
        imported_texts.insert("greeting".to_string(), "hi".to_string());
        let mut imported_const_types = BTreeMap::new();
        imported_const_types.insert("greeting".to_string(), TypeTag::String);
        let arena = lower_with_imports(
            &file,
            &imported_texts,
            &imported_const_types,
            &BTreeMap::new(),
        )
        .expect("should lower");
        let params = skill_params(&arena);
        assert_eq!(params[0].default.as_deref(), Some("\"hi\""));
    }

    #[test]
    fn name_ref_default_resolves_imported_int_const_without_quotes() {
        let src = "\
skill demo(n = max_n)
    flow:
        \"do work\"
";
        let file = parse_file(src);
        let mut imported_texts = BTreeMap::new();
        imported_texts.insert("max_n".to_string(), "42".to_string());
        let mut imported_const_types = BTreeMap::new();
        imported_const_types.insert("max_n".to_string(), TypeTag::Int);
        let arena = lower_with_imports(
            &file,
            &imported_texts,
            &imported_const_types,
            &BTreeMap::new(),
        )
        .expect("should lower");
        let params = skill_params(&arena);
        assert_eq!(params[0].default.as_deref(), Some("42"));
    }

    #[test]
    fn literal_string_default_passes_through_unchanged() {
        // Sanity: literal-string defaults are pre-rendered with quotes by the
        // parser and must reach IR unchanged (the resolver's `default_is_name_ref`
        // guard short-circuits before any texts-map lookup).
        let src = "\
skill demo(risk = \"low\")
    flow:
        \"do work\"
";
        let arena = lower(&parse_file(src)).expect("should lower");
        let params = skill_params(&arena);
        assert_eq!(params[0].default.as_deref(), Some("\"low\""));
    }
}

#[cfg(test)]
mod output_contract_lower_tests {
    //! Issue #85 chunk 4 — `OutputContract` IR node lowering.
    //!
    //! `ReturnExpr::OutputTarget(Identifier { name, .. })` lowers to a new
    //! `IrOutputContract` node carrying:
    //! - `target_name`: the inner identifier
    //! - `ty`: the enclosing decl's `-> DomainType` (`name_to_typetag`)
    //! - `source`: `OutputSource::SynthesizedByAgent` (only variant for now)
    //!
    //! The enclosing skill's `IrSkill.output_contract` slot points at the
    //! pushed node. Block-level coverage rides on `IrBlock.output_contract`.
    //! IR-JSON serialization is chunk 5; expand-step rewriting is chunk 6;
    //! missing-annotation diagnostics are chunks 8/9.
    use super::*;
    use crate::ir::{IrNode, IrOutputContract, OutputSource, OutputTargetForm};
    use crate::parse;

    fn lower_src(src: &str) -> IrArena {
        let (file, _) = parse::parse(src, 0).expect("source should parse");
        lower(&file).expect("source should lower")
    }

    fn first_output_contract(arena: &IrArena) -> &IrOutputContract {
        arena
            .nodes()
            .iter()
            .find_map(|n| match n {
                IrNode::OutputContract(oc) => Some(oc),
                _ => None,
            })
            .expect("expected an OutputContract node in the arena")
    }

    #[test]
    fn skill_return_output_target_lowers_to_output_contract_tracer() {
        let src = "\
skill make_report() -> Report
    flow:
        return <output>
";
        let arena = lower_src(src);
        let oc = first_output_contract(&arena);
        assert_eq!(oc.form, OutputTargetForm::Identifier("output".into()));
        assert_eq!(oc.ty, Some(TypeTag::DomainType("report".into())));
        assert_eq!(oc.source, OutputSource::SynthesizedByAgent);
    }

    #[test]
    fn skill_output_contract_is_referenced_from_skill_node() {
        // The IrSkill must hold the OC's id in its `output_contract` slot
        // so callers (chunk 5 emit-IR, chunk 6 expand) can find it without
        // walking the whole arena.
        let src = "\
skill make_report() -> Report
    flow:
        return <output>
";
        let arena = lower_src(src);
        let oc = first_output_contract(&arena);
        let root = arena.root_skill().expect("arena has a root skill");
        let skill = match arena.get(root) {
            IrNode::Skill(s) => s,
            other => panic!("root_skill node was not Skill: {:?}", other),
        };
        assert_eq!(skill.output_contract, Some(oc.node_id));
    }

    #[test]
    fn block_return_output_target_lowers_with_block_return_type() {
        // A private block declared `-> Path` whose flow ends with
        // `return <out>` must surface an OutputContract whose `ty` carries
        // the block's lowered annotation, and the IrBlock node must point
        // at it via `output_contract`.
        let src = "\
skill drive()
    flow:
        \"go\"

block helper() -> Path
    flow:
        return <out>
";
        let arena = lower_src(src);
        let oc = first_output_contract(&arena);
        assert_eq!(oc.form, OutputTargetForm::Identifier("out".into()));
        assert_eq!(oc.ty, Some(TypeTag::DomainType("path".into())));
        let block = arena
            .nodes()
            .iter()
            .find_map(|n| match n {
                IrNode::Block(b) if b.name == "helper" => Some(b),
                _ => None,
            })
            .expect("expected a block named helper");
        assert_eq!(block.output_contract, Some(oc.node_id));
    }

    #[test]
    fn skill_without_output_target_has_no_output_contract() {
        // Negative: a skill whose flow has no `return <IDENT>` must lower
        // with no OutputContract node and `output_contract: None`.
        let src = "\
skill drive()
    flow:
        \"go\"
";
        let arena = lower_src(src);
        let any_oc = arena
            .nodes()
            .iter()
            .any(|n| matches!(n, IrNode::OutputContract(_)));
        assert!(
            !any_oc,
            "expected no OutputContract node; got arena with one"
        );
        let root = arena.root_skill().expect("arena has a root skill");
        let skill = match arena.get(root) {
            IrNode::Skill(s) => s,
            other => panic!("root_skill node was not Skill: {:?}", other),
        };
        assert!(skill.output_contract.is_none());
    }

    #[test]
    fn output_contract_with_missing_annotation_lowers_with_none_ty() {
        // The missing-annotation diagnostic is chunks 8/9. Chunk 4 must NOT
        // crash and must lower `ty: None` so downstream phases can detect
        // and surface the issue. (Header has no `-> DomainType`.)
        let src = "\
skill drive()
    flow:
        return <out>
";
        let arena = lower_src(src);
        let oc = first_output_contract(&arena);
        assert_eq!(oc.form, OutputTargetForm::Identifier("out".into()));
        assert_eq!(oc.ty, None);
    }

    #[test]
    fn lower_descriptive_output_target_produces_description_form() {
        let src = "\
skill root()
    flow:
        \"go\"

block diagnose() -> Diagnosis
    flow:
        return <\"root cause and severity\">
";
        let arena = lower_src(src);
        let oc = first_output_contract(&arena);
        match &oc.form {
            OutputTargetForm::Description(d) => {
                assert_eq!(d, "root cause and severity");
            }
            other => panic!("expected Description, got {:?}", other),
        }
        assert_eq!(oc.source, OutputSource::SynthesizedByAgent);
    }
}

#[cfg(test)]
mod unmerged_extras_invariant_tests {
    //! Issue #109 chunk 3 — Lower-side defensive guard.
    //!
    //! Analyze rejects any AST whose declarations carry non-empty
    //! `extra_subsections` with `G::analyze::unmerged-duplicate-subsection`
    //! (Error). The pipeline-level `bag.has_error()` gate (lib.rs:110)
    //! prevents Lower from being called in that case. This module pins the
    //! belt-and-suspenders contract: if Lower is somehow invoked directly
    //! with such an AST (skipping Analyze), the `debug_assert!` in `lower`
    //! must trip rather than producing a silently-degraded IR.
    //!
    //! Released builds use `debug_assert!` so the cost is debug-only — the
    //! invariant is genuinely upheld by Analyze in normal flow.
    use super::*;
    use crate::ast::{Decl, DuplicateSubsection, FlowStmt, Skill, SourceFile};
    use crate::span::{Span, Spanned};

    /// Test (d): if a `Skill` AST node reaches `lower` with a non-empty
    /// `extra_subsections`, the defensive `debug_assert!` must panic. This
    /// is a `#[should_panic]` test — it would FAIL the chunk if Lower ever
    /// silently accepted unmerged extras.
    #[test]
    #[should_panic(expected = "extra_subsections")]
    fn lower_panics_on_skill_with_unmerged_extras() {
        let skill = Spanned {
            node: Skill {
                name: "the_skill".to_string(),
                params: Vec::new(),
                description: Some("present".to_string()),
                flow: vec![FlowStmt::InlineString("do work".to_string())],
                flow_present: true,
                body_constraints: Vec::new(),
                body_context: Vec::new(),
                body_bare_names: Vec::new(),
                effects: Vec::new(),
                context_section: Vec::new(),
                constraints_section: Vec::new(),
                return_type: None,
                extra_subsections: vec![DuplicateSubsection::Description("unmerged".to_string())],
                description_span: None,
                context_section_span: None,
                constraints_section_span: None,
                effects_span: None,
                flow_span: None,
                freeform_sections: Vec::new(),
            },
            span: Span::new(0, 0, 10),
        };
        let file = SourceFile {
            decls: vec![Decl::Skill(skill)],
        };
        // Should panic via debug_assert! before producing an IrArena.
        let _ = lower(&file);
    }
}

#[cfg(test)]
mod predicate_shape_lower_tests {
    //! Task 2.5 — verifies that `ConditionClassification` (written by Analyze)
    //! flows through Lower into `IrBranch.predicate_shape`.
    use super::*;
    use crate::analyze::analyze_with_diagnostics;
    use crate::diagnostic::DiagBag;
    use crate::domain_registry::Registry;
    use crate::ir::IrNode;
    use crate::parse;
    use crate::span::LineIndex;

    fn parse_analyze_lower(src: &str) -> IrArena {
        let (file, _) = parse::parse(src, 0).expect("source should parse");
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let mut registry = Registry::new();
        let analyzed =
            analyze_with_diagnostics(file, 0, "test", &line_index, &mut bag, &mut registry);
        lower(&analyzed).expect("source should lower")
    }

    fn root_skill(arena: &IrArena) -> &IrSkill {
        let root = arena.root_skill().expect("arena should have a root skill");
        match arena.get(root) {
            IrNode::Skill(s) => s,
            other => panic!("root_skill node was not Skill: {:?}", other),
        }
    }

    #[test]
    fn lower_copies_classification_into_ir_predicate_shape() {
        let src = r#"
const big = "a big change"

skill foo()
    description: "test"
    flow:
        if big:
            "stop"
"#;
        let arena = parse_analyze_lower(src);
        let skill = root_skill(&arena);
        let step_id = skill.steps[0];
        let branch = match arena.get(step_id) {
            IrNode::Branch(b) => b,
            _ => panic!("expected branch"),
        };
        assert!(branch.predicate_shape.has_predicate_token);
        assert!(!branch.predicate_shape.has_boolean_token);
        assert!(!branch.predicate_shape.has_compositional_operator);
    }
}

#[cfg(test)]
mod flow_assign_lower_tests {
    //! Phase 3 (Lower + IR + emit_ir) for flow-position assignments
    //! (`.flow-assign-spec.md` §8.1, §8.2).
    //!
    //! Verifies that:
    //! 1. Lower copies `FlowStmt::Call.bound_name` into `IrCall.bound_name`.
    //! 2. Lower wires `IrSkill.return_local_ref` and clears legacy
    //!    `return_text` when the skill returns a flow-local name (§8.2
    //!    single-source-of-truth rule).
    //! 3. `is_agent` is set on `IrCall` for agent-shape callees (`subagent`
    //!    via `crate::stdlib_sig`).
    //! 4. `emit_ir::serialize_ir_json` round-trips `bound_name`,
    //!    `local_refs`, `is_agent`, and `return_local_ref`.
    use super::*;
    use crate::analyze::analyze_with_diagnostics;
    use crate::diagnostic::DiagBag;
    use crate::domain_registry::Registry;
    use crate::emit_ir::serialize_ir_json;
    use crate::ir::{IrCall, IrNode, IrSkill};
    use crate::parse;
    use crate::span::LineIndex;

    fn lower_str(src: &str) -> IrArena {
        let (file, _) = parse::parse(src, 0).expect("source should parse");
        let line_index = LineIndex::new(src);
        let mut bag = DiagBag::new();
        let mut registry = Registry::new();
        let analyzed =
            analyze_with_diagnostics(file, 0, "test", &line_index, &mut bag, &mut registry);
        lower(&analyzed).expect("source should lower")
    }

    fn first_ir_skill(arena: &IrArena) -> &IrSkill {
        let root = arena.root_skill().expect("arena should have a root skill");
        match arena.get(root) {
            IrNode::Skill(s) => s,
            other => panic!("root_skill node was not Skill: {:?}", other),
        }
    }

    fn first_ir_call<'a>(arena: &'a IrArena, skill: &IrSkill) -> &'a IrCall {
        for id in &skill.steps {
            if let IrNode::Call(c) = arena.get(*id) {
                return c;
            }
        }
        panic!("expected an IrCall in skill.steps");
    }

    /// Source that uses a same-file block as the assignment RHS so analyze's
    /// no-value check passes; the block returns a domain type. Avoids
    /// pulling in stdlib for the bound_name plumbing tests.
    const VALUE_SRC: &str = r#"
block inspect_repo(scope) -> RepoContext
    "Inspect {scope}."
skill demo()
    flow:
        ctx = inspect_repo("scope")
        return ctx
"#;

    #[test]
    fn lower_copies_bound_name() {
        let arena = lower_str(VALUE_SRC);
        let skill = first_ir_skill(&arena);
        let call = first_ir_call(&arena, skill);
        assert_eq!(call.bound_name.as_deref(), Some("ctx"));
    }

    #[test]
    fn lower_sets_return_local_ref() {
        let arena = lower_str(VALUE_SRC);
        let skill = first_ir_skill(&arena);
        let lref = skill
            .return_local_ref
            .as_ref()
            .expect("skill should have return_local_ref");
        assert_eq!(lref.name, "ctx");
        let producer = first_ir_call(&arena, skill);
        assert_eq!(lref.node_id, producer.node_id);
        // Single-source-of-truth: legacy return_text must be cleared so
        // Expand's return-folding doesn't double-emit (Codex Round 2 High 2).
        assert!(skill.return_text.is_none());
    }

    #[test]
    fn emit_ir_serializes_bound_name_and_local_refs() {
        let arena = lower_str(VALUE_SRC);
        let json_text =
            serialize_ir_json(&arena, "test.glyph", false).expect("arena should produce IR JSON");
        let value: serde_json::Value =
            serde_json::from_str(&json_text).expect("IR JSON should parse");
        let skill = &value["skill"];
        let flow = skill["flow"]
            .as_array()
            .expect("skill.flow should be an array");
        // First flow node is the bound IrCall.
        let call = flow
            .iter()
            .find(|n| n["kind"] == "call")
            .expect("flow should contain a call node");
        assert_eq!(call["bound_name"], serde_json::json!("ctx"));
        // Phase 4 populates entries; Phase 3 emits an empty array but the
        // field MUST be present (per ir-json-schema.md `local_refs` is `yes`).
        assert!(call["local_refs"].is_array(), "local_refs must be an array");
        assert_eq!(call["is_agent"], serde_json::json!(false));
        // Skill-level return_local_ref is an object with name + node_id.
        let lref = &skill["return_local_ref"];
        assert!(lref.is_object(), "return_local_ref should be an object");
        assert_eq!(lref["name"], serde_json::json!("ctx"));
        assert!(
            lref["node_id"].is_string() || lref["node_id"].is_number(),
            "node_id should be a string or number"
        );
    }

    /// `researcher = subagent("...")`: analyze's stdlib-signature lookup
    /// reports `is_agent = true`; lower's `callee_is_agent` mirrors via
    /// `crate::stdlib_sig`. Confirms §9.1 rule 3 (stdlib_sig.is_agent path).
    #[test]
    fn lower_is_agent_for_subagent() {
        let src = r#"
import "@glyph/std" { subagent }

skill demo()
    flow:
        researcher = subagent("investigate")
        return researcher
"#;
        let arena = lower_str(src);
        let skill = first_ir_skill(&arena);
        let call = first_ir_call(&arena, skill);
        assert_eq!(call.bound_name.as_deref(), Some("researcher"));
        assert!(call.is_agent, "subagent callee should be agent-shape");
    }
}

#[cfg(test)]
mod block_body_constraints_lower_tests {
    //! Issue #167 — block-scoped body-level constraints + context lowering.
    //!
    //! Pins:
    //! 1. `BlockDecl.body_constraints` markers lower into `IrConstraint` arena
    //!    nodes (one per marker), and the resulting `NodeId`s land in
    //!    `IrBlock.constraints`.
    //! 2. The `Strength`/`Polarity` mapping mirrors `IrSkill` lowering exactly
    //!    (Require/Avoid/Must/MustAvoid → Soft/Soft/Hard/Hard × Require/Avoid).
    //! 3. `BlockDecl.body_context` entries (both `NameRef` and `InlineString`)
    //!    lower into `IrContext` arena nodes; `NodeId`s land in
    //!    `IrBlock.context`, and `IrContext.name` is set only for `NameRef`.
    //! 4. Skill-side lowering is unchanged: body-level Skill constraints
    //!    continue to land on `IrSkill.constraints`, not on any new field.
    //! 5. Block-scoped constraints are NOT hoisted up to the parent skill's
    //!    `IrSkill.constraints` — they stay on the callee `IrBlock`.
    use super::*;
    use crate::ir::{IrBlock, IrNode};
    use crate::parse;

    fn lower_src(src: &str) -> IrArena {
        let (file, _) = parse::parse(src, 0).expect("source should parse");
        lower(&file).expect("source should lower")
    }

    fn find_block<'a>(arena: &'a IrArena, name: &str) -> &'a IrBlock {
        arena
            .nodes()
            .iter()
            .find_map(|n| match n {
                IrNode::Block(b) if b.name == name => Some(b),
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected IrBlock named `{}`", name))
    }

    /// AC §IR-shape: `require X` on a private block lowers to a Soft+Require
    /// `IrConstraint` whose `NodeId` lives on `IrBlock.constraints`.
    #[test]
    fn body_require_lowers_to_soft_require_constraint_on_block() {
        let src = r#"
const accuracy = "be accurate"
skill driver()
    flow:
        helper()

block helper()
    require accuracy
    flow:
        "do work"
"#;
        let arena = lower_src(src);
        let block = find_block(&arena, "helper");
        assert_eq!(
            block.constraints.len(),
            1,
            "block.constraints should hold one entry"
        );
        let c = match arena.get(block.constraints[0]) {
            IrNode::Constraint(c) => c,
            other => panic!("expected Constraint, got {:?}", other),
        };
        assert_eq!(c.text, "be accurate");
        assert_eq!(c.strength, Strength::Soft);
        assert_eq!(c.polarity, Polarity::Require);
    }

    /// AC §IR-shape: each marker kind maps to the expected strength/polarity,
    /// matching the Skill-side table at `lower.rs:1259-1264`.
    #[test]
    fn body_marker_kinds_map_strength_polarity_like_skill() {
        let src = r#"
const accuracy = "be accurate"
const stale = "ignore stale references"
const safety = "do not break things"
const haste = "do not rush"

skill driver()
    flow:
        helper()

block helper()
    require accuracy
    avoid stale
    must safety
    must avoid haste
    flow:
        "do work"
"#;
        let arena = lower_src(src);
        let block = find_block(&arena, "helper");
        let pairs: Vec<(Strength, Polarity, String)> = block
            .constraints
            .iter()
            .map(|id| match arena.get(*id) {
                IrNode::Constraint(c) => (c.strength, c.polarity, c.text.clone()),
                other => panic!("expected Constraint, got {:?}", other),
            })
            .collect();
        assert_eq!(
            pairs,
            vec![
                (Strength::Soft, Polarity::Require, "be accurate".into()),
                (
                    Strength::Soft,
                    Polarity::Avoid,
                    "ignore stale references".into()
                ),
                (
                    Strength::Hard,
                    Polarity::Require,
                    "do not break things".into()
                ),
                (Strength::Hard, Polarity::Avoid, "do not rush".into()),
            ]
        );
    }

    /// AC §IR-shape: `context name` and `context "inline"` lower into
    /// `IrContext` nodes on `IrBlock.context`. `name` field set only for
    /// the `NameRef` variant.
    #[test]
    fn body_context_lowers_to_context_nodes_on_block() {
        let src = r#"
const project_conventions = "follow project conventions"

skill driver()
    flow:
        helper()

block helper()
    context project_conventions
    context "Always check for security vulnerabilities."
    flow:
        "do work"
"#;
        let arena = lower_src(src);
        let block = find_block(&arena, "helper");
        assert_eq!(
            block.context.len(),
            2,
            "block.context should hold two entries"
        );
        let first = match arena.get(block.context[0]) {
            IrNode::Context(c) => c,
            other => panic!("expected Context, got {:?}", other),
        };
        assert_eq!(first.text, "follow project conventions");
        assert_eq!(first.name.as_deref(), Some("project_conventions"));

        let second = match arena.get(block.context[1]) {
            IrNode::Context(c) => c,
            other => panic!("expected Context, got {:?}", other),
        };
        assert_eq!(second.text, "Always check for security vulnerabilities.");
        assert!(
            second.name.is_none(),
            "InlineString context should not carry a name"
        );
    }

    /// AC §IR-shape: a block with no body-level markers/context has empty
    /// `constraints` / `context` vectors. Pins the default initialization.
    #[test]
    fn block_without_body_markers_has_empty_constraint_and_context_vectors() {
        let src = r#"
skill driver()
    flow:
        helper()

block helper()
    flow:
        "do work"
"#;
        let arena = lower_src(src);
        let block = find_block(&arena, "helper");
        assert!(
            block.constraints.is_empty(),
            "expected empty block.constraints"
        );
        assert!(block.context.is_empty(), "expected empty block.context");
    }

    /// AC §IR-shape: block-scoped constraints must NOT be hoisted to the
    /// parent skill's `IrSkill.constraints`. The caller skill's list stays
    /// untouched by the callee's body-level markers.
    #[test]
    fn block_constraints_are_not_hoisted_to_skill() {
        let src = r#"
const accuracy = "be accurate"
skill driver()
    flow:
        helper()

block helper()
    require accuracy
    flow:
        "do work"
"#;
        let arena = lower_src(src);
        let root = arena.root_skill().expect("arena has a root skill");
        let skill = match arena.get(root) {
            IrNode::Skill(s) => s,
            other => panic!("root_skill was not Skill: {:?}", other),
        };
        assert!(
            skill.constraints.is_empty(),
            "skill.constraints should not absorb callee body-level constraints"
        );
    }

    /// AC §IR-shape: existing Skill-side lowering is unchanged. A
    /// body-level `require X` on the skill still lands on
    /// `IrSkill.constraints`, not on any block field. Regression pin.
    #[test]
    fn skill_body_constraint_lowering_is_unchanged() {
        let src = r#"
const accuracy = "be accurate"
skill driver()
    require accuracy
    flow:
        "do work"
"#;
        let arena = lower_src(src);
        let root = arena.root_skill().expect("arena has a root skill");
        let skill = match arena.get(root) {
            IrNode::Skill(s) => s,
            other => panic!("root_skill was not Skill: {:?}", other),
        };
        assert_eq!(skill.constraints.len(), 1);
        let c = match arena.get(skill.constraints[0]) {
            IrNode::Constraint(c) => c,
            other => panic!("expected Constraint, got {:?}", other),
        };
        assert_eq!(c.text, "be accurate");
        assert_eq!(c.strength, Strength::Soft);
        assert_eq!(c.polarity, Polarity::Require);
    }

    /// AC §IR-shape: also pin the parallel for context — a block carries
    /// its own context list; the skill's `context` is untouched.
    #[test]
    fn skill_body_context_lowering_is_unchanged() {
        let src = r#"
const project_conventions = "follow project conventions"
skill driver()
    context project_conventions
    flow:
        "do work"
"#;
        let arena = lower_src(src);
        let root = arena.root_skill().expect("arena has a root skill");
        let skill = match arena.get(root) {
            IrNode::Skill(s) => s,
            other => panic!("root_skill was not Skill: {:?}", other),
        };
        assert_eq!(skill.context.len(), 1);
        let ctx = match arena.get(skill.context[0]) {
            IrNode::Context(c) => c,
            other => panic!("expected Context, got {:?}", other),
        };
        assert_eq!(ctx.text, "follow project conventions");
        assert_eq!(ctx.name.as_deref(), Some("project_conventions"));
    }
}
