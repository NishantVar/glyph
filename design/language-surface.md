# Glyph Language Surface

This document is the single authoritative reference for Glyph source syntax: declarations, block structure, authoring forms, and the source-to-IR compilation contract.

## 1. Overview

A `.glyph.md` file is a Glyph source module. It is either a **skill file** (contains exactly one `skill` declaration, compiles to a same-basename `.md`) or a **library file** (contains zero `skill` declarations, may emit standalone procedure `.md` files for qualifying `export block` declarations — see §File-Level Rules). The entire file is Glyph source; there is no Markdown passthrough. Markdown structure lives in the compiled output, not in the source.

A skill file contains one `skill` declaration plus supporting imports, value-binding declarations (`text`, `int`, `float`), blocks, and exported blocks. A library file contains only imports, value-binding declarations, and blocks (some exported) — no skill. Both file types compile through the same 7-phase pipeline. The MVP base declaration kinds are `skill`, `block`, `text`, `int`, and `float`, with `export` as visibility modifier on value-binding and block kinds, and `generated` as repair-authorship modifier on both `text` and `block`.

**Novice kernel.** A new author only needs a small subset to write useful skills: `skill`, `require`/`avoid`, `flow:`, quoted inline strings, calls with parentheses, and the `with` modifier on calls (see [data-flow.md](data-flow.md)). Blocks, named text, types, effects, and imports are discoverable later; repair materializes `generated text` and `generated block` definitions for undefined names so novice skills compile without those constructs present in source.

Glyph source optimizes for easy readability, easy maintenance, and forgiving authoring. The source surface may be duck-typed and partially inferred; the compiler is responsible for turning that into a strict IR. The split is: source can be ergonomic; IR and compiled output must be explicit.

## 2. Block Structure

### File-Level Rules

A `.glyph.md` file is a unit of compilation. The following rules apply at the file level:

- **Non-empty.** A file containing only whitespace or comments emits `G::parse::empty-file` (error). There is nothing to compile.
- **At most one `skill` declaration per file.** A file may contain zero or one `skill` declarations. A second `skill` in the same file emits `G::parse::multiple-skills` (error). The reason is pragmatic: compiled output is named after the skill (`<skill_name>.md`), and most coding-agent ecosystems expect one skill per file (e.g., `SKILL.md`). Multi-skill files would collide on output naming. Cross-skill composition is via `import`, not by co-locating skills in one source file.
- **Library files (zero `skill` declarations).** A file containing only `import`, `text` / `int` / `float`, and `block` / `export block` declarations — no `skill` — is a **library file** (e.g., `prefs.glyph.md`, `repo_tools.glyph.md`). Library files are consumed by sibling skill files via `import`. Formal rules:

  - **At least one `export` declaration required.** A file with zero skills AND zero exports has no consumer-visible contribution. This emits `G::analyze::no-exports-in-library` (error). Private helpers (`block`, `text`, `int`, `float`) alongside exports are fine — they support the exports internally.
  - **Compilation.** Library files compile through the same 7-phase pipeline as skill files. The DAG-driven multi-file compile (see `pipeline.md` §Multi-File Compilation Order) runs Phases 1–7 on every file in dependency order; a library file is a DAG node like any other, it just has no `skill` to project.
  - **Emission rules — per-declaration, not per-file.** What a library file emits depends on what it exports:

    | Declaration | Emits standalone `.md`? | Mechanism |
    |---|---|---|
    | `export block` whose expanded prose is **>= 150 words** (above the Tier 1 inline threshold; see `compiled-output.md` §Three-Tier Block Projection) | **Yes** — one procedure `.md` per qualifying block | Library's own Phase 7 emits it into a subdirectory named after the source file (e.g., `repo_tools.glyph.md` → `repo_tools/inspect-repo.md`) |
    | `export block` whose expanded prose is **< 150 words** (Tier 1 — small, inlinable) | **No** — consumers inline the body at each call site | No emission from the library |
    | `export text` / `export int` / `export float` | **No** — compile-time constants, always inlined into consumers | No emission |
    | Private `block`, `text`, `int`, `float`, `import` | **No** — contribute to other compilations only | Validation only |

  - **Empty emission is normal.** A library file that compiles successfully but produces zero `.md` files (e.g., `prefs.glyph.md` with only `export text` constants) is not an error or warning. It contributes names and values to consumers through the validated IR.
  - **Zero consumers.** In DAG-driven compilation, unreferenced library files are never visited — no diagnostic. If a user explicitly compiles a library file (`glyph compile prefs.glyph.md`), it compiles and emits whatever qualifies, succeeding silently even if zero files are produced.
  - **Tier ownership.** Whether an `export block` qualifies for a standalone procedure `.md` is a property of the block itself, decided when the library compiles. Whether a *specific call site* in a consumer inlines that block or references the procedure file is a per-call-site decision in the consumer's Expand Step 1 (the `ResolvedCall.projection_mode` field in `ir-schema.md`). A procedure `.md` may exist but go unused at a call site that projects the block as Tier 1 (inline) or Tier 2 (same-file procedure) — this is intentional, not an error.
  - **Consumer guarantees.** DAG order (libraries compile before consumers) ensures procedure `.md` files exist before consumers reference them via `load`. If a library failed to compile, the consumer's Phase 5 (Validate) catches the missing dependency.
  - **Mixed library files.** A file exporting both `export block` and `export text` declarations is common (e.g., a `repo_tools.glyph.md` exporting both procedures and constants). The emission rules apply per-declaration — blocks may emit procedure files while constants are inlined. No special handling needed.
- **Skill body must contain at least one of `flow:` (with statements) or `constraints:` (with markers).** A skill with empty `description:`, no `flow:`, no `constraints:`, no `effects:` emits `G::analyze::empty-skill-body` (error). A constraint-only skill (no `flow:` at all, but `constraints:` present) is **legal** — its compiled output omits `### Steps` per `compiled-output.md`. An empty `flow:` body (header present but zero statements) emits `G::parse::empty-flow` (error); the author should either remove the header or add a statement.

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

`description:`, `flow:`, `effects:`, `constraints:`

`inputs:`, `outputs:`, and `when_to_use:` are deferred from MVP (see [todo.md](todo.md)). Header parameters cover input definition; `return` in `flow:` covers output; `description:` covers routing.

**Sub-section ordering is permissive.** Inside a `skill`, `block`, or `export block` body, the parser accepts `description:`, `effects:`, `flow:`, `constraints:`, and body-level constraint markers (`require`/`avoid`/`must`) in **any order**. Order is not semantically significant: a `flow:` written above `description:` produces the same IR as the conventional ordering. The only structural rule still enforced is the duplicate-subsection check (`G::parse::duplicate-subsection`, repairable) — each named sub-section may appear at most once per body.

Authors do not need to memorise a canonical layout to write valid source. `glyph fmt` rewrites every body to a canonical order so reviewable source on disk stays consistent across a codebase; see [cli.md](cli.md) §`glyph fmt` for the canonical sequence.

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

The public entrypoint that compiles to Markdown agent instructions. **Exactly one per file** (multi-skill files are rejected — see §File-Level Rules below).

**Grammar:**

```
skill <name>()
skill <name>(<params>)
skill <name>(<params>) -> <ReturnType>
```

**Example:**

```glyph
skill implement_feature(scope = ".", risk = "medium")
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

- Parentheses always required on callable declarations: `skill update_docs()` not `skill update_docs`. This applies to `skill`, `block`, `export block`, and `generated block`. Value-binding declarations (`text`, `int`, `float` and their `export`/`generated` variants) and `import` do not use parentheses.
- Return type is optional. When present, it annotates the IR's `OutputContract`; in MVP compiled output, the `return` expression folds into the final Step rather than producing a separate section (see [compiled-output.md](compiled-output.md)).
- Parameters are optional. In MVP, global preferences resolve at compile time as explicit inputs, not hidden state. Parameters appear in the compiled output's `## Parameters` section with names, descriptions, and optional defaults; the consuming LLM resolves them from user context at runtime.

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
- MVP effects: `none`, `reads_files`, `reads_env`, `writes_files`, `runs_commands`, `uses_network`, `asks_user`, `creates_artifacts`, `spawns_agent`.

### 3.4 `text` / `export text`

Named instruction text. `text` is file-private; `export text` is importable.

**Grammar:**

```
text <name> = <string-rhs>
export text <name> = <string-rhs>

<string-rhs> = <string-literal>
             | <bare-name>            // resolves to a same-file `text` / `export text`
             | <qualified-name>       // resolves to an imported `export text` via whole-module alias
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
- The RHS may be a string literal or a static reference to another `text` / `export text` (same-file bare name or imported via whole-module alias). Lower resolves the reference at compile time and inlines the underlying string content into the IR; the binding is not re-resolved at runtime. References to non-`text` declarations, parameters, locals, or anything that produces a value at flow time are rejected (a `text` body is fixed at compile time, not computed).
- These bindings are not arbitrary string interpolation. The compiler treats them as named instruction resources resolved into IR nodes.

### 3.5 `int` / `export int`

Named integer constant. `int` is file-private; `export int` is importable.

**Grammar:**

```
int <name> = <int-rhs>
export int <name> = <int-rhs>

<int-rhs> = <int-literal>
          | <bare-name>            // resolves to a same-file `int` / `export int`
          | <qualified-name>       // resolves to an imported `export int` via whole-module alias
```

**Example:**

```glyph
int max_attempts = 3
export int default_max_attempts = 3
```

**Rules:**

- No parameters, no return type. An `int` declaration is a named constant, not a callable.
- The `=` is required and separates the name from its value.
- The RHS may be an integer literal or a static reference to another `int` / `export int` (same-file bare name or imported via whole-module alias). No cross-assignment from `float` declarations or from float literals; lossless coercion at call boundaries is per [values-and-names.md](values-and-names.md). References to parameters, locals, or any non-`int` declaration are rejected.
- Lower resolves the reference at compile time and inlines the underlying integer value into the IR; the binding is not re-resolved at runtime.
- These bindings are not arbitrary numeric expressions. The compiler treats them as named constant resources resolved into IR nodes.

### 3.6 `float` / `export float`

Named floating-point constant. `float` is file-private; `export float` is importable.

**Grammar:**

```
float <name> = <float-rhs>
export float <name> = <float-rhs>

<float-rhs> = <float-literal>
            | <bare-name>            // resolves to a same-file `float` / `export float`
            | <qualified-name>       // resolves to an imported `export float` via whole-module alias
```

**Example:**

```glyph
float threshold = 0.8
export float default_temperature = 0.7
```

**Rules:**

- No parameters, no return type. A `float` declaration is a named constant, not a callable.
- The `=` is required and separates the name from its value.
- The RHS may be a float literal or a static reference to another `float` / `export float` (same-file bare name or imported via whole-module alias). No cross-assignment from `int` declarations or from integer literals; lossless coercion at call boundaries is per [values-and-names.md](values-and-names.md). References to parameters, locals, or any non-`float` declaration are rejected.
- Lower resolves the reference at compile time and inlines the underlying float value into the IR; the binding is not re-resolved at runtime.
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

### 3.8 `generated text`

Repair-materialized instruction text. Structurally identical to `text` with a `generated` prefix. Only the LLM repair pass emits this form; authors who want to define bare names manually use `text`.

**Grammar:**

```
generated text <name> = <string-literal>
```

**Example:**

```glyph
generated text root_cause_before_fix = """
    Identify the root cause before proposing or applying a fix.
"""
```

**Rules:**

- Same shape as `text`. No parameters, no return type, no body with sub-sections.
- The `=` is required and separates the name from its value.
- String literals follow `values-and-names.md`: inline `"..."` or block `"""..."""`.
- Not a callable. A bare name resolves to its string content; a parenthesized form is a compile error.
- Not exportable. `export generated text` is invalid. To share, promote to `export text`.
- All `generated text` declarations must appear after all non-generated top-level declarations in the source file.
- Full rules for authorship, stability, placement, promotion, and the no-shadowing interaction are in [repair.md](repair.md).

### 3.9 `generated block`

Repair-materialized block. Structurally a minimal `block` with a `generated` prefix. Only the LLM repair pass emits this form; authors who want to define blocks manually use `block` or `export block`.

**Grammar:**

```
generated block <name>(<params>)
    <one-sentence-body>
```

**Example:**

```glyph
generated block inspect_failure(area)
    "Inspect the failure in {area} and identify what is failing."

generated block summarize_changes()
    "Summarize what was changed and why."
```

**Rules:**

- Same header shape as `block` (parameters allowed, no return type in the generated form).
- Body is a single inline or block string — one sentence. This keeps the machine-generated definition close to the name's meaning and minimizes drift from author intent.
- Used for undefined parens-calls. Bare names without parens still materialize as `generated text`.
- Not exportable. `export generated block` is invalid. To share, promote to `block` or `export block`.
- All `generated block` declarations must appear after all non-generated top-level declarations, alongside `generated text`.
- Full rules for authorship, the one-sentence constraint, placement, promotion, and the no-shadowing interaction are in [repair.md](repair.md).

### 3.10 Parameter Syntax

Parameters appear inside parentheses on `skill`, `block`, and `export block` headers. Four forms:

```
name = "default"              // untyped, with default
name: Type = default_value    // typed, with default
```

- **Default values are mandatory on parameters whose declarations compile to `.md` files** — i.e., `skill` parameters (which surface in the skill's `## Parameters` section) and `export block` parameters (which surface in external procedure files when projected at Tier 3). The consuming LLM uses the default as a fallback when the user does not specify a value. A parameter without a default on a `skill` or `export block` emits `G::analyze::missing-param-default` (**compile error**, not repairable). Author must add an explicit default; the compiler does not synthesize defaults — there is no principled non-LLM repair, and LLM-guessed defaults would introduce non-determinism into a value that ships in compiled output.
- **Private `block` parameters may omit defaults.** Private blocks never compile to standalone files — their parameters are resolved at call sites within the same file. A private block parameter without a default is legal; the caller must supply the argument.
- Type annotations use the `name: Type` slot. The full type system is a Tier 2 concern; this reserves the syntactic position.
- Default values are either Tier 0 literals (strings, numbers, booleans, `none`) **or** a name reference to an in-scope value-binding declaration (`text` / `int` / `float`, including imported ones). Named references must be type-compatible with the parameter and must resolve at compile time — since `text`/`int`/`float` declarations are compile-time constants, this is always satisfied. The compiled `## Parameters` section emits the **resolved literal value**, not the name; consumers see the concrete default at runtime. This makes preferences (`preferences.md`) usable directly as parameter defaults: `summarize(temperature: Float = default_temperature)` resolves to the prefs library's current value at compile time. References to other parameters or to `block` declarations are not permitted; calls and arbitrary expressions are not permitted. A default that is neither a literal nor a name reference to a value-binding is a parse error.
- The compiler infers types for untyped parameters from usage context and repairs source when inference fails.

## 4. Authoring Model

### 4.1 Language Primitives

Authors build skills from defined primitives: `skill`, `export block`, `block`, `flow`, `call`, `if`, `return`. `for_each` is deferred beyond the MVP. Role and constraint markers are disambiguators; the MVP role vocabulary is defined in [ir-and-semantics.md](ir-and-semantics.md).

A `flow` becomes an ordered sequence and a `return` becomes an output contract in compiled form. Function-like calls may pass variables and bind return values; the detailed contract is in [data-flow.md](data-flow.md).

### 4.2 Inferred Instruction Roles

Source instructions need not carry compiler-shaped keywords everywhere. A bare name or inline string compiles into an inferred IR role depending on context and metadata.

The closed MVP role set ([ir-and-semantics.md](ir-and-semantics.md)): `InputContract`, `Step`, `Constraint`, `OutputContract`. (`Context` is deferred — see [todo.md](todo.md).) Obligations and prohibitions are `Constraint` nodes with strength (`soft`/`hard`) and polarity (`require`/`avoid`) attributes. **Constraint markers** — three keywords (`require`, `avoid`, `must`) composing into four forms (`require`, `avoid`, `must`, `must avoid`) — set role, strength, and polarity directly. For the complete marker-to-IR mapping, see [ir-and-semantics.md](ir-and-semantics.md).

**Constraint marker placement.** Constraint markers are legal in three positions: (1) inside a `constraints:` sub-section, (2) at declaration body level, and (3) as a flow statement inside `flow:`, including inside `if`/`elif`/`else` branch bodies. Unconditional markers (positions 1, 2, and flow-top-level in position 3) are normalized into the `constraints:` section via two complementary mechanisms: `glyph fmt` (Phase 3a) performs a source-to-source rewrite for source clarity; Phase 4 (Lower) hoists them at IR level regardless of whether fmt ran (`ir-and-semantics.md` §Body-Level Constraint Normalization, §Flow-Level Constraint Markers). Branch-scoped markers remain inline and render as part of the conditional Step prose (`compiled-output.md` §Constraint Rendering).

Inference uses: position in the skill, metadata from bindings/imports/standard-library, natural meaning of expanded text, and explicit keywords. If inference succeeds, repair adds the smallest explicit marker back into source. Compound names like `avoid_unrelated_edits` are valid identifiers — they are not forcibly split into marker-plus-concept form. The compiler uses the name prefix as evidence for role/polarity inference alongside the resolved text content. `must` is inferred only from hard-strength wording. When inference fails, the compiler emits a diagnostic.

### 4.3 Bare Names and Generated Definitions

Bare instruction names are allowed. Resolution order: (1) same-file binding, (2) explicit import, (3) standard library vocabulary, (4) MVP repair materializes a stable generated definition when context is clear. Generated definitions are cached in source, reviewable, and validated before compilation continues. Expansion happens during repair, not at runtime.

### 4.4 Semantic Shortcuts

Authors can write small function-like or identifier-like instructions directly when the name is instructive enough to expand:

```glyph
skill debug_failure(scope = ".")
    root_cause_before_fix
    reproduce_before_patch
    root_cause_trace()
```

### 4.5 Inline Instructions

One-off instruction text may be placed inline with quoted strings for short cases and block strings for longer cases:

```glyph
skill update_docs(scope = ".")
    "Do not change public behavior while updating documentation."

    flow:
        inspect_docs(scope)
        apply_doc_changes()
        "Mention any docs you could not verify locally."
```

If the same text appears repeatedly, promote it to a `text` block or imported library entry.

## 5. Source-to-IR Pipeline

Glyph source may contain shorthand, omitted annotations, text aliases, imported names, and inline natural-language instructions. The compiler resolves them through a 7-phase pipeline: **Parse → Analyze → Repair → Lower → Validate → Expand → Emit**. See [pipeline.md](pipeline.md) for the canonical reference covering phase responsibilities, the Safety Sandwich pattern, the repair loop, multi-file compilation order, and cacheability.

Source-level takeaways that shape authoring:

- Deterministic parsing and analysis run first; the LLM repair pass runs only when repairable diagnostics remain and is bounded by re-parse + re-analyze cycles.
- Repair is source-to-source: it may rewrite your `.glyph.md`, materialize `generated text` / `generated block` definitions, add minimal markers, and fix structural issues. It does not expand shorthand into agent-facing prose.
- Lower converts the repaired source into the typed IR (resolving positional args to named, desugaring nested calls, filling defaults, propagating effects).
- Expand is parameterless: compilation produces one `.md` output per source file. Parameters appear in a `## Parameters` section with defaults; the consuming LLM resolves them from context at runtime. `{param}` references in Steps and Constraints are preserved as named slots.

If ergonomic source does not compile directly, the LLM repair pass rewrites it into valid Glyph source while preserving shorthand and readability. Repair fixes compiler-blocking issues; it does not expand shorthand into prose or produce agent-facing output.

## 6. Maintenance Rules

- Prefer named `text` blocks for repeated instruction text; prefer imports for team-wide vocabulary.
- Import only explicitly exported declarations; keep `block`s and non-exported `text` private.
- Use aliases when an imported name would collide or read poorly.
- Make every `export block` self-contained (inputs, outputs, constraints, effects declared).
- Use semantic shortcuts when the name communicates intent; use inline quotes for one-off guidance.
- Both marker-plus-concept form (`avoid unrelated_edits`) and compound names (`avoid_unrelated_edits`) are valid; choose whichever reads better in context.
- The compiler should surface unresolved or ambiguous names as diagnostics, not guess silently.

## Deferred

- Full type annotation syntax beyond the `name: Type` slot (Tier 2).
- Package-style, registry-backed, or versioned imports; selective import globs.
- Skill inheritance and specialization (post-MVP, see `todo.md`).
- `for_each` control flow; `if` colon syntax (see [data-flow.md](data-flow.md)); nested private block scoping.
