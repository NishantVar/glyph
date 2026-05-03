# Glyph Semantic Primitives

Glyph source is built from five semantic primitives. These primitives are the conceptual foundation of the language: every author-visible construct either directly expresses one primitive or decomposes into a small combination of them.

The primitives are **compositional**, not mutually exclusive buckets. A single source form can carry more than one primitive. For example, `ctx = inspect_repo(scope)` is an instruction because it runs `inspect_repo(scope)`, and it is also a binding because it gives the result the name `ctx`.

The goal is that no language feature needs a sixth semantic category. New syntax should either be sugar over these primitives or a reason to revisit the primitive set deliberately.

## The Five Primitives

### 1. Instruction

**What it is:** A directive telling the agent to perform an action.

An instruction is imperative. It says "do this." It may be a call to a named block, a bare instruction reference, an inline string that describes a step, or a conditional workflow structure. Control flow (`if`/`elif`/`else`) is instruction structure: the branch gates which child instructions execute, but does not introduce a separate primitive.

**Where it lives in source:**
- Calls in `flow:` (`inspect_repo(scope)`, `validate(plan)`)
- Bare instruction references in `flow:` after resolution
- Inline strings inside `flow:` bodies
- Local assignment statements whose right side performs work (`ctx = inspect_repo(scope)`)
- `if`/`elif`/`else` constructs inside `flow:` and their step-bearing branch bodies
- Single-string `block` and `export block` shorthand bodies
- `generated block` definitions and calls to them
- Calls carrying a `with "..."` modifier; the modifier specializes the instruction at that call site

**IR role:** `Step` for action nodes. `Branch` is a container node whose children carry roles; it is not a sixth role.

---

### 2. Constraint

**What it is:** A behavioral bound that restricts or requires something of the agent.

A constraint does not say "do this step now." It says "when acting, follow this rule." Constraints have strength (`hard`/`soft`) and polarity (`require`/`avoid`). Most constraints apply to an entire skill or block, but branch-scoped constraints are allowed when the rule applies only inside one conditional path.

**Where it lives in source:**
- `require`, `avoid`, `must`, and `must avoid` markers in `constraints:`
- Body-level constraint markers on a `skill`, `block`, or `export block`
- Flow-level constraint markers, which are hoisted when unconditional
- Constraint markers inside `if`/`elif`/`else` branch bodies, which remain branch-scoped
- `const`, `export const`, or `generated const` names referenced from constraint positions
- Inline quoted strings in constraint positions with an explicit marker
- Scoped constraints imported through called blocks; these remain local to the call/procedure region

**IR role:** `Constraint`, with `strength` and `polarity` attributes.

---

### 3. Context

**What it is:** Passive background information that frames the agent's interpretation without directing action.

Context is not imperative. It does not tell the agent to do anything, and it does not restrict what the agent may do. It informs understanding: the environment, domain facts, assumptions, project shape, or other non-normative framing.

**Where it lives in source:**
- `context:` sub-sections on `skill`, `block`, or `export block`
- Body-level `context` markers
- Flow-level `context` markers, which are hoisted when unconditional
- `context` markers inside `if`/`elif`/`else` branch bodies, which remain branch-scoped
- `const`, `export const`, or `generated const` names referenced from context positions
- Inline quoted strings in context positions

**IR role:** `Context` / `ContextNode`.

**Not `description:`:** `description:` is routing and trigger metadata, not execution context. On a skill, it tells an outer agent when to select the skill. On a block, it supplies the predicate text used by `BLOCKNAME.applies()`. It belongs with interface/contract metadata, not with runtime context.

---

### 4. Interface

**What it is:** A callable unit's contract with the outside world.

Interface defines how a skill or block connects to its caller, selector, importer, and compiled-output consumer. It covers what must be provided, what is produced, what capabilities may be used, and when the callable is relevant. It is not an action, a process rule, passive execution context, or a reusable value.

**Where it lives in source:**
- `skill`, `block`, and `export block` headers as callable declarations
- Parameter lists on callable headers: `skill foo(scope, risk = "medium")`
- Parameter type annotations: `name: Type`
- Parameter defaults, including literals and named `const` values
- Return type annotations: `-> ReturnType`
- `return expr` statements, including implicit `return none` where allowed
- `effects:` declarations, as the callable's capability/side-effect contract
- `description:` metadata, as the skill-routing or block-trigger contract

**IR roles / structures:** `InputContract` for parameters, `OutputContract` for return semantics, effect annotations for capabilities, and description metadata for routing/trigger behavior.

---

### 5. Binding

**What it is:** A name associated with a reusable value, callable, module, parameter, or result.

Binding is the naming primitive. By itself, naming does not act, restrict, inform, or define a contract; it makes something addressable elsewhere. Many bindings carry another primitive too: a parameter is part of the interface and also introduces a name; a block name is a callable binding whose body contains instructions; a local assignment in `flow:` is both an instruction step and a binding of that step's output.

**Where it lives in source:**

| Form | Binding introduced |
|---|---|
| `skill name(...)` | Skill entrypoint name |
| `block name(...)` / `export block name(...)` | Callable block name |
| `generated block name(...)` | Repair-materialized callable name |
| `const name = "..."` / `export const name = "..."` | Named string constant |
| `const name = N` / `export const name = N` | Named integer or float constant |
| `generated const name = "..."` | Repair-materialized string constant |
| Parameter `name` in a callable header | Invocation-supplied value name |
| `ctx = inspect_repo(scope)` in `flow:` | Local result name |
| `import "./path" { name }` | Imported declaration name |
| `import "./path" { name as alias }` | Selective import alias |
| `import "./path" as module_alias` | Whole-module alias |

**IR role:** Binding is not one of the closed instruction roles. It appears structurally through declarations, `Param`, `Call.output`, `BindingRef`, import tables, and name-resolution metadata.

---

## Summary Table

| Primitive | Meaning | Common source forms | IR role / structure |
|---|---|---|---|
| Instruction | Do work | Calls, inline flow strings, bare instruction refs, block bodies, conditional branches | `Step`, `Branch` container |
| Constraint | Bound behavior | `require`, `avoid`, `must`, `must avoid`; constraint-position text | `Constraint` |
| Context | Frame interpretation | `context:` entries, `context` markers, context-position text | `ContextNode` |
| Interface | Define callable contract | Parameters, defaults, return type, `return`, `effects:`, `description:` | `InputContract`, `OutputContract`, effects, description metadata |
| Binding | Introduce an addressable name | Declarations, imports, parameters, local assignments, aliases | Declarations, `Param`, `Call.output`, `BindingRef`, import/name-resolution metadata |

---

## Compositional Examples

### Local assignment

```glyph
ctx = inspect_repo(scope)
```

Decomposes into:
- **Instruction:** call `inspect_repo(scope)`
- **Binding:** bind the result to `ctx`

### Exported block declaration

```glyph
export block inspect_failure(scope = ".") -> FailureReport
    description: "Use when the user needs failure diagnosis."
    effects: reads_files, runs_commands
    context:
        "The repository may contain multiple packages."
    constraints:
        avoid unrelated_edits
    flow:
        report = collect_failure_data(scope)
        return report
```

Decomposes into:
- **Binding:** introduce the importable callable name `inspect_failure`
- **Interface:** parameters, default, return type, description, effects, return
- **Context:** repository/package framing
- **Constraint:** avoid unrelated edits
- **Instruction:** collect failure data
- **Binding:** bind the collected result to `report`

### Conditional branch

```glyph
if high_risk:
    must request_review
    run_full_suite()
else:
    context "This is a low-risk path."
    run_smoke_tests()
```

Decomposes into:
- **Instruction:** conditional workflow structure plus executable branch steps
- **Constraint:** branch-scoped `must request_review`
- **Context:** branch-scoped low-risk framing

---

## Design Notes

### `const` spans binding, constraint, and context

`const` is a binding that holds passive string content. The declaration itself belongs to the binding primitive. The content's semantic role is assigned by use position:

- Referenced in `constraints:` or after a constraint marker -> constraint
- Referenced in `context:` or after a `context` marker -> context
- Used as a parameter default -> interface data

This keeps the language small. Authors name reusable text once, and structure determines its role.

### Control flow is instruction structure

`if`/`elif`/`else` is not a separate primitive. It is a container for conditional execution. The branch condition gates which child nodes apply; the child nodes still carry the real semantics: instruction, constraint, context, binding, or output contract.

### Effects are interface metadata, not a primitive

Effects answer "what capabilities can this callable exercise?" That is part of the callable contract, so `effects:` belongs under Interface. Effects are not instructions: a step can have effects, but the effect annotation itself does not tell the agent to perform work.

### Description is routing metadata, not execution context

`description:` can look like context because it is natural language, but it is read by selectors and trigger predicates rather than by the executing workflow as background. A skill's description compiles to frontmatter for selection. A block's description is consulted by `.applies()`. Runtime background belongs in `context:`.

### Interface vs binding

Interface and binding often appear together but answer different questions. Interface says how a callable is invoked, what it returns, when it applies, and what effects it may have. Binding says what name refers to the callable, value, parameter, module, or result. A parameter is both: its declaration is part of the callable interface, and its name is bound for use inside the callable body.

### Primitives are about meaning, not syntax

The same syntax can contribute different primitives depending on structure. A quoted string in `flow:` is an instruction. A quoted string after `avoid` is a constraint. A quoted string in `context:` is context. A quoted string as a parameter default is interface data. Position carries the semantic role.
