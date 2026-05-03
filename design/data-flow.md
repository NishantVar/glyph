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

skill implement_feature(scope = ".", risk = "medium")

block review_changes(files: FileSet, strict = true) -> ReviewResult
```

Source-level parameters may be duck-typed. The IR resolves each parameter to an explicit type or structural contract before output generation.

## Global Preferences

Skills may depend on user or project preferences such as terminal multiplexer, communication style, validation strictness, preferred tools, or project conventions. In Glyph, these are ordinary `export const` declarations in a preferences file, imported like any other name.

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

Local bindings are scoped to the smallest enclosing structural region:

- A binding introduced at the top level of `flow:` (or at the body level of a `skill` / `block` outside any `if`/`elif`/`else`) is scoped to the entire enclosing `skill` or `block` and is visible to all subsequent flow statements in that declaration.
- A binding introduced inside an `if`, `elif`, or `else` branch body is scoped **only to that branch body**. It is not visible after the conditional ends, nor in sibling branches. Re-binding the same name in a different branch is allowed because the scopes do not overlap.

Across either scope, a binding should not silently shadow an existing binding if doing so would make data flow ambiguous (per `values-and-names.md` §No Shadowing — name collisions across overlapping scopes are hard errors).

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

The left side must be a whole-module import alias (`language-surface.md`).

No computed callees and no first-class functions in the MVP.

### UFCS (Uniform Function Call Syntax)

A value may be used as the receiver of a function call using dot syntax. `x.foo(args)` desugars to `foo(x, args)` — the receiver becomes the first argument.

```glyph
import "@glyph/std" { subagent, send }

skill investigate(scope = ".")
    effects: spawns_agent

    flow:
        researcher = subagent(scope) with "investigate this area"
        researcher.send("Check edge cases around token expiry.")
        return researcher
```

`researcher.send(msg)` desugars to `send(researcher, msg)`. The compiler resolves `send` through normal name resolution, then checks that the receiver's type matches the first parameter's declared type.

**Rules:**

- `<value>.<name>(args)` desugars to `<name>(<value>, args)` during Lower.
- The receiver may be any value expression: a binding, a parameter, a call result, or a dot access.
- `<name>` is resolved through the standard name resolution order (`values-and-names.md`): same-file binding, explicit import, stdlib. It must resolve to a callable (`block`, `export block`, or stdlib primitive).
- If the resolved callable's first parameter has a type annotation, the receiver's type must match it (nominal matching per `types.md`). If either side is untyped, no check is performed.
- `with` modifiers work: `researcher.send(msg) with "be thorough"` desugars to `send(researcher, msg) with "be thorough"`.
- UFCS calls may bind results: `result = items.transform(filter)` desugars to `result = transform(items, filter)`.
- Zero additional arguments are allowed: `agent.finish()` desugars to `finish(agent)`.

**Disambiguation from qualified callees:**

Both UFCS and qualified callees use dot syntax. The compiler disambiguates in Analyze:

- If the name before the dot resolves to a **whole-module import alias** → qualified callee (`repo_tools.inspect_repo(scope)`).
- If it resolves to a **value binding, parameter, or other value expression** → UFCS method call (`researcher.send(msg)`).
- If it resolves to a **`block` or `export block` declaration** (or a `module_alias.block_name` form), the method name is `applies`, the call has zero arguments, and the call appears in an `if` / `elif` condition position → block trigger predicate (not UFCS, not a qualified callee — see §Condition Expressions and `ir-and-semantics.md` §Block Trigger Predicate). The form is preserved as a special syntactic shape on the Branch's `condition` string and is not desugared.
- If it resolves to neither → diagnostic (unresolved name).

This check is unambiguous because import aliases and value bindings occupy distinct namespaces and cannot collide (per `values-and-names.md` no-shadowing rules).

### Bare Name vs Call Distinction

A zero-argument call uses empty parentheses to distinguish it from a bare name reference:

```glyph
summarize()                    // zero-argument call
summarize                      // bare name reference (const, parameter, binding)
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
- Applies to bare calls (`foo()`), qualified calls (`Alias.foo()`), UFCS calls (`x.foo()`), and calls inside bindings (`x = foo()`). Does not apply to bare-name statements (no parens).

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

- UFCS calls are desugared: `x.foo(args)` becomes `foo(x, args)` with the receiver as the first positional argument.
- All positional arguments are resolved to named arg-to-param mappings. The IR has no positional arguments.
- Nested calls are desugared into sequential flat calls with compiler-generated temporary bindings.
- Default values are filled in for omitted optional parameters.
- The callee is resolved to its declaration (same-file block, imported block, or standard-library primitive).
- Effects are inferred from the callee's declared or inferred effect set.

### Repair Behavior At Call Sites

The LLM repair pass may add minimal syntax when call-site data flow cannot compile: adding explicit argument names when positional mapping is ambiguous, adding missing type annotations on bindings when inference fails, or renaming a local binding only if the current name collides and no smaller repair exists. Repair should not rewrite the call structure, reorder arguments, or expand shorthand instruction names into prose. Full repair rules are in `repair.md`.

## Control Flow

### Statement Forms Inside `flow:`

Nine statement forms are allowed inside `flow:` blocks. All content defaults to the `Step` IR role unless explicit syntax or resolved metadata says otherwise.

| Form | Example | IR Role |
|------|---------|---------|
| Binding | `ctx = inspect_repo(scope)` | `Step` with output binding |
| Bare call | `apply_changes(plan)` | `Step`, no output binding |
| UFCS call | `researcher.send("check edges")` | `Step`, desugars to `send(researcher, ...)` |
| Bare name | `validate_before_success` | `Step`, resolved via name resolution |
| Inline string | `"Mention any issues found."` | `Step` |
| Constraint marker | `avoid unrelated_edits` | `Constraint` (hoisted or inlined; see below) |
| Context marker | `context project_conventions` | `Context` (hoisted to `context:` or inlined in branch prose) |
| Return | `return summarize(plan)` | `OutputContract` |
| If/elif/else | `if <cond>:` block | `Branch` container |

A call without a binding is a statement call -- the return value, if any, is discarded. Both binding and bare-call forms occupy one line each unless the argument list wraps inside parentheses.

A **constraint marker** (`require <name>`, `avoid <name>`, `must <name>`, `must avoid <name>`, or any of those forms with an inline string in place of the bare name) parses to a `Constraint` IR node admitted in the flow's `FlowNode` union (`ir-schema.md` §Flow Nodes). Lower (`pipeline.md` Phase 4) splits these by location: a constraint marker at flow top-level is hoisted into the enclosing declaration's `constraints` list; a constraint marker inside an `if`/`elif`/`else` branch body stays inline and is rendered as part of the conditional Step prose by Expand. See `ir-and-semantics.md` §Flow-Level Constraint Markers and `compiled-output.md` §Constraint Rendering for the projection rules.

A **context marker** (`context <name>` or `context "<inline string>"`) parses to a `ContextNode` IR node, also admitted in the `FlowNode` union. The same hoisting rules apply: a context marker at flow top-level is hoisted into the declaration's `context` list; a context marker inside a branch body stays inline. See `ir-and-semantics.md` for full context marker rules.

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
- Branch bodies may contain any statement form allowed in `flow:` *except* `return`: bindings, bare calls, bare names, inline strings, constraint markers, and nested `if`/`elif`/`else`. `return` is restricted to the top level of `flow:` (see §Return Semantics).
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
| Block trigger predicate | `if fork_with_plan.applies():` |

The block-trigger form `BLOCKNAME.applies()` is a special syntactic shape (not UFCS) for description-driven dispatch. Receiver must resolve to a `block` / `export block` (or `module_alias.block_name`) carrying `description:`; `applies` takes zero arguments; parens are required. See `ir-and-semantics.md` §Block Trigger Predicate for full semantics, required-when-consulted rule, and resolution behavior. It composes with `and`/`or`/`not` like any other Boolean.

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

> **Compiled-output note:** The `OutputContract` role is an IR-level concept used for type-checking, importer validation, and visualization. In compiled Markdown, the `Return` node **folds into the closing sentence of the final `### Steps` item** — there is no dedicated `### Returns` or `### Output` section in MVP. See `compiled-output.md` §Return Folding and `ir-and-semantics.md` §Roles for details.

### Compiled-Output Projection

Conditional logic in `flow:` projects to a **single numbered Step** with **lettered sub-steps per arm** under `### Steps` in `## Instructions` (`compiled-output.md`). Each arm is introduced by a condition header (`If <condition>:` for `if`/`elif`, `Otherwise:` for `else`), and each Step-projecting node inside the arm becomes a lettered sub-step (`a.`, `b.`, `c.`). Letters reset per arm.

```md
3. If the risk is high and tests exist:
   a. Run the full test suite.
   b. Request a code review.
   If the risk is high but no tests are available:
   a. Flag for manual review.
   Otherwise:
   a. No action needed.
```

Nested branches (a `Branch` inside another `Branch`'s arm) flatten into prose within their parent sub-step rather than producing their own sub-step structure. Only one level of structured sub-steps is supported. The Repair pass auto-extracts deeply nested branches into helper `generated block` declarations (see `repair.md` §4.9).

Branch-scoped constraints inline into adjacent sub-step prose rather than receiving their own letter. Bindings inside arms project their call as a lettered sub-step; the binding name is invisible in compiled output.

## Return Semantics

`return` produces the skill's or block's output value:

```glyph
flow:
    result = validate(plan)
    return summarize(result)
```

Rules:

- `return <expr>` where `<expr>` is a call, binding reference, dot access, literal, `none`, or an output target identifier (`<name>`).
- `return` alone (no expression) is equivalent to `return none`.
- `return <name>` marks an agent-synthesized output target. The name must be identifier-shaped and must not shadow an existing visible binding. Lower records it as an `OutputContract { target_name, ty, source }` using the enclosing declaration's `-> DomainType` annotation for `ty`. Expand folds it into natural prose; the literal `<name>` token must not appear in compiled Markdown.
- **Single, terminal-only.** Exactly **one** `return` statement per `skill`, `block`, or `export block`, and it must appear as the **last statement at the top level of `flow:`**. `return` is **not** allowed inside `if`/`elif`/`else` branch bodies (no early return). Multiple `return` statements in a single `flow:` are a parse error.
- **Implicit vs. explicit:** If `return` is omitted, the body implicitly returns `none`. This applies to `skill` and private `block` declarations. **`export block` requires an explicit `return`** (even `return none`) because its output is a public contract visible to importers (see `language-surface.md` §3.3). Export blocks with a meaningful return must also declare `-> DomainType` on the header; export blocks with no meaningful return omit `->` entirely. The compiler inserts an implicit `Return { value: none }` during Lower only for `skill` and `block` — omitting `return` in an `export block` is a repairable diagnostic (`G::analyze::missing-return`). There is no per-path return-coverage analysis, because `return` is forbidden in branches and only appears once at the end of `flow:` (or not at all).

Parse-level diagnostics enforcing this rule:

| ID | Trigger |
|---|---|
| `G::parse::return-not-terminal` | `return` appears before the last statement of `flow:` |
| `G::parse::return-in-branch` | `return` appears inside an `if`/`elif`/`else` body |
| `G::parse::multiple-returns` | More than one `return` in a single `flow:` |
| `G::parse::output-target-outside-return` | `<name>` output-target form appears outside a terminal top-level `return` |

(See `todo.md` for the deferred consideration of branch-nested early returns.)

Returns become explicit output contracts in the IR. If the return expression is a call, the call is evaluated first and its result becomes the return value, following the same desugaring as nested calls.

### Runtime Semantics

At execution time, a return value is **the agent's final output string**. When a skill is called via `subagent(...)` or bound with `x = skill_name(...)`, the binding name refers to "the agent's output from that step" — a plain text string that subsequent prose references as "the result from step N above" or similar.

Return type annotations (e.g. `-> Plan`) are **advisory only** in MVP. The compiler uses the declared type to shape the prose framing of the final Step ("Your output should be a Plan containing: ...") and to perform nominal matching at call boundaries. There is no runtime parsing, structural enforcement, or schema validation — the target agent produces text, and the author trusts it matches the declared type.

## Closure And Scope Rules

### Exported Block Closure

Only `export block` declarations may be imported by other `.glyph.md` files. The compiler enforces that every exported block is closed before it can be compiled as an importable unit.

A closed exported block may depend on:

- its parameters;
- local bindings declared inside the block;
- same-file `const` declarations;
- explicit imports;
- standard primitives or standard-library entries;
- declared constraints, outputs, and effects.

An exported block must not depend on hidden caller context, private names from an importing file, undeclared globals, or implicit project assumptions.

Private `block`s are not importable and may only be called from the same source file. Any private block reachable from an exported block must itself be closed under the exported block's declared contract. Private blocks may rely on their enclosing skill context in the MVP, including values and instructions already visible in that skill.

### Closure Across Imports

Closure is enforced **once per file, at the export boundary**, never transitively across imports. When file X imports `do_thing` from file Y:

- X sees only `do_thing`'s **declared contract**: parameters, return type, declared `effects:`, declared `constraints:`. Private declarations in Y are invisible to X, even when reachable transitively from `do_thing`'s body.
- Y's compilation must have already produced a `do_thing` whose declared `effects:` is a superset of all effects inferred from its body (including effects of any private blocks it inlines). This check happens locally in Y's Phase 5 (Validate); see `pipeline.md`.
- X's compilation never re-analyses Y's interior. This preserves the multi-file compile order in `imports.md` (a dependency must pass Phase 5 before its importer can Analyze) and keeps cacheability honest.

This is the only model compatible with encapsulation: Y can refactor private helpers without breaking X.

#### Effect Propagation (Hard Rule)

When an `export block` (or any callable being compiled) calls or inlines another callable, the caller's declared `effects:` **must be a superset** of every imported callee's declared effects and every inlined private callee's inferred effects.

- This is a Validate (Phase 5) **error** if violated, not a warning.
- Repair (Phase 3) may add missing effect keywords to the caller's `effects:` when the missing effects are unambiguously implied by the call graph.
- Mechanism: effects are global per-skill (they project to YAML frontmatter in `compiled-output.md`), so the agent runtime sees the full set when it runs the skill. A caller that omits an inlined callee's effect produces a frontmatter that lies about the skill's behavior — hence the error.

#### Constraint Scoping (No Top-Level Propagation)

When a caller calls a `block` or `export block`, the callee's declared `constraints:` **stay scoped to the inlined region** of that call. They are **not** merged into the caller's top-level `### Constraints` section.

- IR representation: when Lower resolves the call, the resolved-call node carries the callee's constraints attached as scoped metadata. They are not added to the caller's top-level `Constraint` IR nodes.
- Expand projection: Phase 6 Step 2 weaves these scoped constraints into the prose of the inlined steps (or as a localized phrase preceding/following the inlined region). See `expand.md` §Scoped Constraint Inlining.
- Compiled output: the caller's `### Constraints` section lists only the caller's own declared constraints. The agent reading the compiled `.md` sees the callee's constraints in context, governing only the inlined span — not the whole skill.

Why the asymmetry with effects: frontmatter is global per-skill, so effects must propagate. Step prose is local, so constraints can — and should — stay scoped to the steps they govern. Hoisting a callee's constraint to the caller's top level would over-apply it to the caller's own steps.

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
- Pipeline or chaining syntax (beyond UFCS).
- Runtime preference injection or override mechanism (see `preferences.md`).
