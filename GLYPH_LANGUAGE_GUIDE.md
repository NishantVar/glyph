# Glyph — End-User Language Guide

This is the single document an author needs to write a skill in Glyph. It teaches the language surface that authors interact with: file shape, declarations, sub-sections, flow statements, values, names, imports, the standard library, and the compilation contract. Compiler internals (parser, IR, repair, expand, diagnostics catalogue) are out of scope — the author only needs to know what they can write and what the compiler will do with it.

---

## 1. What Glyph Is

Glyph is a small DSL for **authoring agent skills**. You write a structured `.glyph` source file. The Glyph compiler turns it into a flat, explicit Markdown skill (`.md`) that a coding agent can follow at runtime.

- The source form is for humans: structured, readable, like a tiny program.
- The compiled form is for agents: explicit prose with sections like `## Parameters`, `### Context`, `### Steps`, `### Constraints`.
- You never write the agent-facing prose by hand. You describe **structure and intent** and the compiler produces the prose.
- Glyph is a language with a compiler, not a runtime. There is no agent execution at compile time — the compiler emits instructions for an agent to follow later.

Two things to internalize early:

1. **Source can be ergonomic; the IR (and compiled output) is strict.** You can omit annotations, skip type names, write inline strings, leave names undefined, and the compiler (with a bounded LLM repair pass) will normalize and fill in. Do not over-decorate source.
2. **There is no string interpolation.** Values flow through parameters and call arguments. A `{name}` token in instruction strings is a *name reference* (parameter or local binding), not template substitution.

---

## 2. Files

A Glyph source file is named `<basename>.glyph`. There are exactly two file kinds:

| File kind | Contents | Compiled output |
|---|---|---|
| **Skill file** | exactly one `skill` declaration plus optional supporting declarations | one `<basename>.md` (the skill) |
| **Library file** | zero `skill` declarations; only `import`, value bindings, `block`, and `export …` declarations | zero or more procedure `.md` files (one per qualifying `export block`); constants are inlined into consumers |

Rules:

- A file may not contain two `skill` declarations.
- A library file must contain at least one `export` declaration.
- A file may not be empty (whitespace/comments only).
- A skill body must contain at least one of `flow:` or `constraints:` (an empty skill is rejected).

You will spend most of your time writing skill files. Library files are how you share constants and reusable blocks across skills.

---

## 3. The Minimum Viable Skill (Novice Kernel)

Authors only need a small subset to write a useful skill:

- `skill` declaration with parameters
- `require` / `avoid` constraint markers
- a `flow:` block
- quoted inline strings as instructions
- function-like calls with parentheses
- the `with` modifier on calls

Everything else — blocks, named constants, types, effects, imports, generated definitions — is discoverable later.

```glyph
skill update_docs(scope = ".")
    description: "Update repository documentation to match current code."
    require accuracy
    avoid stale_references

    flow:
        "Scan {scope} for documentation files."
        "Compare each document against the current code."
        "Update outdated or incorrect sections."
        "Verify all cross-references and links are valid."
```

That compiles to a complete agent-runnable skill. The compiler will:

- materialize stable `generated const` definitions for `accuracy` and `stale_references` (or repair them from same-file `const` if you add them),
- generate a description from the body if you omit one,
- expand each instruction into agent-followable prose under `### Steps`,
- emit constraints under `### Constraints`,
- preserve `{scope}` as a runtime parameter slot.

---

## 4. Indentation, Line Layout, Comments

- **4-space indentation, significant.** No tabs; tabs are a hard error. No braces, no `end` keywords.
- **No trailing colon on top-level declarations.** `skill update_docs()` not `skill update_docs():`. Colons mark *sub-section headers* inside a body (`flow:`, `constraints:`, etc.).
- **Blank lines inside a body are visual separators only** — they do not close the block.
- **Implicit line continuation only inside paired delimiters** (`(...)`, `{...}`, `"""..."""`). No backslash continuation.
- **Line comments use `//`.** No block comments. Comments are stripped from compiled output.

```glyph
// This is a comment.
skill plan_release(scope = ".")    // trailing comment is fine
    flow:
        // Discover candidate features.
        candidates = collect_candidates(scope)

        // Order them by readiness.
        order_candidates(candidates)
```

---

## 5. Declarations

These are the top-level building blocks that may appear at column 0. The MVP set:

| Declaration | Purpose |
|---|---|
| `skill` | the public, compiled entrypoint (one per skill file) |
| `block` | private callable helper, scoped to the file |
| `export block` | importable callable, must be self-contained |
| `const` / `export const` | named compile-time constant (string, integer, or float — kind inferred from the literal) |
| `import` | bring exported names from another file into scope |
| `generated const` / `generated block` | repair-materialized definitions; you don't write these manually |

### 5.1 `skill`

```
skill <name>()
skill <name>(<params>)
skill <name>(<params>) -> <ReturnType>
```

- Parentheses always required, even with no parameters.
- Return type is optional. When present, it folds into the closing sentence of the final Step in compiled output (no separate `### Returns` section). Only **domain types** (`Plan`, `RepoContext`, `Diagnosis`, …) are valid here — there are no primitive type names like `String` or `Int` in author-facing source. Omit `->` entirely if the skill produces no meaningful return value.
- Parameters resolve at runtime: each declared parameter shows up in the compiled `## Parameters` section. The consuming agent must supply each *required* parameter (no default) from the user's request context. Parameters with defaults are optional at runtime.

### 5.2 `block`

A private helper, callable from within the same file.

```glyph
block make_plan(ctx, risk = "medium") -> Plan
    flow:
        analyze(ctx)
        return draft_plan(ctx, risk)
```

**Single-string shorthand.** When a block body contains exactly one instruction string and no other sub-sections, you may omit `flow:`:

```glyph
block summarize_changes()
    "Summarize what was changed and why."
```

The bare string is always treated as an instruction (Step). For metadata about the block itself, use `description:`.

### 5.3 `export block`

An importable, self-contained block. Two-keyword prefix.

```glyph
export block inspect_repo(scope = ".") -> RepoContext
    description: "Inspect the repository structure and identify key files."

    flow:
        "Scan files under {scope}."
        "Identify relationships between source files."
        return repo_context()
```

Hard rules unique to `export block`:

- **Return type required when the block produces a meaningful return value.** Use `-> DomainType` (any identifier — `RepoContext`, `Plan`, `FailureReport`). Primitive type names are not part of the author-facing surface; if the value is "really" a string or int, give it a domain name that tells an agent what role it plays (`BranchName`, `Severity`, `Confirmation`).
- **Omit `->` entirely if the block produces no meaningful return value.** No `-> None` annotation — its absence is the signal.
- **Every parameter must have a default.** A required parameter without a default is a hard compile error (no LLM repair).
- **Must end with an explicit `return`.** A missing `return` on a non-shorthand body is *repairable* — Phase 3 inserts `return none` and leaves a comment — but you should write it explicitly. Even instruction-only blocks should `return none`.
- **Must be closed.** Behavior depends only on declared inputs, local bindings, explicit imports, same-file declarations, the standard library, and declared constraints/effects. No hidden context.
- The single-string shorthand is allowed only for export blocks that omit `->` (no meaningful return); in that case `return none` is implied. A shorthand body cannot stand in for a meaningful return value — use the full `flow:` form with an explicit `return`.

### 5.4 Value bindings — `const`

Named compile-time constants. A single `const` keyword covers strings, integers, and floats; the compiler infers the value kind from the literal on the right side. The `=` is required.

```glyph
const preserve_existing_patterns = """
Prefer the repository's existing patterns, helper APIs, naming, and file organization
before introducing a new abstraction or style.
"""

const max_attempts = 3
const threshold = 0.8

export const safety_first = "Never execute destructive operations without confirmation."
export const default_max_attempts = 3
export const default_temperature = 0.7
```

Rules:

- No parameters, no body, no return type. They are values, not callables.
- The right-hand side may be a literal **or** a static reference to another `const` (same-file bare name, or imported via whole-module alias). The kind of the right side determines the kind of the constant — strings, ints, and floats can all use `const`, but you cannot cross kinds when reassigning (a string-valued reference can't initialise an int-valued reference, etc.).
- String content may be inline `"..."` or block `"""..."""`.
- Lower resolves the reference at compile time; the value is inlined.

**A string `const` is not callable, and bare names in `flow:` mean different things.** A bare string-valued `const` in `flow:` *without* a marker like `context`/`require`/`avoid`/`must` is an error (`G::analyze::const-in-flow`) — `const` is for constraint or context content, not for instruction steps. For instructions, use `block`. (See §10.4 for the full bare-name resolution order.)

### 5.5 `import`

Two forms.

**Whole-module:**

```glyph
import "./repo_tools.glyph" as repo_tools

ctx = repo_tools.inspect_repo(scope)
```

**Selective:**

```glyph
import "./prefs.glyph" { preserve_existing_patterns, validation_strictness }
import "./repo_tools.glyph" { inspect_repo as inspect, has_test_suite }
```

For long lists, the brace body may span multiple lines. A trailing comma is allowed:

```glyph
import "./glyph_authoring_passes.glyph" {
    factor_long_instructions_and_texts,
    sort_declarations,
    compile_and_iterate,
}
```

Items themselves stay on a single line — `name as alias` does not split across lines. Indentation inside the braces is for readability only; the parser does not validate it.

Rules:

- Path is always quoted. Relative paths only (`./...`, `../...`); base directory is the importing file's directory.
- Whole-module form requires `as <alias>` and exposes the file's `skill` (via `M.skill_name`), plus all `export …` declarations.
- Selective form imports only explicitly exported declarations. Trailing comma allowed.
- A single import statement is *either* whole-module *or* selective.
- **No re-exporting.** A consumer must import directly from the file that defines a name.
- **No circular imports.** Refactor shared names into a third file.
- The `@glyph/` prefix is reserved for compiler-shipped modules. Today `@glyph/std` resolves to the standard library (§12); `@glyph/prefs` is reserved for the standard preferences library (§13).

### 5.6 `generated const` / `generated block` (informational)

These are produced by the **repair pass** when your source uses an undefined name and the compiler can confidently materialize a definition. You don't write them by hand — the compiler writes them back into your source after a clarifying repair cycle. Treat them as cached, reviewable, source-level evidence of what the compiler inferred. If you don't like what was generated, either rename to use an explicit `const`/`block`, or edit the generated definition (then it's no longer "generated" in spirit; promote it to a hand-authored declaration). `generated const` always materializes a string-valued constant; numeric `generated const` is not produced by repair.

---

## 6. Parameters

Parameter syntax appears inside the parentheses on `skill`, `block`, and `export block` headers. Forms:

```
name                          // untyped, no default
name = "default"              // untyped, with default
name: Type                    // typed, no default
name: Type = default_value    // typed, with default
```

Defaults can be:

- a literal (string, int, float, bool, `none`),
- or a name reference to an in-scope `const` (same-file or imported).

**Cannot** be a parameter reference, a block reference, an arbitrary expression, or a call.

Type annotations on parameters use **domain types only** — `name: Plan`, `name: BranchName`, `name: Severity`. There are no primitive type names in author-facing source; if the value really is a plain string or int, give it a domain name that tells an agent what role it plays. Type annotations are optional in MVP; the compiler uses them for nominal matching at call boundaries when both sides are annotated.

**Default-availability rules vary by declaration kind:**

| Declaration | Parameter without default? |
|---|---|
| `skill` | Allowed — becomes a runtime-required input the agent must extract from the user. |
| `export block` | **Hard error.** Defaults are mandatory. |
| `block` (private) | Allowed — caller must supply the argument at the call site. |

Type annotations are optional in MVP; the compiler infers or ignores them (see §10 Types).

### 6.1 Parameter slots `{name}` in instruction strings

You may reference a parameter or a local binding inside an instruction string using `{name}`:

```glyph
skill summarize_dir(scope = ".", target)
    flow:
        "Inspect files under {scope}."
        "Write the summary to {target}."
```

Rules:

- The slot grammar is strict: `{IDENTIFIER}` only. Anything else with braces (`{ "key": "value" }`, `{x, y}`) is treated as literal text.
- Slots are legal **only inside instruction-bearing strings**: string-valued `const` bodies, inline strings inside `flow:`, constraint texts, and string arguments to stdlib calls. They are **illegal** in `description:` bodies, parameter defaults, etc.
- A `{name}` that doesn't resolve to a parameter or local binding in scope is a hard error.
- **Parameter references** survive into the compiled Markdown as literal `{name}` slots — the consuming agent fills them at runtime.
- **Local-binding references** (e.g., `{diagnosis}` after `diagnosis = analyze_error(...)`) are rewritten by the compiler into natural-language cross-references in compiled prose ("...based on the diagnosis from your earlier analysis.").

---

## 7. Sub-Sections Inside a Body

A `skill`, `block`, or `export block` body may contain these colon-terminated sub-sections:

| Section | Spelling | Content |
|---|---|---|
| `description:` | singular | one-line summary; goes to compiled YAML frontmatter |
| `effects:` | plural | declared effect keywords (gated — see §11) |
| `context:` | singular (set-like) | background framing the agent should understand |
| `constraints:` | plural | constraint markers |
| `flow:` | singular | ordered workflow steps |

Each named sub-section may appear **at most once** per body.

**Order is permissive** — write them in any order. `glyph fmt` rewrites them into a canonical sequence on disk:

1. `description:`
2. `effects:`
3. `context:`
4. `constraints:`
5. `flow:`

**Long form vs short form** — both accepted, identical IR:

```glyph
// Long form
effects:
    - reads_files
    - runs_commands

// Short form
effects: reads_files, runs_commands
```

### 7.1 `description:`

A concise, one-line summary of when to use the skill. Compiles to `description` in the YAML frontmatter — this is the primary trigger that a coding agent reads when picking a skill.

Body must be **exactly one** quoted string literal *or* a bare-name reference to a same-file `const` / `export const`:

```glyph
skill fix_bug(scope = ".")
    description: "Debug and fix a bug in the codebase with minimal, targeted changes."
```

```glyph
const fix_bug_routing = """
Use when the user reports a bug, regression, or unexpected behavior...
"""

skill fix_bug(scope = ".")
    description: fix_bug_routing
```

- No `{param}` slots inside `description:`.
- On a `block` / `export block`, `description:` is optional unless the block is consulted via `BLOCKNAME.applies()` (§8.7). When consulted, the description is the predicate the agent matches.
- On a `skill`, omitting `description:` triggers repair to generate one from the skill name and body. Prefer to write it explicitly.

### 7.2 `constraints:` and constraint markers

A constraint is a behavioral rule. There are three keywords composing four forms:

| Marker | Strength | Polarity |
|---|---|---|
| `require` | soft | positive (do this) |
| `avoid` | soft | negative (don't do this) |
| `must` | hard | positive |
| `must avoid` | hard | negative |

Two equivalent surface styles:

- **Marker-plus-concept:** `avoid unrelated_edits` — the marker carries polarity, the concept name is polarity-neutral.
- **Compound name:** `avoid_unrelated_edits` — the name itself carries the semantics.

Both are valid; pick whichever reads better. The constraint content can be a bare name (resolves to a same-file `const` or generated definition) or an inline string:

```glyph
skill fix_bug(scope = ".")
    constraints:
        require preserve_existing_patterns
        avoid unrelated_edits
        must "Never modify the database schema without confirmation."
        must avoid breaking_public_api
```

**Placement is flexible.** Constraint markers may appear:

1. inside `constraints:`,
2. directly at the body level (no `constraints:` wrapper),
3. as a flow statement inside `flow:` (including inside an `if`/`elif`/`else` arm).

```glyph
skill fix_bug(scope = ".")
    require preserve_existing_patterns      // body-level marker
    avoid unrelated_edits

    flow:
        ...
```

Body-level and flow-top-level markers are **hoisted** into the declaration's `constraints:` section by the compiler (and by `glyph fmt` in source). Markers inside a branch arm stay inline and render as part of the conditional Step prose ("If X, do not do Y.").

`must` should be reserved for genuinely hard constraints. Strong wording (`must`, `never`) inferred from name prefixes also signals hard strength.

#### Canonical form for constraint text

Constraint text is rendered through a locked four-form template:

| Strength × Polarity | Template |
|---|---|
| `must` (hard require) | `You must <text>.` |
| `must avoid` (hard avoid) | `You must never <text>.` |
| `require` (soft require) | `<Text>.` |
| `avoid` (soft avoid) | `Avoid <text>.` |

Write the text as a **noun phrase or imperative clause**: lowercase first word, no trailing period. The compiler applies defensive normalization for capitalization and trailing periods, but downstream the text is slotted into the template literally — non-canonical text produces grammatical mismatches.

Examples:

- `avoid: leaving references to removed symbols` → `Avoid leaving references to removed symbols.`
- `require: tests pass before merging` → `Tests pass before merging.`
- `must avoid: editing files outside the declared scope` → `You must never editing files outside the declared scope.` *(grammatical mismatch — author should rewrite as `edit files outside the declared scope`)*

A future Repair pass may auto-canonicalize non-conforming text; today, this is the author's responsibility.

### 7.3 `context:` and context markers

Context is informational background — facts the agent should understand while executing. It does **not** direct action and does **not** carry strength/polarity.

```glyph
skill fix_bug(scope = ".")
    context:
        project_conventions
        "This codebase uses a monorepo layout with per-crate Cargo.toml files."
    flow:
        ...
```

Body grammar matches: bare-name references to string-valued `const` / `export const` declarations, inline strings, or `context`-prefixed markers. Multiple entries allowed.

Like constraint markers, `context` markers can also appear at body level or inside `flow:`:

```glyph
skill fix_bug(scope = ".")
    context project_conventions
    context "Monorepo layout with per-crate Cargo.toml files."
    flow:
        ...
```

Same hoisting rule: top-level context markers are hoisted into `context:`; branch-scoped markers stay inline.

No `{param}` slots inside `context:`.

### 7.4 `flow:` and flow statements

`flow:` is the ordered workflow. Statement forms:

| Form | Example | Role |
|---|---|---|
| Binding | `ctx = inspect_repo(scope)` | Step (with output binding) |
| Bare call | `apply_changes(plan)` | Step |
| UFCS call | `researcher.send("check edges")` | Step |
| Bare name | `validate_before_success` | Step (resolved by name resolution) |
| Inline string | `"Mention any issues found."` | Step |
| Constraint marker | `avoid unrelated_edits` | Constraint (hoisted or inlined) |
| Context marker | `context project_conventions` | Context (hoisted or inlined) |
| `return` | `return summarize(plan)` | Output contract |
| `if`/`elif`/`else` | `if risk == "high":` … | Branch |

A statement call without a binding discards its return value.

**A bare string in a body is always a Step.** It is never context or background. For background, use `context:` or `description:`. For named string constants, use `const`.

### 7.5 `effects:` (gated — see §11)

Declared effect keywords. Today disabled by default behind `--enable-effects`.

---

## 8. Calls and Control Flow Inside `flow:`

### 8.1 Positional and named arguments

```glyph
plan = make_plan(ctx, risk)                         // positional
plan = make_plan(ctx, risk = "high")                // mixed
plan = make_plan(ctx = context, risk = "high")      // all named
```

- Positional must precede named — no positional after a named arg.
- A named arg cannot duplicate a parameter already filled positionally.
- All required parameters must be supplied.
- Trailing commas allowed (multi-line argument lists are common):

```glyph
plan = make_plan(
    ctx,
    risk = "high",
    verbose = true,
)
```

### 8.2 Qualified callees (whole-module imports)

```glyph
import "./repo_tools.glyph" as repo_tools

ctx = repo_tools.inspect_repo(scope)
```

The left side of the dot must be a whole-module import alias.

### 8.3 UFCS — `value.method(args)`

`x.foo(args)` desugars to `foo(x, args)` — the receiver becomes the first argument. This is pure syntactic sugar in a single namespace; there is no method dispatch.

```glyph
import "@glyph/std" { subagent, send }

skill investigate(scope = ".")
    flow:
        researcher = subagent(scope) with "investigate this area"
        researcher.send("Now check edge cases around token expiry.")
        return researcher
```

`researcher.send(msg)` desugars to `send(researcher, msg)`. The compiler resolves `send` through normal name resolution.

### 8.4 The `with` modifier

A trailing `with "..."` clause specializes a call site. It does not change the callee's contract — it shapes the wording of the expanded Step for that one invocation.

```glyph
flow:
    inspect_failure(scope) with "focus on auth boundaries"
    summarize_changes() with "include any remaining gaps"
```

- One `with` clause per call site. No chaining (`with ... with ...`).
- Not visible in compiled output as the literal text "with"; the modifier is consumed by the expand pass.
- Works with bare calls, qualified calls, UFCS calls, and bound calls (`x = foo() with "..."`).
- Does not apply to bare-name statements (no parens).

### 8.5 Nested calls

Nested calls are legal:

```glyph
result = validate(make_plan(ctx, risk))
apply_changes(merge(base, overlay))
```

The compiler desugars them into flat IR with synthetic temporaries. Conventionally, prefer intermediate named bindings — they read better and visualize better.

### 8.6 Branching — `if` / `elif` / `else`

Python-style colon-terminated headers, significant indentation:

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

Allowed condition forms:

| Form | Example |
|---|---|
| Boolean identifier or binding | `if is_valid:` |
| Boolean-returning call | `if has_tests(ctx):` |
| Single-level dot access | `if ctx.has_tests:` |
| `not` | `if not is_valid:` |
| Equality / inequality | `if risk == "high":` / `if risk != "low":` |
| `and` / `or` | `if a and b:`, `if a or b:` |
| Parenthesized grouping | `if (a or b) and c:` |
| Block trigger predicate | `if fork_with_plan.applies():` |

Standard Python precedence: `not` > `and` > `or`. No `<`, `>`, `<=`, `>=`, no arithmetic, no `in` — bind a boolean call result instead.

Branch bodies may contain any flow statement form **except `return`** — `return` is restricted to flow top level (see §8.8).

### 8.7 Block trigger predicate — `BLOCKNAME.applies()`

A special syntactic form for description-driven dispatch inside an `if`/`elif` condition.

```glyph
if deep_investigation.applies():
    "Trace symptoms through multiple code layers."
elif has_test_suite.applies():
    "Run the existing test suite first."
```

Rules:

- Receiver must resolve to a `block` / `export block` (or `module_alias.block_name`) carrying a `description:`. The description is the natural-language predicate the consuming agent matches against current context.
- Name `applies` and the empty parens are fixed: `.applies(arg)` is an error; `.applies` (no parens) is an error.
- Only valid inside an `if`/`elif` condition. Cannot bind to a variable, return, or pass as an argument.
- Composes with `and`/`or`/`not`.
- A block consulted via `.applies()` *must* have `description:`. If missing on a same-file block, repair generates one. If missing on an imported block, it's a hard error and the author must add it in the source library.
- Calling a block by name (`my_block()`) is unrelated; `applies()` is opt-in at the call site for description-routed dispatch.

### 8.8 `return` (and output target expressions)

Return-expression forms:

```
return <expr>                            // call, binding, dot access, literal, or `none`
return                                   // equivalent to `return none`
return <output_name>                     // output target (identifier form)
return <"what to synthesize">            // output target (descriptive form)
```

Hard rules:

- **At most one `return` per `flow:`**, and when present it must be the **last statement at the top level** (not inside `if`/`elif`/`else`).
- `return` is forbidden inside branch arms — there is no early return in MVP.
- **`export block` requires an explicit `return`** (even `return none`). For `skill` and private `block`, omitting `return` is fine; the compiler implicitly returns `none`.
- The return type annotation (`-> Plan`, `-> Agent`, etc.) is **advisory** — used to shape compiled prose ("Your output should be a Plan containing…") and for nominal type matching at call boundaries. There is no runtime structural enforcement.

The return expression folds into the closing sentence of the final Step in compiled output. There is no `### Returns` section.

#### Output target expressions (`<name>` and `<"description">`)

When the return value is **synthesized by the agent from prose instructions** rather than produced by a callable expression, name it as an *output target*. This is one of four visually distinct name forms in Glyph:

```glyph
{name}                // prose slot inside an instruction string (parameter or local binding ref)
name                  // ordinary identifier resolving to an existing value or declaration
<name>                // output target — a value the agent must produce (identifier form)
<"description">       // output target with descriptive guidance (descriptive form)
```

**Identifier form** — `return <current_branch>` reads as "the return value is a thing called *current_branch* that you, the agent, will produce." The compiler does *not* resolve `current_branch` to an existing binding; the angle brackets explicitly mark it as agent-synthesized.

```glyph
block read_current_branch(repo_path) -> BranchName
    flow:
        "Run `git rev-parse --abbrev-ref HEAD` in {repo_path} and capture the branch name."
        return <current_branch>

block ask_user_to_confirm(candidates) -> Confirmation
    flow:
        "Show the candidate list and ask for confirmation. Treat only an unambiguous affirmative as true."
        return <confirmed>
```

**Descriptive form** — `return <"…">` lets you say *what* the agent should synthesize without having to coin a name first:

```glyph
export block diagnose_issue(scope) -> Diagnosis
    flow:
        inspect_repo(scope)
        return <"root cause analysis including affected files and severity">
```

Rules:

- Identifier form: exactly `<IDENTIFIER>`. No spaces (`<current branch>` is invalid). Type-looking names (`<Diagnosis>`) are diagnostic-worthy — output target names should follow ordinary value naming (`snake_case`).
- The target name should not collide with an existing visible value. If `name` is already bound, use plain `return name`.
- No expressions inside the angle brackets — `<foo()>`, `<a.b>` are invalid.
- The `-> DomainType` on the header is the **compiler contract** (nominal matching at call boundaries). The `<"description">` is **agent guidance** (what to synthesize). Both may co-exist on the same block.
- Output targets are **terminal-return-only in MVP**. They do not introduce a local binding. Use them only where the producer is prose, judgement, human interaction, or extraction — not where a callable expression already produces the value.
- The compiled Markdown never contains a literal `<name>` or `<"…">` token; Expand turns it into natural output prose.

When in doubt, prefer a normal binding:

```glyph
selected = ask_user(candidates)
apply_candidate(selected)
return selected
```

Reach for `<name>` / `<"…">` only when the producer is the prose itself.

---

## 9. Values

### 9.1 Strings

- Inline: `"..."` (double quotes only; no single quotes).
- Block: `"""..."""` — multiline, common leading indentation stripped (Python `textwrap.dedent`).
- Escapes: `\"` and `\\` only. No `\n`, `\t`, no Unicode escapes.
- **No interpolation, no concatenation.** No `${...}`. No `+` operator on strings.
- The only template-like feature is `{name}` parameter slots inside instruction-bearing strings (§6.1).

### 9.2 Integers and floats

- Integers: standard decimals. No leading zeros. Negative literals allowed: `-1`.
- Floats: digits required on both sides of the point. `0.5` valid; `.5` and `3.` not. No `1e10` scientific notation.
- **Numeric coercion at call boundaries** is automatic and lossless: `3.0` to `Int` becomes `3`; `3` to `Float` becomes `3.0`; `3.7` to `Int` is a compile error (lossy).

### 9.3 Booleans

`true` and `false`. Source is case-insensitive (`True`, `TRUE` accepted), IR normalizes to lowercase.

### 9.4 `none`

Reserved keyword for absence of value. Usable wherever a value is expected: `return none`, `result = none`, `effects: none`. Source case-insensitive; IR is lowercase.

### 9.5 No value-level operators

MVP expressions support only: literals, bindings, calls, dot access. No `+`, `-`, `*`, `/`, no comparisons in arbitrary expressions (only inside `if` conditions, with the limited operator set above). If you need to combine context with a call, use `with`. If you need a derived boolean, bind the result of a call.

---

## 10. Identifiers, Names, Types

### 10.1 Identifiers

- Pattern: `[a-zA-Z_][a-zA-Z0-9_]*`. No hyphens.
- Convention: `snake_case` for values and callables; `PascalCase` for types.
- **Case-normalized.** `makePlan`, `make_plan`, `MakePlan`, `MAKE_PLAN` resolve to the same name.
- Dots are reserved for module-qualified access and single-level dot-property access on bound values (e.g., `ctx.has_tests`).

### 10.2 Reserved keywords

`skill`, `block`, `export`, `import`, `const`, `flow`, `call`, `if`, `elif`, `else`, `return`, `true`, `false`, `none`, `effects`, `constraints`, `inputs`, `outputs`, `when_to_use`, `description`, `as`, `generated`, `input`, `output`, `must`, `require`, `avoid`, `context`, `and`, `or`, `not`.

Cannot be used as identifiers.

### 10.3 No shadowing

If the same normalized name is visible from multiple sources in overlapping scopes, **it's a hard error** — not a warning, not a silent fallback. Fix by renaming one of them. This applies across:

- parameter vs same-file `const`,
- local binding vs parameter,
- import vs same-file declaration.

### 10.4 Bare-name resolution order

A bare identifier (no parens) may resolve to:

1. a `const` declaration in the current file,
2. a parameter of the enclosing skill or block,
3. a local binding,
4. an imported name (selectively-imported stdlib entries from `@glyph/std` enter the namespace at this level — they are not a separate resolver step, and they require an explicit `import`),
5. a repair-generated definition (`generated const` / `generated block`).

A parenthesized form (`foo()` or `foo(x)`) is always a callable. A bare `foo` is never a call.

If a bare name in `flow:` is undefined, the compiler treats it as an intended callable and materializes a `generated block`. Bare names that resolve to a string-valued `const` are a compile error in `flow:` unless prefixed with a marker (`context`/`require`/`avoid`/`must`).

### 10.5 Types

Types in MVP are **semantic labels** for an LLM reading the compiled output, not enforced structural contracts.

**There are no primitive type names in author-facing source.** You never write `String`, `Int`, `Float`, `Bool`, or `None` as type annotations — `-> String` or `value: Int` carries no useful semantic signal to an agent. The compiler still tracks primitive kinds internally (inferred from literals, defaults, and usage) but never surfaces them.

Use **named domain types** instead — any identifier names a domain type:

```glyph
block inspect_repo(scope) -> RepoContext
block make_plan(ctx: RepoContext, risk = "medium") -> Plan
export block validate_changes(files: FileSet, strict = true) -> ValidationResult
```

Examples: `RepoContext`, `Plan`, `FailureReport`, `BranchName`, `Severity`, `Confirmation`, `Diagnosis`. They are opaque tags — no structural definition in MVP. Domain types are **implicitly declared by first use** in a `-> Type` position; no separate `type Foo` declaration is required (or supported). The meaning of the type is contextually defined by the `<"description">` at return sites and by the names used in compiled prose.

The compiler does **nominal matching** at call boundaries: if the type names match, values are compatible; if they differ, it's an error; if either side is untyped, no check is performed. Two blocks returning the same `-> Type` with different `<"…">` descriptions is fine — descriptions are local to each block and don't participate in nominal matching.

The one compiler-known type name is **`Agent`** (see §12) — the handle returned by `subagent()`. It behaves like any other domain type at call boundaries.

Type position is determined by syntax:

- After `:` in a parameter declaration: `name: DomainType`.
- After `->` in a return type: `-> DomainType`.

**Omit `->` entirely when there is no meaningful return value.** There is no `-> None` annotation; the absence of `->` is the signal. The `none` value keyword stays in value positions: `return none`, `result = none`, `effects: none`.

Field access (`result.findings`) is recorded but not validated against the type. Trust your call graph in MVP.

**A note on PascalCase.** PascalCase is convention only — identifiers are case-normalized, so `RepoContext` and `repo_context` resolve to the same name. PascalCase visually distinguishes types from values; pick a domain name that tells an agent the role the value plays (`BranchName`, not `String`).

---

## 11. Effects (Gated)

> **Today, effects are disabled by default.** They are enabled with `--enable-effects`. When the gate is off, the parser rejects any `effects:` sub-section. Skip this section if you're writing the simplest possible skill; come back to it when your skill calls into stdlib or when you start declaring multi-file libraries with explicit contracts.

### 11.1 The nine effect keywords

| Keyword | Meaning |
|---|---|
| `none` | no meaningful effects |
| `reads_files` | inspects files / source / logs |
| `reads_env` | reads env vars, system state, git metadata, project config |
| `writes_files` | creates or modifies files |
| `runs_commands` | invokes shell / test runners / formatters / linters / package managers |
| `uses_network` | network access, package downloads, remote APIs |
| `asks_user` | pauses for human input or approval |
| `creates_artifacts` | produces durable outputs (reports, generated assets, archives) |
| `spawns_agent` | spawns or interacts with subagents (stdlib) |

### 11.2 Syntax

```glyph
// inline
effects: reads_files, runs_commands

// list
effects:
    - reads_files
    - writes_files
    - runs_commands
```

`effects: none` is an explicit assertion of "no side effects" and is incompatible with the call graph showing any effect.

### 11.3 Inference and validation

- **Omit `effects:` entirely** → compiler infers from the call graph and auto-adds the inferred set during repair (warning, informational).
- **Declare `effects:` explicitly** → declared set must be a **superset** of the inferred set. If you under-declare (call graph implies an effect you didn't list), it's an error. Over-declaring is a warning, not an error.
- **Across imports**: a caller's declared effects must be a superset of every imported callee's declared effects and every inlined private callee's inferred effects.

You should usually start by **omitting `effects:`** and let the compiler tell you what it inferred, then promote to an explicit declaration once you want to lock the contract down.

---

## 12. Standard Library

The stdlib is compiler-embedded under the reserved virtual prefix `@glyph/`. Three entries:

| Name | Author-facing? | Purpose |
|---|---|---|
| `subagent` | yes | spawn a delegated agent |
| `send` | yes | message a running subagent |
| `load` | no (compiler-internal) | load and follow a procedure file |

Stdlib names are **not auto-available**. Import explicitly:

```glyph
import "@glyph/std" { subagent, send }
```

### 12.1 `subagent(task) -> Agent`

```glyph
import "@glyph/std" { subagent, send }

skill investigate(scope = ".")
    flow:
        researcher = subagent(scope) with "investigate this area"
        researcher.send("Now check edge cases around token expiry.")
        return researcher
```

Compiles to prose like:

```
1. Spawn a subagent to investigate the given scope. Refer to this agent as "researcher."
2. Send the researcher this follow-up: "Now check edge cases around token expiry."
3. Your result is the researcher agent spawned above — the caller may continue sending it instructions.
```

### 12.2 `send(agent: Agent, message)`

Use UFCS for readability: `agent.send("…")` desugars to `send(agent, "…")`. It has no meaningful return value. Effects are `spawns_agent`.

### 12.3 The `Agent` type

- Compiler-known primitive type. No literal form — the only way to create one is `subagent(...)`.
- Participates in nominal matching like any other type.
- A `block` declaring `-> Agent` must transitively obtain its return value from a `subagent` call (directly, through an imported callee, or through an `Agent` parameter).
- An `Agent` returned from a skill represents the **handle** to the spawned agent (not the agent's findings). If you want the agent's output, pass an instruction string instead.
- No identity equality, no termination primitive, no await — opaque handles only.

### 12.4 Effect boundary at subagent spawns

A skill that spawns a subagent declares `spawns_agent`. It does **not** inherit the spawned skill's effects. The spawned skill is a separate compilation unit with its own effect surface.

---

## 13. Library Files and Preferences

### 13.1 Library file shape

A library file is just a `.glyph` with no `skill`. Example: a preferences file.

```glyph
// prefs.glyph
export const preserve_existing_patterns = "Prefer the repository's existing patterns…"
export const safety_first = "Never execute destructive operations without explicit confirmation."
export const validation_strictness = 2
export const default_temperature = 0.7
```

A consumer imports normally:

```glyph
import "./prefs.glyph" { preserve_existing_patterns, validation_strictness }

skill fix_bug(scope = ".")
    require preserve_existing_patterns
    flow:
        ...
```

### 13.2 Preferences are ordinary constants

There is no `pref(...)` call form, no `reads_prefs` effect, no ambient lookup. A preference is just an `export const`. The compiler infers the value kind (string, integer, float) from the literal. An RHS value is mandatory on every `const` declaration.

Preferences may also serve as **parameter defaults**:

```glyph
import "./prefs.glyph" { default_temperature }

skill summarize(temperature: Temperature = default_temperature)
    flow:
        ...
```

The default is resolved at compile time and the literal value appears in the compiled `## Parameters` section. (Note the parameter type is `Temperature` — a domain name — not `Float`; primitive type names are not part of author-facing source.)

When a preference value changes, recompile the consuming skills.

---

## 14. What Compiled Output Looks Like

The compiler emits one Markdown file per skill with the following shape:

```md
---
name: <skill-name>
description: <one line>
effects: [<keyword>, <keyword>]   # only when --enable-effects AND set is non-empty
---

## Parameters
- **scope**: description (default: ".")
- **target**: description (required)

## Instructions

### Context
- Background point 1.
- Background point 2.

### Steps
1. First step prose.
2. Second step prose. {scope} survives as a runtime slot.
3. If the risk is high and tests exist:
   a. Run the full test suite.
   b. Request a code review.
   Otherwise:
   a. No action needed.

### Constraints
- Strong: must avoid breaking the public API.
- Soft: prefer existing patterns.
```

Notes:

- Frontmatter always has `name` (taken from the `skill` declaration) and `description`. There is no `# <Skill Name>` heading — the frontmatter `name` is the authoritative title.
- `## Parameters` is only present if the skill declares parameters.
- `### Context` only if there's a `context:` section or context markers.
- `### Constraints` only if any unconditional constraints exist.
- `### Steps` is omitted only for pure constraint-only skills (no `flow:` at all). At least one of `### Steps` or `### Constraints` is always present.
- Branches project to a single numbered Step with lettered sub-steps per arm. Letters reset per arm.
- The `return` expression folds into the final Step's closing sentence; there is no `### Returns` section.
- Imports compile away — no import paths or module names appear in the output.

You don't need to know the exact projection to write a skill — but knowing the shape helps you anticipate what the agent will read.

---

## 15. The Authoring Loop

When you run the compiler on your `.glyph`:

1. **Parse** — checks indentation, syntax, declarations.
2. **Analyze** — resolves names, checks closure, infers effects, validates types.
3. **Repair** (LLM-assisted, bounded) — fixes repairable issues: materializes `generated const` / `generated block` for undefined names, adds missing markers, generates a missing `description:`, hoists body-level constraints into `constraints:`, etc. **Repair edits your source file**, leaves comments where it acted, and is idempotent.
4. **Lower** — compiles the repaired source into typed IR.
5. **Validate** — strict invariants on IR.
6. **Expand** (LLM-assisted, per-invocation) — turns IR steps and constraints into agent-facing prose.
7. **Emit** — writes the `.md`.

What you experience day-to-day:

- Diagnostics with IDs like `G::parse::<thing>`, `G::analyze::<thing>`, `G::repair::<thing>`. **Errors** must be fixed; **repairable** diagnostics are auto-fixed; **warnings** are informational.
- Repair may rewrite your source — review the diff. Generated definitions appear after all hand-authored declarations and are clearly marked `generated const` / `generated block`.
- If you want to harden a generated definition, promote it: rename `generated const X` to `const X` (or `export const X`) in source. Repair never overwrites hand-authored declarations.

---

## 16. Style and Maintenance Conventions

- **Use the novice kernel for short skills.** `skill`, `require`/`avoid`, `flow:`, inline strings, calls, `with`. Reach for blocks, named text, and types only when they pay for themselves.
- **Promote to a `const` declaration when an inline string repeats.** Promote to a `block` when an instruction sequence repeats.
- **Promote to `export …` only when another file genuinely needs the name.** Otherwise keep it private.
- **Prefer marker-plus-concept (`avoid unrelated_edits`) when the concept is shared / reusable.** Prefer compound names (`avoid_unrelated_edits`) when the meaning is one self-contained idea.
- **Use `must` sparingly.** Reserve hard constraints for genuinely non-negotiable rules; everyday "should" rules are `require`/`avoid` (soft).
- **Use `description:` to communicate routing intent on skills**, and on blocks consulted via `.applies()`.
- **Use `with` to specialize a call site** instead of constructing a custom block for every single nuance.
- **Name intermediate bindings descriptively**; deeply nested calls are legal but harder to read and visualize.
- **Trust the compiler.** If a name is undefined and the meaning is clear from context, repair will materialize a generated definition. Review and promote if needed.
- **Don't try to write the agent prose yourself.** The Expand pass does that. Your job is structure and intent.

---

## 17. Common Pitfalls

| Pitfall | Cause | Fix |
|---|---|---|
| `tabs not allowed` | tabs in indentation | use 4 spaces |
| `multiple-skills` | two `skill` decls in one file | factor into separate files |
| `empty-skill-body` | skill with no `flow:` and no `constraints:` | add at least one |
| `empty-flow` | `flow:` header present but body has zero statements | remove the header (constraint-only skill) or add a statement |
| `no-exports-in-library` | library file (zero `skill` decls) has zero `export` decls | add at least one `export block` or `export const` |
| `const-in-flow` | a string-valued `const` name appears bare in `flow:` without a marker | wrap with `context`/`require`/`avoid`/`must`, or convert to `block` |
| `missing-param-default` (export block) | an `export block` parameter has no default | add an explicit default |
| `missing-return` (export block) | `export block` body has no `return` | repairable — Phase 3 inserts `return none`; prefer to write it explicitly |
| `import-skill` | tried to selectively import a `skill` from another file | only `export …` declarations are importable; refactor into an `export block` |
| `applies-on-undescribed-block` (imported) | `BLOCKNAME.applies()` on an imported block lacking `description:` | add `description:` in the source library |
| `unknown-param-slot` | `{name}` references a parameter or binding not in scope | rename, declare, or remove the slot |
| `param-slot-in-non-instruction-string` | `{name}` inside `description:` or a parameter default | move the slot into instruction text |
| `circular-import` | files import each other in a cycle | extract shared content into a third file |
| `effects-under-declared` (when effects gated on) | declared `effects:` is missing keywords the call graph implies | add the missing keyword(s), or omit `effects:` to let inference fill it in |
| `no-shadowing` collision | same name from two sources in overlapping scope | rename one, or alias on import |

---

## 18. Worked Examples

### 18.1 Minimal skill (novice kernel)

```glyph
skill update_docs(scope = ".")
    description: "Update repository documentation to match current code."
    require accuracy
    avoid stale_references

    flow:
        "Scan {scope} for documentation files."
        "Compare each document against the current code."
        "Update outdated or incorrect sections."
        "Verify all cross-references and links are valid."

const accuracy = "Ensure all documentation accurately reflects the current code."
const stale_references = "Avoid leaving references to removed or renamed symbols."
```

### 18.2 With branching, blocks, and `.applies()`

```glyph
skill fix_bug(scope = ".")
    description: "Debug and fix a bug with minimal, targeted changes."
    require preserve_existing_patterns
    avoid unrelated_edits
    context:
        "The bug is assumed to be reproducible locally."

    flow:
        inspect_repo(scope) with "focus on the area where the bug was reported"

        if deep_investigation.applies():
            "Trace symptoms across multiple subsystems."
            "Gather extensive evidence from logs, tests, and code."
        else:
            identify_root_cause()

        if has_test_suite.applies():
            "Run the existing test suite to establish a baseline."
        else:
            "Manually verify the fix by inspecting the changed code paths."

        patch_minimally()
        validate_fix()
        return summarize_changes()

const preserve_existing_patterns = "Prefer the repo's existing patterns and helpers."
const unrelated_edits = "Making changes outside the requested scope or fixing unrelated issues."

block deep_investigation()
    description: "The bug spans multiple subsystems or layers."
    flow:
        "Map the full dependency chain of the affected code."
        "Identify every subsystem involved in the bug."
        "Create a minimal reproduction case."

block has_test_suite()
    description: "The project has an established test suite with meaningful coverage."
    flow:
        "Check whether a recognized test framework is configured."
        "Verify that test files exist and are not empty stubs."

block identify_root_cause()
    "Trace the reported symptoms to their origin and confirm with evidence."

block inspect_repo(scope)
    flow:
        "List directories and files under {scope}."
        "Identify the modules involved in the reported bug."

block patch_minimally()
    "Apply the smallest change that fixes the root cause."

block validate_fix()
    flow:
        "Verify the fix resolves the original issue."
        "Run related tests to check for regressions."

block summarize_changes()
    "Summarize what was changed and why."
```

### 18.3 Multi-file skill with library and preferences

```glyph
// prefs.glyph
export const preserve_existing_patterns = "Prefer the repository's existing patterns and helpers."
export const safety_first = "Never execute destructive operations without explicit confirmation."
```

```glyph
// repo_tools.glyph
export block inspect_repo(scope = ".") -> RepoContext
    description: "Inspect the repository structure and identify key files."
    flow:
        "List directories and files under {scope}."
        "Identify source modules and their relationships."
        return <"summary of the repo layout">

export block has_test_suite(scope = ".")
    description: "The project has an established test suite with meaningful coverage."
    flow:
        "Inspect {scope} for test configuration and existing tests."
        return none
```

```glyph
// fix_bug.glyph
import "./prefs.glyph" { preserve_existing_patterns, safety_first }
import "./repo_tools.glyph" { inspect_repo, has_test_suite }

skill fix_bug(scope = ".")
    description: "Debug and fix a bug with minimal, targeted changes."
    require preserve_existing_patterns
    must safety_first

    flow:
        ctx = inspect_repo(scope) with "focus on where the bug was reported"

        if has_test_suite.applies():
            "Run tests to establish a baseline before any change."

        "Identify the root cause from {ctx} before proposing a fix."
        "Apply the smallest possible patch."
        "Verify the fix resolves the issue and runs the test suite cleanly."
        return <"short summary of what was changed and why">
```

### 18.4 Subagent delegation

```glyph
import "@glyph/std" { subagent, send }

skill investigate(scope = ".")
    description: "Delegate investigation of a code area to a subagent."

    flow:
        researcher = subagent(scope) with "trace the failure end-to-end"
        researcher.send("Begin with the entrypoint and trace data flow downstream.")
        researcher.send("Surface every assumption you make.")
        return researcher
```

---

## 19. Quick Reference Card

```
File:        <name>.glyph           — skill file (one `skill`) or library file (no `skill`)
Indent:      4 spaces, significant; no tabs
Comments:    // line comments only
Strings:     "inline"   """block"""   no interpolation; only `{name}` slots in instruction strings

Top-level declarations:
  skill <name>(<params>) [-> Type]
  block <name>(<params>) [-> Type]
  export block <name>(<params>) [-> Type]   # default required on every param; explicit return required
  const <name> = "..." | <int> | <float> | bare-name | qualified-name
  export const <name> = "..." | <int> | <float> | bare-name | qualified-name
  import "<path>" as <alias>                 # whole-module
  import "<path>" { name, name as alias }    # selective
  import "@glyph/std" { subagent, send }     # stdlib

Sub-section headers (inside skill / block / export block body):
  description:   one-line string or const-name reference (singular)
  effects:       list / inline list (gated by --enable-effects)
  context:       bare names, inline strings, or `context "..."` markers
  constraints:   require / avoid / must / must avoid markers
  flow:          ordered statements (only one per body)

Constraint markers:
  require <name|"string">          # soft positive
  avoid   <name|"string">          # soft negative
  must    <name|"string">          # hard positive
  must avoid <name|"string">       # hard negative

Flow statement forms:
  x = call(args)                   # binding
  call(args)                       # statement call
  receiver.method(args)            # UFCS desugars to method(receiver, args)
  Alias.callee(args)               # qualified call
  call(args) with "modifier"       # site modifier
  bare_block_name                  # shorthand call; string constants need a marker
  "inline instruction"
  context <name|"string">          # context marker
  require / avoid / must … <name|"string">   # constraint marker
  if <cond>:                        elif <cond>:    else:
  return <expr>                    # exactly one, top-level, last

Conditions:
  is_valid | foo(ctx) | ctx.has_tests | not x | a == b | a != b |
  a and b | a or b | (a or b) and c | block_name.applies()

Stdlib type:  Agent
Stdlib calls: subagent(task) -> Agent ;   send(agent: Agent, message)

Values: "..."  """..."""  3  -1  0.8  true  false  none
Reserved keywords: skill, block, export, import, const, flow, call, if, elif, else,
  return, true, false, none, effects, constraints, inputs, outputs, when_to_use, description,
  as, generated, input, output, must, require, avoid, context, and, or, not
```

---

## 20. What's Out of Scope (for the author)

You will likely never write these and don't need to think about them:

- The 7-phase compiler pipeline internals.
- The IR JSON schema or `ir-and-semantics.md` projection rules.
- Diagnostic-ID catalogue mechanics (you'll see the IDs in compile errors; the message tells you what to do).
- The Expand pass's prose-shaping LLM.
- `generated const` / `generated block` *authorship* — you read them; the compiler writes them.
- Pipeline cacheability and multi-file compilation order.

You write structure and intent. The compiler does the rest.
