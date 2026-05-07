//! IR node schema (walking-skeleton subset) and arena per `design/build-foundation.md` §A4.

use crate::kind_infer::TypeTag;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::HashMap;

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
    /// Issue #85: `return <IDENT>` output-target form. Carries the
    /// agent-synthesized output target's name and the enclosing decl's
    /// `-> DomainType` annotation. Chunk 5 wires JSON; chunk 6 rewrites
    /// in expand; chunks 8/9 surface diagnostics.
    OutputContract(IrOutputContract),
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
    /// Issue #84 chunk 6: lowered form of the `-> DomainType` annotation on
    /// the skill header (per `design/language-surface.md` §3.1 line 161).
    /// `None` means the author wrote no annotation. Built-ins lower to their
    /// `TypeTag` variant; everything else lowers to
    /// `DomainType(canonicalize_identifier(name))`. The IR-JSON emitter in
    /// `emit_ir::serialize_ir_json` consumes this; the serde derive here is
    /// incidental (no production caller serializes IR via the derive), so
    /// the field is `#[serde(skip)]` to avoid forcing `TypeTag: Serialize`
    /// (designer flag: do not match-by-Debug).
    #[serde(skip)]
    pub return_type: Option<TypeTag>,
    /// Issue #85: optional `OutputContract` IR node id, populated when the
    /// skill's flow ends with `return <IDENT>`. The contract carries the
    /// target name and the lowered form of the skill's `-> DomainType`
    /// annotation. Chunk 5 wires emit-IR (JSON); the field is `#[serde(skip)]`
    /// here for the same reason as `return_type` — no production caller
    /// serializes IR via the derive.
    #[serde(skip)]
    pub output_contract: Option<NodeId>,
    /// Source-text spelling of the `-> DomainType` annotation (e.g. `"Diagnosis"`).
    /// `return_type` above stores the canonicalized `TypeTag::DomainType("diagnosis")`,
    /// which loses the original casing — but the §8.4 return-prose templates render
    /// the type as the author wrote it (`` Produce `name` (`Foo`). ``) and look up
    /// the `TypeRegistry` description by source-text key. `None` mirrors `return_type`.
    #[serde(skip)]
    pub return_type_text: Option<String>,
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
    /// Lowered from `Param.description` (span dropped — emitter only needs content).
    pub description: Option<String>,
    /// Lowered from `Param.type_annotation` (span dropped — emitter only needs content).
    pub type_annotation: Option<String>,
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
    /// Issue #84 chunk 6: lowered form of the `-> DomainType` annotation on
    /// the block header (per `design/language-surface.md` §3.2 line 198).
    /// Per planner h.1 decision, Block `return_type` is stored on IR but NOT
    /// emitted as a top-level Block JSON kind; it surfaces in IR-JSON via
    /// the caller's `IrCall.return_type` lookup. `#[serde(skip)]` for the
    /// same reason as `IrSkill::return_type`.
    #[serde(skip)]
    pub return_type: Option<TypeTag>,
    /// Issue #85: same role as `IrSkill::output_contract`, populated when a
    /// private block's flow ends with `return <IDENT>`.
    #[serde(skip)]
    pub output_contract: Option<NodeId>,
    /// Source-text type name; mirrors `IrSkill::return_type_text`. See that
    /// field's doc for why the canonicalized `return_type` is insufficient.
    #[serde(skip)]
    pub return_type_text: Option<String>,
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
    /// Issue #84 chunk 6: the callee's lowered `-> DomainType` annotation, when
    /// the callee is a same-file block (resolved at lower time via the
    /// `lower::blocks` map). `None` for stdlib calls and cross-file calls
    /// (D17: cross-file `Call.return_type` resolution via imported
    /// `block_return_types` is a deferred follow-up). The IR-JSON emitter
    /// renders this slot via `opt_typetag_to_json`. `#[serde(skip)]` for the
    /// same reason as `IrSkill::return_type`.
    #[serde(skip)]
    pub return_type: Option<TypeTag>,
    /// Issue #85: the callee block's `output_contract` form, hoisted onto the
    /// Call so expand- and emit-time gates don't need an arena lookup keyed by
    /// block name. Populated at lower time for same-file callees and during
    /// the import fix-up step in `compile_source_with_resolved_imports` for
    /// cross-file Tier-1 callees. `None` for stdlib calls and for callees
    /// without a `return <…>` contract.
    #[serde(skip)]
    pub callee_output_contract: Option<OutputTargetForm>,
    /// Callee block's source-text `-> DomainType` spelling (e.g. `"Diagnosis"`).
    /// Mirrors `callee_output_contract`'s plumbing: same-file lower populates
    /// from the block declaration; cross-file fix-up populates from
    /// `ResolvedImports::block_return_types`. Used by the §8.4 return-prose
    /// templates when the callee's contract drives a Tier-1 last step (skill
    /// has no OC of its own). `None` for stdlib and untyped callees.
    #[serde(skip)]
    pub callee_return_type_text: Option<String>,
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

/// Issue #86: tagged form distinguishing identifier vs descriptive output
/// targets. Replaces #85's flat `target_name: String` field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum OutputTargetForm {
    Identifier(String),
    Description(String),
}

/// Issue #85/#86: lowered form of `return <IDENT>` or `return <"description">`.
/// Captures the agent-synthesized output target plus the enclosing decl's
/// `-> DomainType`. The name `ty` (rather than `type`) avoids the Rust keyword.
/// JSON wiring is chunk 5 (updated in #86 chunk 2).
#[derive(Clone, Debug, Serialize)]
pub struct IrOutputContract {
    pub node_id: NodeId,
    pub form: OutputTargetForm,
    /// Lowered enclosing-decl annotation (`Skill`/`Block`/`ExportBlock`'s
    /// `-> DomainType`). `None` is permitted at lowering time — the
    /// missing-annotation diagnostic is chunk 8/9's job.
    /// `#[serde(skip)]` matches the rest of the `TypeTag` slots in this
    /// module (`TypeTag` has no `Serialize`; the IR-JSON emitter reads the
    /// field directly).
    #[serde(skip)]
    pub ty: Option<TypeTag>,
    pub source: OutputSource,
}

/// Issue #85: the provenance of an output contract. Today's only variant is
/// `SynthesizedByAgent` — the agent fabricates the named output. Future
/// variants (e.g. caller-supplied) would join this enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputSource {
    SynthesizedByAgent,
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

/// Per-compilation-unit map of type name → canonical description.
/// Built during lowering from same-file `TypeDecl`s plus selectively-imported
/// `export type` decls (Phase B.7). Consumed by the emitter for per-param
/// type-level lookup (spec §7.1) and the return-prose fold (spec §8.4).
#[derive(Clone, Debug, Default)]
pub struct TypeRegistry {
    pub descriptions: HashMap<String, String>,
}

/// Single arena per file. Lower allocates IDs in pre-order traversal.
#[derive(Debug, Default)]
pub struct IrArena {
    nodes: Vec<IrNode>,
    /// The root skill, if any.
    root_skill: Option<NodeId>,
    /// Type-description registry built from same-file `type` decls.
    /// Cross-file imports folded in by Phase B.7.
    pub type_registry: TypeRegistry,
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

#[cfg(test)]
mod tests {
    #[test]
    fn output_contract_constructs_both_forms() {
        use crate::ir::{IrOutputContract, NodeId, OutputSource, OutputTargetForm};
        let id_form = IrOutputContract {
            node_id: NodeId(0),
            form: OutputTargetForm::Identifier("current_branch".into()),
            ty: None,
            source: OutputSource::SynthesizedByAgent,
        };
        let desc_form = IrOutputContract {
            node_id: NodeId(1),
            form: OutputTargetForm::Description("root cause analysis".into()),
            ty: None,
            source: OutputSource::SynthesizedByAgent,
        };
        assert!(matches!(id_form.form, OutputTargetForm::Identifier(ref n) if n == "current_branch"));
        assert!(matches!(desc_form.form, OutputTargetForm::Description(ref d) if d == "root cause analysis"));
    }
}
