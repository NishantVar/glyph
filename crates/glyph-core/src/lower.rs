//! Phase 4 (Lower) — converts the loose AST into the typed IR arena.
//!
//! Walking-skeleton scope: handles only the constructs in `update_docs.glyph.md`.
//! Per `design/build-foundation.md` §A4, IDs are allocated in pre-order source
//! traversal starting at `n0`.

use crate::ast::{
    BlockDecl, ConstValue, ConstraintMarkerKind, ContextEntry, Decl, FlowStmt, ReturnExpr, Skill,
    SourceFile,
};
use crate::ir::{
    IrArena, IrBlock, IrBranch, IrCall, IrConstraint, IrContext, IrElifBranch,
    IrInlineInstruction, IrNode, IrOutputContract, IrParam, IrSkill, NodeId, OutputSource,
    OutputTargetForm, Polarity, Role, Strength,
};
use crate::output_target::OutputTargetExpr;
use crate::domain_registry::canonicalize_identifier;
use crate::kind_infer::{infer_primitive, Literal as KindLiteral, TypeTag};
use std::collections::BTreeMap;

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
        match stmt {
            FlowStmt::InlineString(s) => parts.push(s.clone()),
            _ => {} // Other flow stmt types not handled for Tier 1 inline in this slice.
        }
    }
    Ok(parts.join(" "))
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

/// Lower a list of flow statements into IR nodes, returning node IDs.
/// Used for branch body lowering. Constraint/context markers inside branch
/// bodies stay inline (not hoisted) per pipeline.md §Phase 4.
fn lower_flow_body(
    stmts: &[FlowStmt],
    arena: &mut IrArena,
    texts: &BTreeMap<String, String>,
    blocks: &BTreeMap<String, &BlockDecl>,
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
                }));
                ids.push(id);
            }
            FlowStmt::Call { target, args, site_modifier } => {
                let resolved_body = if let Some(block) = blocks.get(target.node.as_str()) {
                    let body_text = resolve_block_body_text(block, texts)?;
                    Some(body_text)
                } else {
                    None
                };
                // Issue #84 chunk 6: same-file callee return-type lookup. Reads
                // `BlockDecl::return_type` from the same `blocks` map used for
                // body-text resolution; stdlib calls (no map entry) → None.
                // Cross-file resolution is deferred to D17.
                let return_type = blocks
                    .get(target.node.as_str())
                    .and_then(|b| b.return_type.as_ref())
                    .map(|s| name_to_typetag(s.node.as_str()));
                let next = NodeId(arena.len() as u32);
                let id = arena.push(IrNode::Call(IrCall {
                    node_id: next,
                    target: target.node.clone(),
                    args: args.clone(),
                    resolved_body,
                    site_modifier: site_modifier.clone(),
                    projection_tier: None,
                    procedure_path: None,
                    return_type,
                }));
                ids.push(id);
            }
            FlowStmt::Branch { condition, then_body, elif_branches, else_body } => {
                let branch_id = NodeId(arena.len() as u32);
                // Reserve a slot for the Branch node.
                arena.push(IrNode::InlineInstruction(IrInlineInstruction {
                    node_id: branch_id,
                    text: String::new(),
                    role: Role::Step,
                }));
                let then_ids = lower_flow_body(then_body, arena, texts, blocks)?;
                let mut ir_elifs = Vec::new();
                for elif in elif_branches {
                    let elif_ids = lower_flow_body(&elif.body, arena, texts, blocks)?;
                    ir_elifs.push(IrElifBranch {
                        condition: elif.condition.clone(),
                        body: elif_ids,
                    });
                }
                let ir_else = if let Some(eb) = else_body {
                    Some(lower_flow_body(eb, arena, texts, blocks)?)
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
                    applies_descriptions: None,
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
    lower_with_imports(file, &BTreeMap::new())
}

/// Lower with additional imported text values available for constraint/context resolution.
pub fn lower_with_imports(
    file: &SourceFile,
    imported_texts: &BTreeMap<String, String>,
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

    // Find the skill declaration (exactly one in walking skeleton).
    let skill: &Skill = file
        .decls
        .iter()
        .find_map(|d| match d {
            Decl::Skill(s) => Some(&s.node),
            _ => None,
        })
        .ok_or(LowerError::NoSkill)?;

    let mut arena = IrArena::new();

    // Reserve n0 for the skill (pre-order: container before children).
    let params: Vec<IrParam> = skill
        .params
        .iter()
        .map(|p| IrParam {
            name: p.name.clone(),
            default: p.default.clone(),
        })
        .collect();
    let skill_return_type: Option<TypeTag> = skill
        .return_type
        .as_ref()
        .map(|s| name_to_typetag(s.node.as_str()));
    let skill_id = arena.push(IrNode::Skill(IrSkill {
        node_id: NodeId(0),
        name: skill.name.clone(),
        description: skill.description.clone().unwrap_or_default(),
        effects: skill.effects.iter().filter(|e| e.as_str() != "none").cloned().collect(),
        params,
        steps: Vec::new(),
        context: Vec::new(),
        constraints: Vec::new(),
        return_text: None,
        return_type: skill_return_type.clone(),
        output_contract: None,
    }));

    // Lower block declarations to IrBlock nodes.
    for d in &file.decls {
        if let Decl::Block(b) = d {
            let block = &b.node;
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
            let flow_statements: Vec<String> = block
                .flow
                .iter()
                .filter_map(|stmt| match stmt {
                    FlowStmt::InlineString(s) => Some(s.clone()),
                    FlowStmt::Call { target, .. } => Some(format!("call {}", target.node)),
                    FlowStmt::Branch { condition, .. } => Some(format!("if {}", condition)),
                    FlowStmt::ConstraintMarker(m) => Some(format!("constraint {}", m.name.node)),
                    FlowStmt::ContextMarker(_) => Some("context".to_string()),
                    FlowStmt::Return(_) => Some("return".to_string()),
                    FlowStmt::BareName(n) => Some(n.node.clone()),
                })
                .collect();
            let block_return_type: Option<TypeTag> = block
                .return_type
                .as_ref()
                .map(|s| name_to_typetag(s.node.as_str()));
            // Issue #85: scan for a top-level `return <IDENT>` in this
            // block's flow. If present, push an `IrOutputContract` node now
            // (so its id < the block's id is fine — the block holds an
            // optional reference, not a strict pre-order requirement) and
            // store the id in `IrBlock.output_contract`.
            let block_output_contract: Option<NodeId> =
                lower_output_contract_for_flow(&block.flow, &mut arena, block_return_type.clone());
            let next = NodeId(arena.len() as u32);
            arena.push(IrNode::Block(IrBlock {
                node_id: next,
                name: block.name.clone(),
                description: block.description.clone(),
                body_text,
                flow_statements,
                resolved_word_count: None,
                outgoing_calls,
                return_type: block_return_type,
                output_contract: block_output_contract,
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
    for stmt in &skill.flow {
        match stmt {
            FlowStmt::InlineString(text) => {
                let next = NodeId(arena.len() as u32);
                let id = arena.push(IrNode::InlineInstruction(IrInlineInstruction {
                    node_id: next,
                    text: text.clone(),
                    role: Role::Step,
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
                let next = NodeId(arena.len() as u32);
                let id = arena.push(IrNode::Context(IrContext {
                    node_id: next,
                    text: resolved,
                }));
                flow_hoisted_context_ids.push(id);
            }
            FlowStmt::Call { target, args, site_modifier } => {
                // Create an IrCall node. Resolve callee body if block exists.
                let resolved_body = if let Some(block) = blocks.get(target.node.as_str()) {
                    let body_text = resolve_block_body_text(block, &texts)?;
                    Some(body_text)
                } else {
                    None // Analyze already flagged undefined-call.
                };
                // Issue #84 chunk 6: same-file callee return-type lookup —
                // see the matching site in `lower_flow_body` above for the
                // shared rationale.
                let return_type = blocks
                    .get(target.node.as_str())
                    .and_then(|b| b.return_type.as_ref())
                    .map(|s| name_to_typetag(s.node.as_str()));
                let next = NodeId(arena.len() as u32);
                let id = arena.push(IrNode::Call(IrCall {
                    node_id: next,
                    target: target.node.clone(),
                    args: args.clone(),
                    resolved_body,
                    site_modifier: site_modifier.clone(),
                    projection_tier: None,
                    procedure_path: None,
                    return_type,
                }));
                step_ids.push(id);
            }
            FlowStmt::Return(expr) => {
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
                return_text = text;
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
                    let next = NodeId(arena.len() as u32);
                    let oc_id = arena.push(IrNode::OutputContract(IrOutputContract {
                        node_id: next,
                        form,
                        ty: skill_return_type.clone(),
                        source: OutputSource::SynthesizedByAgent,
                    }));
                    skill_output_contract = Some(oc_id);
                }
            }
            FlowStmt::BareName(_) => {
                // BareName in flow is caught by Analyze before Lower runs.
                // If we somehow reach here, skip silently — the diagnostic
                // was already emitted.
            }
            FlowStmt::Branch { condition, then_body, elif_branches, else_body } => {
                let branch_id = NodeId(arena.len() as u32);
                // Reserve a placeholder slot.
                arena.push(IrNode::InlineInstruction(IrInlineInstruction {
                    node_id: branch_id,
                    text: String::new(),
                    role: Role::Step,
                }));
                let then_ids = lower_flow_body(then_body, &mut arena, &texts, &blocks)?;
                let mut ir_elifs = Vec::new();
                for elif in elif_branches {
                    let elif_ids = lower_flow_body(&elif.body, &mut arena, &texts, &blocks)?;
                    ir_elifs.push(IrElifBranch {
                        condition: elif.condition.clone(),
                        body: elif_ids,
                    });
                }
                let ir_else = if let Some(eb) = else_body {
                    Some(lower_flow_body(eb, &mut arena, &texts, &blocks)?)
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
                    applies_descriptions: None,
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
            let next = NodeId(arena.len() as u32);
            let id = arena.push(IrNode::Context(IrContext {
                node_id: next,
                text: resolved,
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

    // Patch the skill node now that step/context/constraint IDs are known.
    {
        let nodes = arena.nodes_mut();
        if let IrNode::Skill(s) = &mut nodes[skill_id.0 as usize] {
            s.steps = step_ids;
            s.context = context_ids;
            s.constraints = constraint_ids;
            s.return_text = return_text;
            s.output_contract = skill_output_contract;
        }
    }
    arena.set_root_skill(skill_id);

    Ok(arena)
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
        assert_eq!(
            block.return_type,
            Some(TypeTag::DomainType("plan".into()))
        );
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
        assert_eq!(
            call.return_type,
            Some(TypeTag::DomainType("plan".into()))
        );
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
        let json_str = crate::emit_ir::serialize_ir_json(&arena, "drive.glyph.md", false)
            .expect("arena should serialize");
        let v: serde_json::Value = serde_json::from_str(&json_str).expect("parse");
        // Find the call inside skill.flow[*] with target == "make_plan".
        let flow = v["skill"]["flow"]
            .as_array()
            .expect("skill.flow should be array");
        let call_json = flow
            .iter()
            .find(|n| n["kind"] == "call" && n["target"] == "make_plan")
            .unwrap_or_else(|| panic!("expected a call to make_plan in skill.flow; got {}", json_str));
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
        let json_str = crate::emit_ir::serialize_ir_json(&arena, "make_report.glyph.md", false)
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

        let json_str = crate::emit_ir::serialize_ir_json(&arena, "greet.glyph.md", false)
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

        let json_str = crate::emit_ir::serialize_ir_json(&arena, "drive.glyph.md", false)
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

        let json_str = crate::emit_ir::serialize_ir_json(&arena, "drive.glyph.md", false)
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
import \"./lib.glyph.md\" { do_thing }

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

        let json_str = crate::emit_ir::serialize_ir_json(&arena, "main.glyph.md", false)
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

        let json_str = crate::emit_ir::serialize_ir_json(&arena, "drive.glyph.md", false)
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
        let arena = lower_with_imports(&file, &imported).expect("should lower");
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
        assert!(!found_imported, "imported value should not shadow local const");
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
        assert!(!any_oc, "expected no OutputContract node; got arena with one");
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
