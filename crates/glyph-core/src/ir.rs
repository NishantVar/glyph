//! IR node schema (walking-skeleton subset) and arena per `design/build-foundation.md` §A4.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct NodeId(pub u32);

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IrNode {
    Skill(IrSkill),
    InlineInstruction(IrInlineInstruction),
    Constraint(IrConstraint),
    Context(IrContext),
}

#[derive(Clone, Debug, Serialize)]
pub struct IrSkill {
    pub node_id: NodeId,
    pub name: String,
    pub description: String,
    pub effects: Vec<String>,
    /// Header parameters in source order. Empty for parameterless skills (the
    /// walking-skeleton case); the emitter omits the `## Parameters` section
    /// when this is empty per `design/compiled-output.md` §`## Parameters`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<IrParam>,
    /// Step nodes (in source order).
    pub steps: Vec<NodeId>,
    /// Top-level context nodes.
    pub context: Vec<NodeId>,
    /// Top-level constraint nodes.
    pub constraints: Vec<NodeId>,
}

/// Resolved parameter metadata threaded through Phase 6 Step 1 into the
/// emitter. The emitter renders this as the bulleted entries under
/// `## Parameters` per `design/compiled-output.md`.
#[derive(Clone, Debug, Serialize)]
pub struct IrParam {
    pub name: String,
    /// Pre-rendered default value (e.g., `"."` including quotes for strings).
    /// `None` indicates a runtime-required skill parameter.
    pub default: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct IrInlineInstruction {
    pub node_id: NodeId,
    pub text: String,
    pub role: Role,
}

#[derive(Clone, Debug, Serialize)]
pub struct IrConstraint {
    pub node_id: NodeId,
    pub text: String,
    pub strength: Strength,
    pub polarity: Polarity,
}

#[derive(Clone, Debug, Serialize)]
pub struct IrContext {
    pub node_id: NodeId,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum Role {
    Step,
    Constraint,
    Context,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Strength {
    Soft,
    Hard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Polarity {
    Require,
    Avoid,
}

/// Single arena per file. Lower allocates IDs in pre-order traversal.
#[derive(Default)]
pub struct IrArena {
    nodes: Vec<IrNode>,
    /// The root skill, if any.
    root_skill: Option<NodeId>,
}

impl IrArena {
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocate the next NodeId. Caller fills the slot via `push`.
    pub fn next_id(&self) -> NodeId {
        NodeId(self.nodes.len() as u32)
    }

    pub fn push(&mut self, node: IrNode) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(node);
        id
    }

    pub fn get(&self, id: NodeId) -> &IrNode {
        &self.nodes[id.0 as usize]
    }

    pub fn set_root_skill(&mut self, id: NodeId) {
        self.root_skill = Some(id);
    }

    pub fn root_skill(&self) -> Option<NodeId> {
        self.root_skill
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn nodes(&self) -> &[IrNode] {
        &self.nodes
    }

    pub(crate) fn nodes_mut(&mut self) -> &mut Vec<IrNode> {
        &mut self.nodes
    }
}
