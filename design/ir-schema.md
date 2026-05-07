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
  return_type:       TypeTag?              // optional per `language-surface.md` §3.1; when present, annotates the IR's `OutputContract` and the `return` expression folds into the final Step prose
  effects:           [EffectKeyword]       // full inferred set (union of all callees)
  context:           [ContextNode]         // top-level declared context only
  constraints:       [Constraint]          // top-level declared constraints only
  flow:              [FlowNode]            // ordered
  output_contract:   OutputContract?       // present when flow ends with `return <name>` or `return <"description">`
}

Block {
  name:              String
  description:       String?               // present iff `BLOCKNAME.applies()` is consulted somewhere reachable; see `ir-and-semantics.md` §Block Trigger Predicate
  params:            [Param]
  return_type:       TypeTag?
  effects:           [EffectKeyword]
  context:           [ContextNode]         // top-level declared context only
  constraints:       [Constraint]
  flow:              [FlowNode]
  output_contract:   OutputContract?       // present when flow ends with `return <name>` or `return <"description">`
}

ExportBlock {
  name:              String
  description:       String?               // present iff `BLOCKNAME.applies()` is consulted somewhere reachable; see `ir-and-semantics.md` §Block Trigger Predicate
  params:            [Param]
  return_type:       TypeTag?              // present when the export block has a meaningful return; absent when it omits `->` entirely (`types.md` §Return Type Requirements / Issue-82, `language-surface.md` §3.3). When present, it is part of the public contract callers see; absence means "no meaningful return" — there is no `-> None` representation post-#82.
  effects:           [EffectKeyword]       // declared must be superset of inferred
  context:           [ContextNode]         // top-level declared context only
  constraints:       [Constraint]
  flow:              [FlowNode]
  output_contract:   OutputContract?       // present when flow ends with `return <name>` or `return <"description">`
}
```

**Derived field on `ExportBlock` (post-Phase-6-Step-1, in-memory only).** After a library file's Phase 6 Step 1 runs, each `ExportBlock` node additionally carries a `resolved_word_count: Int` field — the word count of the export block's resolved expanded prose, computed once during the library's own compilation. When a downstream skill compiles, its Phase 6 Step 1 reads this derived field from the imported `ExportBlock` to make the per-call-site projection-tier decision (inline vs. same-file procedure vs. external file). The field propagates via the import-resolution mechanism only; it is **not** part of the JSON serialization defined in `ir-json-schema.md` and does not appear in `--emit-ir` output. It is an implementation detail of in-memory IR nodes during a single multi-file build, not part of the public IR contract. See `pipeline.md` §Multi-File Compilation Order and `compiled-output.md` §Three-Tier Block Projection.

**Const declarations: erase-and-inline (no IR node).** `const`, `export const`, and `generated const` declarations from `language-surface.md` §3.4 / §3.6 are **not** a top-level `IrNode` kind. They have no entry in this section and no presence in the post-Lower IR as their own nodes. The `Const`-shaped row absent from the table above is deliberate, not an omission; the schema's "Completeness caveat" allows future extensions but const decls do not require one — they erase at the lowering boundary and surface only as inlined values at reference sites:

- **As a parameter default** (`Param.default: Value?` — see §Parameters below) the const's literal is inlined into the `Value` union (`StringLit`, `IntLit`, `FloatLit`, `BoolLit`, `NoneLit` — see §Enums). The literal kind lives in the `Value` variant; the matching `TypeTag` lives on the sibling `Param.type` field when the parameter is annotated.
- **As a bare-name reference in `flow:` / `constraints:` / `context:`** the const's resolved string content becomes the `resolved_text: String` of an `InstructionRef` (§Flow Nodes) or the `text: String` of a hoisted `Constraint` / `ContextNode`. No `TypeTag` accompanies these — const-as-instruction is always string-typed.

Primitive `TypeTag` is **inferred at the lowering boundary** from the const's RHS literal (string / int / float / bool — see §Enums for the full primitive set). The inference is internal to Lower; the inferred tag flows out only via the `Value` variant chosen for inlining and via `Param.type` when the const is bound to a parameter. There is no "const-decl carries its inferred TypeTag" channel because there is no const decl in the IR to carry it.

Cross-refs:
- `pipeline.md` §Phase 6 (Expand Step 1) — bare-name inlining for `const` / `generated const` references.
- `compiled-output.md` §Authoring Constructs Compile Away — the user-facing erasure contract; `const` declarations themselves emit nothing to compiled Markdown, only their inlined content surfaces at reference sites.
- `ir-json-schema.md` §Node kinds in the JSON — the JSON schema also has no `const_decl` kind, by the same erase-and-inline contract.

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
FlowNode = Call | InlineInstruction | InstructionRef | Branch | Return | Constraint | ContextNode
```

A `Constraint` is admissible as a flow node so authors can write `require`/`avoid`/`must` markers directly inside `flow:` (including inside `if`/`elif`/`else` bodies). Lower (`pipeline.md` Phase 4) post-processes flow-resident constraints:

- A `Constraint` at flow top-level is **hoisted** out of the flow into the enclosing declaration's `constraints` list (deduplicated against existing entries by canonical text+polarity+strength).
- A `Constraint` inside a `Branch` body (`then_body`, `elif_branches[*].body`, or `else_body`) **stays inline** in that branch and is rendered as part of the conditional Step prose by Expand. It does not surface in `### Constraints`. See `compiled-output.md` §Constraint Rendering and `ir-and-semantics.md` §Body-Level Constraint Normalization.

A `ContextNode` is admissible as a flow node so authors can write `context` markers directly inside `flow:` (including inside `if`/`elif`/`else` bodies). Lower (`pipeline.md` Phase 4) post-processes flow-resident context nodes:

- A `ContextNode` at flow top-level is **hoisted** out of the flow into the enclosing declaration's `context` list.
- A `ContextNode` inside a `Branch` body (`then_body`, `elif_branches[*].body`, or `else_body`) **stays inline** in that branch and is rendered as part of the conditional Step prose by Expand.

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
  resolved_text:     String                // content of the referenced const/generated const
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
  value:             Expr | OutputTargetForm // call, binding ref, literal, dot access, none, `<name>`, or `<"description">`
}
```

### OutputContract

```
OutputContract {
  form:              OutputTargetForm      // identifier form (`<name>`) or descriptive form (`<"…">`)
  ty:                TypeTag?              // enclosing declaration's `-> DomainType`, if any
  source:            OutputSource          // currently SynthesizedByAgent
}

OutputTargetForm = Identifier(name: String) | Description(text: String)
// Identifier(name) corresponds to `return <name>` — `name` is the bare identifier inside the angle
// brackets, stored in canonical form per `values-and-names.md` §Case Normalization.
// Description(text) corresponds to `return <"…">` — `text` is the verbatim string content inside the
// brackets, with inline-string escapes resolved (`\"` and `\\` per `values-and-names.md` §Inline Strings).
// Descriptive form is terminal-return-only in MVP; mid-flow output targets, if added later, must use the
// identifier form. See `values-and-names.md` §No Value-Level Operators and `data-flow.md` §Return Semantics.
```

`OutputContract` is a sidecar contract for agent-synthesized output. It does not appear as an ordered `FlowNode`; it annotates the enclosing `Skill`, `Block`, or `ExportBlock` and folds into the final Step prose during Expand. The `form` discriminates which Expand folding rule applies (see `compiled-output.md` §Return Folding and `expand.md` §3.3).

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

## ContextNode

```
ContextNode {
  text:              String                // resolved context text
  name:              Option<String>        // source name when entry was a NameRef; None for InlineString
}
```

`name` carries the **source identifier** of the referenced `const` / `export const`
verbatim (e.g. `project_overview` as written in the source), for any
`context: <NameRef>` entry. Inline string entries (`context: "literal"`) leave it
absent. Kebab-case is an Emit-time rendering transform applied to this identifier
when producing the per-entry `- **kebab-name**` lead-in in `### Context` — it is
not stored in the IR. Downstream tooling that wants a stable handle should consume
this field as the source identifier.

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
Role = InputContract | Step | Constraint | Context | OutputContract

Strength = soft | hard

Polarity = require | avoid

EffectKeyword = none | reads_files | reads_env | writes_files
             | runs_commands | uses_network | asks_user
             | creates_artifacts | spawns_agent

ProjectionMode = inline | same_file_procedure | external_file

OutputSource = SynthesizedByAgent

TypeTag = String | Int | Float | Bool | None | Agent
        | DomainType(name: String)
// DomainType covers author-defined opaque type names (RepoContext, Plan, etc.).
// The `name` is stored in canonical form per `values-and-names.md` §Case Normalization;
// nominal matching at call boundaries is canonical-name string equality.

Value = StringLit(content: String)
      | IntLit(value: Int)
      | FloatLit(value: Float)
      | BoolLit(value: Bool)
      | NoneLit
```

## Node Identifiers

Phase 4 (Lower) assigns every IR node a **stable, file-local identifier** used for Phase 6b structural validation, Phase 5 uniqueness checks, and diagnostic messages. This section is the canonical spec; `pipeline.md` §Phase 4, `expand.md` §3.1, and `ir-json-schema.md` §Node ID Convention reference it.

### Format

`n<u32>` — lowercase `n` followed by a non-negative decimal integer with no leading zeros (except `n0`). Examples: `n0`, `n1`, `n27`. The underlying Rust type is `NodeId(pub u32)` (see `build-foundation.md` §A4). Maximum value is `4,294,967,295`; bounded in practice by node count per file (tens to low thousands).

**JSON serialization:** Always a JSON string (`"n0"`, `"n1"`), never a bare integer. See `ir-json-schema.md` for the full JSON contract.

### Allocation

Lower assigns IDs in **pre-order source traversal**: container nodes (`Skill`, `Block`, `ExportBlock`, `Branch`) receive an ID before their children, and children are visited in source order. The counter is monotonically increasing per file, starting at `n0`.

**What counts as a node:** Every `IrNode` enum variant receives an ID — uniformly, with no exceptions. Concretely:

- Every top-level compilation unit (`Skill`, `Block`, `ExportBlock`).
- Every `Param` on a declaration.
- Every node in the `FlowNode` union (`Call`, `InlineInstruction`, `InstructionRef`, `Branch`, `Return`, `Constraint`, `ContextNode`).
- Every `ElifBranch` inside a `Branch`.
- Every `Constraint` in a declaration's `constraints` list or a Call's `scoped_constraints`.
- Every `ContextNode` in a declaration's `context` list or inside a `Branch` body.
- Every `Expr` sub-node (`CallExpr`, `BindingRef`, `Literal`, `PropertyAccess`, `NoneExpr`) — including those nested inside Call `args` maps and Return `value`.

### Scope

IDs are **per-file**. Each file's counter starts at `n0`. No global uniqueness across a project. Cross-file node references do not arise in the MVP pipeline — importers interact with the dependency's validated declarative contract (parameters, types, effects), not with individual IR node IDs. `InstructionRef` nodes that reference a `const` declaration in another file use the declaration's **name**, not a remote node ID — the reference is resolved by name during Analyze, and the resolved content is inlined by Expand Step 1.

If a future multi-file IR view requires cross-file addressing, the scheme is `(file_path, node_id)`.

### Synthetic Nodes

Nodes introduced by Lower (compiler-generated temporary bindings from nested-call desugaring, default-value fills, implicit `Return`) share the `n` prefix. No distinct prefix for synthetic nodes. The author-vs-compiler distinction is tracked in node provenance metadata, not in the ID. A separate prefix would add complexity without benefit — Phase 6b and diagnostics do not need to distinguish synthetic from authored by ID alone.

### Stability

**Identical source → identical IDs.** If the post-repair `.glyph` source is **byte-identical**, Lower produces **identical IDs** — same AST structure, same traversal, same monotonic assignment. This is the guarantee the cache key relies on (`pipeline.md` §Cacheability). Whitespace-only changes that do not alter AST structure also produce identical IDs.

**Edits invalidate all IDs.** Changes that alter the AST (added, removed, or reordered nodes) reassign IDs from scratch. Inserting a new `step` mid-skill shifts all downstream IDs. Consumers must not cache or persist node IDs across source edits.

### Behavior Under Repair

IDs are assigned from scratch on post-repair source. Phases 1–3 (Parse, Analyze, Repair) run before Lower, so pre-Lower diagnostics do not reference IR node IDs. Phase 3c (constraint conflict scan) uses **declaration-local constraint indices** from the annotated AST (`c0`, `c1`, …), not IR node IDs — see `repair.md` §4.10.

### Collision

Collisions cannot arise within a file: monotonic allocation from a single counter is injective by construction. Phase 5 (Validate) confirms this with `G::validate::duplicate-node-id` as a defense-in-depth check.

### External Visibility

Node IDs appear in:

- **IR JSON** (`ir-json-schema.md`) — every node in the `--emit-ir` JSON output carries its `node_id` as a string attribute.
- **Compiler diagnostics** — Phase 5 (Validate) and Phase 6b (`glyph validate-output`) errors name the offending node by ID.
- **Phase 6b retry feedback** — the LLM receives node IDs in violation reports so it can target fixes (`expand.md` §5.3).

Node IDs **never** appear in compiled `.md` output. They are internal to the compiler and the agent workflow. They are stable within a build for a given source, so CI logs that quote an ID remain meaningful for the duration of that build, but they are not guaranteed stable across builds with different source content.

## Resolved IR (Post-Step 1)

After Expand Step 1 (deterministic resolution), every node carries resolved content. The schema is the same as above with one addition per node that holds text content:

```
ResolvedCall {
  ...Call fields...
  resolved_body_text:  String              // callee body with {param} slots preserved as literal
                                           // {name} and {local} slots preserved as literal {name};
                                           // readers cross-reference local_refs to identify which
                                           // {name} tokens are local bindings vs. parameters
  local_refs:          [LocalRef]          // one entry per {local} slot in resolved_body_text;
                                           // empty when the body has no local-binding references
  projection_mode:     ProjectionMode      // inline | same_file_procedure | external_file
  callee_flow:         [ResolvedFlowNode]? // present only when projection_mode != inline
  callee_context:      [ContextNode]?      // present only when projection_mode != inline
  callee_constraints:  [Constraint]?       // present only when projection_mode != inline
  procedure_path:      String?             // relative file path; present only when external_file
}

LocalRef {
  name:                String              // the local binding name (matches {name} in the text)
  node_id:             String              // the producing node's IR ID (e.g. "n7")
}

ResolvedConstraint {
  ...Constraint fields...
  // text field already contains resolved content; {param} slots preserved,
  // {local} slots tagged as local_ref
}

ResolvedContextNode {
  ...ContextNode fields...
  // text field already contains resolved content; {param} slots preserved,
  // {local} slots tagged as local_ref
}

// Name slot tagging (applies to resolved_body_text and constraint text):
// A {name} slot in the resolved text is classified by Step 1 as either:
//   - param_ref: name matches a declared parameter → preserved as literal {name}
//     in compiled output for the consuming LLM to fill at runtime.
//     Not listed in local_refs.
//   - local_ref: name matches a local binding (e.g., from x = call(...)) →
//     listed in the local_refs array on the enclosing ResolvedCall.
//     The {name} token stays literal in resolved_body_text; the local_refs
//     entry carries the producing node's ID. Step 2 must resolve every
//     local_ref into natural-language prose; none may survive in output.

ResolvedInstructionRef {
  ...InstructionRef fields...
  // resolved_text already contains resolved content
}

ResolvedBranch {
  ...Branch fields...
  applies_descriptions: {String: String}?  // present when any condition (top-level, elif) uses `BLOCKNAME.applies()`;
                                           // map key is the block name as it appears in source, value is the
                                           // resolved `description:` string of that block. See
                                           // `ir-and-semantics.md` §Block Trigger Predicate. Step 2 reads this
                                           // side-map to render description-driven prose; Step 1 populates it.
}
```

Step 2 (LLM reshaping) receives the resolved IR. See `expand.md` §3.1 for the full input contract.

## Cross-References

- **IR JSON serialization** (`ir-json-schema.md`): the JSON projection of this schema, used by `--emit-ir` and `validate-output`. Specifies envelope, per-node JSON shapes, enum casing, versioning.
- **IR roles and semantics** (`ir-and-semantics.md`): defines the five MVP roles, constraint model, effect vocabulary, and section-to-IR mapping. This schema is the structural companion to that document.
- **Pipeline** (`pipeline.md` §Phase 4): Lower produces nodes conforming to this schema.
- **Expand** (`expand.md` §3.1): Step 2 input contract references node fields defined here.
- **Data flow** (`data-flow.md` §IR Call-Node Normalization): the `Call` normalization described there must produce nodes matching the `Call` shape above.
- **Compiled output** (`compiled-output.md`): projection rules map from this schema to the compiled Markdown sections.
- **Agent skill** (`agent-skill.md`): the agent reads the JSON-serialized IR during Step 2 and feeds it to `validate-output` for Phase 6b checks.
