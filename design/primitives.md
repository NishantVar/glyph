# Glyph Semantic Primitives

Every statement in a Glyph skill file belongs to one of five semantic primitives. These primitives are the conceptual foundation of the language: everything an author writes is one of these five things, and the language surface is designed so that authors rarely have to decide which one they're expressing — structure makes it obvious.

## The Five Primitives

### 1. Instruction

**What it is:** A directive telling the agent to perform an action.

An instruction is imperative. It says "do this." It may be a call to a named block, an inline string that describes a step, or a conditional step. Control flow (`if`/`elif`/`else`) is a form of instruction — a conditional instruction gates which steps execute, but is still imperative in nature.

**Where it lives in source:**
- Strings inside a `flow:` body — always treated as instruction steps, never as context
- Call expressions (`inspect_repo(scope)`, `validate(plan)`) in `flow:`
- `if`/`elif`/`else` constructs inside `flow:`, including their branch bodies
- Single-string `block` shorthand bodies
- `generated block` definitions (repair-materialized instructions)

**IR role:** `Step` (conditional steps represented as `Branch` nodes wrapping child `Step` nodes)

---

### 2. Constraint

**What it is:** A behavioral bound that restricts or requires something of the agent.

A constraint does not say "do this step." It says "when acting, always / never / prefer." Constraints are modal — they apply across the entire skill or block, not at a specific point in execution. They have strength (`hard`/`soft`) and polarity (`require`/`avoid`).

**Where it lives in source:**
- `require`, `avoid`, `must`, `must avoid` markers in `constraints:` sub-section
- `text` bindings referenced inside `constraints:` (e.g., `avoid unrelated_edits`)
- Inline quoted strings inside `constraints:` with a marker
- `export text` bindings imported and used in `constraints:`
- `generated text` definitions for undefined bare names in constraint positions
- Constraint markers inside `if`/`elif`/`else` branch bodies (branch-scoped constraints)

**IR role:** `Constraint` (with `strength` and `polarity` attributes)

---

### 3. Context

**What it is:** Passive background information that frames the agent's interpretation without directing action.

Context is not imperative. It does not tell the agent to do anything or restrict what it may do. It informs the agent's understanding: what environment it is operating in, what assumptions hold, what the skill is for. Context is read by the agent as background, not acted upon as a step.

**Where it lives in source:**
- `description:` sub-section on any `skill`, `block`, or `export block` — the primary home for in-declaration context
- `text` bindings referenced inside a `context:` sub-section
- Inline quoted strings inside a `context:` sub-section
- `export text` bindings imported and used in `context:`

**IR role:** `Context` (deferred from MVP; `description:` content is preserved in the IR as a metadata field but not yet a first-class role node)

**Note on `text` duality:** A `text` binding is passive string content with no callable interface. Whether it expresses a constraint or context depends entirely on where it is placed — in `constraints:` it is a constraint, in `context:` it is context. This is intentional: the sub-section is the semantic signal, not the declaration keyword.

---

### 4. Interface

**What it is:** The skill's external contract — what it accepts as input and promises as output.

Interface defines the callable boundary of a skill or block. It is not an action, a restriction, background information, or a named value — it is the declaration of how the skill connects to the world outside it. Parameters declare what the skill requires from its caller; the return type declares what it produces.

**Where it lives in source:**
- Parameter list on `skill`, `block`, and `export block` headers: `skill foo(scope, risk = "medium")`
- Return type annotation: `-> ReturnType`
- Parameter type annotations: `name: Type`
- Parameter defaults (literals or named `text`/`int`/`float` constants)

**IR roles:** `InputContract` (parameters), `OutputContract` (return type)

---

### 5. Binding

**What it is:** A named value that gives something an identity reusable elsewhere in the skill.

A binding introduces a name into scope and associates it with a value. It does not act, restrict, inform, or define a contract — it declares. Bindings appear as named constants at file scope (`text`, `int`, `float`) and as local result names inside `flow:`. The compiler resolves all file-scope bindings statically; local bindings are resolved as data flows through the skill.

**Where it lives in source:**

| Form | Kind |
|---|---|
| `text name = "..."` / `export text name = "..."` | Named string constant |
| `int name = N` / `export int name = N` | Named integer constant |
| `float name = N` / `export float name = N` | Named float constant |
| `ctx = inspect_repo(scope)` — local assignment in `flow:` | Local binding |
| `import "./path" { name }` | Import binding |
| `generated text name = "..."` | Repair-materialized string binding |

**Note:** A `text` binding is itself a binding (it names a string value). Whether the string it holds serves as a constraint or context depends on where the name is referenced, not on the binding declaration itself.

---

## Summary Table

| Primitive | Declaration / syntax form | Sub-section | IR role |
|---|---|---|---|
| Instruction | `block`, inline string, call, `if`/`elif`/`else` | `flow:` | `Step`, `Branch` |
| Constraint | `text` (in constraint position), inline string with marker | `constraints:` | `Constraint` |
| Context | `text` (in context position), `description:` | `context:`, `description:` | `Context` (deferred) |
| Interface | parameters, `-> ReturnType` | header | `InputContract`, `OutputContract` |
| Binding | `text`, `int`, `float`, local assignment, `import` | file scope, `flow:` | local IR nodes |

---

## Design Notes

### `text` spans two primitives — constraint and context

`text` is a binding that holds a passive string value. As a binding it belongs to the binding primitive. But the *content* it carries can serve as either a constraint or context depending on where it is referenced:

- Referenced in `constraints:` → the content is a constraint
- Referenced in `context:` → the content is context

Forcing authors to choose between `text` (constraint) and `context` (context) at declaration time would add friction for a distinction the sub-section already makes clear. The language stays small; position carries the semantic weight.

### Control flow is instruction

`if`/`elif`/`else` could be treated as a separate primitive ("condition"), but a conditional step is still imperative — it says "if X, do Y." The condition gates which instructions execute; it does not act, restrict, inform, or name a value on its own. Keeping it inside the instruction primitive keeps the primitive count minimal and the mental model simple.

### Interface vs binding

These are distinct. Interface is the callable boundary: it defines how a skill is invoked and what it returns. Binding is naming a value: it gives a string, integer, float, or computed result a name for reuse. A parameter is interface; `text preserve_patterns = "..."` is a binding. The difference matters because interface shapes the compiled output's `## Parameters` section and drives nominal type-checking, while bindings are resolved and inlined at compile time.

### Primitives are about meaning, not syntax

The same syntactic form (`text`) can express different primitives depending on position. The compiler infers semantic role from structure. This is intentional: a small surface language with positional disambiguation keeps authoring simple while giving the IR enough information to be precise.
