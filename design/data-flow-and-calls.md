# Glyph Data Flow And Calls

This document defines how Glyph source passes values between skills, blocks, and function-like calls. It records the semantic contract, not final syntax.

## Goals

Data flow in Glyph should be:

- readable at the source level;
- explicit enough for analysis and visualization;
- permissive enough for duck-typed authoring;
- strict enough in the IR for validation and compilation.

Authors should be able to call blocks or primitive operations with variables, bind returned values, and pass those values into later calls.

## Core Model

Glyph has values, local bindings, parameters, calls, and returns.

Example:

```glyph
block inspect_repo(scope) -> RepoContext
    ...

block make_plan(ctx, risk = "medium") -> Plan
    ...

skill implement_feature(scope, risk = "medium")
    flow:
        ctx = inspect_repo(scope)
        plan = make_plan(ctx, risk)
        apply_changes(plan)
        result = validate(plan)
        return summarize(result)
```

The source may omit many types. The compiler infers what it can, repairs source only when needed, and normalizes calls into typed IR nodes.

The MVP has two block forms:

- `block` for private helpers inside the current source file.
- `export block` for importable helpers that must be self-contained.

## Parameters

`skill`, `block`, and `export block` definitions may declare parameters.

Skill parameters may be omitted entirely when the skill does not need explicit invocation inputs.

Parameters may have:

- a name;
- an optional type annotation;
- an optional default value;
- an optional role or effect annotation, if later design requires it.

Examples:

```glyph
skill update_docs()

skill implement_feature(scope, risk = "medium")

block review_changes(files: FileSet, strict = true) -> ReviewResult
```

Source-level parameters may be duck-typed. The IR should resolve each parameter to an explicit type or structural contract before output generation.

## Global Preference Parameters

Skills may accept global preference parameters: selected user or project preferences stored in one configured place and passed into the skill as explicit named inputs.

Global preference parameters are not hidden ambient context. The source should make the dependency visible, and the IR should represent the resolved preference as an input to the skill.

Illustrative syntax, not final:

```glyph
skill open_terminal(terminal_mux = pref("terminal.mux"))

skill implement_feature(scope, validation_strictness = pref("validation.strictness"))
```

Global preferences are appropriate for stable user choices such as terminal multiplexer, communication style, validation strictness, preferred tools, or project conventions.

In the MVP, global preferences resolve at compile time and are injected into the compiled Markdown as ordinary explicit skill inputs or instructions. If a preference changes, affected skills should be recompiled. The compiler may maintain a reverse dependency map from preference keys to source files to identify which compiled files are stale.

Runtime preference injection may be added later through a Glyph-aware loader or hook that substitutes preference values before the agent reads the compiled skill, but that is not the MVP default.

An `export block` should not read global preferences implicitly; if an exported block needs a preference, the skill or caller should pass that value as a normal parameter so the exported block remains closed.

## Calls

Calls may pass arguments positionally, by name, or both if the final syntax permits it.

Examples:

```glyph
plan = make_plan(ctx, risk)
review = review_changes(files, strict = true)
summary = summarize(result)
```

The compiler must resolve:

- which callable is being invoked;
- which argument maps to which parameter;
- which arguments are required;
- which defaults are used;
- what value, if any, the call returns;
- what effects, if any, the call has.

## Local Bindings

A call result can be bound to a local name and reused later.

Example:

```glyph
ctx = inspect_repo(scope)
plan = make_plan(ctx)
apply_changes(plan)
```

Local bindings are scoped to the enclosing `skill` or `block`. Nested private blocks may introduce nested scopes, Python-style. A binding should not silently shadow an existing binding if doing so would make data flow ambiguous.

Bindings may also hold non-call values:

```glyph
risk = "high"
max_attempts = 3
```

The source may omit types. The compiler infers or repairs types as needed.

## Return Values

Skills, private blocks, and exported blocks may return values.

Example:

```glyph
block validate(plan) -> ValidationResult
    result = run_tests(plan)
    return result
```

Returns should become explicit output contracts in the IR. Every `export block` must have an explicit return path, even if it returns `none`. This allows instruction-only exported blocks while preserving a clear import contract. Private blocks may omit return type annotations; if a block declares a return type, every return path must match it after type inference and repair.

## Exported Block Closure

Only `export block` declarations may be imported as blocks by other `.glyph.md` files. The compiler must enforce that every exported block is closed before it can be compiled as an importable unit.

A closed exported block may depend on:

- its parameters;
- local bindings declared inside the block;
- same-file `text` declarations;
- explicit imports;
- standard primitives or standard-library entries;
- declared constraints;
- declared outputs;
- declared effects.

An exported block must not depend on hidden caller context, private names from an importing file, undeclared globals, or implicit project assumptions. Closed does not mean pure: an exported block may be effectful if its meaningful effects are declared.

Private `block`s are not importable and may only be called from the same source file. They may serve as implementation details for a `skill` or `export block`, but any private block reachable from an exported block must itself be closed under the exported block's declared contract.

Private blocks may rely on their enclosing skill context in the MVP, including values and instructions already visible in that skill. This is intentionally more permissive than `export block` closure. The exact solution for analyzing and visualizing private block context dependencies remains an open design problem.

An exported block may call another imported exported block. The caller should inherit or expose the callee's relevant effects and constraints so import contracts remain visible to downstream callers.

## Duck Typing And Structural Compatibility

Glyph source should support Python-like duck typing: a call can accept a value if the value has the structure the callee needs, even when the author did not name an exact type.

Example:

```glyph
block summarize_findings(report_like)
    return concise_summary(report_like.findings)
```

The source may stay lightweight, but the IR must record an explicit structural requirement such as:

```text
ParameterRequirement(report_like has field findings)
```

This keeps authoring flexible while preserving compile-time analyzability.

## Effects

Some calls only compute values. Others read files, call tools, edit code, ask the user, or produce external artifacts.

The design should distinguish value flow from effects:

```glyph
ctx = inspect_repo(scope)       // reads repository
apply_changes(plan)            // writes files
result = run_validation(plan)  // executes validation commands
```

The IR should record meaningful effects so the compiler can validate ordering, surface risks, and generate reliable agent instructions.

Effects may apply to skills, exported blocks, private blocks, and primitive calls. For exported blocks, effect declarations are part of the import contract. An imported exported block should carry its effect metadata with it so callers can be validated without inspecting hidden implementation context.

If a skill or block has no meaningful effects, the source may omit `effects:` and the compiler should treat that as `effects: none`.

The MVP effect vocabulary should start coarse:

- `none` for pure or near-pure blocks that do not rely on external actions.
- `reads_files` for inspecting files, repository state, logs, or other local artifacts.
- `reads_env` for reading environment variables, system state, git metadata, or project configuration that is not file content.
- `writes_files` for creating or modifying files.
- `runs_commands` for invoking shell commands, test runners, formatters, package managers, or similar tools.
- `uses_network` for web access, package downloads, API calls, or remote service calls.
- `asks_user` for pausing to request human input or approval.
- `creates_artifacts` for producing durable outputs such as reports, generated assets, compiled Markdown files, archives, or exported data.

This list is intentionally small. New effects are introduced through additive-only changes; existing effect keywords are never renamed or removed once stabilized. The full extension policy is defined in [effects.md](effects.md).

## IR Normalization

Source calls should normalize into explicit IR nodes.

Example source:

```glyph
review = review_changes(files, strict = true)
```

Possible IR shape:

```text
Call {
  target: review_changes,
  args: {
    files: BindingRef(files),
    strict: Boolean(true)
  },
  output: Binding(review),
  return_type: ReviewResult,
  effects: [reads_files]
}
```

The exact IR syntax is not decided. The semantic requirement is that calls become analyzable nodes with explicit target, arguments, output binding, type, and effects.

## Repair Behavior

The LLM repair pass may add minimal syntax when data flow cannot compile.

Allowed repairs include:

- adding missing type annotations when inference fails;
- adding explicit argument names when positional mapping is ambiguous;
- renaming a local binding only if the current name collides and no smaller repair exists;
- adding missing return annotations when the return contract is ambiguous.

Repair should not rewrite the workflow or expand shorthand instruction names into prose.

## Visualization

Data flow should be visualizable as a graph:

- parameters are entry nodes;
- calls are operation nodes;
- bindings are value edges;
- returns are exit nodes;
- effects are annotations on call nodes.

This is one reason hidden ambient context should be minimized. If a call depends on a value, that value should be visible either as an argument or as a declared dependency.

## Open Syntax Choices

The following details remain open:

- ~~whether argument passing should allow both positional and named arguments~~ — decided in `calls-and-args.md`: positional-then-named, no positional after named.
- ~~whether return types use `-> Type`, a block field, or inference only~~ — decided in `declaration-headers.md`: `-> ReturnType` syntax.
- whether nested blocks create nested variable scopes;
- how much structural type information authors can write directly.
- whether private blocks reachable from an `export block` must be explicitly annotated as closed or can be proven closed by the compiler.
