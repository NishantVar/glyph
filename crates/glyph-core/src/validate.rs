//! Phase 5 (Validate) — walking-skeleton trivial pass.
//!
//! Confirms the IR has a root skill and that node IDs are unique by construction.
//! Full invariant checks (recursive-call, malformed-branch, etc.) ship in later slices.

use crate::ir::{IrArena, IrNode};
use std::collections::HashSet;

#[derive(Debug)]
pub enum ValidateError {
    NoRootSkill,
    DuplicateNodeId(u32),
}

pub fn validate(arena: &IrArena) -> Result<(), ValidateError> {
    if arena.root_skill().is_none() {
        return Err(ValidateError::NoRootSkill);
    }
    let mut seen = HashSet::new();
    for n in arena.nodes() {
        let id = match n {
            IrNode::Skill(s) => s.node_id.0,
            IrNode::InlineInstruction(i) => i.node_id.0,
            IrNode::Constraint(c) => c.node_id.0,
        };
        if !seen.insert(id) {
            return Err(ValidateError::DuplicateNodeId(id));
        }
    }
    Ok(())
}
