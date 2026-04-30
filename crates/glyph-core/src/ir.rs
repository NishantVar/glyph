//! IR node schema (walking-skeleton subset) and arena per `design/build-foundation.md` §A4.

use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct NodeId(pub u32);

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IrNode {
    Skill(IrSkill),
    InlineInstruction(IrInlineInstruction),
    Constraint(IrConstraint),
    Context(IrContext),
    Block(IrBlock),
    Call(IrCall),
    Branch(IrBranch),
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
    /// Return expression text, if any. Populated from `return <expr>` in flow.
    /// `None` means no explicit return (implicit `none`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_text: Option<String>,
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

#[derive(Clone, Debug, Serialize)]
pub struct IrBlock {
    pub node_id: NodeId,
    pub name: String,
    pub description: Option<String>,
    /// Resolved body text (concatenated flow inline strings).
    pub body_text: String,
    /// Individual flow statement strings, preserved for Tier 2 procedure emission.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flow_statements: Vec<String>,
    /// Word count of the resolved body text, computed in Expand Step 1.
    #[serde(default)]
    pub resolved_word_count: Option<u32>,
    /// Names of blocks called from this block's flow (outgoing call edges).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outgoing_calls: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct IrCall {
    pub node_id: NodeId,
    /// Target block name.
    pub target: String,
    /// Positional args (identifiers or string values).
    pub args: Vec<String>,
    /// Resolved callee body text for Tier 1 inline expansion.
    /// Populated during Lower; None if callee not found.
    pub resolved_body: Option<String>,
    /// `with` modifier text, if present. Stored for IR JSON (`--emit-ir`)
    /// consumption by the agent in Step 2. Not applied during Step 1 emit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_modifier: Option<String>,
    /// Projection tier assigned by Expand Step 1.
    /// `None` before expand runs; `Some(2)` = same-file procedure; `Some(3)` = external file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection_tier: Option<u8>,
    /// Relative file path for Tier 3 (external-file) projections.
    /// E.g., `repo_tools/inspect-repo.md`. Populated by Expand Step 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub procedure_path: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct IrBranch {
    pub node_id: NodeId,
    pub condition: String,
    /// Flow nodes in the `if` body.
    pub then_body: Vec<NodeId>,
    /// `elif` arms.
    pub elif_branches: Vec<IrElifBranch>,
    /// Optional `else` body.
    pub else_body: Option<Vec<NodeId>>,
    /// Maps block names to their resolved `description:` text for
    /// `BLOCKNAME.applies()` calls in conditions. Populated by Expand Step 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applies_descriptions: Option<BTreeMap<String, String>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct IrElifBranch {
    pub condition: String,
    pub body: Vec<NodeId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    InputContract,
    Step,
    Constraint,
    Context,
    OutputContract,
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
#[derive(Debug, Default)]
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
