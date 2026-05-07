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

Glyph's author-facing surface has no primitive or generic-collection type names. Authors never write the primitives `String`, `Int`, `Float`, `Bool`, `None`, the generic collections `List`, `Set`, `Map`, `Array`, `Dict`, `Tuple`, or the catch-alls `Object`, `Any` as type annotations — only semantic domain types like `Plan`, `RepoContext`, or `Diagnosis` are valid in `-> ReturnType` and `name: Type` positions. (Parameterized collection forms like `List[T]` are deferred entirely; see §Deferred. The `Agent` `TypeTag` variant in `ir-schema.md` §Enums is a stdlib-specific kind for `subagent()` results, not a generic type, and is **not** in the banned set.) Pipeline-precedence note: `-> None` in author-facing source is intercepted at parse time by `G::parse::none-as-return-type` (repairable, Phase 3a auto-fix per `diagnostics.md`); the analyze-tier `G::analyze::generic-type-name` warning therefore does not surface for `None` in return-type position end-to-end. The validator that owns the 13-name list still includes `None` for defense in depth at any future call sites where parse interception does not apply.

The compiler still tracks primitive kinds internally in the IR, inferred from literals, defaults, and usage context. The `TypeTag` enum in `ir-schema.md` retains `string`, `int`, `float`, `bool`, and `none` variants for internal analysis, coercion checks, and default-value validation. These kinds are never surfaced to authors.

**Rationale.** Glyph's compiled output is consumed by LLMs. `-> String` carries no useful semantic signal to an LLM. `-> BranchName` does. Primitive type names are a holdover from languages where types serve compilers; in Glyph, types serve the agent reading the compiled output.

### Condition-Position Kind Routing

When a bare name appears in `if`/`elif` condition position, Phase 2 (Analyze) consults its inferred primitive kind to determine the semantics:

| Inferred kind | Condition-position treatment |
|---|---|
| `bool` | Boolean predicate — standard boolean evaluation |
| `string` | Natural-language predicate — resolves via `resolved_predicates` side-map; produces `predicate_const` token kind |
| Opaque domain type | Treated as boolean (no regression from pre-existing behavior) |
| `int` or `float` | Hard error: `G::analyze::condition-non-boolean-non-predicate` |

This routing means that a `const` declared with a string value (e.g., `const complex_change_required = "a complex structural change is needed"`) is automatically usable as a natural-language predicate in condition position without any author-visible annotation. The `==` comparison operator carves out an exception: `if risk == "high":` is a boolean equality check — the string is an operand, not a predicate body, and does not trigger the string-predicate path.

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

The meaning of a domain type is contextually reinforced by return-site output targets and surrounding prose. Authors choose between two output-target forms depending on whether a short identifier or richer guidance better names what the agent must synthesize:

```glyph
// Identifier form — short, name-shaped target
export block diagnose_issue(scope) -> Diagnosis
    flow:
        inspect_repo(scope)
        return <diagnosis>

// Descriptive form — quoted guidance describing what to synthesize
export block diagnose_issue(scope) -> Diagnosis
    flow:
        inspect_repo(scope)
        return <"root cause analysis including affected files and severity">
```

The `-> Diagnosis` on the header serves as the compiler contract (nominal matching). The output target — `<diagnosis>` (identifier form) or `<"…">` (descriptive form) — names the value the agent must synthesize in the final output and is **complementary** to the type annotation, not a substitute. Both forms inherit `ty` from the enclosing return annotation and lower to the same `OutputContract` shape, discriminated by `OutputContract.form` (`ir-schema.md` §OutputContract). Use the identifier form when the target reads cleanly as a name; use the descriptive form when the guidance for the synthesizer is what carries the meaning. Descriptive form is terminal-return-only in MVP — mid-flow output targets, if added later, must use the identifier form (`values-and-names.md` §No Value-Level Operators).

Two blocks returning the same `-> Type` with different output-target names, descriptions, or prose guidance are valid — that guidance is local to each block's compiled output and does not participate in nominal matching.

### What The Compiler Checks

In the MVP, the compiler performs nominal matching at call boundaries:

- If `inspect_repo` declares `-> RepoContext` and the caller passes the result to a parameter annotated `: RepoContext`, the names match. No error.
- If the caller passes a `RepoContext` to a parameter annotated `: Plan`, the names differ. Compile error.
- If either side omits the type annotation, no check is performed. The compiler trusts the author.

Field access such as `result.findings` is not validated against the type in MVP. The compiler records field access in the IR for visualization and future validation, but does not reject unverifiable access.

### Naming Convention

Domain type names follow the same identifier rules as all other names: `[a-zA-Z_][a-zA-Z0-9_]*` per `values-and-names.md`, Allowed Characters section. PascalCase is recommended for types. The no-shadowing rule from `values-and-names.md`, No Shadowing section, applies: if a type name collides with a parameter or binding after case normalization, the compiler rejects the program.

### Explicit `type` Declarations

A domain type may also be introduced explicitly with a top-level `type` decl that pairs the type name with a description:

```glyph
type RepoContext = <"the inspected repo state, including file tree and dependencies">
type RiskLevel   = <"one of: low, medium, high; severity of the change">

export type Diagnosis = <"root cause analysis including affected files and severity">
```

Syntax:

```
type        <Name> = <description>
export type <Name> = <description>
```

The RHS uses the same `<"…">` (inline) and `<"""…""">` (block-string) form as per-param descriptions and descriptive output targets. The decl has no body, no parameters, and no sub-sections.

`type` decls live at the top level of a file alongside `const`, `block`, `export block`, and `import`, and are represented in the AST by a dedicated `TypeDecl` node.

**Semantics — what a `type` decl does.** A `type` decl associates a description with a type name. The description is consumed at compile time and surfaces in two places in the compiled output:

1. The `## Parameters` block, when a parameter is annotated `: Foo` and no per-param description is supplied (see `compiled-output.md`).
2. The closing prose of the final Step in a `-> Foo` block, per the locked return-prose templates (see `compiled-output.md`).

The type decl itself emits no Markdown — `type` decls are compile-time only and do not appear in the compiled `.md`.

`type` decls **complement** implicit declaration. Implicit declaration via first use in `-> Foo` continues to register the nominal type; an explicit `type Foo` decl in the same scope simply attaches a description to that same nominal type. Nominal matching at call boundaries is unchanged.

**Lookup precedence for descriptions.** When the compiler renders a parameter slot or the return prose of a `-> Foo` block, the effective description is computed in priority order:

1. Per-site description (per-param `<"...">` on the slot, or `return <"...">` at the call site) — wins.
2. The type-level description from a `type Foo` decl in scope (same file or imported) — fallback.
3. None.

Per-site descriptions therefore override the type-level default; absence of either leaves the slot or return prose without a description.

**Same-file duplicates.** A second `type Foo` decl in the same file is a hard error (`G::analyze::duplicate-type-decl`). A `type` name also participates in the universal value namespace defined in `values-and-names.md`, No Shadowing section: `type Foo` collides with any other in-scope `Foo` (`const Foo`, `block Foo`, `export block Foo`, an `import` alias, or a parameter named `Foo`). The canonical pairing of `type Foo` with `-> Foo` annotations is **not** a collision — both refer to the same nominal type.

**Cross-file usage.** `export type` decls are importable selectively: `import "./types.glyph" { Foo }` brings the type and its description into scope. A library file that contains only `export type` decls (no `skill`, no `export block`, no `export const`) satisfies the library-export rule and compiles cleanly with no body — see `imports.md`.

Whole-module qualified references to types (e.g., `types.Foo` after `import "./types.glyph" as types`) are not supported in MVP; type slots accept bare identifiers only. Authors who need to expose a type to consumers must use selective import. See Deferred for the parking-lot entry.

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
- **Output target returns:** Both output-target forms — `return <name>` (identifier) and `return <"description">` (descriptive) — use the enclosing `-> DomainType` as the IR `OutputContract.ty`. The form does not change typing: identifier and descriptive forms inherit `ty` from the same channel, and both lower to the same `OutputContract` shape (discriminated by `OutputContract.form`, see `ir-schema.md` §OutputContract). If the annotation is omitted, the output target still lowers but carries `ty: null`; authors should prefer a semantic domain type when the synthesized output is part of a public or reusable contract.

## Interaction With Export Block Closure

Export blocks that return a meaningful value must declare `-> DomainType` so callers have a clear contract (see `data-flow.md` for the export-block closure rule). Export blocks with no meaningful return omit `->` entirely. In MVP, this contract is nominal:

```glyph
// repo_tools.glyph
export block inspect_repo(scope) -> RepoContext
    effects: reads_files, runs_commands

    flow:
        scan_files(scope)
        read_git_history(scope)
        return repo_context()
```

```glyph
// fix_bug.glyph
import "./repo_tools.glyph" { inspect_repo }

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

- **Structural type definitions.** Giving named types an explicit shape (fields, nested types) that the compiler can validate against. The MVP `type Name = <"…">` form attaches only a description to a nominal type; a structural form (e.g., `type Name = { field1, field2 }`) is the most likely next addition when cross-file structural validation becomes a real authoring need.
- **Whole-module qualified type references.** `import "./types.glyph" as types` followed by `param: types.Foo` or `-> types.Foo`. Type slots in MVP accept bare identifiers only; qualified type refs require new TypeRef grammar, AST shape, and canonical-identity rules. Authors must use selective import (`import "./types.glyph" { Foo }`) until this lands.
- **Inline structural contracts.** A `name: { findings, severity }` annotation form for duck-typed parameters. Currently, the compiler infers field access from usage but does not expose a source syntax for declaring structural shape inline.
- **Structural type checking.** Validating that field access like `result.findings` is compatible with the declared or inferred type. The compiler records field access in the IR but does not reject unverifiable access in MVP.
- **Collection types.** `List[T]`, `Set[T]`, `Map[K, V]`, or similar parameterized types. Existing examples use opaque names like `FileSet` which serve the same documentary purpose without requiring generic type machinery.
- **Callable types.** Blocks are called by name. First-class function values and their types are not needed.
- **Enum and union types.** Per `values-and-names.md`, Enums And Symbols section, enumerated values are represented as strings in MVP. Dedicated enum types with exhaustiveness checking may be added later.
- **Type aliases.** A shorthand for renaming or combining existing types.
- **`Never` / bottom type.** For blocks that unconditionally fail or diverge.
- **Divergent output-target guidance warnings.** When the same `-> Type` appears on multiple blocks with substantially different output-target names or prose guidance, the compiler could warn that the type name may be overloaded. Deferred because output guidance is local to each block's compiled output and does not affect nominal matching.
- **LLM-phase semantic type mismatch detection.** The LLM repair pass could potentially catch semantic type mismatches (passing a string where an int is expected) using name and usage context. Deferred post-MVP; for now, the compiler infers primitive kinds from literals and defaults only.

## Open Syntax Choices

The semantic commitments above are stronger than the exact syntax. These details can still change:

- Whether the compiled output renders type names in parentheses, as labels, or in another format.
