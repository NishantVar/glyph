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
    MalformedBranch(u32),
}

impl ValidateError {
    /// Canonical `G::validate::*` diagnostic string ID.
    pub fn diagnostic_id(&self) -> &'static str {
        match self {
            ValidateError::NoRootSkill => "G::validate::no-root-skill",
            ValidateError::DuplicateNodeId(_) => "G::validate::duplicate-node-id",
            ValidateError::UnresolvedCallee(_) => "G::validate::unresolved-callee",
            ValidateError::RecursiveCall(_) => "G::validate::recursive-call",
            ValidateError::EmptyStep(_) => "G::validate::empty-step",
            ValidateError::MalformedBranch(_) => "G::validate::malformed-branch",
        }
    }
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

    // Check for malformed branches (empty then_body or empty condition).
    for n in arena.nodes() {
        if let IrNode::Branch(br) = n {
            if br.condition.trim().is_empty() || br.then_body.is_empty() {
                return Err(ValidateError::MalformedBranch(br.node_id.0));
            }
            for elif in &br.elif_branches {
                if elif.condition.trim().is_empty() || elif.body.is_empty() {
                    return Err(ValidateError::MalformedBranch(br.node_id.0));
                }
            }
            if let Some(eb) = &br.else_body {
                if eb.is_empty() {
                    return Err(ValidateError::MalformedBranch(br.node_id.0));
                }
            }
        }
    }

    // Check for unresolved callees (IrCall with resolved_body == None).
    // Skip Tier 3 calls — they reference external procedure files and
    // intentionally have no resolved_body.
    for n in arena.nodes() {
        if let IrNode::Call(c) = n {
            if c.resolved_body.is_none() && c.projection_tier != Some(3) {
                return Err(ValidateError::UnresolvedCallee(c.target.clone()));
            }
        }
    }

    // Check for recursive calls — full cycle detection in the block call graph.
    // Build adjacency map, then DFS for cycles.
    {
        let mut adjacency: std::collections::HashMap<&str, &[String]> =
            std::collections::HashMap::new();
        for n in arena.nodes() {
            if let IrNode::Block(b) = n {
                adjacency.insert(&b.name, &b.outgoing_calls);
            }
        }
        // DFS cycle detection using coloring: White (unvisited), Gray (in stack), Black (done).
        let mut color: std::collections::HashMap<&str, u8> = std::collections::HashMap::new(); // 0=white, 1=gray, 2=black
        for &name in adjacency.keys() {
            color.insert(name, 0);
        }
        fn dfs<'a>(
            node: &'a str,
            adjacency: &std::collections::HashMap<&'a str, &'a [String]>,
            color: &mut std::collections::HashMap<&'a str, u8>,
        ) -> Option<String> {
            color.insert(node, 1); // gray
            if let Some(neighbors) = adjacency.get(node) {
                for neighbor in *neighbors {
                    match color.get(neighbor.as_str()).copied() {
                        Some(1) => return Some(neighbor.clone()), // back edge = cycle
                        Some(0) => {
                            if let Some(cyclic) = dfs(neighbor, adjacency, color) {
                                return Some(cyclic);
                            }
                        }
                        _ => {} // black or unknown (non-block target) — skip
                    }
                }
            }
            color.insert(node, 2); // black
            None
        }
        // Sort keys for deterministic error reporting.
        let mut names: Vec<&str> = adjacency.keys().copied().collect();
        names.sort();
        for name in names {
            if color.get(name).copied() == Some(0) {
                if let Some(cyclic) = dfs(name, &adjacency, &mut color) {
                    return Err(ValidateError::RecursiveCall(cyclic));
                }
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
        IrNode::Branch(br) => br.node_id.0,
        IrNode::OutputContract(oc) => oc.node_id.0,
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
            return_text: None,
            return_type: None,
            output_contract: None,
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
            return_text: None,
            return_type: None,
            output_contract: None,
        }));
        arena.set_root_skill(NodeId(0));
        // Manually push a node with the same ID.
        arena
            .nodes_mut()
            .push(IrNode::InlineInstruction(IrInlineInstruction {
                node_id: NodeId(0), // duplicate!
                text: "step".into(),
                role: Role::Step,
            }));
        let err = validate(&arena).unwrap_err();
        assert_eq!(err, ValidateError::DuplicateNodeId(0));
        assert_eq!(err.diagnostic_id(), "G::validate::duplicate-node-id");
    }

    #[test]
    fn validate_empty_step() {
        let arena = make_skill_arena_with_step("");
        let err = validate(&arena).unwrap_err();
        assert_eq!(err, ValidateError::EmptyStep(1));
        assert_eq!(err.diagnostic_id(), "G::validate::empty-step");
    }

    #[test]
    fn validate_whitespace_only_step_is_empty() {
        let arena = make_skill_arena_with_step("   ");
        let err = validate(&arena).unwrap_err();
        assert_eq!(err, ValidateError::EmptyStep(1));
        assert_eq!(err.diagnostic_id(), "G::validate::empty-step");
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
            return_text: None,
            return_type: None,
            output_contract: None,
        }));
        arena.push(IrNode::Call(IrCall {
            node_id: NodeId(1),
            target: "missing_block".into(),
            args: vec![],
            resolved_body: None, // unresolved!
            site_modifier: None,
            projection_tier: None,
            procedure_path: None,
            return_type: None,
            callee_output_contract: None,
        }));
        arena.set_root_skill(skill_id);
        let err = validate(&arena).unwrap_err();
        assert_eq!(err, ValidateError::UnresolvedCallee("missing_block".into()));
        assert_eq!(err.diagnostic_id(), "G::validate::unresolved-callee");
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
            return_text: None,
            return_type: None,
            output_contract: None,
        }));
        arena.push(IrNode::Call(IrCall {
            node_id: NodeId(1),
            target: "foo".into(),
            args: vec![],
            resolved_body: Some("Do something.".into()),
            site_modifier: None,
            projection_tier: None,
            procedure_path: None,
            return_type: None,
            callee_output_contract: None,
        }));
        // Block "foo" that calls itself (direct recursion).
        arena.push(IrNode::Block(IrBlock {
            node_id: NodeId(2),
            name: "foo".into(),
            description: None,
            body_text: "Do something.".into(),
            flow_statements: vec!["Do something.".into()],
            resolved_word_count: None,
            outgoing_calls: vec!["foo".into()], // self-referencing!
            return_type: None,
            output_contract: None,
        }));
        arena.set_root_skill(NodeId(0));

        let err = validate(&arena).unwrap_err();
        assert_eq!(err, ValidateError::RecursiveCall("foo".into()));
        assert_eq!(err.diagnostic_id(), "G::validate::recursive-call");
    }

    #[test]
    fn validate_malformed_branch_empty_then_body() {
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
            return_text: None,
            return_type: None,
            output_contract: None,
        }));
        arena.push(IrNode::Branch(crate::ir::IrBranch {
            node_id: NodeId(1),
            condition: "x == 1".into(),
            then_body: vec![], // empty — malformed!
            elif_branches: vec![],
            else_body: None,
            resolved_predicates: None,
            predicate_shape: crate::ir::BranchPredicateShape::default(),
        }));
        arena.set_root_skill(NodeId(0));
        let err = validate(&arena).unwrap_err();
        assert_eq!(err, ValidateError::MalformedBranch(1));
        assert_eq!(err.diagnostic_id(), "G::validate::malformed-branch");
    }

    #[test]
    fn validate_indirect_recursive_call() {
        // A calls B, B calls A — indirect cycle detection.
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
            return_text: None,
            return_type: None,
            output_contract: None,
        }));
        arena.push(IrNode::Call(IrCall {
            node_id: NodeId(1),
            target: "foo".into(),
            args: vec![],
            resolved_body: Some("Do something.".into()),
            site_modifier: None,
            projection_tier: None,
            procedure_path: None,
            return_type: None,
            callee_output_contract: None,
        }));
        // Block "foo" calls "bar".
        arena.push(IrNode::Block(IrBlock {
            node_id: NodeId(2),
            name: "foo".into(),
            description: None,
            body_text: "Do something.".into(),
            flow_statements: vec!["Do something.".into()],
            resolved_word_count: None,
            outgoing_calls: vec!["bar".into()],
            return_type: None,
            output_contract: None,
        }));
        // Block "bar" calls "foo" — completing the cycle.
        arena.push(IrNode::Block(IrBlock {
            node_id: NodeId(3),
            name: "bar".into(),
            description: None,
            body_text: "Do something else.".into(),
            flow_statements: vec!["Do something else.".into()],
            resolved_word_count: None,
            outgoing_calls: vec!["foo".into()],
            return_type: None,
            output_contract: None,
        }));
        arena.set_root_skill(NodeId(0));

        let err = validate(&arena).unwrap_err();
        // Should detect the cycle involving either "foo" or "bar".
        match &err {
            ValidateError::RecursiveCall(name) => {
                assert!(
                    name == "foo" || name == "bar",
                    "expected cycle participant, got: {}",
                    name
                );
            }
            other => panic!("expected RecursiveCall, got: {:?}", other),
        }
        assert_eq!(err.diagnostic_id(), "G::validate::recursive-call");
    }
}
