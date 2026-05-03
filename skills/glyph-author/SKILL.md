---
name: glyph-author
description: Use when authoring or editing a Glyph source file (any file ending in `.glyph.md`), porting an existing SKILL.md or agent skill into Glyph, or when the user asks for a skill "in Glyph". Glyph is a small DSL whose source compiles to agent-facing Markdown; do not invent ad-hoc syntax — match the closed primitive set, the permitted declaration forms, and the closed effect/constraint vocabulary documented here. Even if the user just says "write a skill that does X" while inside or referencing a Glyph project, prefer Glyph syntax over freeform Markdown.
---

# Authoring Glyph

Glyph source files end in `.glyph.md`. The whole file is Glyph source — there is no Markdown passthrough. Markdown structure (`#`, `##`, prose paragraphs) belongs in the **compiled output**, not in the source you write. If you find yourself reaching for a Markdown header, you are no longer writing Glyph.

This skill is the in-context reference. For deeper detail, the two files in `references/` are loaded only when needed:

- `references/cheatsheet.md` — one-page syntactic reference (every declaration, header, marker, and form). Read when you are uncertain about exact spelling.
- `references/examples.md` — five annotated, compilable examples covering the full surface. Read when starting from a blank file or when porting an existing skill.

## Mental model: five primitives

Every Glyph construct is one (or a composition) of five semantic primitives. Hold these in mind while planning a skill — they are how you decide *which* form to reach for.

| Primitive | Meaning | Where it shows up in source |
|---|---|---|
| **Instruction** | Imperative work to do | Calls inside `flow:`, inline strings inside `flow:`, conditional bodies, single-string shorthand block bodies |
| **Constraint** | Behavioral bound on the agent | `require` / `avoid` / `must` / `must avoid` markers (in `constraints:`, at body level, or as flow statements) |
| **Context** | Passive framing — *not* directive, *not* restrictive | `context:` sub-section, `context` markers, inline strings under `context:` |
| **Interface** | Contract with the outside world | Header (parameters, defaults, `-> ReturnType`), `effects:`, `description:`, `return` statement |
| **Binding** | A name pointing at a value, callable, module, or result | Declaration names, parameters, `x = call()` locals, import names |

The most common authoring mistake is collapsing categories: putting context inside `flow:` as a bare string (it becomes an instruction!), putting an instruction inside `context:`, or adding "must" wording inside a `flow:` step instead of using a constraint marker. Use the primitive that matches the *meaning*, not the one that looks visually convenient.

## File anatomy

A `.glyph.md` file is either:

- **A skill file** — exactly one `skill` declaration, plus any imports / value bindings / blocks it needs.
- **A library file** — zero `skill` declarations, at least one `export` declaration, plus private helpers.

Top-level declarations live at column 0. Body content is indented exactly **4 spaces** per level. Tabs and 2-space indents are compile errors.

```
Level 0 (col 0):   skill, block, export block, const, import
Level 1 (col 4):   sub-section headers (flow:, effects:, ...) and body-level markers
Level 2 (col 8):   flow statements, effect list items
Level 3 (col 12):  if/elif/else body statements
```

A skill body must contain at least one of `flow:` (with statements) or `constraints:` (with markers).

## Declarations (top-level)

| Form | Use it for |
|---|---|
| `skill name(params) -> DomainType` | The single public entrypoint. `-> DomainType` is optional; omit `->` if there is no meaningful return. **No trailing colon.** Parens always required, even when empty: `skill update_docs()`. |
| `block name(params) -> DomainType` | Private helper, callable only inside this file. `->` optional. |
| `export block name(params) -> DomainType` | Importable, self-contained reusable block. **Every parameter must have a default**, every body must `return` explicitly, and `effects:` must be declared. Declare `-> DomainType` if the block returns a meaningful value; otherwise omit `->`. |
| `const NAME = <literal>` / `export const NAME = <literal>` | Named constant. The compiler infers value kind (string, integer, float) from the literal: `"..."`, `3`, `0.8`. Used in `constraints:` or `context:`, or as a parameter default. *Not* legal as a bare instruction in `flow:`. |
| `import "<path>" { name, name as alias }` | Selective import. |
| `import "<path>" as alias` | Whole-module import (alias required). |

Rules that catch authors out:

- **No trailing colon on declarations.** `skill foo()` is correct; `skill foo():` is a parse error. Sub-section headers (one level in) *do* take a trailing colon.
- **`export block` parameters must have defaults.** A parameter without a default emits `G::analyze::missing-param-default`. (Skill parameters without defaults are fine — they signal "the LLM must extract this from the user request".)
- **`export block` requires explicit `return`** — even `return none` for instruction-only exports (which omit `->` on the header).
- **No primitive type names in annotations.** Do **not** write `-> String`, `-> Int`, `-> Float`, `-> Bool`, `-> None`. Only domain types (`Plan`, `RepoContext`, `BranchName`, `Confirmation`, ...) are valid in `-> Type` and `name: Type` positions. Domain types are implicitly declared the first time you use one in `-> Type` position — no `type Foo` declaration needed. For "no meaningful return", omit `->` entirely.
- **`const` is a value, not a step.** A bare `const` name in `flow:` is `G::analyze::const-in-flow`. Reference it inside `constraints:` (`avoid unrelated_edits`) or `context:`, or use a `block` for instructions.

## Sub-section headers

Inside a declaration body, five colon-terminated headers structure the content. Their order is **not** semantically meaningful — `glyph fmt` canonicalizes — but each may appear at most once.

```
description:    routing/trigger metadata (not runtime context)
effects:        capability declaration (closed vocabulary, see below)
context:        passive framing the agent should hold while working
constraints:    behavioral bounds (must / avoid / require)
flow:           ordered work to perform
```

Two body shapes are allowed for any of these:

```glyph
// Long form
effects:
    - reads_files
    - writes_files

// Short form
effects: reads_files, writes_files
```

Use the short form when there is a single short value; long form when readability benefits from one-per-line.

`description:` is *not* execution context. It is consulted by skill selectors and `BLOCKNAME.applies()` predicates. Runtime framing belongs in `context:`.

## Effects (closed vocabulary — exactly 9 keywords)

```
none  reads_files  reads_env  writes_files  runs_commands
uses_network  asks_user  creates_artifacts  spawns_agent
```

Pick the smallest accurate set. If you import a callee with effect `X`, you must declare `X` too — the compiler will refuse otherwise. Inventing new effect names is a hard error; pick from this list or omit `effects:` and let the inferencer fill in.

## Constraint markers (3 keywords → 4 forms)

```
require <name-or-string>       // soft, positive obligation
avoid   <name-or-string>       // soft, prohibition
must    <name-or-string>       // hard, positive obligation
must avoid <name-or-string>    // hard, prohibition
```

Where they go:

1. Inside `constraints:` — the canonical home.
2. At declaration body level — `glyph fmt` will hoist into `constraints:`.
3. Inside `flow:` (top-level or inside a branch). Top-level ones are hoisted; branch-scoped ones stay inline and govern only that branch.

The right side may be a bare name (resolved to a `const` declaration or import) or an inline string. Long, repeated wording deserves promotion to a `const`; one-off, situational wording can stay inline.

## Context

`context:` carries passive framing — *what is true*, not *what to do*. A bare string under `context:` is context. The exact same string under `flow:` would be an **instruction**. Position determines role.

```glyph
context:
    "The repository may contain multiple packages."
    "Test commands are project-specific; do not assume a stack."
```

A `context <name-or-string>` marker also works at body level or inside flow (top-level hoisted, branch-scoped inline).

## `flow:` — what you can write

Nine statement forms. Anything else is a parse error.

| Form | Example |
|---|---|
| Binding | `ctx = inspect_repo(scope)` |
| Bare call | `apply_changes(plan)` |
| UFCS call | `agent.send("look at edge cases")` |
| Bare name | `validate_before_success` (resolves to a block or generated block) |
| Inline string | `"Mention any unverifiable docs."` |
| Constraint marker | `must avoid skipping_tests` |
| Context marker | `context "The build is currently red."` |
| Return | `return summarize(plan)` |
| If/elif/else | `if risk == "high":` |

`return` rules: **exactly one, at the very end of `flow:`**, never inside an `if`/`elif`/`else` body. If you find yourself wanting an early return, factor the conditional into a helper `block` and let the helper return.

Bindings introduced inside a branch are scoped to that branch only. Bindings at flow top-level are visible to everything below them.

### Calls

```glyph
make_plan(ctx, risk = "high")              // positional then named
plan = make_plan(ctx, "high")              // positional, result bound
plan = make_plan(ctx) with "be conservative"  // per-call site modifier
```

The `with "..."` modifier shapes the prose at this one call site. It does not change effects, constraints, or types.

### UFCS (Uniform Function Call Syntax)

`x.foo(args)` desugars to `foo(x, args)`. Use it when the receiver reads naturally as the subject:

```glyph
researcher = subagent("audit auth flow")
researcher.send("Now look at session expiry.")
```

UFCS preserves call modifiers: `agent.send(msg) with "be terse"` works. Prefer UFCS over the positional form `send(researcher, msg)` when the receiver is a meaningful binding.

### Branching

```glyph
if risk == "high":
    run_full_suite(ctx)
    must request_review
elif ctx.has_tests:
    run_smoke_tests(ctx)
else:
    "Note that no tests are available."
```

Permitted condition forms: bare boolean, `not`, `==`, `!=`, `and`, `or`, parenthesized grouping, single-level dot access, calls. No `<` / `>` / arithmetic — bind a call result first if you need them.

## Three name forms in source

Glyph distinguishes three visually distinct ways a name can appear, and each compiles differently. Picking the right form is what makes the compiled output coherent — wrong forms either silently ship typos into the prompt or mis-categorize the value.

| Form | Meaning | Where to use it |
|---|---|---|
| `name` | Ordinary identifier — refers to an existing parameter, local binding, declaration, or import | In expressions, call arguments, condition expressions, RHS of `=` |
| `{name}` | **Slot inside instruction text** — the compiler resolves it: parameters become runtime slots in the compiled output; bindings and `const` values are inlined as natural-prose cross-references | Inside `"..."` strings: instructions, constraint texts, context strings, `with` modifiers |
| `<name>` | **Output target (identifier form)** — a value the agent must synthesize. The type comes from the enclosing `-> DomainType` annotation | Currently only in `return <name>` |
| `<"description">` | **Output target (descriptive form)** — a quoted descriptive string that tells the agent what to synthesize. Complements `-> DomainType` (which is the compiler contract) with agent guidance | Currently only in `return <"...">` |

### `{name}` slots vs. backticks vs. angle brackets

Inside an instruction string, use `{name}` to reference any in-scope parameter, binding, or `const`. The compiler validates the reference; a typo emits `G::analyze::unknown-param-slot`. Backticks `` ` `` are for **literal code** — shell commands, file paths, language keywords. Angle brackets are not Glyph syntax inside strings; using `<repo_path>` to mean "the repo_path parameter" silently ships a non-reference into the compiled prompt.

```glyph
// Wrong — these are invisible to the compiler
"Run git -C <repo_path> rev-parse"
"Verify that `repo_path` exists and is a git working tree"

// Right — `{repo_path}` is a resolved slot; backticks frame literal code
"Run `git -C {repo_path} rev-parse`"
"Verify that {repo_path} exists and is a git working tree"
```

The same rule applies inside shell command strings: write `git -C {repo_path}`, not `git -C <repo_path>`.

### `<name>` and `<"description">` output targets in `return`

Some block returns describe values the agent must *synthesize* from prose-guided work — extracting a value from command output, judging a confirmation, summarizing findings. The old anti-pattern was `return "<current_branch>"`, but that is a string literal, not the named output. Use one of two output-target forms instead:

```glyph
// Identifier form: <name> — when a short name captures the value
block read_current_branch(repo_path) -> BranchName
    flow:
        "Run `git rev-parse --abbrev-ref HEAD` in {repo_path} and capture the branch name."
        return <current_branch>

block ask_user_to_confirm(candidates) -> Confirmation
    flow:
        "Show {candidates} and ask for confirmation. Treat only an unambiguous affirmative as true."
        return <confirmed>

// Descriptive form: <"..."> — when a description guides the agent better than a name
export block diagnose_issue(scope) -> Diagnosis
    flow:
        inspect_repo(scope)
        return <"root cause analysis including affected files and severity">
```

Note `-> BranchName`, `-> Confirmation`, `-> Diagnosis` — these are **domain types**. Do not write `-> Text`, `-> Bool`, or `-> String`; primitive type names are not part of the author-facing surface. Domain types are implicitly declared by their first appearance in `-> Type` position.

The `-> DomainType` annotation is the **compiler contract** (used for nominal matching at call boundaries). The `<"description">` is **agent guidance** (what to synthesize). They are complementary; both may be present on the same block.

When the return value comes from a real call (`return summarize(plan)`) or an existing binding (`return result`), do **not** use either output-target form — they are only for prose-produced values. Identifier-form syntax is strict: `<IDENTIFIER>` only, no spaces, no expressions, no calls, no dots. Both forms are initially permitted only in the terminal `return`.

## Standard library

Imported, never auto-available:

```glyph
import "@glyph/std" { subagent, send }
```

| Entry | Signature | Purpose |
|---|---|---|
| `subagent(task) -> Agent` | Spawns a subagent for the given task; returns a handle. Effect: `spawns_agent`. |
| `send(agent: Agent, message)` | Sends a follow-up to a spawned agent. Effect: `spawns_agent`. UFCS: `agent.send(msg)`. |

`Agent` is a compiler-known type — the only way to obtain one is `subagent(...)`. If you call either, declare `effects: spawns_agent` on the enclosing `skill`/`block`.

(There is also a `load` primitive, but it is compiler-internal — never write `load(...)` yourself.)

## Authoring style

These are the heuristics that make a Glyph file readable, visualizable, and reviewable like code rather than prose:

1. **Plan in primitives.** Before writing, sketch which steps are *instructions*, which rules are *constraints*, which framing is *context*, and what the *interface* (parameters, return) looks like. The five-primitive table is your planning rubric.
2. **Make data flow visible.** Prefer named locals (`ctx = ...`, `plan = ...`) over deeply nested calls. Hidden ambient context kills the visualization story. When you need to *name* a value that comes from prose-guided agent work (extracting a branch name, judging a confirmation, summarizing findings), introduce a thin `block` whose flow describes the operation and ends with `return <name>` — that gives the value a real binding downstream code can reference via `{name}`. Thin blocks are the idiom, not a smell.
3. **Promote repetition to a `const`.** If the same constraint or context wording appears more than once, lift it to a `const` declaration and reference the bare name. The compiled output is the same; the source becomes maintainable.
4. **Keep `export block`s closed.** Everything they need flows through parameters, imports, or stdlib. They should read as standalone procedures.
5. **Prefer `must avoid <name>` for hard prohibitions.** It compiles to stronger language than `avoid` and signals to reviewers that the rule is non-negotiable.
6. **Use UFCS for receiver-shaped reads.** `agent.send(...)` reads as "tell the agent ...". `send(agent, ...)` reads as "perform a send action on these arguments". Pick the one that matches what you mean.
7. **One `with` per call site, sparingly.** Reach for `with "..."` only when this *one* invocation needs framing the callee's body cannot carry. Frequent `with` clauses are a smell that the callee should be parameterized.
8. **Trust the compiler.** If you omit `effects:` the inferencer adds the right set; if you write a bare name in `flow:` the repairer can materialize a `generated block`. You do not need to over-specify.

## Common mistakes

- Writing a Markdown `# Title` or `## Section` header in source — there are none. Use a Glyph declaration instead.
- Adding `:` after a top-level declaration (`skill foo():`).
- Referencing a `const` as a bare instruction in `flow:`.
- Forgetting `return none` on an instruction-only `export block` (also remember to omit `->` on the header).
- Writing primitive type names like `-> String`, `-> Int`, `-> Bool`, `-> None` — only domain types are valid; for "no return", omit `->` entirely.
- Writing `text NAME = ...`, `int NAME = ...`, or `float NAME = ...` — these keywords are gone; use `const NAME = ...` and let the compiler infer the kind from the literal.
- Forgetting `effects: spawns_agent` when calling `subagent` or `send`.
- Putting a `return` inside an `if`/`elif`/`else` body.
- Using 2-space or tab indentation.
- Treating `description:` as a place to dump runtime framing — it is for selectors only.
- Using backticks or angle brackets to reference Glyph names inside instruction strings (`` `repo_path` ``, `<repo_path>`) — use `{repo_path}` so the compiler can resolve and validate it.
- Writing `return "<output_name>"` (a string literal that *looks like* an output target) — use `return <output_name>` or `return <"description">`, the real output-target forms.

## When the syntax does not cover what you need

Glyph is intentionally small. If you reach for a feature that doesn't exist (`for_each`, runtime concurrency, exception handling, `<`/`>` in conditions, `match`, agent equality, runtime preference overrides, chained dot access), the design is to fall back to *prose inside an inline string or `const` declaration* rather than invent syntax. The compiled output is for an LLM to read; prose carries semantics the compiler does not need to track. Examples:

```glyph
flow:
    "For each file in scope, run the linter and collect errors."   // stand-in for for_each
    "Wait for all spawned investigators to report before merging."  // stand-in for await
```

This keeps the source compilable while still expressing intent. Add a comment (`//`) noting the limitation if a reader might be surprised.
