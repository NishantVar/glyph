# Glyph IR JSON Schema

This document is the canonical reference for the JSON serialization of the Glyph IR produced by `glyph compile --emit-ir` and consumed by `glyph validate-output`. It specifies the top-level envelope, per-node-kind JSON shapes, enum serialization, versioning, and stability policy.

The JSON represents the **post-Step-1 resolved IR** — the IR after Expand Step 1 (deterministic resolution). Bare names are inlined, projection tiers are assigned, parameter slots are preserved as `{param}` references, and `with` modifiers are attached to Call nodes. This is the IR the agent reads to perform Step 2 (LLM reshaping).

**Bidirectional role.** This schema is the contract for two compiler subcommands:

- **`glyph compile --emit-ir`** produces `foo.ir.json` — the post-Step-1 resolved IR.
- **`glyph validate-output`** consumes `foo.ir.json` + `foo.md` and cross-references the Markdown structure against the IR to enforce Phase 6b checks.

Both subcommands must agree on this schema. A change to the IR JSON shape requires updating both producers and consumers.

**Scope.** `--emit-ir` operates on **skill files only** (files containing exactly one `skill` declaration). Library files (zero `skill` declarations) do not produce `.ir.json` output — the agent never runs Step 2 on a library file, and `validate-output` has no use for library IR. If a future visualizer needs library IR, a separate flag or subcommand can be added.

**Node kinds in the JSON.** The JSON output contains `Skill` as the root and flow-level nodes (`Call`, `InlineInstruction`, `InstructionRef`, `Branch`, `Return`, `Constraint`, `ContextNode`) plus `OutputContract`, `Param`, `ElifBranch`, and `Expr` sub-nodes. `OutputContract` is serialized as a sidecar object on `skill.output_contract` or `call.callee_output_contract`, never as a member of a `flow` array. `Block` and `ExportBlock` compilation units from `ir-schema.md` do **not** appear as separate nodes in the JSON — their content is inlined into `Call` nodes via the resolved fields (`resolved_body_text`, `callee_flow`, `callee_context`, `callee_constraints`, `callee_output_contract`). The JSON `"call"` kind represents a **resolved call** (post-Step-1), carrying both the base `Call` fields from `ir-schema.md` and the resolved fields from `ir-schema.md` §Resolved IR (`ResolvedCall`).

**Const declarations have no JSON kind.** `const`, `export const`, and `generated const` declarations (`language-surface.md` §3.4 / §3.6) do **not** serialize as their own JSON nodes. There is no `"const"` or `"const_decl"` kind in `--emit-ir` output, by design — this absence mirrors the IR schema's erase-and-inline contract for const decls (`ir-schema.md` §Top-Level Compilation Units). Const-derived values surface in the JSON only via the inlined sites:

- **`Param.default`** (see §Param) — when a const is bound as a parameter default, the resolved literal appears in the Value-union shape (`{"kind": "string|int|float|bool|none", "value": ...}`).
- **`InstructionRef.resolved_text`** (see §InstructionRef (resolved)) — when a const is referenced bare-name in `flow:` / `constraints:` / `context:`, its string content is inlined into `resolved_text`. No `TypeTag` field accompanies it, since const-as-instruction is always string-typed.

The matching `TypeTag` for primitive consts is inferred at the lowering boundary and flows into the JSON via the `Value` variant chosen and via `Param.type` when applicable; it is never serialized as a free-standing const-decl attribute. The library-files-emit-no-IR-JSON rule (above, in §Scope) is independent — it applies to whole files regardless of declaration kind.

## Top-Level Envelope

```json
{
  "ir_version": 1,
  "compiler": "glyph 0.1.0",
  "source_file": "fix_bug.glyph.md",
  "skill": { ... }
}
```

| Field | Type | Description |
|---|---|---|
| `ir_version` | integer | Monotonic schema version. Starts at `1`. Bumps on any breaking shape change (field removal, rename, type change). Adding new fields does not bump this. |
| `compiler` | string | Freeform compiler identifier for debugging. Format: `"glyph <semver>"`. Not parsed by consumers — human-readable only. |
| `source_file` | string | Relative path to the `.glyph.md` source file that produced this IR. |
| `skill` | object | The root `Skill` node (see §Skill below). Always exactly one. |

**The envelope is per-skill.** A `.glyph.md` source file produces at most one `foo.ir.json` file, rooted in its single `Skill` node (recall: MVP allows exactly one `skill` per file — `G::parse::multiple-skills`). **Library files (zero `skill` declarations) produce no IR JSON output.** A library has no Skill to root the envelope on, so `--emit-ir` is a silent no-op for IR on libraries: no `foo.ir.json` is written. The CLI's stdout NDJSON wrapper still emits a normal `{"file": ..., "diagnostics": [], "emitted": [...]}` line for the library file, listing any procedure `.md` artifacts produced (per `cli.md` §Diagnostic Output). Library IR caching is deferred until incremental compilation exists; until then, emitting library IR would be dead bytes with no consumer. See `cli.md` §IR JSON Output for the corresponding CLI contract.

**Agent behavior on `ir_version`:** If `ir_version > KNOWN_MAX`, warn and attempt to proceed (ignore unknown fields). If `ir_version` introduces a shape the agent cannot parse, hard fail with a clear message naming the version mismatch.

## Node ID Convention

Every IR node carries a `"node_id"` field — a string of the form `"n<integer>"` (e.g., `"n0"`, `"n1"`, `"n27"`). IDs are allocated in pre-order source traversal by Phase 4 (Lower), starting at `n0` per file. Every node kind, including `Param` and `Expr` sub-nodes, receives an ID.

The canonical spec for node ID format, allocation, scope, stability, and collision semantics lives in `ir-schema.md` §Node Identifiers. This document specifies only the JSON serialization: the ID is always a JSON string, never a bare integer.

## Node Types

### Skill

```json
{
  "node_id": "n0",
  "kind": "skill",
  "name": "fix_bug",
  "description": "Debug and fix a bug with minimal changes.",
  "params": [ ... ],
  "return_type": null,
  "effects": ["reads_files", "writes_files"],
  "context": [ ... ],
  "constraints": [ ... ],
  "flow": [ ... ],
  "output_contract": null
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `node_id` | string | yes | |
| `kind` | string | yes | Always `"skill"`. |
| `name` | string | yes | Skill declaration name. |
| `description` | string | yes | Always present after Repair. |
| `params` | array of Param | yes | May be empty. |
| `return_type` | TypeTag or null | yes | Resolved from the `skill <name>(...) -> <ReturnType>` declaration header (`language-surface.md` §3.1). Skill return types are optional in source; when omitted, this field is `null`. The annotation folds into the final Step prose during Expand and does not surface as a separate JSON section. |
| `effects` | array of string | yes | Inferred effect set. Empty array when `none`. |
| `context` | array of ContextNode | yes | Top-level declared context entries. May be empty. |
| `constraints` | array of Constraint | yes | Top-level declared constraints only. May be empty. |
| `flow` | array of FlowNode | yes | Ordered flow nodes. |
| `output_contract` | OutputContract or null | yes | Output target contract for `return <name>` or `return <"description">`, or `null` when the skill has no output-target return. This field is a sibling of `flow`, not a flow entry, so it does not affect step counts. |

### Param

```json
{
  "node_id": "n1",
  "kind": "param",
  "name": "scope",
  "type": "string",
  "default": { "kind": "string", "value": "." }
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `node_id` | string | yes | |
| `kind` | string | yes | Always `"param"`. |
| `name` | string | yes | Parameter name. |
| `type` | TypeTag | no | Omitted when duck-typed. |
| `default` | Value | no | Omitted when required (no default). |

### Call (resolved)

```json
{
  "node_id": "n3",
  "kind": "call",
  "target": "inspect_failure",
  "args": {
    "area": { "node_id": "n4", "kind": "binding_ref", "name": "scope" }
  },
  "output": null,
  "return_type": null,
  "effects": ["reads_files"],
  "site_modifier": "focus on auth boundaries",
  "role": "step",
  "scoped_constraints": [],
  "resolved_body_text": "Inspect the failure in {scope} and identify what is failing.",
  "local_refs": [],
  "projection_mode": "inline",
  "callee_flow": null,
  "callee_context": null,
  "callee_constraints": null,
  "callee_output_contract": null,
  "procedure_path": null
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `node_id` | string | yes | |
| `kind` | string | yes | Always `"call"`. |
| `target` | string | yes | Resolved declaration name (or qualified name). |
| `args` | object | yes | Map of parameter name to Expression. Values carry `node_id`. |
| `output` | string or null | yes | Binding name if `x = call(...)`, else `null`. |
| `return_type` | TypeTag or null | yes | Resolved from callee, or `null`. |
| `effects` | array of string | yes | Inferred from callee. |
| `site_modifier` | string or null | yes | `with` modifier text, or `null`. |
| `role` | string | yes | Role enum value. |
| `scoped_constraints` | array of Constraint | yes | Callee's constraints scoped to this call. May be empty. |
| `resolved_body_text` | string | yes | Callee body with `{param}` and `{local}` slots preserved as literal `{name}` tokens. Readers cross-reference `local_refs` to distinguish local-binding slots from parameter slots. Post-Step-1. |
| `local_refs` | array of LocalRef | yes | One entry per local-binding `{name}` slot in `resolved_body_text`. Empty array when the body has no local-binding references. Each entry: `{ "name": "<binding>", "node_id": "<producer>" }`. Step 2 must resolve every entry into natural-language prose; Phase 6b checks via `G::expand::unresolved-local-ref`. |
| `projection_mode` | string | yes | `"inline"`, `"same_file_procedure"`, or `"external_file"`. |
| `callee_flow` | array of FlowNode or null | yes | Present only when `projection_mode != "inline"`. |
| `callee_context` | array of ContextNode or null | yes | Present only when `projection_mode != "inline"`. |
| `callee_constraints` | array of Constraint or null | yes | Present only when `projection_mode != "inline"`. |
| `callee_output_contract` | OutputContract or null | yes | Callee block's output target contract when the resolved callee has `return <name>` or `return <"description">`, otherwise `null`. Present even for inline projections so Phase 6b can detect literal `<name>` or `<"…">` leaks (`G::expand::output-target-leak` covers both forms). |
| `procedure_path` | string or null | yes | Relative file path. Present only when `projection_mode == "external_file"`. |

**Worked example — Call with `local_refs`:**

Given source `diagnosis = analyze_error(scope)` followed by a call whose body references `{diagnosis}`:

```json
{
  "node_id": "n8",
  "kind": "call",
  "target": "propose_fix",
  "args": {
    "scope": { "node_id": "n9", "kind": "binding_ref", "name": "scope" }
  },
  "output": null,
  "return_type": null,
  "effects": [],
  "site_modifier": null,
  "role": "step",
  "scoped_constraints": [],
  "resolved_body_text": "Propose a fix based on {diagnosis} within {scope}.",
  "local_refs": [
    { "name": "diagnosis", "node_id": "n7" }
  ],
  "projection_mode": "inline",
  "callee_flow": null,
  "callee_context": null,
  "callee_constraints": null,
  "callee_output_contract": null,
  "procedure_path": null
}
```

Here `{scope}` is a parameter slot (not in `local_refs`) and `{diagnosis}` is a local-binding slot (listed in `local_refs` with the producing node `n7`). Step 2 must resolve `{diagnosis}` into prose; `{scope}` passes through to compiled output.

### OutputContract

`OutputContract` JSON has two shapes discriminated by the `form` field — one for each `OutputTargetForm` variant defined in `ir-schema.md` §OutputContract. Both shapes carry the `form` discriminator alongside the form-specific value field; the discriminator is always present, never elided. Identifier form emits both `form` and `target_name`; descriptive form emits both `form` and `description`. The "absent otherwise" rule on each value field below applies cross-form (i.e., `target_name` is absent in descriptive payloads, `description` is absent in identifier payloads), not within a single form.

**Identifier form** (`return <name>`):

```json
{
  "node_id": "n9",
  "kind": "output_contract",
  "form": "identifier",
  "target_name": "current_branch",
  "ty": { "domain_type": "branchname" },
  "source": "synthesized_by_agent"
}
```

**Descriptive form** (`return <"…">`):

```json
{
  "node_id": "n9",
  "kind": "output_contract",
  "form": "description",
  "description": "root cause analysis including affected files and severity",
  "ty": { "domain_type": "diagnosis" },
  "source": "synthesized_by_agent"
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `node_id` | string | yes | |
| `kind` | string | yes | Always `"output_contract"`. |
| `form` | string | yes | Discriminator: `"identifier"` or `"description"`. Selects which of `target_name` / `description` is present. |
| `target_name` | string | conditional | Identifier from `return <name>`, without angle brackets. **Required when `form == "identifier"`; absent otherwise.** Stored in canonical form per `values-and-names.md` §Case Normalization. |
| `description` | string | conditional | Verbatim string content from `return <"…">`, with inline-string escapes resolved (`\"` and `\\` per `values-and-names.md` §Inline Strings). **Required when `form == "description"`; absent otherwise.** Empty string is not valid — empty `<"">` is rejected by Phase 1 with `G::parse::malformed-output-target`. |
| `ty` | TypeTag or null | yes | Enclosing declaration's resolved `-> DomainType`, or `null` if omitted. Both forms inherit type from the same channel; the form does not change typing. |
| `source` | string | yes | OutputSource enum value. Currently `"synthesized_by_agent"`. |

`OutputContract` objects appear in two places: `skill.output_contract` for the root skill and `call.callee_output_contract` for resolved block calls. They never appear inside a `flow` array. Descriptive form is terminal-return-only in MVP — both surface positions accept either form, but mid-flow output targets (if added later) must use the identifier form (see `ir-schema.md` §OutputContract).

### InlineInstruction

```json
{
  "node_id": "n5",
  "kind": "inline_instruction",
  "text": "Don't propose a fix until you've confirmed the root cause.",
  "role": "step"
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `node_id` | string | yes | |
| `kind` | string | yes | Always `"inline_instruction"`. |
| `text` | string | yes | Literal string content from source. |
| `role` | string | yes | Role enum value. |

### InstructionRef (resolved)

```json
{
  "node_id": "n6",
  "kind": "instruction_ref",
  "name": "validate_before_success",
  "resolved_text": "Validate that the fix works before reporting success.",
  "role": "step",
  "constraint_attrs": null
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `node_id` | string | yes | |
| `kind` | string | yes | Always `"instruction_ref"`. |
| `name` | string | yes | Resolved name of the referenced const declaration. |
| `resolved_text` | string | yes | Content of the referenced `const`/`generated const`. |
| `role` | string | yes | Role enum value. |
| `constraint_attrs` | object or null | yes | Present only when `role` is `"constraint"`. Shape: `{"strength": "...", "polarity": "..."}`. |

### Branch

```json
{
  "node_id": "n7",
  "kind": "branch",
  "condition": "has_failing_tests",
  "then_body": [ ... ],
  "elif_branches": [
    {
      "node_id": "n9",
      "kind": "elif_branch",
      "condition": "has_linting_errors",
      "body": [ ... ]
    }
  ],
  "else_body": [ ... ],
  "applies_descriptions": null
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `node_id` | string | yes | |
| `kind` | string | yes | Always `"branch"`. |
| `condition` | string | yes | Condition expression as text. |
| `then_body` | array of FlowNode | yes | Flow nodes for the `if` arm. |
| `elif_branches` | array of ElifBranch | yes | May be empty. |
| `else_body` | array of FlowNode or null | yes | `null` when no `else` clause. |
| `applies_descriptions` | object or null | yes | Map of `{block_name: resolved_description}` for every block referenced via `BLOCKNAME.applies()` in this Branch's own `condition` or any `elif_branches[*].condition`. `null` when no condition uses `.applies()`. Populated by Expand Step 1. See `ir-and-semantics.md` §Block Trigger Predicate. |

`Branch` is a container node. It carries no `role` — its children carry their own roles.

**Example with `.applies()`:**

```json
{
  "node_id": "n7",
  "kind": "branch",
  "condition": "fork_with_plan.applies()",
  "then_body": [ ... ],
  "elif_branches": [
    {
      "node_id": "n9",
      "kind": "elif_branch",
      "condition": "fork_with_summary.applies()",
      "body": [ ... ]
    }
  ],
  "else_body": [ ... ],
  "applies_descriptions": {
    "fork_with_plan": "Fork a terminal pre-loaded with the current plan.",
    "fork_with_summary": "Fork a terminal with a conversation-history summary as the prompt for the new agent."
  }
}
```

### ElifBranch

```json
{
  "node_id": "n9",
  "kind": "elif_branch",
  "condition": "has_linting_errors",
  "body": [ ... ]
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `node_id` | string | yes | |
| `kind` | string | yes | Always `"elif_branch"`. |
| `condition` | string | yes | Condition expression as text. |
| `body` | array of FlowNode | yes | Flow nodes for this arm. |

### Return

```json
{
  "node_id": "n11",
  "kind": "return",
  "value": { "node_id": "n12", "kind": "call_expr", "target": "summarize_changes", "args": {} }
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `node_id` | string | yes | |
| `kind` | string | yes | Always `"return"`. |
| `value` | Expression | yes | The return expression. |

### Constraint

```json
{
  "node_id": "n13",
  "kind": "constraint",
  "text": "Making changes outside the requested scope.",
  "strength": "hard",
  "polarity": "avoid"
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `node_id` | string | yes | |
| `kind` | string | yes | Always `"constraint"`. |
| `text` | string | yes | Resolved constraint text. May contain `{param}` references. |
| `strength` | string | yes | `"soft"` or `"hard"`. |
| `polarity` | string | yes | `"require"` or `"avoid"`. |

Constraints in `scoped_constraints` on a Call node use the same shape.

### ContextNode

```json
{
  "node_id": "n14",
  "kind": "context",
  "text": "This codebase follows a monorepo layout."
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `node_id` | string | yes | |
| `kind` | string | yes | Always `"context"`. |
| `text` | string | yes | Resolved context text. |

## Expression Union

Expressions appear as values in Call `args`, Return `value`, and Param `default`. Every Expression carries a `node_id` and a `kind` discriminator.

| Kind | Fields | Description |
|---|---|---|
| `"call_expr"` | `node_id`, `target`, `args` | Function call expression. In practice, only appears as a Return value (Lower desugars nested calls in args to sequential calls with temp bindings). |
| `"binding_ref"` | `node_id`, `name` | Reference to a local binding or parameter. |
| `"literal"` | `node_id`, `value` | A literal value. `value` uses the Value union shape. |
| `"property_access"` | `node_id`, `object`, `property` | Single-level property access (e.g., `result.findings`). |
| `"none_expr"` | `node_id` | The `none` value. |

### Expression Examples

```json
{ "node_id": "n4", "kind": "binding_ref", "name": "scope" }

{ "node_id": "n5", "kind": "literal", "value": { "kind": "string", "value": "." } }

{ "node_id": "n6", "kind": "call_expr", "target": "summarize_changes", "args": {} }

{ "node_id": "n7", "kind": "property_access", "object": "result", "property": "findings" }

{ "node_id": "n8", "kind": "none_expr" }
```

## Value Union

Values appear in Param `default` fields and inside `literal` expressions. They use a `kind` discriminator.

| Kind | Fields | Description |
|---|---|---|
| `"string"` | `value` (string) | String literal. |
| `"int"` | `value` (integer) | Integer literal. |
| `"float"` | `value` (number) | Float literal. |
| `"bool"` | `value` (boolean) | Boolean literal. |
| `"none"` | (no extra fields) | The `none` value. |

### Value Examples

```json
{ "kind": "string", "value": "." }
{ "kind": "int", "value": 42 }
{ "kind": "float", "value": 3.14 }
{ "kind": "bool", "value": true }
{ "kind": "none" }
```

## Enum Serialization

All enums serialize as **lowercase snake_case strings**. One convention, no exceptions.

| Enum | JSON values |
|---|---|
| Role | `"input_contract"`, `"step"`, `"constraint"`, `"context"`, `"output_contract"` |
| Strength | `"soft"`, `"hard"` |
| Polarity | `"require"`, `"avoid"` |
| EffectKeyword | `"none"`, `"reads_files"`, `"reads_env"`, `"writes_files"`, `"runs_commands"`, `"uses_network"`, `"asks_user"`, `"creates_artifacts"`, `"spawns_agent"` |
| ProjectionMode | `"inline"`, `"same_file_procedure"`, `"external_file"` |

### TypeTag Serialization

Built-in types serialize as plain strings. Domain types serialize as an object.

| Type | JSON |
|---|---|
| Built-in | `"string"`, `"int"`, `"float"`, `"bool"`, `"none"`, `"agent"` |
| Domain type | `{"domain_type": "repo_context"}` |

The `"domain_type"` value is the **canonical form** of the author's type name per `values-and-names.md` §Case Normalization (e.g., `RepoContext` and `repo_context` both serialize as `"repo_context"`). Cross-file nominal matching at call boundaries (`types.md` §What The Compiler Checks) compares canonical names by string equality.

## Versioning and Stability

### `ir_version` semantics

- Monotonic integer starting at `1`.
- Bumps on any **breaking** shape change: field removal, field rename, field type change, enum value rename.
- Does **not** bump on additive changes: new optional fields, new enum values, new node kinds. Consumers must ignore unknown fields and unknown enum values gracefully.
- Independent of the compiler version. The `compiler` field tracks which binary produced the JSON; `ir_version` tracks the schema shape.

### Pre-1.0 stability commitment

The IR JSON schema is explicitly **unstable** while the compiler is pre-1.0. Any change is allowed, but the `ir_version` field bumps on breaking changes so consumers can detect incompatibility. Post-1.0, the stability policy tightens: breaking changes require a major version bump with a migration path.

### No JSON Schema file for MVP

The schema is specified in this document only. A machine-readable JSON Schema (draft 2020-12) file is not produced for MVP. Revisit if external tooling (IDE extensions, CI validators) demands machine-readable schema validation.

## Worked Example

A complete `fix_bug.ir.json` for the `fix_bug` skill from `expand.md` §8.

```json
{
  "ir_version": 1,
  "compiler": "glyph 0.1.0",
  "source_file": "fix_bug.glyph.md",
  "skill": {
    "node_id": "n0",
    "kind": "skill",
    "name": "fix_bug",
    "description": "Debug and fix a bug in the codebase with minimal, targeted changes.",
    "params": [
      {
        "node_id": "n1",
        "kind": "param",
        "name": "scope",
        "type": "string",
        "default": { "kind": "string", "value": "." }
      }
    ],
    "return_type": null,
    "effects": ["reads_files", "writes_files", "runs_commands"],
    "context": [
      {
        "node_id": "n2",
        "kind": "context",
        "text": "This skill assumes the bug is reproducible in the local environment."
      }
    ],
    "constraints": [
      {
        "node_id": "n3",
        "kind": "constraint",
        "text": "Making changes outside {scope}.",
        "strength": "soft",
        "polarity": "avoid"
      },
      {
        "node_id": "n4",
        "kind": "constraint",
        "text": "Follow the repository's existing patterns before introducing new abstractions.",
        "strength": "soft",
        "polarity": "require"
      }
    ],
    "flow": [
      {
        "node_id": "n5",
        "kind": "call",
        "target": "inspect_failure",
        "args": {
          "area": { "node_id": "n6", "kind": "binding_ref", "name": "scope" }
        },
        "output": null,
        "return_type": null,
        "effects": ["reads_files"],
        "site_modifier": "focus on auth boundaries",
        "role": "step",
        "scoped_constraints": [],
        "resolved_body_text": "Inspect the failure in {scope} and identify what is failing.",
        "projection_mode": "inline",
        "callee_flow": null,
        "callee_context": null,
        "callee_constraints": null,
        "procedure_path": null
      },
      {
        "node_id": "n7",
        "kind": "call",
        "target": "identify_root_cause",
        "args": {},
        "output": null,
        "return_type": null,
        "effects": [],
        "site_modifier": null,
        "role": "step",
        "scoped_constraints": [],
        "resolved_body_text": "Identify the root cause of the issue.",
        "projection_mode": "inline",
        "callee_flow": null,
        "callee_context": null,
        "callee_constraints": null,
        "procedure_path": null
      },
      {
        "node_id": "n8",
        "kind": "inline_instruction",
        "text": "Don't propose a fix until you've confirmed the root cause.",
        "role": "step"
      },
      {
        "node_id": "n9",
        "kind": "call",
        "target": "patch_minimally",
        "args": {},
        "output": null,
        "return_type": null,
        "effects": ["writes_files"],
        "site_modifier": null,
        "role": "step",
        "scoped_constraints": [],
        "resolved_body_text": "Apply the smallest change that fixes the issue.",
        "projection_mode": "inline",
        "callee_flow": null,
        "callee_context": null,
        "callee_constraints": null,
        "procedure_path": null
      },
      {
        "node_id": "n10",
        "kind": "instruction_ref",
        "name": "validate_before_success",
        "resolved_text": "Validate that the fix works before reporting success.",
        "role": "step",
        "constraint_attrs": null
      },
      {
        "node_id": "n11",
        "kind": "return",
        "value": {
          "node_id": "n12",
          "kind": "call_expr",
          "target": "summarize_changes",
          "args": {}
        }
      }
    ]
  }
}
```

## Cross-References

- **IR node schema** (`ir-schema.md`): canonical pseudocode schema for all IR node types and enums. This document is the JSON projection of that schema.
- **IR node identifiers** (`ir-schema.md` §Node Identifiers): canonical spec for node ID format, allocation, scope, and stability.
- **Expand** (`expand.md` §3.1): Step 2 input contract references the resolved IR shape serialized here.
- **CLI** (`cli.md`): `--emit-ir` flag and `validate-output` subcommand both use this schema.
- **Agent skill** (`agent-skill.md`): the agent reads this JSON during Step 2 and passes it to `validate-output` during Phase 6b.
- **Build foundation** (`build-foundation.md` §A4): IR arena representation and custom serialization pass that produces this JSON.
