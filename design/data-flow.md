# Glyph Data Flow

This document defines how values move through Glyph: parameters, local bindings, calls, control flow, and returns. It is the single reference for data-flow semantics, call-site syntax, and control-flow body syntax.

## Parameters

`skill`, `block`, and `export block` definitions may declare parameters. Skill parameters may be omitted entirely when the skill does not need explicit invocation inputs.

Parameters may have:

- a name;
- an optional type annotation;
- an optional default value;
- an optional role or effect annotation, if later design requires it.

```glyph
skill update_docs()

skill implement_feature(scope, risk = "medium")

block review_changes(files: FileSet, strict = true) -> ReviewResult
```

Source-level parameters may be duck-typed. The IR resolves each parameter to an explicit type or structural contract before output generation.

## Global Preferences

Skills may depend on user or project preferences such as terminal multiplexer, communication style, validation strictness, preferred tools, or project conventions. In Glyph, these are ordinary `export text`, `export int`, or `export float` declarations in a preferences file, imported like any other name.

```glyph
import "./prefs.glyph.md" { terminal_mux, validation_strictness }

skill open_terminal()
    flow:
        launch(terminal_mux)
```

There is no dedicated `pref(...)` call form. Preferences are indistinguishable from any other imported constant at the syntax and effect level. The `reads_prefs` effect was considered and rejected; preferences do not carry a special effect.

Compile-time resolution: preference values are inlined into the compiled Markdown at compile time. If a preference value changes, affected skills should be recompiled.

An `export block` that reads a preference does so through an explicit import, keeping its closure contract intact.

See `preferences.md` for the full preferences design. Override mechanism and standard prefs library are deferred (see `todo.md`).

## Local Bindings

A call result or literal value can be bound to a local name using `=`:

```glyph
ctx = inspect_repo(scope)
plan = make_plan(ctx)
apply_changes(plan)
```

The left side is a bare identifier. The right side is a call expression, literal, dot access, or another identifier reference. Bindings may hold non-call values:

```glyph
risk = "high"
max_attempts = 3
```

Local bindings are scoped to the enclosing `skill` or `block`. A binding should not silently shadow an existing binding if doing so would make data flow ambiguous.

The source may omit types. The compiler infers or repairs types as needed.

## Calls And Arguments

### Positional And Named Arguments

Glyph supports both positional and named arguments at call sites. Positional arguments must precede named arguments -- no positional argument may follow a named argument in the same call.

```glyph
plan = make_plan(ctx, risk)                    // all positional
plan = make_plan(ctx, risk = "high")           // mixed: positional then named
plan = make_plan(ctx = context, risk = "high") // all named
```

Rules:

- Positional arguments are matched to parameters left-to-right in declaration order.
- Named arguments use `name = value` syntax, where `name` matches a declared parameter name.
- A named argument may not duplicate a parameter already filled by a positional argument. This is a compile error.
- All required parameters (those without defaults) must be supplied, either positionally or by name.
- The compiler resolves all positional arguments to named arg-to-param mappings during IR normalization; the IR contains only named arguments.

### Default-Value Interaction

Omitting an argument uses the parameter's declared default from the header (`language-surface.md`). To skip an optional positional parameter and supply a later one, the caller must switch to named arguments.

```glyph
// Given: block make_plan(ctx, risk = "medium", verbose = false)
make_plan(ctx)                         // risk and verbose use defaults
make_plan(ctx, "high")                 // risk = "high", verbose uses default
make_plan(ctx, verbose = true)         // named arg skips risk, which uses default
```

There is no placeholder or sentinel value for "use the default." Omission or named-argument skipping are the only mechanisms.

### Trailing Commas And Line Wrapping

Trailing commas are allowed in all argument lists, both single-line and multi-line. They are optional and never required.

Call arguments may span multiple lines using the implicit line-continuation rule for paired delimiters defined in `language-surface.md`. Inside parentheses, indentation is not structurally significant. No backslash continuation is needed or supported.

```glyph
plan = make_plan(
    ctx,
    risk = "high",
    verbose = true,
)
```

### Qualified Callees

A callee is either a bare identifier resolved through the standard name resolution order (see `values-and-names.md`), or a single-level qualified name where the left side is a whole-module import alias:

```glyph
import "./repo_tools.glyph.md" as repo_tools

ctx = repo_tools.inspect_repo(scope)
repo_tools.validate_changes(ctx)
```

The left side must be a whole-module import alias (`language-surface.md`). Dots are reserved for module-qualified access (`values-and-names.md`).

No computed callees, no first-class functions, and no method-style calls in the MVP.

### Bare Name vs Call Distinction

A zero-argument call uses empty parentheses to distinguish it from a bare name reference:

```glyph
summarize()                    // zero-argument call
summarize                      // bare name reference (text, parameter, binding)
```

See `values-and-names.md` for the full name resolution rules, reserved keywords, and identifier conventions.

### `with` Modifier

A call may carry a trailing `with "modifier string"` clause. The modifier is a short natural-language prompt that specializes the called definition at this specific call site:

```glyph
flow:
    inspect_failure(scope) with "focus on auth boundaries"
    summarize_changes() with "include any remaining gaps"
```

Rules:

- Syntax: `<call-expression> with <string-literal>`. The string is a single inline `"..."` or block `"""..."""`; no interpolation.
- The modifier attaches to the call site, not the binding. With a binding, it still attaches to the call: `report = inspect_failure(scope) with "focus on auth"`.
- The modifier is consumed by the expand pass. It shapes the generated prose for that one invocation and does **not** survive into compiled output (no "with modifier" text appears in the `.md`).
- The modifier does not change the callee's declared effects, constraints, return type, or parameters. It only adjusts the wording of the expanded Step.
- Exactly one `with` clause per call site in MVP. No chained `with ... with ...`.
- Applies to bare calls (`foo()`), qualified calls (`Alias.foo()`), and calls inside bindings (`x = foo()`). Does not apply to bare-name statements (no parens).

IR representation: the modifier is stored on the `Call` IR node as an optional `site_modifier: String` field. See the call-node normalization below.

### Nested Calls

Nested calls are allowed in the MVP. A call may appear as an argument to another call:

```glyph
result = validate(make_plan(ctx, risk))
apply_changes(merge(base, overlay))
```

The compiler desugars nested calls into flat IR nodes by introducing temporary bindings, keeping the IR as a flat, visualizable data-flow graph.

Deeply nested calls (three or more levels) are legal but discouraged by convention -- intermediate bindings with descriptive names improve readability.

### IR Call-Node Normalization

Every source call normalizes to an explicit IR node:

```text
Call {
  target: <resolved identifier or qualified name>,
  args: { <param_name>: <value_or_ref>, ... },
  output: Binding(<name>) | none,
  return_type: <resolved type>,
  effects: [<inferred effect set>],
  site_modifier: <string> | none
}
```

Key normalization steps:

- All positional arguments are resolved to named arg-to-param mappings. The IR has no positional arguments.
- Nested calls are desugared into sequential flat calls with compiler-generated temporary bindings.
- Default values are filled in for omitted optional parameters.
- The callee is resolved to its declaration (same-file block, imported block, or standard-library primitive).
- Effects are inferred from the callee's declared or inferred effect set.

### Repair Behavior At Call Sites

The LLM repair pass may add minimal syntax when call-site data flow cannot compile: adding explicit argument names when positional mapping is ambiguous, adding missing type annotations on bindings when inference fails, or renaming a local binding only if the current name collides and no smaller repair exists. Repair should not rewrite the call structure, reorder arguments, or expand shorthand instruction names into prose. Full repair rules are in `repair.md`.

## Control Flow

### Statement Forms Inside `flow:`

Six statement forms are allowed inside `flow:` blocks. All content defaults to the `Step` IR role unless explicit syntax or resolved metadata says otherwise.

| Form | Example | IR Role |
|------|---------|---------|
| Binding | `ctx = inspect_repo(scope)` | `Step` with output binding |
| Bare call | `apply_changes(plan)` | `Step`, no output binding |
| Bare name | `validate_before_success` | `Step`, resolved via name resolution |
| Inline string | `"Mention any issues found."` | `Step` |
| Return | `return summarize(plan)` | `OutputContract` |
| If/elif/else | `if <cond>:` block | `Branch` container |

A call without a binding is a statement call -- the return value, if any, is discarded. Both binding and bare-call forms occupy one line each unless the argument list wraps inside parentheses.

### Branching: `if`/`elif`/`else`

Branching uses Python-style colon-terminated headers with significant indentation:

```glyph
flow:
    ctx = inspect_repo(scope)
    risk = assess_risk(ctx)

    if risk == "high" and ctx.has_tests:
        run_full_suite(ctx)
        request_review(ctx)
    elif risk == "high":
        "Flag for manual review -- no test suite available."
    elif ctx.needs_update:
        apply_changes(ctx)
    else:
        "No action needed."

    return summarize(ctx)
```

Rules:

- `if <condition>:` introduces the first branch. Required.
- `elif <condition>:` introduces additional branches. Zero or more allowed.
- `else:` introduces the fallback branch. Optional. If omitted and no branch condition matches, execution continues to the next statement after the `if` chain.
- `elif` and `else` appear at the same indentation level as their matching `if`.
- No parentheses are required around conditions, but parentheses are allowed for grouping complex expressions.
- Branch bodies may contain any statement form allowed in `flow:`: bindings, bare calls, bare names, inline strings, `return`, and nested `if`/`elif`/`else`.
- `if`, `elif`, `else`, `return`, and `flow` are reserved keywords (see `values-and-names.md`).

### Condition Expressions

Condition expressions inside `if` and `elif` are intentionally minimal in the MVP.

| Form | Example |
|------|---------|
| Boolean identifier or binding | `if is_valid:` |
| Boolean-returning call | `if has_tests(ctx):` |
| Single-level dot access | `if ctx.has_tests:` |
| `not` operator | `if not is_valid:` |
| Equality | `if risk == "high":` |
| Inequality | `if risk != "low":` |
| `and` | `if risk == "high" and ctx.has_tests:` |
| `or` | `if a or b:` |
| Parenthesized grouping | `if (a or b) and c:` |

Standard Python precedence: `not` binds tightest, then `and`, then `or`. Parentheses override precedence.

If a condition requires excluded operators (`<`, `>`, arithmetic, `in`, etc.), the author should bind the result of a call that computes the condition:

```glyph
exceeds_threshold = check_threshold(score, 0.8)
if exceeds_threshold:
    ...
```

### Dot Access

Single-level property dot access is allowed anywhere a value is expected: in `if` conditions, call arguments, bindings, and `return` expressions. It is distinct from module-qualified access (`repo_tools.inspect_repo`), which uses a whole-module import alias on the left side.

Only single-level dot access is allowed in the MVP. Chained access (`ctx.config.timeout`) is deferred.

### No Nested `flow:`

`flow:` is a sub-section header, not a nestable control-flow construct. Writing `flow:` inside a `flow:` body is a compile error. If a skill or block needs a sub-workflow, factor it into a separate `block` declaration and call it.

### Indentation

Control-flow indentation follows `language-surface.md` with 4-space indent units:

```
Level 0 (col 0):   top-level declarations (skill, block)
Level 1 (col 4):   section headers (flow:, effects:), body-level constraints
Level 2 (col 8):   flow statements, if/elif/else headers
Level 3 (col 12):  if/elif/else body statements
Level 4 (col 16):  nested if body (if inside else, etc.)
```

Blank lines inside `if`/`elif`/`else` bodies are visual separators and do not close or break blocks. Deeper nesting is structurally supported but discouraged -- consider extracting a helper `block`.

### IR Mapping

| Source Form | IR Node | Role |
|-------------|---------|------|
| `x = call(...)` | `Call { output: Binding(x), ... }` | `Step` |
| `call(...)` | `Call { output: none, ... }` | `Step` |
| `bare_name` | `InstructionRef { name }` | `Step` |
| `"text"` | `InlineInstruction { text }` | `Step` |
| `return expr` | `Return { value }` | `OutputContract` |
| `if/elif/else` | `Branch { condition, then_body, elif_branches, else_body }` | Container |
| `x.prop` | `PropertyAccess { object, property }` | Value expression |

The `Branch` IR node is a container. Its children carry their own roles. The branch itself does not have an instruction role -- it structures the execution path.

### Compiled-Output Projection

Conditional logic in `flow:` flattens into prose instructions in `### Steps` under `## Instructions` (`compiled-output.md`). Each `if`/`elif`/`else` chain compiles into a single numbered step with conditional sub-instructions. Simple single-branch `if` statements may compile into a single conditional sentence within a step.

## Return Semantics

`return` produces the skill's or block's output value:

```glyph
flow:
    result = validate(plan)
    return summarize(result)
```

Rules:

- `return <expr>` where `<expr>` is a call, binding reference, dot access, literal, or `none`.
- `return` alone (no expression) is equivalent to `return none`.
- `return` may appear anywhere inside `flow:`, including inside `if`/`elif`/`else` branches, enabling early return.
- Every `export block` must have an explicit `return` on every code path. The compiler validates this statically.
- For `skill` and private `block`, a definition that ends without `return` implicitly returns `none`. If the declaration produces a value, it must use an explicit `return` statement.

Returns become explicit output contracts in the IR. If the return expression is a call, the call is evaluated first and its result becomes the return value, following the same desugaring as nested calls.

## Closure And Scope Rules

### Exported Block Closure

Only `export block` declarations may be imported by other `.glyph.md` files. The compiler enforces that every exported block is closed before it can be compiled as an importable unit.

A closed exported block may depend on:

- its parameters;
- local bindings declared inside the block;
- same-file `text` declarations;
- explicit imports;
- standard primitives or standard-library entries;
- declared constraints, outputs, and effects.

An exported block must not depend on hidden caller context, private names from an importing file, undeclared globals, or implicit project assumptions.

Private `block`s are not importable and may only be called from the same source file. Any private block reachable from an exported block must itself be closed under the exported block's declared contract. Private blocks may rely on their enclosing skill context in the MVP, including values and instructions already visible in that skill.

An exported block may call another imported exported block. The caller should inherit or expose the callee's relevant effects and constraints so import contracts remain visible to downstream callers.

### Duck Typing And Structural Compatibility

Glyph source supports Python-like duck typing: a call can accept a value if the value has the structure the callee needs, even when the author did not name an exact type. The source may stay lightweight, but the IR must record an explicit structural requirement (e.g., `ParameterRequirement(report_like has field findings)`). This keeps authoring flexible while preserving compile-time analyzability.

### Effects At Call Sites

Effects are declared on callables (via `ir-and-semantics.md`), not at call sites. The compiler infers and propagates effects through the call graph.
### Visualization

Data flow should be visualizable as a graph: parameters are entry nodes, calls are operation nodes, bindings are value edges, returns are exit nodes, and effects are annotations on call nodes. This is one reason hidden ambient context should be minimized -- if a call depends on a value, that value should be visible as an argument or declared dependency.

## Interaction With Other Design Areas

`language-surface.md` (parameter definition syntax), `language-surface.md` (indentation, line continuation, colon-terminated headers), `values-and-names.md` (identifier rules, name resolution, bare name vs call, reserved keywords), `ir-and-semantics.md` (effect propagation, no call-site effect syntax), `types.md` (nominal matching at call boundaries), `ir-and-semantics.md` (`flow:` section), `ir-and-semantics.md` (`Step`, `OutputContract`, `Branch`), `compiled-output.md` (conditional logic flattens to prose).

## Deferred

- `for_each` loop syntax (post-MVP).
- Chained dot access (`ctx.config.timeout`) and deeper qualified nesting (`a.b.c`).
- Comparison operators (`<`, `>`, `<=`, `>=`) in conditions.
- Pattern matching or `match`/`case` constructs.
- Exception handling / `try`/`catch`.
- Spread/splat arguments, variadic parameters.
- Method-style calls (`x.method()`), pipeline or chaining syntax.
- Runtime preference injection or override mechanism (see `preferences.md`).
