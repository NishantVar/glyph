# Glyph Types

This document defines the MVP type vocabulary for Glyph source files. It covers named domain types, implicit type declaration, the role of types in compilation, and deferred structural features.

## Status

Fills the `name: Type` and `-> ReturnType` slots reserved by `language-surface.md`.

## Design Posture

Types in MVP Glyph are **semantic labels**, not compiler-enforced structural contracts.

A type annotation like `-> ReviewResult` or `ctx: RepoContext` improves two things:

1. **Compiled output clarity.** The compiled Markdown can say "Input: a RepoContext" or "Output: a ValidationResult," which helps the agent understand what it is working with.
2. **Nominal matching at call boundaries.** The compiler can verify that a block returning `Plan` is wired to a parameter expecting `Plan`, catching obvious wiring mistakes.

Types in MVP do not carry structural definitions. The compiler does not check whether `RepoContext` has a `.file_tree` field or whether `ValidationResult` has `.passed`. Field access is trusted in MVP. Structural type definitions and checking are deferred to post-MVP.

This follows foundations: forgiving source, strict IR and foundations: core language is small. Most Glyph skills are linear flows where values pass through by name and the agent figures out the domain semantics from context. Heavy cross-file structural validation is the exception, not the rule, and can be added later without breaking existing source.

## Primitive Kinds (IR-Only)

Glyph's author-facing surface has no primitive type names. Authors never write `String`, `Int`, `Float`, `Bool`, or `None` as type annotations — only semantic domain types like `Plan`, `RepoContext`, or `Diagnosis` are valid in `-> ReturnType` and `name: Type` positions.

The compiler still tracks primitive kinds internally in the IR, inferred from literals, defaults, and usage context. The `TypeTag` enum in `ir-schema.md` retains `string`, `int`, `float`, `bool`, and `none` variants for internal analysis, coercion checks, and default-value validation. These kinds are never surfaced to authors.

**Rationale.** Glyph's compiled output is consumed by LLMs. `-> String` carries no useful semantic signal to an LLM. `-> BranchName` does. Primitive type names are a holdover from languages where types serve compilers; in Glyph, types serve the agent reading the compiled output.

### Casing

PascalCase is the recommended convention for all type names. This matches every existing example in the design docs (`RepoContext`, `Plan`, `FileSet`, `ReviewResult`, `ValidationResult`, `FailureReport`, `Summary`).

PascalCase is convention, not enforcement. Since identifiers are case-normalized per `values-and-names.md`, Case Normalization section, `RepoContext` and `repo_context` resolve to the same name. The compiler accepts either; PascalCase is recommended because it visually distinguishes type names from value names and parameters.

## Named Domain Types

Authors may use any identifier as a type name in parameter annotations and return types. Domain types are the only type names valid in author-facing source.

```glyph
block inspect_repo(scope) -> RepoContext

block make_plan(ctx: RepoContext, risk = "medium") -> Plan

export block validate_changes(files: FileSet, strict = true) -> ValidationResult
```

Named domain types such as `RepoContext`, `Plan`, `FileSet`, and `ValidationResult` are **opaque tags**. They have no formal definition in MVP source. The compiler tracks them by name for nominal matching but does not assign them structural shape.

### Implicit Declaration

Domain types are implicitly declared by first use in a `-> Type` position. No explicit `type Foo` declaration is needed in MVP. When the compiler encounters `-> Diagnosis` for the first time, it registers `Diagnosis` as a known domain type.

The meaning of a domain type is contextually defined by `<"...">` descriptions at return sites (see `output-target-expression-note.md`). For example:

```glyph
export block diagnose_issue(scope) -> Diagnosis
    flow:
        inspect_repo(scope)
        return <"root cause analysis including affected files and severity">
```

The `-> Diagnosis` on the header serves as the compiler contract (nominal matching). The `<"root cause analysis including affected files and severity">` serves as agent guidance (what to synthesize). These are complementary.

Two blocks returning the same `-> Type` with different `<"...">` descriptions is valid — descriptions are local to each block's compiled output and do not participate in nominal matching.

### What The Compiler Checks

In the MVP, the compiler performs nominal matching at call boundaries:

- If `inspect_repo` declares `-> RepoContext` and the caller passes the result to a parameter annotated `: RepoContext`, the names match. No error.
- If the caller passes a `RepoContext` to a parameter annotated `: Plan`, the names differ. Compile error.
- If either side omits the type annotation, no check is performed. The compiler trusts the author.

Field access such as `result.findings` is not validated against the type in MVP. The compiler records field access in the IR for visualization and future validation, but does not reject unverifiable access.

### Naming Convention

Domain type names follow the same identifier rules as all other names: `[a-zA-Z_][a-zA-Z0-9_]*` per `values-and-names.md`, Allowed Characters section. PascalCase is recommended for types. The no-shadowing rule from `values-and-names.md`, No Shadowing section, applies: if a type name collides with a parameter or binding after case normalization, the compiler rejects the program.

## `none` Value (No `None` Type Annotation)

`none` is a reserved keyword and value representing the absence of a value (`values-and-names.md`, None section). It is valid in value positions: `return none`, `result = none`, `effects: none`.

`None` as a type annotation (`-> None`) is dropped. A block that produces no meaningful return value simply omits the `->` from its declaration header:

```glyph
export block emit_safety_warning()
    effects: none

    flow:
        "Warn the user about destructive operations before proceeding."
        return none
```

No `->` on a declaration means "no meaningful return value." The `none` value keyword remains for use in `return none`, `effects: none`, and other value positions.

### No `Never` Type

The MVP does not include a bottom type. Omitting `->` covers the "returns nothing" case. A `Never` type for blocks that unconditionally fail or diverge may be added post-MVP if real need emerges.

## Type-Name Identifier Rules

Types are not a separate lexical class. They are identifiers that follow the same rules as all other identifiers in `values-and-names.md`.

The parser knows a name is in type position based on syntax:

- After `:` in a parameter declaration: `name: Type`.
- After `->` in a return type: `-> ReturnType`.

No separate type keyword, sigil, or capitalization rule is needed for the parser to distinguish types from values. PascalCase is a human convention, not a parser requirement.

## Interaction With Declaration Headers

`language-surface.md` reserves the `name: Type` and `-> ReturnType` slots. This document fills those slots. Only domain type names are valid in these positions — primitive type names are not part of the author-facing surface.

```glyph
// Parameter type annotation (optional, domain types only)
skill implement_feature(scope: PathSpec, risk: RiskLevel = "medium")

// Return type annotation (optional on skill and block)
block validate(plan: Plan) -> ValidationResult

// Export block with meaningful return: -> DomainType required
export block inspect_failure(scope: PathSpec) -> FailureReport

// Export block with no meaningful return: omit -> entirely
export block emit_safety_warning()
```

Parameter type annotations are always optional. When omitted, the compiler infers the type from usage context or defaults to an untyped parameter that accepts any value. When present, the compiler performs nominal matching at call sites.

### Return Type Requirements

- **`export block` with meaningful return:** `-> DomainType` is required. Missing `->` on an export block that returns a value is a repairable diagnostic; the repair pass infers a domain type name from the block name and return expression.
- **`export block` with no meaningful return:** omit `->` entirely.
- **`block` and `skill`:** `-> DomainType` is optional. The repair pass may suggest a domain type but never enforces one.

## Interaction With Export Block Closure

Export blocks that return a meaningful value must declare `-> DomainType` so callers have a clear contract (see `data-flow.md` for the export-block closure rule). Export blocks with no meaningful return omit `->` entirely. In MVP, this contract is nominal:

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

skill fix_bug(scope = ".")
    flow:
        ctx = inspect_repo(scope)
        plan = diagnose(ctx)
        apply_fix(plan)
```

The compiler knows `ctx` has type `RepoContext` because `inspect_repo` declares `-> RepoContext`. If `diagnose` declares its parameter as `: RepoContext`, the names match. If it omits the annotation, no check is performed.

The compiled Markdown for `fix_bug` includes "receives a RepoContext from inspect_repo" in its input documentation, which helps the agent understand the data flow even though the compiler does not structurally validate the shape.

## Interaction With Compiled Output

See `compiled-output.md` for how types surface in the final Markdown.

## Deferred

The following type features are deferred beyond the MVP:

- **Explicit `type` declarations.** A `type Name = { field1, field2 }` declaration form that gives named types an explicit structural shape the compiler can check. MVP uses implicit declaration (first use in `-> Type` creates the type). Explicit declarations are the most likely next addition when cross-file structural validation becomes a real authoring need.
- **Structural type definitions.** Giving named types an explicit shape (fields, nested types) that the compiler can validate against. Blocked on explicit `type` declarations.
- **Inline structural contracts.** A `name: { findings, severity }` annotation form for duck-typed parameters. Currently, the compiler infers field access from usage but does not expose a source syntax for declaring structural shape inline.
- **Structural type checking.** Validating that field access like `result.findings` is compatible with the declared or inferred type. The compiler records field access in the IR but does not reject unverifiable access in MVP.
- **Collection types.** `List[T]`, `Set[T]`, `Map[K, V]`, or similar parameterized types. Existing examples use opaque names like `FileSet` which serve the same documentary purpose without requiring generic type machinery.
- **Callable types.** Blocks are called by name. First-class function values and their types are not needed.
- **Enum and union types.** Per `values-and-names.md`, Enums And Symbols section, enumerated values are represented as strings in MVP. Dedicated enum types with exhaustiveness checking may be added later.
- **Type aliases.** A shorthand for renaming or combining existing types.
- **`Never` / bottom type.** For blocks that unconditionally fail or diverge.
- **Divergent description warnings.** When the same `-> Type` appears on multiple blocks with substantially different `<"...">` descriptions, the compiler could warn that the type name may be overloaded. Deferred because descriptions are local to each block's compiled output and do not affect nominal matching.
- **LLM-phase semantic type mismatch detection.** The LLM repair pass could potentially catch semantic type mismatches (passing a string where an int is expected) using name and usage context. Deferred post-MVP; for now, the compiler infers primitive kinds from literals and defaults only.

## Open Syntax Choices

The semantic commitments above are stronger than the exact syntax. These details can still change:

- Whether the compiled output renders type names in parentheses, as labels, or in another format.
