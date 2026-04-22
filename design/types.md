# Glyph Types

This document defines the MVP type vocabulary for Glyph source files. It covers primitive type names, named domain types, the role of types in compilation, and deferred structural features.

## Status

Fills the `name: Type` and `-> ReturnType` slots reserved by `declaration-headers.md`.

## Design Posture

Types in MVP Glyph are **semantic labels**, not compiler-enforced structural contracts.

A type annotation like `-> ReviewResult` or `ctx: RepoContext` improves two things:

1. **Compiled output clarity.** The compiled Markdown can say "Input: a RepoContext" or "Output: a ValidationResult," which helps the agent understand what it is working with.
2. **Nominal matching at call boundaries.** The compiler can verify that a block returning `Plan` is wired to a parameter expecting `Plan`, catching obvious wiring mistakes.

Types in MVP do not carry structural definitions. The compiler does not check whether `RepoContext` has a `.file_tree` field or whether `ValidationResult` has `.passed`. Field access is trusted in MVP. Structural type definitions and checking are deferred to post-MVP.

This follows principle 3 (forgiving source, strict IR) and principle 7 (keep the core language small). Most Glyph skills are linear flows where values pass through by name and the agent figures out the domain semantics from context. Heavy cross-file structural validation is the exception, not the rule, and can be added later without breaking existing source.

## Primitive Types

Each value kind from `values-and-literals.md` has a canonical type name.

| Value kind | Type name | Literal examples |
|---|---|---|
| Inline or block string | `String` | `"hello"`, `"""..."""` |
| Integer | `Int` | `3`, `-1` |
| Float | `Float` | `0.8`, `3.14` |
| Boolean | `Bool` | `true`, `false` |
| Absence of value | `None` | `none` |

### Casing

PascalCase is the recommended convention for all type names. This matches every existing example in the design docs (`RepoContext`, `Plan`, `FileSet`, `ReviewResult`, `ValidationResult`, `FailureReport`, `Summary`).

PascalCase is convention, not enforcement. Since identifiers are case-normalized per `values-and-literals.md:99-103`, `String` and `string` resolve to the same name. The compiler accepts either; PascalCase is recommended because it visually distinguishes type names from value names and parameters.

### No `Number` Supertype

`values-and-literals.md:62-65` already handles integer-to-float and float-to-integer conversion through lossless coercion at call boundaries. A `Number` supertype would introduce subtyping machinery with no authoring benefit in the MVP.

## Named Domain Types

Authors may use any identifier as a type name in parameter annotations and return types.

```glyph
block inspect_repo(scope) -> RepoContext

block make_plan(ctx: RepoContext, risk = "medium") -> Plan

export block validate_changes(files: FileSet, strict = true) -> ValidationResult
```

Named domain types such as `RepoContext`, `Plan`, `FileSet`, and `ValidationResult` are **opaque tags**. They have no formal definition in MVP source. The compiler tracks them by name for nominal matching but does not assign them structural shape.

### What The Compiler Checks

In the MVP, the compiler performs nominal matching at call boundaries:

- If `inspect_repo` declares `-> RepoContext` and the caller passes the result to a parameter annotated `: RepoContext`, the names match. No error.
- If the caller passes a `RepoContext` to a parameter annotated `: Plan`, the names differ. Compile error.
- If either side omits the type annotation, no check is performed. The compiler trusts the author.

Field access such as `result.findings` is not validated against the type in MVP. The compiler records field access in the IR for visualization and future validation, but does not reject unverifiable access.

### Naming Convention

Domain type names follow the same identifier rules as all other names: `[a-zA-Z_][a-zA-Z0-9_]*` per `values-and-literals.md:96-97`. PascalCase is recommended for types. The no-shadowing rule from `values-and-literals.md:140-150` applies: if a type name collides with a parameter or binding after case normalization, the compiler rejects the program.

## `none` Value And `None` Type

`none` is a reserved keyword and value representing the absence of a value (`values-and-literals.md:80-89`). Since identifiers are case-normalized, `none` and `None` resolve to the same name.

Recommended convention:

- **Value position:** lowercase `none` — `return none`, `result = none`, `effects: none`.
- **Type position:** PascalCase `None` — `-> None`, `name: None`.

The compiler accepts either casing in either position. The convention exists for readability.

`None` as a return type means "this block produces no meaningful value." It is required on export blocks that exist only for their instructions or effects:

```glyph
export block emit_safety_warning() -> None
    effects: none

    flow:
        "Warn the user about destructive operations before proceeding."
        return none
```

### No `Never` Type

The MVP does not include a bottom type. `None` covers the "returns nothing" case. A `Never` type for blocks that unconditionally fail or diverge may be added post-MVP if real need emerges.

## Type-Name Identifier Rules

Types are not a separate lexical class. They are identifiers that follow the same rules as all other identifiers in `values-and-literals.md`.

The parser knows a name is in type position based on syntax:

- After `:` in a parameter declaration: `name: Type`.
- After `->` in a return type: `-> ReturnType`.

No separate type keyword, sigil, or capitalization rule is needed for the parser to distinguish types from values. PascalCase is a human convention, not a parser requirement.

## Interaction With Declaration Headers

`declaration-headers.md` reserves the `name: Type` and `-> ReturnType` slots. This document fills those slots:

```glyph
// Parameter type annotation (optional)
skill implement_feature(scope: String, risk: String = "medium")

// Return type annotation (optional on skill and block, mandatory on export block)
block validate(plan: Plan) -> ValidationResult

export block inspect_failure(scope: String) -> FailureReport
```

Parameter type annotations are always optional. When omitted, the compiler infers the type from usage context or defaults to an untyped parameter that accepts any value. When present, the compiler performs nominal matching at call sites.

Return type annotations are optional on `skill` and `block`, mandatory on `export block` per `declaration-headers.md:93`. On export blocks, the return type is part of the import contract.

## Interaction With Export Block Closure

Export blocks must declare `-> ReturnType` so callers have a clear contract (`declaration-headers.md:93`, principle 19). In MVP, this contract is nominal:

```glyph
// repo_tools.glyph.md
export block inspect_repo(scope) -> RepoContext
    effects: reads_files, runs_commands

    flow:
        scan_files(scope)
        read_git_history(scope)
        return repo_context()
```

```glyph
// fix_bug.glyph.md
import "./repo_tools.glyph.md" { inspect_repo }

skill fix_bug(scope)
    flow:
        ctx = inspect_repo(scope)
        plan = diagnose(ctx)
        apply_fix(plan)
```

The compiler knows `ctx` has type `RepoContext` because `inspect_repo` declares `-> RepoContext`. If `diagnose` declares its parameter as `: RepoContext`, the names match. If it omits the annotation, no check is performed.

The compiled Markdown for `fix_bug` includes "receives a RepoContext from inspect_repo" in its input documentation, which helps the agent understand the data flow even though the compiler does not structurally validate the shape.

## Interaction With Compiled Output

Type names appear in the compiled Markdown output in two places:

- **Inputs section.** Parameter types are rendered as input descriptions: "scope (String)", "plan (Plan)".
- **Output section.** Return types are rendered as output descriptions: "Produces a ValidationResult."

When type annotations are omitted, the compiled output describes the parameter or return value without a type label.

## Deferred

The following type features are deferred beyond the MVP:

- **Structural type definitions.** A `type Name = { field1, field2 }` declaration form that gives named types an explicit shape the compiler can check. This is the most likely next addition when cross-file structural validation becomes a real authoring need.
- **Inline structural contracts.** A `name: { findings, severity }` annotation form for duck-typed parameters. Currently, the compiler infers field access from usage but does not expose a source syntax for declaring structural shape inline.
- **Structural type checking.** Validating that field access like `result.findings` is compatible with the declared or inferred type. The compiler records field access in the IR but does not reject unverifiable access in MVP.
- **Collection types.** `List[T]`, `Set[T]`, `Map[K, V]`, or similar parameterized types. Existing examples use opaque names like `FileSet` which serve the same documentary purpose without requiring generic type machinery.
- **Callable types.** Blocks are called by name. First-class function values and their types are not needed.
- **Enum and union types.** Per `values-and-literals.md:157-164`, enumerated values are represented as strings in MVP. Dedicated enum types with exhaustiveness checking may be added later.
- **Type aliases.** A shorthand for renaming or combining existing types.
- **`Never` / bottom type.** For blocks that unconditionally fail or diverge.

## Open Syntax Choices

The semantic commitments above are stronger than the exact syntax. These details can still change:

- Whether the compiled output renders type names in parentheses, as labels, or in another format.
- The exact nominal matching rules when one side has a type and the other does not.
- Whether the compiler should warn when the same domain type name is used inconsistently across files (e.g., `RepoContext` in one file means something different than in another).
