# Glyph Language Surface

This document is the single authoritative reference for Glyph source syntax: declarations, block structure, authoring forms, and the source-to-IR compilation contract.

## 1. Overview

A `.glyph.md` file is a Glyph source module. It compiles to exactly one Markdown file by replacing `.glyph.md` with `.md` (e.g. `skill.glyph.md` -> `skill.md`). The entire file is Glyph source; there is no Markdown passthrough. Markdown structure lives in the compiled output, not in the source.

Each `.glyph.md` file must contain exactly one `skill` declaration. It may also contain imports, value-binding declarations (`text`, `int`, `float`), blocks, and exported blocks that support that skill. The MVP base declaration kinds are `skill`, `block`, `text`, `int`, and `float`, with `export` as visibility modifier on value-binding and block kinds, and `generated` as repair-authorship modifier on `text`. Later design may add `agent`, `abstract agent`, or `trait`.

Glyph source optimizes for easy readability, easy maintenance, and forgiving authoring. The source surface may be duck-typed and partially inferred; the compiler is responsible for turning that into a strict IR. The split is: source can be ergonomic; IR and compiled output must be explicit.

## 2. Block Structure

### 2.1 Significant Indentation

Glyph uses Python-style significant indentation. No braces, no `end` keywords.

A block is the set of contiguous lines indented deeper than the introducing line. It closes when the next non-blank, non-comment line appears at equal or lesser indentation.

### 2.2 Indentation Unit

The indentation unit is **4 spaces**.

- Tabs are rejected as a compile error.
- Mixed indentation (tabs and spaces on the same line) is a compile error.
- Indentation must increase by exactly 4 spaces per nesting level; other increments are a compile error.
- The LLM repair pass may auto-fix tab-indented source to 4 spaces.

### 2.3 Nesting Levels

Three primary indentation levels in practice:

- **Level 0** (column 0): top-level declarations (`skill`, `block`, `text`, `int`, `float`, `import`).
- **Level 1** (column 4): declaration body -- constraints, sub-section headers, bare instructions.
- **Level 2** (column 8): sub-block body -- flow steps, effect list items, nested content.

Deeper nesting is supported for constructs such as `if` inside `flow:` or nested private blocks.

### 2.4 No Trailing Colon on Declarations

Top-level declarations introduce their body by indentation alone. They do not use trailing colons. Colons are reserved for sub-section headers within declaration bodies. This creates a visual distinction: declarations are structural headers; colons introduce labeled sub-blocks within them.

### 2.5 Sub-Section Headers

Sub-sections within a declaration body use a colon-terminated keyword. MVP sub-section keywords:

`flow:`, `effects:`, `inputs:`, `outputs:`, `constraints:`, `when_to_use:`

Two forms are allowed:

**Long form** -- keyword on its own line, indented body below:

```glyph
effects:
    - reads_files
    - writes_files
    - runs_commands
```

**Short form** -- keyword and content on the same line:

```glyph
effects: reads_files, runs_commands
```

The compiler normalizes both forms to the same IR representation.

### 2.6 Blank Lines

Blank lines inside blocks are freely allowed and do not close or break blocks. They are visual separators only. A block continues until the next non-blank, non-comment line at equal or lesser indentation.

### 2.7 Line Continuation

Line continuation is implicit inside paired delimiters only. No backslash continuation.

- Parenthesized expressions (call arguments) span until closing `)`.
- Braced import lists (`{ ... }`) span until closing `}`.
- Triple-quoted block strings (`"""`) span until closing `"""`.

Inside paired delimiters, indentation is not structurally significant.

### 2.8 Comments

Line comments use `//`. No block comments. Comments are stripped from compiled output and preserved by repair. Comment-only lines are invisible to the indentation parser. Trailing comments do not affect indentation measurement.

## 3. Declarations

### 3.1 `skill`

The public entrypoint that compiles to Markdown agent instructions. One per file.

**Grammar:**

```
skill <name>()
skill <name>(<params>)
skill <name>(<params>) -> <ReturnType>
```

**Example:**

```glyph
skill implement_feature(scope, risk = "medium")
    require preserve_existing_patterns
    avoid unrelated_edits

    effects: reads_files, writes_files, runs_commands

    flow:
        ctx = inspect_repo(scope)
        plan = make_plan(ctx, risk)
        apply_changes(plan)
        validate(plan)
        return summarize(plan)
```

**Rules:**

- Parentheses always required on callable declarations: `skill update_docs()` not `skill update_docs`. This applies to `skill`, `block`, and `export block`. Value-binding declarations (`text`, `int`, `float` and their `export`/`generated` variants) and `import` do not use parentheses.
- Return type is optional. When present, it becomes the `## Output` section in compiled Markdown.
- Parameters are optional. In MVP, global preferences resolve at compile time as explicit inputs, not hidden state.

### 3.2 `block`

Private helper block, scoped to the current file.

**Grammar:**

```
block <name>()
block <name>(<params>)
block <name>(<params>) -> <ReturnType>
```

**Example:**

```glyph
block make_plan(ctx, risk = "medium") -> Plan
    flow:
        analyze(ctx)
        return draft_plan(ctx, risk)
```

**Rules:**

- Private blocks may rely on enclosing skill context in the MVP.
- Return type annotation is optional; if declared, every return path must match after type inference.
- Private blocks may be top-level declarations or nested inside a `skill` when nesting improves readability. The exact static analysis model for context dependency is deferred.

### 3.3 `export block`

Importable, self-contained reusable block. Two-keyword prefix.

**Grammar:**

```
export block <name>(<params>) -> <ReturnType>
```

**Example:**

```glyph
export block inspect_failure(scope) -> FailureReport
    effects: reads_files, runs_commands

    flow:
        reproduce(scope)
        collect_logs(scope)
        return failure_report()
```

**Rules:**

- Every `export block` must have an explicit `-> ReturnType`, even `-> none`, so callers have a clear contract.
- Must be **closed**: behavior determined by declared inputs, local bindings, explicit imports, same-file reusable text, standard primitives, declared constraints, declared outputs, and declared effects. Closed does not mean pure; an exported block may read files, run tools, or produce artifacts if those effects are declared.
- An `export block` may call imported `export block`s or same-file private blocks if the compiler can prove those private blocks are closed under the exported block's contract.
- Every `export block` must end in an explicit `return`. Instruction-only exported blocks should still return `none`.
- `effects:` appears in the body, not on the header line.
- MVP effects: `none`, `reads_files`, `reads_env`, `writes_files`, `runs_commands`, `uses_network`, `asks_user`, `creates_artifacts`.

### 3.4 `text` / `export text`

Named instruction text. `text` is file-private; `export text` is importable.

**Grammar:**

```
text <name> = <string-literal>
export text <name> = <string-literal>
```

**Example:**

```glyph
text preserve_existing_patterns = """
Prefer the repository's existing patterns, helper APIs, naming, and file organization
before introducing a new abstraction or style.
"""

export text safety_rules = """
Never execute destructive operations without confirmation.
"""
```

**Rules:**

- No parameters, no return type. A `text` declaration is a named constant, not a callable.
- The `=` is required and separates the name from its value.
- String literals follow `values-and-names.md`: inline `"..."` or block `"""..."""`.
- These bindings are not arbitrary string interpolation. The compiler treats them as named instruction resources resolved into IR nodes.

### 3.5 `int` / `export int`

Named integer constant. `int` is file-private; `export int` is importable.

**Grammar:**

```
int <name> = <int-literal>
export int <name> = <int-literal>
```

**Example:**

```glyph
int max_attempts = 3
export int default_max_attempts = 3
```

**Rules:**

- No parameters, no return type. An `int` declaration is a named constant, not a callable.
- The `=` is required and separates the name from its value.
- RHS must be an integer literal. No cross-assignment from float literals; lossless coercion at call boundaries is per [values-and-names.md](values-and-names.md).
- These bindings are not arbitrary numeric expressions. The compiler treats them as named constant resources resolved into IR nodes.

### 3.6 `float` / `export float`

Named floating-point constant. `float` is file-private; `export float` is importable.

**Grammar:**

```
float <name> = <float-literal>
export float <name> = <float-literal>
```

**Example:**

```glyph
float threshold = 0.8
export float default_temperature = 0.7
```

**Rules:**

- No parameters, no return type. A `float` declaration is a named constant, not a callable.
- The `=` is required and separates the name from its value.
- RHS must be a float literal. No cross-assignment from integer literals; lossless coercion at call boundaries is per [values-and-names.md](values-and-names.md).
- These bindings are not arbitrary numeric expressions. The compiler treats them as named constant resources resolved into IR nodes.

### 3.7 `import`

Brings exported declarations from another `.glyph.md` file into scope. Two forms.

**Grammar -- whole-module:**

```
import "<path>" as <alias>
```

**Grammar -- selective:**

```
import "<path>" {
    <name>,
    <name> as <alias>,
    ...
}
```

**Example:**

```glyph
import "./repo_tools.glyph.md" as repo_tools

import "./coding_agent_safety.glyph.md" {
    unrelated_edits,
    preserve_existing_patterns as existing_patterns,
    validate_before_success,
}
```

**Rules:**

- Path is always a quoted string.
- Whole-module import requires `as <alias>`. No bare module imports.
- Whole-module import exposes: the file's `skill` entrypoint, all `export block` declarations, and all exported value-binding declarations (`export text`, `export int`, `export float`). Private `text`, `int`, `float`, and private `block` stay hidden.
- Selective import uses `{ name, name as alias, ... }`. Trailing comma allowed. Only explicitly exported declarations may be named.
- A single import statement is either whole-module or selective, not both.
- Circular imports are rejected in the MVP.
- MVP imports are path-based. Package names, registries, and versioned imports are deferred.

### 3.8 Parameter Syntax

Parameters appear inside parentheses on `skill`, `block`, and `export block` headers. Four forms:

```
name                          // untyped, required
name = "default"              // untyped, with default
name: Type                    // typed, required
name: Type = default_value    // typed, with default
```

- Required parameters (no default) must precede optional parameters. Same ordering rule as Python.
- Type annotations use the `name: Type` slot. The full type system is a Tier 2 concern; this reserves the syntactic position.
- Default values must be Tier 0 literals: strings, numbers, booleans, or `none`.
- The compiler infers types for untyped parameters from usage context and repairs source when inference fails.

## 4. Authoring Model

### 4.1 Language Primitives

Authors build skills from defined primitives: `skill`, `export block`, `block`, `flow`, `call`, `if`, `return`. `for_each` is deferred beyond the MVP. Role and constraint markers are disambiguators; the MVP role vocabulary is defined in [ir-and-semantics.md](ir-and-semantics.md).

A `flow` becomes an ordered sequence and a `return` becomes an output contract in compiled form. Function-like calls may pass variables and bind return values; the detailed contract is in [data-flow.md](data-flow.md).

### 4.2 Inferred Instruction Roles

Source instructions need not carry compiler-shaped keywords everywhere. A bare name or inline string compiles into an inferred IR role depending on context and metadata.

The closed MVP role set ([ir-and-semantics.md](ir-and-semantics.md)): `InputContract`, `Step`, `Constraint`, `Context`, `OutputContract`. Prohibitions and preferences are `Constraint` nodes with separate strength and polarity attributes. **Constraint markers** (`require`, `avoid`, `prefer`, `always`, and composed forms like `always avoid`) set role, strength, and polarity directly. For the complete marker-to-IR mapping, see [ir-and-semantics.md](ir-and-semantics.md).

Inference uses: position in the skill, metadata from bindings/imports/standard-library, natural meaning of expanded text, and explicit keywords. If inference succeeds, repair adds the smallest explicit marker back into source. Compound names like `avoid_unrelated_edits` repair to `avoid unrelated_edits` with author notification. `always` is inferred only from invariant-level wording. When inference fails, the compiler emits a diagnostic.

### 4.3 Bare Names and Generated Definitions

Bare instruction names are allowed. Resolution order: (1) same-file binding, (2) explicit import, (3) standard library vocabulary, (4) MVP repair materializes a stable generated definition when context is clear. Generated definitions are cached in source, reviewable, and validated before compilation continues. Expansion happens during repair, not at runtime.

### 4.4 Semantic Shortcuts

Authors can write small function-like or identifier-like instructions directly when the name is instructive enough to expand:

```glyph
skill debug_failure(scope)
    root_cause_before_fix
    reproduce_before_patch
    root_cause_trace()
```

### 4.5 Inline Instructions

One-off instruction text may be placed inline with quoted strings for short cases and block strings for longer cases:

```glyph
skill update_docs(scope)
    "Do not change public behavior while updating documentation."

    flow:
        inspect_docs(scope)
        apply_doc_changes()
        "Mention any docs you could not verify locally."
```

If the same text appears repeatedly, promote it to a `text` block or imported library entry.

## 5. Source-to-IR Pipeline

Glyph source may contain shorthand, omitted annotations, text aliases, imported names, and inline natural-language instructions. The compiler resolves them before Markdown output generation:

1. **Parse** `.glyph.md` source into a loose source AST.
2. **Diagnose** syntax, name resolution, role inference, constraint attribute inference, and type inference deterministically.
3. **Repair** source-preserving LLM pass if deterministic passes report repairable diagnostics.
4. **Re-parse** and re-check the repaired source.
5. **Resolve** local names, imported libraries, and known standard names. Undefined bare names are materialized as stable generated definitions during repair.
6. **Infer** instruction role, constraint strength, and constraint polarity when source omits them.
7. **Normalize** values, instruction text, blocks, calls, and constraints into explicit IR nodes.
8. **Type** the IR.
9. **Validate** the IR before producing compiled Markdown output.

If ergonomic source does not compile directly, the LLM repair pass rewrites it into valid Glyph source while preserving shorthand and readability. Repair fixes compiler-blocking issues; it does not expand shorthand into prose or produce agent-facing output.

## 6. Maintenance Rules

- Prefer named `text` blocks for repeated instruction text; prefer imports for team-wide vocabulary.
- Import only explicitly exported declarations; keep `block`s and non-exported `text` private.
- Use aliases when an imported name would collide or read poorly.
- Make every `export block` self-contained (inputs, outputs, constraints, effects declared).
- Use semantic shortcuts when the name communicates intent; use inline quotes for one-off guidance.
- Prefer marker-plus-concept form (`avoid unrelated_edits`) over compound names (`avoid_unrelated_edits`).
- The compiler should surface unresolved or ambiguous names as diagnostics, not guess silently.

## Deferred

- Full type annotation syntax beyond the `name: Type` slot (Tier 2).
- Package-style, registry-backed, or versioned imports; selective import globs.
- `agent`, `abstract agent`, `trait` declarations; global preference parameter syntax (post-MVP).
- `for_each` control flow; `if` colon syntax (see [data-flow.md](data-flow.md)); nested private block scoping.
