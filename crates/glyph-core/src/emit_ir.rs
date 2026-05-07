//! IR JSON serialization — custom serializer that walks the post-Step-1 IR arena
//! and produces a nested-tree JSON per `design/ir-json-schema.md`.
//!
//! The JSON is nested (children inlined under parents) rather than a flat arena dump.
//! Each node carries its `node_id` as `"n<integer>"`.

use crate::ir::{
    IrArena, IrBranch, IrCall, IrConstraint, IrContext, IrElifBranch, IrInlineInstruction, IrNode,
    IrOutputContract, IrParam, NodeId, OutputSource, OutputTargetForm, Polarity, Role, Strength,
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

/// Issue #85 chunk 5: serialize an `OutputSource` enum to its JSON string.
/// `SynthesizedByAgent` is the only variant today; future provenance variants
/// would extend this. Spelled literally (no match-by-Debug) to match the
/// `#[serde(rename_all = "snake_case")]` declaration on `OutputSource`.
fn output_source_str(s: OutputSource) -> &'static str {
    match s {
        OutputSource::SynthesizedByAgent => "synthesized_by_agent",
    }
}

/// Issue #85/#86: serialize an `IrOutputContract` arena entry. For identifier
/// form: `{ node_id, kind, form: "identifier", target_name, ty, source }`.
/// For description form: `{ node_id, kind, form: "description", description,
/// ty, source }`. `ty` defers to `opt_typetag_to_json` (FC8 designer relay).
fn serialize_output_contract(oc: &IrOutputContract) -> Value {
    let mut m = Map::new();
    m.insert("node_id".into(), Value::String(node_id_str(oc.node_id)));
    m.insert("kind".into(), Value::String("output_contract".into()));
    match &oc.form {
        OutputTargetForm::Identifier(name) => {
            m.insert("form".into(), Value::String("identifier".into()));
            m.insert("target_name".into(), Value::String(name.clone()));
        }
        OutputTargetForm::Description(desc) => {
            m.insert("form".into(), Value::String("description".into()));
            m.insert("description".into(), Value::String(desc.clone()));
        }
    }
    m.insert("ty".into(), opt_typetag_to_json(&oc.ty));
    m.insert(
        "source".into(),
        Value::String(output_source_str(oc.source).into()),
    );
    Value::Object(m)
}

/// Issue #85 chunk 5: resolve an `Option<NodeId>` slot (either
/// `IrSkill.output_contract` or `IrBlock.output_contract`) to its emitted
/// JSON value: `Value::Null` when the slot is empty, otherwise the
/// `serialize_output_contract` output of the pointed-at arena node. The
/// helper unifies the skill-level field and the call-site
/// `callee_output_contract` lookup so both call sites use the same fallback
/// for an unexpected non-OutputContract arena entry (panic-free `null`).
fn output_contract_json(arena: &IrArena, slot: Option<NodeId>) -> Value {
    match slot {
        Some(id) => match arena.get(id) {
            IrNode::OutputContract(oc) => serialize_output_contract(oc),
            _ => Value::Null,
        },
        None => Value::Null,
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
        m.insert("default".into(), json!({ "kind": "string", "value": raw }));
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
    if let Some(name) = &c.name {
        m.insert("name".into(), Value::String(name.clone()));
    }
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
    m.insert("return_type".into(), opt_typetag_to_json(&c.return_type));

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
    // Issue #85 chunk 5 (planner D5 — α): the callee block's
    // `OutputContract`, denormalized onto the call site under
    // `callee_output_contract` (mirrors the existing `callee_*` convention).
    // Always emit the field — null when the callee is unresolved or has no
    // contract; the pinned object otherwise. Inline resolved calls carry it
    // too so validate-output can run the output-target leak check using only
    // the emitted IR JSON.
    let callee_output_contract = match find_block_by_name(arena, &c.target) {
        Some(b) => output_contract_json(arena, b.output_contract),
        None => Value::Null,
    };
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
    // Hoisted out of the branches above: the value was computed once, and the
    // field must appear in every call's JSON.
    m.insert("callee_output_contract".into(), callee_output_contract);

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

    // resolved_predicates
    m.insert(
        "resolved_predicates".into(),
        match &br.resolved_predicates {
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

    // predicate_shape
    m.insert(
        "predicate_shape".into(),
        serde_json::json!({
            "has_boolean_token": br.predicate_shape.has_boolean_token,
            "has_predicate_token": br.predicate_shape.has_predicate_token,
            "has_compositional_operator": br.predicate_shape.has_compositional_operator,
        }),
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
pub fn serialize_ir_json(
    arena: &IrArena,
    source_file: &str,
    enable_effects: bool,
) -> Option<String> {
    let root_id = arena.root_skill()?;
    let skill = match arena.get(root_id) {
        IrNode::Skill(s) => s,
        _ => return None,
    };

    let mut envelope = Map::new();
    envelope.insert("ir_version".into(), json!(2));
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
    let effects: Vec<Value> = if enable_effects {
        skill
            .effects
            .iter()
            .map(|e| Value::String(e.clone()))
            .collect()
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

    // Issue #85 chunk 5 (planner D5 — α): the skill's own
    // `OutputContract` node, surfaced after `flow` to mirror the IR struct
    // field order. `Value::Null` when the skill's flow doesn't end with
    // `return <IDENT>`; otherwise the pinned object produced by
    // `serialize_output_contract`. Callers that walk the JSON for role
    // preservation (e.g. `validate_output.rs`) only iterate `skill.flow`,
    // so this sibling field doesn't perturb step counts.
    skill_obj.insert(
        "output_contract".into(),
        output_contract_json(arena, skill.output_contract),
    );

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
        assert_eq!(
            typetag_to_json(&TypeTag::String),
            Value::String("string".into())
        );
        assert_eq!(typetag_to_json(&TypeTag::Int), Value::String("int".into()));
        assert_eq!(
            typetag_to_json(&TypeTag::Float),
            Value::String("float".into())
        );
        assert_eq!(
            typetag_to_json(&TypeTag::Bool),
            Value::String("bool".into())
        );
        assert_eq!(
            typetag_to_json(&TypeTag::None),
            Value::String("none".into())
        );
        assert_eq!(
            typetag_to_json(&TypeTag::Agent),
            Value::String("agent".into())
        );
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

#[cfg(test)]
mod output_contract_emit_tests {
    //! Issue #85 chunk 5 — `IrOutputContract` IR-JSON serialization.
    //!
    //! Surface contract:
    //! - Skill-level: `skill_obj.output_contract` is a JSON object (or `null`)
    //!   sibling to `flow`, positioned after `flow`.
    //! - Pinned shape (object form):
    //!   `{ node_id, kind: "output_contract", target_name, ty, source }`.
    //! - Block-level: surfaced via the call site as `callee_output_contract`
    //!   on `IrCall` JSON (planner D5 — α with `callee_*` denormalization
    //!   convention). Tier 1 (undefined callee) lowers to `null` (behavior #9).
    //!
    //! Tests use the parse → lower → `serialize_ir_json` round-trip path so
    //! they exercise the same plumbing the CLI does.
    use super::*;
    use crate::{expand, lower, parse};
    use serde_json::Value;

    fn ir_json(src: &str) -> Value {
        let (file, _) = parse::parse(src, 0).expect("source should parse");
        let arena = lower::lower(&file).expect("source should lower");
        let s = serialize_ir_json(&arena, "test.glyph", false)
            .expect("arena has a root skill so JSON is produced");
        serde_json::from_str(&s).expect("emitter output is valid JSON")
    }

    /// Variant of [`ir_json`] that runs `expand_step1` between lower and emit
    /// so block calls receive their final projection tier before JSON
    /// serialization.
    fn ir_json_after_expand(src: &str) -> Value {
        let (file, _) = parse::parse(src, 0).expect("source should parse");
        let arena = lower::lower(&file).expect("source should lower");
        let arena = expand::expand_step1(arena);
        let s = serialize_ir_json(&arena, "test.glyph", false)
            .expect("arena has a root skill so JSON is produced");
        serde_json::from_str(&s).expect("emitter output is valid JSON")
    }

    /// Locate a `call` node by its `target` in a flow array. Returns the
    /// first match (tests construct flows with one call per target).
    fn find_call<'a>(flow: &'a Value, target: &str) -> &'a Value {
        flow.as_array()
            .expect("flow is an array")
            .iter()
            .find(|n| {
                n.get("kind").and_then(|k| k.as_str()) == Some("call")
                    && n.get("target").and_then(|t| t.as_str()) == Some(target)
            })
            .unwrap_or_else(|| panic!("expected a call to `{target}` in flow"))
    }

    // Behavior #6 (planner D5 — α): block-level OutputContract surfaces on
    // the call site as `callee_output_contract`, mirroring the existing
    // `callee_*` denormalization convention. The block has ≥4 flow statements
    // so it gets promoted to Tier 2.
    #[test]
    fn block_call_site_carries_callee_output_contract_object() {
        let src = "\
skill drive()
    flow:
        helper()

block helper() -> Path
    flow:
        \"a\"
        \"b\"
        \"c\"
        return <out>
";
        let v = ir_json_after_expand(src);
        let flow = v.pointer("/skill/flow").expect("skill flow array present");
        let call = find_call(flow, "helper");
        assert_eq!(
            call.get("projection_mode").and_then(|m| m.as_str()),
            Some("same_file_procedure"),
            "block with >=4 flow statements promotes to Tier 2"
        );
        let coc = call
            .get("callee_output_contract")
            .expect("call site must carry the `callee_output_contract` field");
        assert!(
            coc.is_object(),
            "callee_output_contract is an object when the callee block has \
             one; got {coc}"
        );
        let obj = coc.as_object().unwrap();
        assert_eq!(
            obj.get("kind").and_then(|k| k.as_str()),
            Some("output_contract"),
            "callee_output_contract reuses the same pinned shape as the \
             skill-level field"
        );
        assert_eq!(obj.get("target_name").and_then(|n| n.as_str()), Some("out"),);
        assert_eq!(
            obj.get("ty"),
            Some(&serde_json::json!({ "domain_type": "path" })),
            "ty mirrors the *callee block*'s lowered annotation, not the \
             caller skill's"
        );
        assert_eq!(
            obj.get("source").and_then(|s| s.as_str()),
            Some("synthesized_by_agent"),
        );
    }

    #[test]
    fn inline_block_call_site_carries_callee_output_contract_object() {
        let src = "\
skill drive()
    flow:
        helper()

block helper() -> Path
    flow:
        return <out>
";
        let v = ir_json_after_expand(src);
        let flow = v.pointer("/skill/flow").expect("skill flow array present");
        let call = find_call(flow, "helper");
        assert_eq!(
            call.get("projection_mode").and_then(|m| m.as_str()),
            Some("inline"),
            "single-return helper remains Tier 1 inline"
        );
        let coc = call
            .get("callee_output_contract")
            .expect("call site must carry the `callee_output_contract` field");
        assert!(
            coc.is_object(),
            "inline callee_output_contract is an object when the callee block \
             has one; got {coc}"
        );
        assert_eq!(coc.get("target_name").and_then(|n| n.as_str()), Some("out"));
    }

    // Behavior #9 (planner D5 add-on): an unresolved (Tier 1 / undefined)
    // callee leaves `callee_output_contract: null` — explicit JSON null,
    // not a missing field. Locks the invariant against future tier-resolution
    // shifts. Triggers the inline branch of `serialize_call` because no
    // expand step ran.
    #[test]
    fn tier_one_call_callee_output_contract_is_null() {
        let src = "\
skill drive()
    flow:
        unresolved_target()
";
        let v = ir_json(src);
        let flow = v.pointer("/skill/flow").expect("skill flow array present");
        let call = find_call(flow, "unresolved_target");
        assert_eq!(
            call.get("projection_mode").and_then(|m| m.as_str()),
            Some("inline"),
            "undefined callee stays Tier 1 inline; behavior #9 pins null at \
             this slot"
        );
        assert!(
            call.as_object()
                .unwrap()
                .contains_key("callee_output_contract"),
            "Tier 1 calls must still carry the `callee_output_contract` key \
             (planner D5 — null, not absent)"
        );
        assert_eq!(
            call.get("callee_output_contract"),
            Some(&Value::Null),
            "Tier 1 calls lower `callee_output_contract` to JSON null"
        );
    }

    // Behavior #7: the OutputContract IR node MUST NOT appear inside the
    // `skill.flow` array. The contract is surfaced only as a sibling field
    // (`output_contract`) on the skill object. This isolates `validate_output`
    // (which walks `skill.flow` for role/step counting) from the new node.
    #[test]
    fn output_contract_node_does_not_appear_in_flow_array() {
        let src = "\
skill make_report() -> Report
    flow:
        return <output>
";
        let v = ir_json(src);
        let flow = v.pointer("/skill/flow").expect("skill flow array present");
        for node in flow.as_array().expect("flow is an array") {
            assert_ne!(
                node.get("kind").and_then(|k| k.as_str()),
                Some("output_contract"),
                "output_contract must be a sibling of flow, not a flow entry; \
                 found one in flow: {node}"
            );
        }
    }

    // Behavior #8: idempotency — emitting the same arena twice produces a
    // byte-identical JSON string. Guards against future changes that might
    // introduce hash-iteration order or arena-walk nondeterminism for the
    // new field.
    #[test]
    fn emit_is_byte_identical_across_runs_with_output_contract() {
        let src = "\
skill make_report() -> Report
    flow:
        return <output>
";
        let (file, _) = parse::parse(src, 0).expect("source should parse");
        let arena = lower::lower(&file).expect("source should lower");
        let a = serialize_ir_json(&arena, "test.glyph", false).expect("first emit");
        let b = serialize_ir_json(&arena, "test.glyph", false).expect("second emit");
        assert_eq!(a, b, "two emits of the same arena must be byte-identical");
    }

    // Behavior #5: `validate_output` runs cleanly over an emitted IR JSON
    // that carries an `output_contract` slot. The check exercises the full
    // role-preservation pipeline rather than re-deriving it; it asserts no
    // violation references the new field. Step-counting filters by
    // `kind ∈ {call, inline_instruction, instruction_ref, branch}` inside
    // `flow` and the `output_contract` lives outside that array, so this
    // test is the integration witness for that mechanical separation.
    #[test]
    fn validate_output_does_not_flag_output_contract_field() {
        use crate::validate_output::validate_output;
        let src = "\
skill make_report() -> Report
    flow:
        \"do the thing\"
        return <output>
";
        let (file, _) = parse::parse(src, 0).expect("source should parse");
        let arena = lower::lower(&file).expect("source should lower");
        let ir_json = serialize_ir_json(&arena, "test.glyph", false).expect("emit");
        // Minimal compiled-output md skeleton matching the single-step skill.
        let md = "\
# make_report

## Description

(intentionally empty for this synthetic test)

## Instructions

### Steps

1. Do the thing
";
        let violations = validate_output(&ir_json, md);
        for v in &violations {
            let s = format!("{v:?}");
            assert!(
                !s.contains("output_contract"),
                "no validate_output violation may reference the new \
                 output_contract field; got {s}"
            );
        }
    }

    // Behavior #3a: a built-in `-> String` annotation lowers the contract's
    // `ty` slot to the lowercase JSON string `"string"` per FC8 designer
    // relay (cite: ir-json-schema.md §TypeTag Serialization). Confirms the
    // emit path defers to `opt_typetag_to_json` rather than re-implementing
    // it.
    #[test]
    fn skill_output_contract_ty_emits_lowercase_string_for_builtin() {
        let src = "\
skill stringify() -> String
    flow:
        return <out>
";
        let v = ir_json(src);
        let ty = v
            .pointer("/skill/output_contract/ty")
            .expect("ty field present");
        assert_eq!(ty, &Value::String("string".into()));
    }

    // Behavior #3b: a skill with `return <IDENT>` but no `-> DomainType`
    // header annotation lowers `ty: null`. Chunk 4 lowering pins
    // `IrOutputContract.ty = None` for this case so chunks 8/9 can surface
    // the missing-annotation diagnostic; the JSON path must preserve the
    // `null` (not absent, not silently coerced) so downstream consumers can
    // detect it.
    #[test]
    fn skill_output_contract_with_missing_annotation_emits_null_ty() {
        let src = "\
skill drive()
    flow:
        return <out>
";
        let v = ir_json(src);
        let oc = v
            .pointer("/skill/output_contract")
            .expect("output_contract present (return <IDENT> drives lowering)");
        assert!(
            oc.is_object(),
            "output_contract present even when ty is null"
        );
        assert_eq!(
            oc.get("ty"),
            Some(&Value::Null),
            "missing `-> DomainType` lowers ty to JSON null"
        );
        assert_eq!(
            oc.get("target_name").and_then(|n| n.as_str()),
            Some("out"),
            "target_name still present alongside null ty"
        );
    }

    // Behavior #2: a skill with no `return <IDENT>` must still emit the
    // `output_contract` slot — as JSON `null`, not as a missing field. The
    // pinned slot lets downstream JSON consumers (chunk 6 expand, chunk 13
    // golden test) probe the field without distinguishing "absent" from
    // "explicitly null".
    #[test]
    fn skill_without_output_target_emits_null_slot() {
        let src = "\
skill drive()
    flow:
        \"go\"
";
        let v = ir_json(src);
        let skill = v.get("skill").expect("skill object present");
        assert!(
            skill.as_object().unwrap().contains_key("output_contract"),
            "skill_obj must always carry the `output_contract` key (planner D5 \
             — slot is null, not absent)"
        );
        assert_eq!(
            skill.get("output_contract"),
            Some(&Value::Null),
            "skill without `return <IDENT>` lowers `output_contract` to JSON null"
        );
    }

    // Behavior #1 (tracer): a skill whose flow ends with `return <output>` and
    // whose header is `-> Report` round-trips through `serialize_ir_json` so
    // that `skill.output_contract` is the pinned object
    // `{ node_id, kind: "output_contract", target_name, ty, source }`. This
    // proves the path is wired end-to-end before the variant tests pin
    // individual slots.
    #[test]
    fn skill_output_contract_round_trips_to_pinned_json_shape() {
        let src = "\
skill make_report() -> Report
    flow:
        return <output>
";
        let v = ir_json(src);
        let oc = v.pointer("/skill/output_contract").expect(
            "skill_obj.output_contract must be present when the skill's flow ends with return <IDENT>",
        );
        assert!(
            oc.is_object(),
            "output_contract must serialize as a JSON object; got {oc}"
        );
        let obj = oc.as_object().unwrap();
        assert_eq!(
            obj.get("kind").and_then(|k| k.as_str()),
            Some("output_contract"),
            "kind discriminator must be the snake_case literal \"output_contract\""
        );
        assert_eq!(
            obj.get("target_name").and_then(|n| n.as_str()),
            Some("output"),
            "target_name carries the inner identifier verbatim"
        );
        assert_eq!(
            obj.get("ty"),
            Some(&serde_json::json!({ "domain_type": "report" })),
            "ty mirrors the enclosing skill's lowered `-> DomainType` annotation \
             (FC8 designer relay)"
        );
        assert_eq!(
            obj.get("source").and_then(|s| s.as_str()),
            Some("synthesized_by_agent"),
            "source must be the snake_case literal for OutputSource::SynthesizedByAgent"
        );
        let nid = obj
            .get("node_id")
            .and_then(|n| n.as_str())
            .expect("node_id present and a string");
        assert!(
            nid.starts_with('n') && nid[1..].chars().all(|c| c.is_ascii_digit()),
            "node_id follows the `n<integer>` convention; got {nid:?}"
        );
    }

    /// Issue #86 chunk 2 follow-up: lock the JSON shape for both
    /// `OutputTargetForm` arms by calling `serialize_output_contract`
    /// directly. The description arm is unreachable via `parse → lower`
    /// until Task 3 wires up the `<"…">` parser, so this is the only
    /// guard against the description shape regressing in the meantime.
    #[test]
    fn serialize_output_contract_shape_for_both_forms() {
        use crate::ir::{IrOutputContract, NodeId, OutputSource, OutputTargetForm};

        // --- description arm ---
        let desc_oc = IrOutputContract {
            node_id: NodeId(7),
            form: OutputTargetForm::Description("root cause analysis".into()),
            ty: None,
            source: OutputSource::SynthesizedByAgent,
        };
        let desc_json = serialize_output_contract(&desc_oc);
        let desc_obj = desc_json
            .as_object()
            .expect("description arm serializes to a JSON object");
        assert_eq!(
            desc_obj.get("form").and_then(|v| v.as_str()),
            Some("description"),
            "description arm carries form discriminator \"description\""
        );
        assert_eq!(
            desc_obj.get("description").and_then(|v| v.as_str()),
            Some("root cause analysis"),
            "description arm carries the decoded description text"
        );
        assert!(
            !desc_obj.contains_key("target_name"),
            "description arm must NOT carry `target_name`; the two arms are \
             mutually exclusive in JSON shape"
        );
        assert_eq!(
            desc_obj.get("kind").and_then(|v| v.as_str()),
            Some("output_contract"),
        );
        assert_eq!(desc_obj.get("node_id").and_then(|v| v.as_str()), Some("n7"),);
        assert_eq!(desc_obj.get("ty"), Some(&Value::Null));
        assert_eq!(
            desc_obj.get("source").and_then(|v| v.as_str()),
            Some("synthesized_by_agent"),
        );

        // --- identifier arm (sibling assertion to lock both shapes) ---
        let id_oc = IrOutputContract {
            node_id: NodeId(3),
            form: OutputTargetForm::Identifier("current_branch".into()),
            ty: None,
            source: OutputSource::SynthesizedByAgent,
        };
        let id_json = serialize_output_contract(&id_oc);
        let id_obj = id_json
            .as_object()
            .expect("identifier arm serializes to a JSON object");
        assert_eq!(
            id_obj.get("form").and_then(|v| v.as_str()),
            Some("identifier"),
            "identifier arm carries form discriminator \"identifier\""
        );
        assert_eq!(
            id_obj.get("target_name").and_then(|v| v.as_str()),
            Some("current_branch"),
            "identifier arm carries the inner identifier verbatim"
        );
        assert!(
            !id_obj.contains_key("description"),
            "identifier arm must NOT carry `description`; the two arms are \
             mutually exclusive in JSON shape"
        );
        assert_eq!(
            id_obj.get("kind").and_then(|v| v.as_str()),
            Some("output_contract"),
        );
        assert_eq!(id_obj.get("node_id").and_then(|v| v.as_str()), Some("n3"),);
        assert_eq!(id_obj.get("ty"), Some(&Value::Null));
        assert_eq!(
            id_obj.get("source").and_then(|v| v.as_str()),
            Some("synthesized_by_agent"),
        );
    }
}
