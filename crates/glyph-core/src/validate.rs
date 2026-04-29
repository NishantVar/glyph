//! Phase 5 (Validate) — IR invariant checks.
//!
//! Checks the IR for structural correctness after lowering and before expansion.

use crate::ir::{IrArena, IrNode};
use std::collections::HashSet;

#[derive(Debug, PartialEq, Eq)]
pub enum ValidateError {
    NoRootSkill,
    DuplicateNodeId(u32),
    UnresolvedCallee(String),
    RecursiveCall(String),
    EmptyStep(u32),
}

pub fn validate(arena: &IrArena) -> Result<(), ValidateError> {
    if arena.root_skill().is_none() {
        return Err(ValidateError::NoRootSkill);
    }

    // Check duplicate node IDs.
    let mut seen = HashSet::new();
    for n in arena.nodes() {
        let id = node_id(n);
        if !seen.insert(id) {
            return Err(ValidateError::DuplicateNodeId(id));
        }
    }

    // Check for empty steps (InlineInstruction with empty text).
    for n in arena.nodes() {
        if let IrNode::InlineInstruction(i) = n {
            if i.text.trim().is_empty() {
                return Err(ValidateError::EmptyStep(i.node_id.0));
            }
        }
    }

    // Check for unresolved callees (IrCall with resolved_body == None).
    for n in arena.nodes() {
        if let IrNode::Call(c) = n {
            if c.resolved_body.is_none() {
                return Err(ValidateError::UnresolvedCallee(c.target.clone()));
            }
        }
    }

    // Check for recursive calls (direct self-recursion via outgoing_calls).
    for n in arena.nodes() {
        if let IrNode::Block(b) = n {
            if b.outgoing_calls.contains(&b.name) {
                return Err(ValidateError::RecursiveCall(b.name.clone()));
            }
        }
    }

    Ok(())
}

fn node_id(n: &IrNode) -> u32 {
    match n {
        IrNode::Skill(s) => s.node_id.0,
        IrNode::InlineInstruction(i) => i.node_id.0,
        IrNode::Constraint(c) => c.node_id.0,
        IrNode::Context(ctx) => ctx.node_id.0,
        IrNode::Block(b) => b.node_id.0,
        IrNode::Call(c) => c.node_id.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;

    fn make_skill_arena_with_step(step_text: &str) -> IrArena {
        let mut arena = IrArena::new();
        arena.push(IrNode::Skill(IrSkill {
            node_id: NodeId(0),
            name: "test".into(),
            description: "test".into(),
            effects: vec![],
            params: vec![],
            steps: vec![NodeId(1)],
            context: vec![],
            constraints: vec![],
        }));
        arena.push(IrNode::InlineInstruction(IrInlineInstruction {
            node_id: NodeId(1),
            text: step_text.into(),
            role: Role::Step,
        }));
        arena.set_root_skill(NodeId(0));
        arena
    }

    #[test]
    fn validate_duplicate_node_id() {
        let mut arena = IrArena::new();
        arena.push(IrNode::Skill(IrSkill {
            node_id: NodeId(0),
            name: "test".into(),
            description: "test".into(),
            effects: vec![],
            params: vec![],
            steps: vec![],
            context: vec![],
            constraints: vec![],
        }));
        arena.set_root_skill(NodeId(0));
        // Manually push a node with the same ID.
        arena.nodes_mut().push(IrNode::InlineInstruction(IrInlineInstruction {
            node_id: NodeId(0), // duplicate!
            text: "step".into(),
            role: Role::Step,
        }));
        let err = validate(&arena).unwrap_err();
        assert_eq!(err, ValidateError::DuplicateNodeId(0));
    }

    #[test]
    fn validate_empty_step() {
        let arena = make_skill_arena_with_step("");
        let err = validate(&arena).unwrap_err();
        assert_eq!(err, ValidateError::EmptyStep(1));
    }

    #[test]
    fn validate_whitespace_only_step_is_empty() {
        let arena = make_skill_arena_with_step("   ");
        let err = validate(&arena).unwrap_err();
        assert_eq!(err, ValidateError::EmptyStep(1));
    }

    #[test]
    fn validate_unresolved_callee() {
        let mut arena = IrArena::new();
        let skill_id = arena.push(IrNode::Skill(IrSkill {
            node_id: NodeId(0),
            name: "test".into(),
            description: "test".into(),
            effects: vec![],
            params: vec![],
            steps: vec![NodeId(1)],
            context: vec![],
            constraints: vec![],
        }));
        arena.push(IrNode::Call(IrCall {
            node_id: NodeId(1),
            target: "missing_block".into(),
            args: vec![],
            resolved_body: None, // unresolved!
        }));
        arena.set_root_skill(skill_id);
        let err = validate(&arena).unwrap_err();
        assert_eq!(err, ValidateError::UnresolvedCallee("missing_block".into()));
    }

    #[test]
    fn validate_recursive_call() {
        let mut arena = IrArena::new();
        arena.push(IrNode::Skill(IrSkill {
            node_id: NodeId(0),
            name: "test".into(),
            description: "test".into(),
            effects: vec![],
            params: vec![],
            steps: vec![NodeId(1)],
            context: vec![],
            constraints: vec![],
        }));
        arena.push(IrNode::Call(IrCall {
            node_id: NodeId(1),
            target: "foo".into(),
            args: vec![],
            resolved_body: Some("Do something.".into()),
        }));
        // Block "foo" that calls itself (direct recursion).
        arena.push(IrNode::Block(IrBlock {
            node_id: NodeId(2),
            name: "foo".into(),
            description: None,
            body_text: "Do something.".into(),
            resolved_word_count: None,
            outgoing_calls: vec!["foo".into()], // self-referencing!
        }));
        arena.set_root_skill(NodeId(0));

        let err = validate(&arena).unwrap_err();
        assert_eq!(err, ValidateError::RecursiveCall("foo".into()));
    }
}
