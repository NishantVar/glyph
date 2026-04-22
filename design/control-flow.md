# Glyph Control-Flow Body Syntax

This document defines the MVP syntax for statements inside `flow:` blocks, including bindings, calls, `if`/`elif`/`else` branching, `return`, condition expressions, dot access, indentation rules, and IR mapping.

## Status

MVP Tier 3. Builds on:

- `block-structure.md` (Tier 1) — significant indentation, colon-terminated sub-section headers, 4-space indent unit
- `section-vocabulary.md` (Tier 2) — `flow:` as the ordered workflow section, content defaults to `Step` role
- `calls-and-args.md` (Tier 2) — call-site syntax, positional-then-named arguments, binding with `=`, nested calls
- `ir-roles.md` (Tier 0) — `Step`, `OutputContract`, `Context` roles
- `values-and-literals.md` (Tier 0) — identifier rules, name resolution, reserved keywords, bare name vs parenthesized call distinction
- `data-flow-and-calls.md` — local bindings, return values, call semantics

## Statement Forms Inside `flow:`

Six statement forms are allowed inside `flow:` blocks. All content inside `flow:` defaults to the `Step` IR role unless explicit syntax or resolved metadata says otherwise (`ir-roles.md`, `section-vocabulary.md:146`).

| Form | Example | IR Role |
|------|---------|---------|
| Binding | `ctx = inspect_repo(scope)` | `Step` with output binding |
| Bare call | `apply_changes(plan)` | `Step`, no output binding |
| Bare name | `validate_before_success` | `Step`, resolved via name resolution (`values-and-literals.md:119-131`) |
| Inline string | `"Mention any issues found."` | `Step` or `Context` (inferred) |
| Return | `return summarize(plan)` | `OutputContract` |
| If/elif/else | `if <cond>:` block | `Branch` container; body contains typed child nodes |

### Binding

A call result or literal value bound to a local name using `=`, as established in `calls-and-args.md:96-104` and `data-flow-and-calls.md:115-134`:

```glyph
flow:
    ctx = inspect_repo(scope)
    risk = "high"
    max_attempts = 3
```

The left side is a bare identifier. The right side is a call expression, literal, dot access, or another identifier reference.

### Bare Call

A call without a binding. The return value, if any, is discarded (`calls-and-args.md:107-111`):

```glyph
flow:
    apply_changes(plan)
    validate(plan)
```

### Bare Name

A bare identifier reference resolved through name resolution (`values-and-literals.md:119-131`). Distinguished from a zero-argument call by the absence of parentheses (`values-and-literals.md:131-136`):

```glyph
flow:
    validate_before_success
    preserve_existing_patterns
```

### Inline String

A one-off instruction as a quoted string (`authoring-surface.md:300-315`):

```glyph
flow:
    "Mention any docs you could not verify locally."
```

### Return

See the Return section below.

### If/Elif/Else

See the Branching section below.

## Branching: `if`/`elif`/`else`

### Syntax

Branching uses Python-style colon-terminated headers with significant indentation:

```glyph
flow:
    ctx = inspect_repo(scope)
    risk = assess_risk(ctx)

    if risk == "high" and ctx.has_tests:
        run_full_suite(ctx)
        request_review(ctx)
    elif risk == "high":
        "Flag for manual review — no test suite available."
    elif ctx.needs_update:
        apply_changes(ctx)
    else:
        "No action needed."

    return summarize(ctx)
```

The colon on `if`/`elif`/`else` does not conflict with sub-section header colons. `if`, `elif`, and `else` are reserved keywords (`values-and-literals.md:113`), not members of the sub-section header vocabulary (`section-vocabulary.md`). The parser distinguishes them by keyword identity.

### Rules

- `if <condition>:` introduces the first branch. Required.
- `elif <condition>:` introduces additional branches. Zero or more allowed.
- `else:` introduces the fallback branch. Optional. If omitted and no branch condition matches, execution continues to the next statement after the `if` chain.
- `elif` and `else` appear at the same indentation level as their matching `if`.
- No parentheses are required around conditions, but parentheses are allowed for grouping complex expressions.
- Branch bodies may contain any statement form allowed in `flow:`: bindings, bare calls, bare names, inline strings, `return`, and nested `if`/`elif`/`else`.

### Reserved Keywords

`elif` and `else` are added to the reserved keyword list established in `values-and-literals.md:113`. The full control-flow keyword set is: `if`, `elif`, `else`, `return`, `flow`.

## Condition Expression Vocabulary

Condition expressions inside `if` and `elif` are intentionally minimal in the MVP.

### Allowed Forms

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

### Operator Precedence

Standard Python precedence: `not` binds tightest, then `and`, then `or`. Parentheses override precedence.

```glyph
// Parsed as: (not a) or b
if not a or b:

// Explicit grouping
if not (a or b):
```

### Not Included In MVP

The following are excluded from the MVP condition vocabulary:

- Comparison operators: `<`, `>`, `<=`, `>=`
- Arithmetic operators: `+`, `-`, `*`, `/`
- Membership: `in`, `not in`
- Identity: `is`, `is not`
- Ternary expressions
- Nested boolean combinations beyond what `and`/`or`/`not`/parentheses provide

If a condition requires excluded operators, the author should bind the result of a call that computes the condition:

```glyph
exceeds_threshold = check_threshold(score, 0.8)
if exceeds_threshold:
    ...
```

## Dot Access

Single-level property dot access is allowed anywhere a value is expected: in `if` conditions, call arguments, bindings, and `return` expressions.

```glyph
flow:
    if ctx.has_tests:
        run_tests(ctx.test_dir)
    summary = format_output(ctx.findings)
    return summary
```

Property dot access reads a named property from a bound value. It is distinct from module-qualified access (`repo_tools.inspect_repo`), which is already established in `calls-and-args.md:136-153`. The parser distinguishes them by what the left side resolves to:

- If the left side is a whole-module import alias, it is module-qualified access.
- If the left side is a local binding, parameter, or other value reference, it is property dot access.

Only single-level dot access is allowed in the MVP. Chained access (`ctx.config.timeout`) is deferred (`calls-and-args.md:203`).

### IR Representation

Dot access normalizes to a `PropertyAccess` IR node:

```text
PropertyAccess {
  object: BindingRef(ctx),
  property: "has_tests",
  resolved_type: <inferred>
}
```

## Return

### Syntax

`return` produces the skill's or block's output value:

```glyph
flow:
    result = validate(plan)
    return summarize(result)
```

### Rules

- `return <expr>` where `<expr>` is a call, binding reference, dot access, literal, or `none`.
- `return` alone (no expression) is equivalent to `return none`.
- `return` may appear anywhere inside `flow:`, including inside `if`/`elif`/`else` branches, enabling early return:

```glyph
flow:
    if not scope.is_valid:
        return none
    ctx = inspect_repo(scope)
    return summarize(ctx)
```

- Every `export block` must have an explicit `return` on every code path (`data-flow-and-calls.md:148`). The compiler validates this statically.
- For `skill` and private `block`, there is no implicit return. If the declaration produces a value, it must use an explicit `return` statement. A `skill` or `block` that ends without `return` implicitly returns `none`.

### IR Mapping

`return` normalizes to a `Return` IR node that contributes to the `OutputContract`:

```text
Return {
  value: Call { target: summarize, args: { result: BindingRef(result) }, ... }
}
```

If the return expression is a call, the call is evaluated first and its result becomes the return value. This follows the same desugaring as nested calls (`calls-and-args.md:116-131`).

## No Nested `flow:`

`flow:` is a sub-section header (`section-vocabulary.md`), not a nestable control-flow construct. Writing `flow:` inside a `flow:` body is a compile error.

If a skill or block needs a sub-workflow, factor it into a separate `block` declaration and call it:

```glyph
block run_validation(ctx) -> ValidationResult
    flow:
        run_tests(ctx)
        check_coverage(ctx)
        return validation_report(ctx)

skill implement_feature(scope)
    flow:
        ctx = inspect_repo(scope)
        apply_changes(ctx)
        result = run_validation(ctx)
        return summarize(result)
```

## Indentation

Control-flow indentation follows `block-structure.md` with 4-space indent units:

```
Level 0 (col 0):   top-level declarations (skill, block)
Level 1 (col 4):   section headers (flow:, effects:), body-level constraints
Level 2 (col 8):   flow statements, if/elif/else headers
Level 3 (col 12):  if/elif/else body statements
Level 4 (col 16):  nested if body (if inside else, etc.)
```

- Blank lines inside `if`/`elif`/`else` bodies are visual separators and do not close or break blocks (`block-structure.md:57-59`).
- `elif` and `else` appear at the same indent level as their matching `if`.
- Deeper nesting is structurally supported but discouraged by convention. If branching logic grows beyond two nesting levels, consider extracting a helper `block`.

## IR Mapping Summary

| Source Form | IR Node | Role |
|-------------|---------|------|
| `x = call(...)` | `Call { output: Binding(x), ... }` | `Step` |
| `call(...)` | `Call { output: none, ... }` | `Step` |
| `bare_name` | `InstructionRef { name }` | `Step` |
| `"text"` | `InlineInstruction { text }` | `Step` or `Context` (inferred) |
| `return expr` | `Return { value }` | `OutputContract` |
| `if/elif/else` | `Branch { condition, then_body, elif_branches, else_body }` | Container |
| `x.prop` | `PropertyAccess { object, property }` | Value expression |

The `Branch` IR node is a container. Its children carry their own roles (`Step`, `OutputContract`, etc.). The branch itself does not have an instruction role — it structures the execution path.

### Branch IR Shape

```text
Branch {
  condition: BinaryOp { op: "==", left: BindingRef(risk), right: String("high") },
  then_body: [
    Call { target: run_full_suite, ... },
  ],
  elif_branches: [
    {
      condition: PropertyAccess { object: BindingRef(ctx), property: "needs_update" },
      body: [
        Call { target: apply_changes, ... },
      ]
    }
  ],
  else_body: [
    InlineInstruction { text: "No action needed." }
  ]
}
```

## Compiled Output Projection

Conditional logic in `flow:` flattens into prose instructions in `### Steps` under `## Instructions` (`compiled-output.md:152`). The compiled output does not use code-like branching syntax:

```md
### Steps

1. Inspect the repository within the given scope.
2. Assess the risk level.
3. If the risk is high and tests exist, run the full test suite and request review.
   If the risk is high but no tests exist, flag for manual review.
   If the context needs an update, apply changes.
   Otherwise, no action is needed.
4. Return a summary of the context.
```

Each `if`/`elif`/`else` chain compiles into a single numbered step with conditional sub-instructions. Simple single-branch `if` statements may compile into a single conditional sentence within a step.

## Interaction With Other Design Areas

- **Block structure** (`block-structure.md`): `if`/`elif`/`else` follow the same significant-indentation rules. The colon on `if <cond>:` is structurally identical to the colon on sub-section headers but disambiguated by keyword identity. Deferred question from `block-structure.md:153` is resolved here: `if` uses a colon.
- **Calls and args** (`calls-and-args.md`): Calls inside branches use the same positional-then-named argument syntax. Dot access for property reads extends the value vocabulary available at call sites.
- **Values and literals** (`values-and-literals.md`): `elif` and `else` are added to the reserved keyword list. `and`, `or`, `not` are added as reserved operator keywords. Condition operands follow the same literal and identifier rules.
- **Section vocabulary** (`section-vocabulary.md`): `flow:` content rules are extended to include `if`/`elif`/`else` branching. The `flow:` section description in `section-vocabulary.md:25` already lists `if` as allowed content.
- **IR roles** (`ir-roles.md`): `Branch` is a structural container, not an instruction role. Children of branches carry their own roles. This follows the principle that roles classify author intent, not structural position.
- **Compiled output** (`compiled-output.md`): Conditional logic flattens to prose per `compiled-output.md:152`. No code-like branching in compiled Markdown.

## Deferred

- `for_each` loop syntax (post-MVP per principle 7).
- Chained dot access (`ctx.config.timeout`) — deferred with deeper qualified nesting (`calls-and-args.md:203`).
- Comparison operators (`<`, `>`, `<=`, `>=`) in conditions.
- Pattern matching or `match`/`case` constructs.
- Exception handling / `try`/`catch`.
- Guard clauses or precondition syntax distinct from `if`.
- Whether the compiler should warn on unreachable code after unconditional `return` inside branches.
