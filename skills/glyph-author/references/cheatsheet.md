# Glyph Cheatsheet

One-page syntactic reference. Pair with `examples.md` for full runnable forms.

## File rules

- Source files end in `.glyph.md`. The whole file is Glyph source.
- A skill file: exactly one `skill` declaration.
- A library file: zero `skill`s, ≥1 `export` declaration.
- Indentation: 4 spaces, no tabs, no mixed indentation.
- Comments: `//` (line only). No block comments.
- A skill body must contain at least one of `flow:` or `constraints:`.

## Top-level declarations (column 0, no trailing colon)

```
skill <name>(<params>) -> <DomainType>
block <name>(<params>) -> <DomainType>
export block <name>(<params>) -> <DomainType>
const <NAME> = <literal-rhs>
export const <NAME> = <literal-rhs>
import "<path>" { <name>, <name> as <alias>, ... }
import "<path>" as <alias>
```

Parens always required on callable declarations: `skill update_docs()`, not `skill update_docs`.

`-> <DomainType>` is **optional** on `skill`/`block`/`export block`. Omit `->` entirely when there is no meaningful return value (no `-> None` — that form is gone). `<DomainType>` must be a **domain type** (`Plan`, `BranchName`, `Confirmation`, ...). Primitive type names (`String`, `Int`, `Float`, `Bool`, `None`) are not part of the author surface; the compiler infers primitive kinds internally. Domain types are implicitly declared by first use in `-> Type` position.

`const` is one keyword for all named constants; the compiler infers value kind (string, integer, float) from the literal: `"..."`, `3`, `0.8`. The old `text`/`int`/`float` keywords are gone.

`generated const` and `generated block` exist but only the LLM repair pass writes them — never write them by hand.

## Sub-section headers (column 4, trailing colon required)

```
description:    routing/trigger metadata
effects:        capability declaration
context:        passive framing
constraints:    behavioral bounds
flow:           ordered work
```

Each appears at most once per body. Order is permissive — `glyph fmt` canonicalizes.

Long form (one item per line):

```glyph
effects:
    - reads_files
    - writes_files
```

Short form (single line):

```glyph
effects: reads_files, writes_files
```

## Name forms (4)

| Form | Meaning |
|---|---|
| `name` | Identifier — refers to existing parameter, binding, declaration, or import |
| `{name}` | Slot **inside instruction text**: parameters preserved as runtime slots, bindings/`const` values inlined as prose. Compiler validates the reference. |
| `<name>` | Output target (identifier form) — a value the agent must synthesize. Type from enclosing `-> DomainType`. Currently only in terminal `return`. |
| `<"description">` | Output target (descriptive form) — quoted descriptive string telling the agent what to synthesize. Complements `-> DomainType`. Currently only in terminal `return`. |

Use `{name}` (not backticks, not angle brackets) to reference Glyph names *inside* instruction strings — even inside shell command strings (`git -C {repo_path}`, not `git -C <repo_path>`). Backticks are for literal code only.

`<name>` syntax is strict: `<IDENTIFIER>` only — no spaces, no expressions, no calls, no dots. `<"description">` uses a normal quoted string. Use either instead of the `return "<placeholder>"` anti-pattern when a block returns a prose-synthesized value.

## Effects (closed vocabulary, exactly 9)

```
none  reads_files  reads_env  writes_files  runs_commands
uses_network  asks_user  creates_artifacts  spawns_agent
```

If callee has effect `X`, caller must declare `X`.

## Constraint markers

3 keywords composing into 4 forms:

```
require <name-or-string>       // soft, positive
avoid   <name-or-string>       // soft, prohibition
must    <name-or-string>       // hard, positive
must avoid <name-or-string>    // hard, prohibition
```

Legal positions:

1. Inside `constraints:` (canonical).
2. At declaration body level (hoisted by `glyph fmt`).
3. As a flow statement (top-level hoisted; branch-scoped stays inline).

## Context markers

```
context <name-or-string>
```

Same three positions as constraint markers. Top-level hoisted into `context:`; branch-scoped stays inline.

Inside a `context:` sub-section, plain inline strings are context (no `context` keyword needed).

## Statement forms inside `flow:` (exactly 9)

| Form | Example |
|---|---|
| Binding | `ctx = inspect_repo(scope)` |
| Bare call | `apply_changes(plan)` |
| UFCS call | `agent.send(msg)` |
| Bare name | `validate_before_success` |
| Inline string | `"Mention any unverifiable docs."` |
| Constraint marker | `must avoid skipping_tests` |
| Context marker | `context "The build is currently red."` |
| Return | `return summarize(plan)` |
| If/elif/else | `if risk == "high":` |

## Condition expressions (inside `if` / `elif`)

| Form | Example |
|---|---|
| Boolean ident/binding | `if is_valid:` |
| Boolean call | `if has_tests(ctx):` |
| Single-level dot | `if ctx.has_tests:` |
| `not` | `if not is_valid:` |
| `==`, `!=` | `if risk == "high":` |
| `and`, `or` | `if risk == "high" and ctx.has_tests:` |
| Parens | `if (a or b) and c:` |
| Block trigger | `if fork_with_plan.applies():` |

No `<`, `>`, arithmetic, `in`. Bind a call result first if you need them.

## Calls

```glyph
foo()                                   // zero-arg call (parens REQUIRED to distinguish from bare name)
foo(a, b)                               // positional
foo(a, name = b)                        // positional then named
result = foo(a) with "be thorough"      // call modifier (one per call site)
result = mod.foo(a)                     // qualified callee (mod is a whole-module import alias)
result = a.foo(b)                       // UFCS: desugars to foo(a, b)
```

`foo` (no parens) is a *bare name*, not a call. In `flow:` it resolves to a callable (block, generated block, or stdlib entry); not a `const`.

## Return

- Exactly one `return` per `skill`/`block`/`export block`.
- Must be the **last** statement at the top level of `flow:`.
- **Never** inside an `if`/`elif`/`else` body.
- `return` alone equals `return none`.
- `export block` requires explicit `return` (even `return none`).
- `skill` and private `block` may omit it (implicit `return none`).

Return-value forms:

```glyph
return none                              // no meaningful value
return existing_binding                  // return an existing in-scope value
return some_call(args)                   // return a callee result
return <output_name>                     // identifier output target (prose-synthesized, named)
return <"description of the output">     // descriptive output target (prose-synthesized, described)
```

Use `<output_name>` or `<"description">` only when the producer is a prose instruction, not a callable expression. Don't write `return "<output_name>"` — that's a string literal, not an output target. The enclosing block's `-> DomainType` is the compiler contract; the `<"description">` (when used) is the agent guidance.

## Parameter syntax (4 forms)

```
name                              // untyped, no default (skill: runtime-required; block / export block: required at every call site)
name = "default"                  // untyped, with default
name: DomainType                  // typed, no default
name: DomainType = default_value  // typed, with default
```

- **Skill** params without a default = runtime-required (LLM extracts from user prompt).
- **`block` / `export block`** params without a default are required at every call site — `call name(...)` that omits the positional argument fires `G::analyze::missing-required-arg` at the call (uniform across private, same-file export, and cross-file imported export blocks).
- **Type slot is domain-only.** Use `: PathSpec`, `: RiskLevel`, `: Plan`, etc. — not `: String`/`: Int`/`: Bool`. Primitive type names are not part of the author surface.

Default values: literal (string/int/float/bool/`none`) or a name reference to a `const` (compile-time constant).

## Standard library — `@glyph/std`

```glyph
import "@glyph/std" { subagent, send }
```

| Name | Signature | Effect | Notes |
|---|---|---|---|
| `subagent` | `(task) -> Agent` | `spawns_agent` | Spawns a delegated subagent. |
| `send` | `(agent: Agent, message)` | `spawns_agent` | Send a follow-up. UFCS: `agent.send(msg)`. |

`Agent` is the only way to address a spawned subagent. Obtained only from `subagent(...)`.

## Single-string shorthand

A `block` or `export block` whose body is exactly one instruction string and has no other sub-sections may omit `flow:`:

```glyph
block summarize_changes()
    "Summarize what was changed and why."
```

Compiles identically to the `flow: \n "Summarize ..."` form. The bare string is always treated as an instruction (`Step`), never context.

`export block` shorthand only works for blocks with no meaningful return (omit `->` on the header); the compiler implicitly inserts `return none`.

## Error IDs you might see (and what they mean)

| ID | Cause | Fix |
|---|---|---|
| `G::parse::empty-file` | File is whitespace/comments only | Add a declaration. |
| `G::parse::multiple-skills` | Two `skill` declarations in one file | Move one to its own file. |
| `G::parse::empty-flow` | `flow:` header with no statements | Add a statement or remove the header. |
| `G::parse::return-not-terminal` | `return` not at the end of flow | Move it to the last statement. |
| `G::parse::return-in-branch` | `return` inside `if`/`elif`/`else` | Factor into a helper block. |
| `G::analyze::const-in-flow` | Bare `const` name used in `flow:` | Use a `block` for instructions. |
| `G::analyze::missing-return` | `export block` has no `return` | Add `return none` (or a real return). |
| `G::analyze::missing-required-arg` | `call name(...)` omits a positional argument for a callee parameter without a default | Pass the argument at the call, or add a default to the callee parameter. |
| `G::analyze::no-exports-in-library` | Library file with zero exports | Add `export` to at least one declaration, or move the skill into the file. |
| `G::analyze::effects-under-declared` | Caller's `effects:` missing a callee's effect | Add the missing effect or remove the declaration entirely (let inference fill it). |
