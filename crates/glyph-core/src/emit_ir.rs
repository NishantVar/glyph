//! IR JSON serialization — custom serializer that walks the post-Step-1 IR arena
//! and produces a nested-tree JSON per `design/ir-json-schema.md`.
//!
//! The JSON is nested (children inlined under parents) rather than a flat arena dump.
//! Each node carries its `node_id` as `"n<integer>"`.

use crate::ir::{
    IrArena, IrBranch, IrCall, IrConstraint, IrContext, IrElifBranch,
    IrInlineInstruction, IrNode, IrParam, NodeId, Polarity, Role, Strength,
};
use crate::kind_infer::TypeTag;
use serde_json::{json, Map, Value};

/// Serialize a `TypeTag` to its IR JSON representation per
/// `design/ir-json-schema.md` §TypeTag Serialization (FC8 designer relay):
/// built-ins lower to lowercase JSON strings; `DomainType(name)` lowers to a
/// single-key object `{"domain_type": "<canonical_name>"}`. The payload is
/// already canonical (lower-time `name_to_typetag` performs the
/// ASCII-lowercase + `_` strip per D6); this helper does no further
/// canonicalization.
fn typetag_to_json(tag: &TypeTag) -> Value {
    match tag {
        // Built-ins → lowercase JSON strings, spelled literally per FC8
        // designer relay (no match-by-Debug).
        TypeTag::String => Value::String("string".into()),
        TypeTag::Int => Value::String("int".into()),
        TypeTag::Float => Value::String("float".into()),
        TypeTag::Bool => Value::String("bool".into()),
        TypeTag::None => Value::String("none".into()),
        TypeTag::Agent => Value::String("agent".into()),
        TypeTag::DomainType(name) => {
            let mut m = Map::new();
            m.insert("domain_type".into(), Value::String(name.clone()));
            Value::Object(m)
        }
    }
}

/// Convenience wrapper: serialize an `Option<TypeTag>` field. `None` lowers
/// to JSON `null` (matches the slot's pre-chunk-6 hardcoded behavior at
/// `serialize_call`); `Some(t)` defers to [`typetag_to_json`].
fn opt_typetag_to_json(tag: &Option<TypeTag>) -> Value {
    match tag {
        Some(t) => typetag_to_json(t),
        None => Value::Null,
    }
}

/// Format a NodeId as the `"n<integer>"` string per ir-json-schema.md §Node ID Convention.
fn node_id_str(id: NodeId) -> String {
    format!("n{}", id.0)
}

/// Serialize a Role enum to its JSON string per ir-json-schema.md §Enum Serialization.
fn role_str(role: Role) -> &'static str {
    match role {
        Role::InputContract => "input_contract",
        Role::Step => "step",
        Role::Constraint => "constraint",
        Role::Context => "context",
        Role::OutputContract => "output_contract",
    }
}

/// Serialize a Strength enum to its JSON string.
fn strength_str(strength: Strength) -> &'static str {
    match strength {
        Strength::Soft => "soft",
        Strength::Hard => "hard",
    }
}

/// Serialize a Polarity enum to its JSON string.
fn polarity_str(polarity: Polarity) -> &'static str {
    match polarity {
        Polarity::Require => "require",
        Polarity::Avoid => "avoid",
    }
}

/// Map projection_tier to the projection_mode JSON string.
fn projection_mode_str(tier: Option<u8>) -> &'static str {
    match tier {
        Some(2) => "same_file_procedure",
        Some(3) => "external_file",
        _ => "inline",
    }
}

/// Serialize a Param node to JSON.
fn serialize_param(param: &IrParam, node_id: &str) -> Value {
    let mut m = Map::new();
    m.insert("node_id".into(), Value::String(node_id.into()));
    m.insert("kind".into(), Value::String("param".into()));
    m.insert("name".into(), Value::String(param.name.clone()));
    // type: omitted when duck-typed (always omitted in current IR)
    if let Some(ref default) = param.default {
        // default is pre-rendered with surrounding quotes for strings (e.g., `"."`).
        // Strip the quotes for the JSON value.
        let raw = default
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(default);
        m.insert(
            "default".into(),
            json!({ "kind": "string", "value": raw }),
        );
    }
    Value::Object(m)
}

/// Serialize a Constraint node to JSON.
fn serialize_constraint(c: &IrConstraint) -> Value {
    let mut m = Map::new();
    m.insert("node_id".into(), Value::String(node_id_str(c.node_id)));
    m.insert("kind".into(), Value::String("constraint".into()));
    m.insert("text".into(), Value::String(c.text.clone()));
    m.insert(
        "strength".into(),
        Value::String(strength_str(c.strength).into()),
    );
    m.insert(
        "polarity".into(),
        Value::String(polarity_str(c.polarity).into()),
    );
    Value::Object(m)
}

/// Serialize a ContextNode to JSON.
fn serialize_context(c: &IrContext) -> Value {
    let mut m = Map::new();
    m.insert("node_id".into(), Value::String(node_id_str(c.node_id)));
    m.insert("kind".into(), Value::String("context".into()));
    m.insert("text".into(), Value::String(c.text.clone()));
    Value::Object(m)
}

/// Serialize an InlineInstruction node to JSON.
fn serialize_inline_instruction(i: &IrInlineInstruction) -> Value {
    let mut m = Map::new();
    m.insert("node_id".into(), Value::String(node_id_str(i.node_id)));
    m.insert("kind".into(), Value::String("inline_instruction".into()));
    m.insert("text".into(), Value::String(i.text.clone()));
    m.insert("role".into(), Value::String(role_str(i.role).into()));
    Value::Object(m)
}

/// Serialize a Call (resolved) node to JSON.
fn serialize_call(c: &IrCall, arena: &IrArena) -> Value {
    let mut m = Map::new();
    m.insert("node_id".into(), Value::String(node_id_str(c.node_id)));
    m.insert("kind".into(), Value::String("call".into()));
    m.insert("target".into(), Value::String(c.target.clone()));

    // args: map of param name to expression. Current IR stores positional
    // string args; serialize as binding_ref expressions.
    let mut args_map = Map::new();
    for (i, arg) in c.args.iter().enumerate() {
        let arg_node_id = format!("n{}_{}", c.node_id.0, i);
        args_map.insert(
            arg.clone(),
            json!({
                "node_id": arg_node_id,
                "kind": "binding_ref",
                "name": arg
            }),
        );
    }
    m.insert("args".into(), Value::Object(args_map));

    m.insert("output".into(), Value::Null);
    // Issue #84 chunk 6: callee return-type slot. `None` lowers to JSON null
    // (the slot's pre-chunk-6 hardcoded behavior); `Some(TypeTag)` lowers per
    // FC8 designer relay (built-ins → lowercase string, DomainType → object).
    m.insert(
        "return_type".into(),
        opt_typetag_to_json(&c.return_type),
    );

    // effects: inherit from callee block if available, else empty.
    m.insert("effects".into(), json!([]));

    // site_modifier
    m.insert(
        "site_modifier".into(),
        match &c.site_modifier {
            Some(s) => Value::String(s.clone()),
            None => Value::Null,
        },
    );

    m.insert("role".into(), Value::String("step".into()));

    // scoped_constraints: empty for now.
    m.insert("scoped_constraints".into(), json!([]));

    // resolved_body_text
    m.insert(
        "resolved_body_text".into(),
        Value::String(c.resolved_body.clone().unwrap_or_default()),
    );

    // local_refs: empty for current IR (no local-binding tracking yet).
    m.insert("local_refs".into(), json!([]));

    // projection_mode
    m.insert(
        "projection_mode".into(),
        Value::String(projection_mode_str(c.projection_tier).into()),
    );

    // callee_description: present on non-inline calls when the callee block has a description.
    // callee_flow, callee_context, callee_constraints
    let is_inline = c.projection_tier.is_none() || c.projection_tier == Some(1);
    if is_inline {
        m.insert("callee_flow".into(), Value::Null);
        m.insert("callee_context".into(), Value::Null);
        m.insert("callee_constraints".into(), Value::Null);
    } else {
        // For non-inline projections, populate from callee block if available.
        let block = find_block_by_name(arena, &c.target);
        if let Some(block) = block {
            // callee_description: emit when the block has a description set.
            if let Some(ref desc) = block.description {
                m.insert("callee_description".into(), Value::String(desc.clone()));
            }

            // callee_flow: serialize the block's flow statements as inline instructions.
            let flow: Vec<Value> = block
                .flow_statements
                .iter()
                .enumerate()
                .map(|(i, stmt)| {
                    json!({
                        "node_id": format!("n{}_{}", block.node_id.0, i),
                        "kind": "inline_instruction",
                        "text": stmt,
                        "role": "step"
                    })
                })
                .collect();
            m.insert("callee_flow".into(), Value::Array(flow));
            m.insert("callee_context".into(), json!([]));
            m.insert("callee_constraints".into(), json!([]));
        } else {
            m.insert("callee_flow".into(), Value::Null);
            m.insert("callee_context".into(), Value::Null);
            m.insert("callee_constraints".into(), Value::Null);
        }
    }

    // procedure_path
    m.insert(
        "procedure_path".into(),
        match &c.procedure_path {
            Some(p) => Value::String(p.clone()),
            None => Value::Null,
        },
    );

    Value::Object(m)
}

/// Serialize a Branch node to JSON.
fn serialize_branch(br: &IrBranch, arena: &IrArena) -> Value {
    let mut m = Map::new();
    m.insert("node_id".into(), Value::String(node_id_str(br.node_id)));
    m.insert("kind".into(), Value::String("branch".into()));
    m.insert("condition".into(), Value::String(br.condition.clone()));

    // then_body
    let then_nodes: Vec<Value> = br
        .then_body
        .iter()
        .map(|id| serialize_flow_node(arena, *id))
        .collect();
    m.insert("then_body".into(), Value::Array(then_nodes));

    // elif_branches
    let elifs: Vec<Value> = br
        .elif_branches
        .iter()
        .map(|elif| serialize_elif(elif, arena))
        .collect();
    m.insert("elif_branches".into(), Value::Array(elifs));

    // else_body
    m.insert(
        "else_body".into(),
        match &br.else_body {
            Some(body) => {
                let nodes: Vec<Value> = body
                    .iter()
                    .map(|id| serialize_flow_node(arena, *id))
                    .collect();
                Value::Array(nodes)
            }
            None => Value::Null,
        },
    );

    // applies_descriptions
    m.insert(
        "applies_descriptions".into(),
        match &br.applies_descriptions {
            Some(descs) => {
                let obj: Map<String, Value> = descs
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                    .collect();
                Value::Object(obj)
            }
            None => Value::Null,
        },
    );

    Value::Object(m)
}

/// Serialize an ElifBranch to JSON.
fn serialize_elif(elif: &IrElifBranch, arena: &IrArena) -> Value {
    let mut m = Map::new();
    // ElifBranch doesn't have its own node_id in current IR; generate a synthetic one.
    m.insert("kind".into(), Value::String("elif_branch".into()));
    m.insert("condition".into(), Value::String(elif.condition.clone()));

    let body: Vec<Value> = elif
        .body
        .iter()
        .map(|id| serialize_flow_node(arena, *id))
        .collect();
    m.insert("body".into(), Value::Array(body));

    Value::Object(m)
}

/// Serialize any flow node (dispatches by kind).
fn serialize_flow_node(arena: &IrArena, id: NodeId) -> Value {
    match arena.get(id) {
        IrNode::InlineInstruction(i) => serialize_inline_instruction(i),
        IrNode::Call(c) => serialize_call(c, arena),
        IrNode::Branch(br) => serialize_branch(br, arena),
        IrNode::Constraint(c) => serialize_constraint(c),
        IrNode::Context(c) => serialize_context(c),
        _ => json!({"node_id": node_id_str(id), "kind": "unknown"}),
    }
}

/// Find a Block node in the arena by name.
fn find_block_by_name<'a>(arena: &'a IrArena, name: &str) -> Option<&'a crate::ir::IrBlock> {
    arena.nodes().iter().find_map(|n| {
        if let IrNode::Block(b) = n {
            if b.name == name {
                return Some(b);
            }
        }
        None
    })
}

/// Serialize the full IR arena to JSON per `ir-json-schema.md`.
///
/// Returns the JSON string. The arena must have a root skill set.
/// Returns `None` if the arena has no root skill (library file — no IR JSON produced).
pub fn serialize_ir_json(arena: &IrArena, source_file: &str) -> Option<String> {
    let root_id = arena.root_skill()?;
    let skill = match arena.get(root_id) {
        IrNode::Skill(s) => s,
        _ => return None,
    };

    let mut envelope = Map::new();
    envelope.insert("ir_version".into(), json!(1));
    envelope.insert(
        "compiler".into(),
        Value::String(format!("glyph {}", env!("CARGO_PKG_VERSION"))),
    );
    envelope.insert("source_file".into(), Value::String(source_file.into()));

    // Serialize Skill node.
    let mut skill_obj = Map::new();
    skill_obj.insert("node_id".into(), Value::String(node_id_str(skill.node_id)));
    skill_obj.insert("kind".into(), Value::String("skill".into()));
    skill_obj.insert("name".into(), Value::String(skill.name.clone()));
    skill_obj.insert(
        "description".into(),
        Value::String(skill.description.clone()),
    );

    // params
    let params: Vec<Value> = skill
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            // Params don't have their own NodeId in current IrParam; generate synthetic.
            let param_nid = format!("n0_{}", i);
            serialize_param(p, &param_nid)
        })
        .collect();
    skill_obj.insert("params".into(), Value::Array(params));

    // return_type: `{"domain_type": "<canonical>"}` for domain types,
    // lowercase JSON string for built-ins, JSON `null` when no `->` annotation
    // was authored (issue #84 chunk 6, FC8 designer relay). Canonical schema
    // position is between `params` and `effects` per `design/ir-json-schema.md`
    // §Node Types → Skill (FC9 correction). serde_json::Map is BTreeMap by
    // default so the serialized JSON is alphabetical regardless of insert
    // order, but the insert sequence here mirrors the schema for readability
    // and matches the IR struct field ordering used in `ir-schema.md`.
    skill_obj.insert(
        "return_type".into(),
        opt_typetag_to_json(&skill.return_type),
    );

    // effects
    let effects: Vec<Value> = skill
        .effects
        .iter()
        .map(|e| Value::String(e.clone()))
        .collect();
    skill_obj.insert("effects".into(), Value::Array(effects));

    // context
    let context: Vec<Value> = skill
        .context
        .iter()
        .map(|id| {
            if let IrNode::Context(c) = arena.get(*id) {
                serialize_context(c)
            } else {
                json!(null)
            }
        })
        .collect();
    skill_obj.insert("context".into(), Value::Array(context));

    // constraints
    let constraints: Vec<Value> = skill
        .constraints
        .iter()
        .map(|id| {
            if let IrNode::Constraint(c) = arena.get(*id) {
                serialize_constraint(c)
            } else {
                json!(null)
            }
        })
        .collect();
    skill_obj.insert("constraints".into(), Value::Array(constraints));

    // flow
    let flow: Vec<Value> = skill
        .steps
        .iter()
        .map(|id| serialize_flow_node(arena, *id))
        .collect();
    skill_obj.insert("flow".into(), Value::Array(flow));

    envelope.insert("skill".into(), Value::Object(skill_obj));

    // Use serde_json to_string_pretty for human-readable output.
    // We build maps in spec-defined insertion order. serde_json::Map preserves
    // insertion order, giving deterministic output across runs.
    Some(serde_json::to_string_pretty(&Value::Object(envelope)).unwrap())
}

#[cfg(test)]
mod tests {
    //! Issue #84 chunk 6 — IR `return_type` propagation. Unit tests for the
    //! `TypeTag → JSON` helper per `design/ir-json-schema.md` §TypeTag
    //! Serialization (FC8 designer relay).
    use super::*;
    use crate::kind_infer::TypeTag;

    // f.1 (tracer): DomainType lowers to a single-key object `{"domain_type":
    // "<canonical name verbatim>"}`. Helper performs no further canonicalization
    // — the lower-time `name_to_typetag` (chunk 6) is the sole canonicalization
    // boundary.
    #[test]
    fn typetag_to_json_domain_type_emits_object_with_canonical_name() {
        let v = typetag_to_json(&TypeTag::DomainType("report".into()));
        assert_eq!(v, serde_json::json!({ "domain_type": "report" }));
    }

    // f.2: each built-in TypeTag variant lowers to its lowercase JSON string.
    // Per FC8 designer relay (cite: `design/ir-json-schema.md` §TypeTag
    // Serialization L412–421). Match-by-Debug is explicitly forbidden; the
    // arms in `typetag_to_json` must spell the lowercase form literally.
    #[test]
    fn typetag_to_json_builtins_emit_lowercase_strings() {
        assert_eq!(typetag_to_json(&TypeTag::String), Value::String("string".into()));
        assert_eq!(typetag_to_json(&TypeTag::Int),    Value::String("int".into()));
        assert_eq!(typetag_to_json(&TypeTag::Float),  Value::String("float".into()));
        assert_eq!(typetag_to_json(&TypeTag::Bool),   Value::String("bool".into()));
        assert_eq!(typetag_to_json(&TypeTag::None),   Value::String("none".into()));
        assert_eq!(typetag_to_json(&TypeTag::Agent),  Value::String("agent".into()));
    }

    // f.3: the `Option<TypeTag>` call-site wrapper — `None` lowers to JSON
    // `null` (matches the slot's pre-chunk-6 hardcoded behavior at
    // `serialize_call`). `Some(t)` defers to `typetag_to_json`.
    #[test]
    fn opt_typetag_to_json_none_emits_null() {
        assert_eq!(opt_typetag_to_json(&None), Value::Null);
    }

    #[test]
    fn opt_typetag_to_json_some_defers_to_inner_helper() {
        assert_eq!(
            opt_typetag_to_json(&Some(TypeTag::DomainType("plan".into()))),
            serde_json::json!({ "domain_type": "plan" })
        );
        assert_eq!(
            opt_typetag_to_json(&Some(TypeTag::String)),
            Value::String("string".into())
        );
    }
}
