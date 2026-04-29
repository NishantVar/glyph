//! Phase 4 (Lower) — converts the loose AST into the typed IR arena.
//!
//! Walking-skeleton scope: handles only the constructs in `update_docs.glyph.md`.
//! Per `design/build-foundation.md` §A4, IDs are allocated in pre-order source
//! traversal starting at `n0`.

use crate::ast::{ConstraintMarkerKind, Decl, FlowStmt, Skill, SourceFile};
use crate::ir::{
    IrArena, IrConstraint, IrInlineInstruction, IrNode, IrSkill, NodeId, Polarity, Role, Strength,
};
use std::collections::BTreeMap;

#[derive(Debug)]
pub enum LowerError {
    NoSkill,
    UndefinedConstraintRef(String),
}

pub fn lower(file: &SourceFile) -> Result<IrArena, LowerError> {
    // Collect text declarations into a name → value map.
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    for d in &file.decls {
        if let Decl::Text(t) = d {
            texts.insert(t.node.name.clone(), t.node.value.clone());
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
    let skill_id = arena.push(IrNode::Skill(IrSkill {
        node_id: NodeId(0),
        name: skill.name.clone(),
        description: skill.description.clone().unwrap_or_default(),
        effects: skill.effects.clone(),
        steps: Vec::new(),
        constraints: Vec::new(),
    }));

    // Lower flow → Step nodes.
    let mut step_ids: Vec<NodeId> = Vec::new();
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

    // Patch the skill node now that step/constraint IDs are known.
    {
        let nodes = arena.nodes_mut();
        if let IrNode::Skill(s) = &mut nodes[skill_id.0 as usize] {
            s.steps = step_ids;
            s.constraints = constraint_ids;
        }
    }
    arena.set_root_skill(skill_id);

    Ok(arena)
}
