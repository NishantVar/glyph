//! Phase 4 (Lower) — converts the loose AST into the typed IR arena.
//!
//! Walking-skeleton scope: handles only the constructs in `update_docs.glyph.md`.
//! Per `design/build-foundation.md` §A4, IDs are allocated in pre-order source
//! traversal starting at `n0`.

use crate::ast::{BlockDecl, ConstraintMarkerKind, ContextEntry, Decl, FlowStmt, Skill, SourceFile};
use crate::ir::{
    IrArena, IrConstraint, IrContext, IrInlineInstruction, IrNode, IrParam, IrSkill, NodeId,
    Polarity, Role, Strength,
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

pub fn lower(file: &SourceFile) -> Result<IrArena, LowerError> {
    // Collect text declarations into a name → value map.
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    for d in &file.decls {
        if let Decl::Text(t) = d {
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
        effects: skill.effects.clone(),
        params,
        steps: Vec::new(),
        context: Vec::new(),
        constraints: Vec::new(),
    }));

    // Lower flow → Step nodes. Constraint/context markers at flow top-level
    // are hoisted into the declaration's constraint/context lists (Phase 4 Lower
    // per pipeline.md). BareName flow statements are skipped (they are caught
    // by Analyze as G::analyze::text-in-flow before reaching Lower).
    let mut step_ids: Vec<NodeId> = Vec::new();
    let mut flow_hoisted_constraint_ids: Vec<NodeId> = Vec::new();
    let mut flow_hoisted_context_ids: Vec<NodeId> = Vec::new();
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
            FlowStmt::Call { target, .. } => {
                // Tier 1 inline expansion: resolve callee body and inline as Step.
                if let Some(block) = blocks.get(target.as_str()) {
                    let body_text = resolve_block_body_text(block, &texts)?;
                    let next = NodeId(arena.len() as u32);
                    let id = arena.push(IrNode::InlineInstruction(IrInlineInstruction {
                        node_id: next,
                        text: body_text,
                        role: Role::Step,
                    }));
                    step_ids.push(id);
                }
                // If block not found, Analyze already flagged it.
            }
            FlowStmt::BareName(_) => {
                // BareName in flow is caught by Analyze before Lower runs.
                // If we somehow reach here, skip silently — the diagnostic
                // was already emitted.
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
        }
    }
    arena.set_root_skill(skill_id);

    Ok(arena)
}
