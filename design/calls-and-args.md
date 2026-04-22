# Glyph Call-Site And Argument Syntax

This document defines how arguments are passed at call sites in Glyph source: positional and named argument rules, delimiters, defaults, trailing commas, line wrapping, callee resolution, binding, nesting, and IR normalization.

## Status

MVP Tier 2. Formalizes the call-site conventions already illustrated in `data-flow-and-calls.md` and `authoring-surface.md`, building on the parameter declaration syntax fixed in `declaration-headers.md`.

## Positional And Named Arguments

Glyph supports both positional and named arguments at call sites. Positional arguments must precede named arguments — no positional argument may follow a named argument in the same call.

```glyph
plan = make_plan(ctx, risk)                    // all positional
plan = make_plan(ctx, risk = "high")           // mixed: positional then named
plan = make_plan(ctx = context, risk = "high") // all named
```

This mirrors the parameter ordering rule in `declaration-headers.md:204` (required parameters before optional) and aligns with principle 4 (Python-like readability).

### Rules

- Positional arguments are matched to parameters left-to-right in declaration order.
- Named arguments use `name = value` syntax, where `name` matches a declared parameter name.
- A named argument may not duplicate a parameter already filled by a positional argument. This is a compile error.
- All required parameters (those without defaults) must be supplied, either positionally or by name.
- The compiler resolves all positional arguments to named arg-to-param mappings during IR normalization; the IR contains only named arguments.

### Rejected Alternatives

- **Named-only:** Too verbose for the common 1–2 argument case. Glyph skills typically have short parameter lists where position is unambiguous.
- **Free mixing (positional after named):** Ambiguous for IR normalization. The compiler would need complex resolution rules and repair would struggle with ambiguous mappings.

## Argument Separator And Delimiters

Arguments are comma-separated inside parentheses. This is already established across all existing examples in `data-flow-and-calls.md`, `block-structure.md`, and `declaration-headers.md`.

```glyph
result = validate(plan, strict = true)
```

Parentheses appear only when arguments exist, matching the declaration-side rule in `declaration-headers.md:16`. A zero-argument call uses empty parentheses to distinguish it from a bare name reference:

```glyph
summarize()                    // zero-argument call
summarize                      // bare name reference (text, parameter, binding)
```

This distinction is established in `values-and-literals.md:131-136`.

## Default-Value Interaction

Omitting an argument uses the parameter's declared default from the header (`declaration-headers.md:196-199`). To skip an optional positional parameter and supply a later one, the caller must switch to named arguments.

```glyph
// Given: block make_plan(ctx, risk = "medium", verbose = false)
make_plan(ctx)                         // risk and verbose use defaults
make_plan(ctx, "high")                 // risk = "high", verbose uses default
make_plan(ctx, verbose = true)         // named arg skips risk, which uses default
```

There is no placeholder or sentinel value for "use the default." Omission or named-argument skipping are the only mechanisms.

## Trailing Commas

Trailing commas are allowed in all argument lists, both single-line and multi-line. They are optional and never required.

```glyph
make_plan(ctx, risk = "high",)         // single-line trailing comma: allowed

make_plan(
    ctx,
    risk = "high",
    verbose = true,                    // multi-line trailing comma: allowed
)
```

This is consistent with selective import syntax (`declaration-headers.md:176-179`) and the multi-line call example in `block-structure.md:80-84`.

## Line Wrapping

Call arguments may span multiple lines using the implicit line-continuation rule for paired delimiters defined in `block-structure.md:63-68`. Inside parentheses, indentation is not structurally significant. The logical line is not complete until delimiters balance.

```glyph
plan = make_plan(
    ctx,
    risk = "high",
    verbose = true,
)
```

No backslash continuation is needed or supported. This matches `block-structure.md:69`.

## Call Results And Bindings

A call result can be bound to a local name using `=`:

```glyph
ctx = inspect_repo(scope)
plan = make_plan(ctx, risk)
apply_changes(plan)
```

This confirms the `x = call(...)` shape established in `data-flow-and-calls.md:119-133`. The left side is a bare identifier; the right side is a call expression. Bindings may also hold non-call values (`risk = "high"`, `max_attempts = 3`).

A call without a binding is a statement call — the return value, if any, is discarded:

```glyph
apply_changes(plan)            // statement call, result discarded
```

Both forms occupy one line each (unless the argument list wraps across lines inside parentheses).

## Nested Calls

Nested calls are allowed in the MVP. A call may appear as an argument to another call:

```glyph
result = validate(make_plan(ctx, risk))
apply_changes(merge(base, overlay))
```

The compiler desugars nested calls into flat IR nodes by introducing temporary bindings. The source form above normalizes to:

```text
Call { target: make_plan, args: {ctx: ..., risk: ...}, output: Binding(_tmp_1), ... }
Call { target: validate, args: {plan: BindingRef(_tmp_1)}, output: Binding(result), ... }
```

This keeps the source ergonomic for authors while preserving a flat, visualizable data-flow graph in the IR (principle 17, `data-flow-and-calls.md:264-273`). Each desugared call becomes its own node in the graph with explicit edges.

Deeply nested calls (three or more levels) are legal but discouraged by convention — intermediate bindings with descriptive names improve readability (principle 1).

## Callee Resolution

A callee is one of two forms:

**Bare identifier** — resolved through the standard name resolution order in `values-and-literals.md:119-131`:

```glyph
plan = make_plan(ctx)
```

**Qualified name** — single-level dot access where the left side is a whole-module import alias:

```glyph
import "./repo_tools.glyph.md" as repo_tools

ctx = repo_tools.inspect_repo(scope)
repo_tools.validate_changes(ctx)
```

The left side must be a whole-module import alias (`declaration-headers.md:155-158`). Dots are reserved for module-qualified access (`values-and-literals.md:97`).

No computed callees, no first-class functions, and no method-style calls in the MVP.

## Effects At Call Sites

Effects are declared on callables (via the `effects:` clause in the body per `effects.md`), not at call sites. The compiler infers and propagates effects through the call graph (`effects.md:76-83`). Call-site syntax does not include effect annotations.

## IR Call-Node Normalization

Every source call normalizes to an explicit IR node with resolved target, named arguments, output binding, return type, and effects (citing `data-flow-and-calls.md:234-246`):

```text
Call {
  target: <resolved identifier or qualified name>,
  args: { <param_name>: <value_or_ref>, ... },
  output: Binding(<name>) | none,
  return_type: <resolved type>,
  effects: [<inferred effect set>]
}
```

Key normalization steps:

- All positional arguments are resolved to named arg-to-param mappings. The IR has no positional arguments.
- Nested calls are desugared into sequential flat calls with compiler-generated temporary bindings.
- Default values are filled in for omitted optional parameters.
- The callee is resolved to its declaration (same-file block, imported block, or standard-library primitive).
- Effects are inferred from the callee's declared or inferred effect set.

## Repair Behavior

The LLM repair pass may add minimal syntax when call-site data flow cannot compile (`data-flow-and-calls.md:252-260`):

- Adding explicit argument names when positional mapping is ambiguous.
- Adding missing type annotations on bindings when inference fails.
- Renaming a local binding only if the current name collides and no smaller repair exists.

Repair should not rewrite the call structure, reorder arguments, or expand shorthand instruction names into prose.

## Interaction With Other Design Areas

- **Declaration headers** (`declaration-headers.md`): Parameter syntax at the definition side is fixed. Call-site syntax mirrors it: `name = value` for named arguments matches `name = default` for parameter defaults.
- **Block structure** (`block-structure.md`): Line continuation inside parentheses is defined there. This document confirms that call-site argument lists use the same rule.
- **Values and literals** (`values-and-literals.md`): Argument values must be Tier 0 literals, parameter references, local bindings, imported names, or call expressions. The distinction between bare names and parenthesized calls is defined there.
- **Effects** (`effects.md`): Effect propagation through calls is defined there. This document confirms no call-site effect syntax.
- **Types** (`types.md`): Type checking at call boundaries uses nominal matching. The type system validates that argument values match parameter type annotations.

## Deferred

- Deeper qualified nesting (`a.b.c`) for nested module access.
- Spread/splat arguments for unpacking collections into argument lists.
- Variadic parameters accepting a variable number of arguments.
- Method-style calls (`x.method()`).
- Pipeline or chaining syntax.
- `pref(...)` global preference call syntax (noted in `data-flow-and-calls.md:79`, not finalized).
