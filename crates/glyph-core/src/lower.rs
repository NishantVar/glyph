//! Phase 4 (Lower) — converts the loose AST into the typed IR arena.
//!
//! Walking-skeleton scope: handles only the constructs in `update_docs.glyph.md`.
//! Per `design/build-foundation.md` §A4, IDs are allocated in pre-order source
//! traversal starting at `n0`.

use crate::ast::{BlockDecl, ConstraintMarkerKind, ContextEntry, Decl, FlowStmt, ReturnExpr, Skill, SourceFile};
use crate::ir::{
    IrArena, IrBlock, IrBranch, IrCall, IrConstraint, IrContext, IrElifBranch,
    IrInlineInstruction, IrNode, IrParam, IrSkill, NodeId, Polarity, Role, Strength,
};
use std::collections::BTreeMap;

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
            .get(name)
            .cloned()
            .ok_or_else(|| LowerError::UndefinedContextRef(name.clone())),
    }
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
                    .get(&marker.name)
                    .cloned()
                    .ok_or_else(|| LowerError::UndefinedConstraintRef(marker.name.clone()))?;
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
                let resolved_body = if let Some(block) = blocks.get(target.as_str()) {
                    let body_text = resolve_block_body_text(block, texts)?;
                    Some(body_text)
                } else {
                    None
                };
                let next = NodeId(arena.len() as u32);
                let id = arena.push(IrNode::Call(IrCall {
                    node_id: next,
                    target: target.clone(),
                    args: args.clone(),
                    resolved_body,
                    site_modifier: site_modifier.clone(),
                    projection_tier: None,
                    procedure_path: None,
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
    // Collect const declarations into a name → value map.
    let mut texts: BTreeMap<String, String> = imported_texts.clone();
    for d in &file.decls {
        if let Decl::Const(t) = d {
            texts.insert(t.node.name.clone(), t.node.value.clone());
        }
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
    }));

    // Lower block declarations to IrBlock nodes.
    for d in &file.decls {
        if let Decl::Block(b) = d {
            let block = &b.node;
            let body_text = resolve_block_body_text(block, &texts)?;
            // Collect outgoing call targets from the block's flow.
            let outgoing_calls: Vec<String> = block
                .flow
                .iter()
                .filter_map(|stmt| match stmt {
                    FlowStmt::Call { target, .. } => Some(target.clone()),
                    _ => None,
                })
                .collect();
            // Collect individual flow statement strings for Tier 2 procedure emission.
            let flow_statements: Vec<String> = block
                .flow
                .iter()
                .filter_map(|stmt| match stmt {
                    FlowStmt::InlineString(s) => Some(s.clone()),
                    FlowStmt::Call { target, .. } => Some(format!("call {}", target)),
                    FlowStmt::Branch { condition, .. } => Some(format!("if {}", condition)),
                    FlowStmt::ConstraintMarker(m) => Some(format!("constraint {}", m.name)),
                    FlowStmt::ContextMarker(_) => Some("context".to_string()),
                    FlowStmt::Return(_) => Some("return".to_string()),
                    FlowStmt::BareName(n) => Some(n.clone()),
                })
                .collect();
            let next = NodeId(arena.len() as u32);
            arena.push(IrNode::Block(IrBlock {
                node_id: next,
                name: block.name.clone(),
                description: block.description.clone(),
                body_text,
                flow_statements,
                resolved_word_count: None,
                outgoing_calls,
            }));
        }
    }

    // Lower flow → Step nodes. Constraint/context markers at flow top-level
    // are hoisted into the declaration's constraint/context lists (Phase 4 Lower
    // per pipeline.md). BareName flow statements are skipped (they are caught
    // by Analyze as G::analyze::const-in-flow before reaching Lower).
    let mut step_ids: Vec<NodeId> = Vec::new();
    let mut flow_hoisted_constraint_ids: Vec<NodeId> = Vec::new();
    let mut flow_hoisted_context_ids: Vec<NodeId> = Vec::new();
    let mut return_text: Option<String> = None;
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
                    .get(&marker.name)
                    .cloned()
                    .ok_or_else(|| LowerError::UndefinedConstraintRef(marker.name.clone()))?;
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
                let resolved_body = if let Some(block) = blocks.get(target.as_str()) {
                    let body_text = resolve_block_body_text(block, &texts)?;
                    Some(body_text)
                } else {
                    None // Analyze already flagged undefined-call.
                };
                let next = NodeId(arena.len() as u32);
                let id = arena.push(IrNode::Call(IrCall {
                    node_id: next,
                    target: target.clone(),
                    args: args.clone(),
                    resolved_body,
                    site_modifier: site_modifier.clone(),
                    projection_tier: None,
                    procedure_path: None,
                }));
                step_ids.push(id);
            }
            FlowStmt::Return(expr) => {
                // Capture the return expression text for return folding in Expand.
                let text = match expr {
                    ReturnExpr::None => None,
                    ReturnExpr::Call { target, args } => {
                        if args.is_empty() {
                            Some(format!("{}()", target))
                        } else {
                            Some(format!("{}({})", target, args.join(", ")))
                        }
                    }
                    ReturnExpr::Name(name) => Some(name.clone()),
                    ReturnExpr::Inline(s) => Some(s.clone()),
                };
                return_text = text;
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
            .get(&marker.name)
            .cloned()
            .ok_or_else(|| LowerError::UndefinedConstraintRef(marker.name.clone()))?;
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
        }
    }
    arena.set_root_skill(skill_id);

    Ok(arena)
}
