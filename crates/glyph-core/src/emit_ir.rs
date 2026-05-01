//! IR JSON serialization — custom serializer that walks the post-Step-1 IR arena
//! and produces a nested-tree JSON per `design/ir-json-schema.md`.
//!
//! The JSON is nested (children inlined under parents) rather than a flat arena dump.
//! Each node carries its `node_id` as `"n<integer>"`.

use crate::ir::{
    IrArena, IrBranch, IrCall, IrConstraint, IrContext, IrElifBranch,
    IrInlineInstruction, IrNode, IrParam, NodeId, Polarity, Role, Strength,
};
use serde_json::{json, Map, Value};

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
    m.insert("return_type".into(), Value::Null);

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
pub fn serialize_ir_json(arena: &IrArena, source_file: &str, enable_effects: bool) -> Option<String> {
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

    // effects
    let effects: Vec<Value> = if enable_effects {
        skill.effects.iter().map(|e| Value::String(e.clone())).collect()
    } else {
        Vec::new()
    };
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
    use super::*;
    use crate::ir::{IrArena, IrNode, IrSkill, IrInlineInstruction, NodeId, Role};

    fn arena_with_effects() -> IrArena {
        let mut arena = IrArena::new();
        let step_id = arena.push(IrNode::InlineInstruction(IrInlineInstruction {
            node_id: NodeId(0),
            text: "Do something.".into(),
            role: Role::Step,
        }));
        let skill_id = arena.push(IrNode::Skill(IrSkill {
            node_id: NodeId(1),
            name: "test_skill".into(),
            description: "A test skill.".into(),
            effects: vec!["fs:write".into(), "net:http".into()],
            params: vec![],
            steps: vec![step_id],
            context: vec![],
            constraints: vec![],
            return_text: None,
        }));
        arena.set_root_skill(skill_id);
        arena
    }

    #[test]
    fn serialize_ir_json_emits_empty_effects_when_disabled() {
        let arena = arena_with_effects();
        let json_str = serialize_ir_json(&arena, "test.glyph.md", false).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let effects = v["skill"]["effects"].as_array().unwrap();
        assert!(effects.is_empty(), "effects should be empty when enable_effects is false");
    }

    #[test]
    fn serialize_ir_json_emits_actual_effects_when_enabled() {
        let arena = arena_with_effects();
        let json_str = serialize_ir_json(&arena, "test.glyph.md", true).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let effects = v["skill"]["effects"].as_array().unwrap();
        assert_eq!(effects.len(), 2, "effects should contain 2 entries when enable_effects is true");
        assert_eq!(effects[0], "fs:write");
        assert_eq!(effects[1], "net:http");
    }
}
