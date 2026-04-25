# Glyph IR Node Schema

This document is the single canonical reference for the shape of every IR node type the MVP compiler produces. All other documents that reference IR node fields (`expand.md` §3.1, `pipeline.md` §Phase 4, `data-flow.md` §IR Call-Node Normalization, `compiled-output.md`) defer to this schema.

The schema is expressed as structured pseudocode. Each node type lists every field, its type, and whether it is optional (`?` suffix). Enum types are defined at the end.

**Completeness caveat:** This schema covers all IR node types identified across the current design documents. Implementation may reveal additional fields or node types not yet anticipated — implementers should extend this schema following the same conventions and update this document accordingly.

## Top-Level Compilation Units

```
Skill {
  name:              String
  description:       String                // always present after Repair
  params:            [Param]
  effects:           [EffectKeyword]       // full inferred set (union of all callees)
  constraints:       [Constraint]          // top-level declared constraints only
  flow:              [FlowNode]            // ordered
}

Block {
  name:              String
  params:            [Param]
  return_type:       TypeTag?
  effects:           [EffectKeyword]
  constraints:       [Constraint]
  flow:              [FlowNode]
}

ExportBlock {
  name:              String
  params:            [Param]
  return_type:       TypeTag               // mandatory on export block
  effects:           [EffectKeyword]       // declared must be superset of inferred
  constraints:       [Constraint]
  flow:              [FlowNode]
}
```

## Parameters

```
Param {
  name:              String
  type:              TypeTag?              // omitted when duck-typed
  default:           Value?                // omitted when required
}
```

## Flow Nodes

`FlowNode` is the union of all node types that can appear inside `flow:`.

```
FlowNode = Call | InlineInstruction | InstructionRef | Branch | Return
```

### Call

```
Call {
  target:            String                // resolved declaration name (or qualified name)
  args:              {String: Expr}        // named args only (positional resolved in Lower)
  output:            String?               // binding name, if `x = call(...)`
  return_type:       TypeTag?              // resolved from callee declaration
  effects:           [EffectKeyword]       // inferred from callee
  site_modifier:     String?               // `with` modifier text, if present
  role:              Role
  scoped_constraints: [Constraint]         // callee's constraints, scoped to this call
}
```

### InlineInstruction

```
InlineInstruction {
  text:              String                // the literal string content
  role:              Role                  // typically Step
}
```

### InstructionRef

```
InstructionRef {
  name:              String                // resolved name
  resolved_text:     String                // content of the referenced text/generated text
  role:              Role
  constraint_attrs:  ConstraintAttrs?      // present only when role is Constraint
}
```

### Branch

```
Branch {
  condition:         String                // condition expression as text
  then_body:         [FlowNode]
  elif_branches:     [ElifBranch]
  else_body:         [FlowNode]?           // omitted when no else clause
}

ElifBranch {
  condition:         String
  body:              [FlowNode]
}
```

`Branch` is a container node. It does not carry an instruction role itself — its children carry their own roles.

### Return

```
Return {
  value:             Expr                  // call, binding ref, literal, dot access, or none
}
```

## Constraint

```
Constraint {
  text:              String                // resolved constraint text
  strength:          Strength              // soft | hard
  polarity:          Polarity              // require | avoid
}

ConstraintAttrs {
  strength:          Strength
  polarity:          Polarity
}
```

## Expressions

`Expr` is the union of value expressions that can appear in call arguments, bindings, return values, and conditions.

```
Expr = CallExpr | BindingRef | Literal | PropertyAccess | NoneExpr

CallExpr {
  target:            String
  args:              {String: Expr}
}

BindingRef {
  name:              String
}

Literal {
  value:             Value
}

PropertyAccess {
  object:            String                // binding or parameter name
  property:          String                // single-level only in MVP
}

NoneExpr {}
```

## Enums

```
Role = InputContract | Step | Constraint | OutputContract

Strength = soft | hard

Polarity = require | avoid

EffectKeyword = none | reads_files | reads_env | writes_files
             | runs_commands | uses_network | asks_user
             | creates_artifacts | spawns_agent

TypeTag = String | Int | Float | Bool | None | Agent
        | DomainType(name: String)
// DomainType covers author-defined opaque type names (RepoContext, Plan, etc.)

Value = StringLit(content: String)
      | IntLit(value: Int)
      | FloatLit(value: Float)
      | BoolLit(value: Bool)
      | NoneLit
```

## Resolved IR (Post-Step 1)

After Expand Step 1 (deterministic resolution), every node carries resolved content. The schema is the same as above with one addition per node that holds text content:

```
ResolvedCall {
  ...Call fields...
  resolved_body_text:  String              // callee body with {param} slots preserved
}

ResolvedConstraint {
  ...Constraint fields...
  // text field already contains resolved content; {param} slots preserved
}

ResolvedInstructionRef {
  ...InstructionRef fields...
  // resolved_text already contains resolved content
}
```

Step 2 (LLM reshaping) receives the resolved IR. See `expand.md` §3.1 for the full input contract.

## Cross-References

- **IR roles and semantics** (`ir-and-semantics.md`): defines the four MVP roles, constraint model, effect vocabulary, and section-to-IR mapping. This schema is the structural companion to that document.
- **Pipeline** (`pipeline.md` §Phase 4): Lower produces nodes conforming to this schema.
- **Expand** (`expand.md` §3.1): Step 2 input contract references node fields defined here.
- **Data flow** (`data-flow.md` §IR Call-Node Normalization): the `Call` normalization described there must produce nodes matching the `Call` shape above.
- **Compiled output** (`compiled-output.md`): projection rules map from this schema to the compiled Markdown sections.
