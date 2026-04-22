# Glyph Declaration Headers

This document defines the exact header-line syntax for each MVP top-level declaration in Glyph source files. It covers keyword order, parameter lists, return type markers, and terminators.

## Status

Formalizes the header shapes already illustrated in `authoring-surface.md` and `data-flow-and-calls.md`.

## General Rules

All declaration headers share these properties:

- **No trailing colon.** Top-level declaration headers introduce their body through indentation on the next line. Colons are reserved for body-level sub-section headers (`flow:`, `effects:`, `constraints:`) as defined in `block-structure.md`.
- **No braces.** Body delimitation is indentation-based (principle 4, Python-like readability).
- **Parentheses always required on callable declarations.** `skill update_docs()` not `skill update_docs`. This applies to `skill`, `block`, and `export block` — all callable forms. Matches Python's `def foo():` convention. Value-binding declarations (`text`, `int`, `float` and their `export` / `generated` variants) and `import` do not use parentheses.

## 1. `skill`

One per file. The public entrypoint that compiles to Markdown agent instructions.

### Grammar

```
skill <name>()
skill <name>(<params>)
skill <name>(<params>) -> <ReturnType>
```

### Examples

```glyph
skill update_docs()

skill implement_feature(scope, risk = "medium")

skill implement_feature(scope, risk = "medium") -> Summary
```

### Notes

- Return type is optional and uncommon on skills. When present, it documents what the skill's `return` statement produces; this becomes the `## Output` section in compiled Markdown.
- A file must contain exactly one `skill` declaration.

## 2. `block`

Private helper block, scoped to the current file.

### Grammar

```
block <name>()
block <name>(<params>)
block <name>(<params>) -> <ReturnType>
```

### Examples

```glyph
block helper()

block validate(plan) -> ValidationResult

block make_plan(ctx, risk = "medium") -> Plan
```

### Notes

- Private blocks may rely on enclosing skill context in the MVP.
- Return type annotation is optional; if declared, every return path must match after type inference.

## 3. `export block`

Importable, self-contained reusable block. Two-keyword prefix.

### Grammar

```
export block <name>() -> <ReturnType>
export block <name>(<params>) -> <ReturnType>
```

### Examples

```glyph
export block inspect_failure(scope) -> FailureReport

export block validate_changes(files: FileSet, strict = true) -> ValidationResult

export block emit_safety_warning() -> None
```

### Notes

- Every `export block` must have an explicit `-> ReturnType`, even `-> none`, so callers have a clear contract (principle 19, `data-flow-and-calls.md`).
- Must be closed: behavior determined by declared inputs, local bindings, explicit imports, declared constraints, declared outputs, and declared effects.
- `effects:` clause appears in the body, not on the header line.

## 4. `text`

Named text block, private to the current file.

### Grammar

```
text <name> = <string-literal>
```

### Examples

```glyph
text preserve_existing_patterns = """
Prefer the repository's existing patterns, helper APIs, naming, and file organization
before introducing a new abstraction or style.
"""

text short_note = "Keep changes minimal."
```

### Notes

- No parameters, no return type. A `text` declaration is a named constant, not a callable.
- String literals follow `values-and-literals.md`: inline `"..."` or block `"""..."""`.
- The `=` is required and separates the name from its value.

## 5. `export text`

Importable named text block. Two-keyword prefix.

### Grammar

```
export text <name> = <string-literal>
```

### Examples

```glyph
export text safety_rules = """
Never execute destructive operations without confirmation.
"""

export text unrelated_edits = """
Do not modify code outside the specified scope.
"""
```

### Notes

- Identical to `text` except importable by other `.glyph.md` files.

## 6. `int`

Named integer value, private to the current file.

### Grammar

```
int <name> = <int-literal>
```

### Examples

```glyph
int max_attempts = 3

int offset = -1
```

### Notes

- No parameters, no return type. An `int` declaration is a named constant.
- Right-hand side must be an integer literal per `values-and-literals.md`. Arbitrary expressions are not supported in MVP.
- The `=` is required and separates the name from its value.

## 7. `export int`

Importable named integer value. Two-keyword prefix.

### Grammar

```
export int <name> = <int-literal>
```

### Examples

```glyph
export int default_max_attempts = 3
```

### Notes

- Identical to `int` except importable by other `.glyph.md` files.

## 8. `float`

Named floating-point value, private to the current file.

### Grammar

```
float <name> = <float-literal>
```

### Examples

```glyph
float threshold = 0.8

float ratio = 3.14
```

### Notes

- No parameters, no return type. A `float` declaration is a named constant.
- Right-hand side must be a float literal per `values-and-literals.md`. Integer literals on the right-hand side are rejected; use `int` instead. Lossless coercion at call boundaries is governed by `values-and-literals.md`, not by this declaration.
- The `=` is required and separates the name from its value.

## 9. `export float`

Importable named floating-point value. Two-keyword prefix.

### Grammar

```
export float <name> = <float-literal>
```

### Examples

```glyph
export float default_temperature = 0.7
```

### Notes

- Identical to `float` except importable by other `.glyph.md` files.

## 10. `import`

Brings exported declarations from another `.glyph.md` file into scope. Two forms: whole-module and selective.

### Grammar — Whole-Module

```
import "<path>" as <alias>
```

### Grammar — Selective

```
import "<path>" {
    <name>,
    <name> as <alias>,
    ...
}
```

### Examples

```glyph
import "./repo_tools.glyph.md" as repo_tools

import "./coding_agent_safety.glyph.md" {
    unrelated_edits,
    preserve_existing_patterns as existing_patterns,
    validate_before_success,
}
```

### Rules

- Path is always a quoted string.
- Whole-module import requires `as <alias>`. No bare module imports; the alias forces an explicit namespace.
- Whole-module import exposes: the file's `skill` entrypoint, all `export block` declarations, and all `export text` declarations. Private `text` and private `block` stay hidden.
- Selective import uses `{ name, name as alias, ... }`. Trailing comma allowed. Only explicitly exported declarations may be named.
- A single import statement is either whole-module or selective, not both.
- Circular imports are rejected in the MVP (boundary 11, `authoring-surface.md`).

## Parameter Syntax

Parameters appear inside parentheses on `skill`, `block`, and `export block` headers. Four forms combine two optional features: type annotation and default value.

```
name                          // untyped, required
name = "default"              // untyped, with default
name: Type                    // typed, required
name: Type = default_value    // typed, with default
```

### Rules

- Required parameters (no default) must precede optional parameters (with default). Same ordering rule as Python.
- Type annotations use the `name: Type` slot. The full type system is defined in `types.md`; this document reserves the syntactic position only.
- Default values must be literals: strings, numbers, booleans, or `none`.
- The compiler infers types for untyped parameters from usage context and repairs source when inference fails.

## Interaction With Block Structure

Top-level declaration headers do not use a trailing colon. The body begins on the next line at one indent level deeper (4 spaces per `block-structure.md`). Body-level sub-section headers (`flow:`, `effects:`, `constraints:`, etc.) use colon-terminated syntax as defined in `block-structure.md`.

Example showing the boundary:

```glyph
export block inspect_failure(scope) -> FailureReport      // header, no colon
    effects: reads_files, runs_commands                    // body sub-section, colon

    flow:                                                  // body sub-section, colon
        reproduce(scope)
        collect_logs(scope)
        return failure_report()
```

## Interaction With Comments

Line comments (`//`) as defined in `comments.md` may appear on header lines:

```glyph
skill implement_feature(scope, risk = "medium")  // main entrypoint
```

Comments are stripped from compiled output.

## Drift From Existing Examples

The examples in `authoring-surface.md` and `data-flow-and-calls.md` already match this specification. No drift detected. The only formalization beyond existing examples is:

- Making `-> ReturnType` mandatory on `export block` (implicit in `data-flow-and-calls.md` but not shown as a hard rule).
- Making `as <alias>` mandatory on whole-module imports (implicit in examples but not stated as a rule).
- Requiring `()` on all callable declarations even with no parameters, matching Python's `def foo():` convention.

## Deferred

- Full type annotation syntax beyond the `name: Type` slot (see `types.md`).
- Package-style, registry-backed, or versioned imports.
- `agent`, `abstract agent`, `trait` declaration headers (post-MVP).
- `bool` declarations (post-MVP; use `"true"` / `"false"` strings or an untyped local binding until then).
- Whether selective imports support glob or wildcard patterns.
