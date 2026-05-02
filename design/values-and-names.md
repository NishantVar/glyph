# Glyph Values And Names

This document records the MVP decisions for Glyph's primitive value surface: strings, numbers, booleans, `none`, identifiers, reserved keywords, and name resolution rules.

## Strings

### Inline Strings

Inline strings use double quotes only. Single quotes are not supported.

```glyph
"Do not change public behavior while updating documentation."
```

MVP escape sequences are limited to `\"` for a literal double quote and `\\` for a literal backslash. Extended escapes (`\n`, `\t`, Unicode) are deferred.

### Block Strings

Block strings use triple double quotes for multiline instruction text.

```glyph
const preserve_existing_patterns = """
    Prefer the repository's existing patterns, helper APIs, naming,
    and file organization before introducing a new abstraction or style.
"""
```

Common leading indentation is stripped, similar to Python's `textwrap.dedent`. Authors may indent block strings naturally inside declarations without introducing unwanted whitespace in the compiled output.

### No Interpolation

Strings are opaque instruction text. There is no source-time interpolation syntax (no `${...}`, no expression splicing, no concatenation). Values flow through parameters and call arguments, not string splicing.

This follows foundations: not a prompt template system and foundations: text reuse is not prompt templating, plus the maintenance rule against ad hoc string concatenation.

**Name slot exception (`{name}`).** A `{name}` slot — a strict identifier (`[a-zA-Z_][a-zA-Z0-9_]*`) inside single curly braces — is **not** source-time interpolation. It is a *name reference* that can resolve to a declared parameter or a local binding in scope. The two kinds compile differently:

- **Parameter references** (`{name}` resolves to a declared parameter) are preserved verbatim as runtime slots in the compiled Markdown for the consuming LLM to fill from user context at runtime (`compiled-output.md` §Parameter References In Steps). The compiler never substitutes the slot's value.
- **Local binding references** (`{name}` resolves to a local binding from an assignment like `diagnosis = analyze_error(...)`) are resolved by the Expand pass (Step 2) into natural-language cross-references in the compiled prose. They do **not** survive as literal `{name}` slots — the consuming LLM already produced the referenced value in a prior step and does not need a placeholder for its own output. For example, `"Apply the fix based on {diagnosis}"` might compile to "Apply the fix based on the diagnosis from your earlier analysis."

Slots are legal **only inside instruction-bearing string positions**: `const` / `generated const` bodies, `generated block` body strings, inline instruction strings inside `flow:`, constraint texts, and string arguments to stdlib calls whose body becomes a compiled instruction (`subagent`, `send`). A `{name}` token in any other string position (a parameter default value, a `description:` field, etc.) emits `G::parse::param-slot-in-non-instruction-string` (repairable: the braces are stripped and the content is treated as literal).

The slot grammar is strict: `{IDENTIFIER}` only. Anything else with braces — `{ "key": "value" }`, `{x, y}`, `if x { ... }`, or any other non-identifier content — is literal text and is parsed as such. There is no escape mechanism; an author who wants the literal text `{name}` where `name` happens to also be an in-scope parameter or binding must rephrase the instruction.

A slot whose `name` does not resolve to a parameter or a local binding in scope at the slot's source position is a hard error (`G::analyze::unknown-param-slot`, not repairable). The resolution scope is the enclosing declaration's parameters plus the local bindings visible at that point; neither imports nor `const` declarations participate in slot resolution (those are reused via bare-name reference, a separate mechanism).

### No Value-Level Operators

MVP expressions contain only four forms: bindings, literals, calls, and dot access (`data-flow.md` §IR Mapping). There are no value-level operators — no `+`, `-`, `*`, `/`, comparisons, or any other infix/prefix operator in expression position.

String concatenation via `+` is explicitly forbidden. Authors who need to combine context with a call should use the `with` modifier (`data-flow.md`) to pass specialization context at the call site. The Expand LLM weaves parameter context into prose instructions — manual string assembly is redundant with the pipeline's job.

If the parser encounters an operator token in expression position, it emits a `G::parse::operator-in-expression` diagnostic (repairable). The Repair pass can mechanically rewrite patterns like `f("prefix " + x)` into `f(x) with "prefix"`.

General-purpose operators (arithmetic, comparison, string manipulation) are deferred post-MVP.

## Numbers

### Integers

Standard decimal integers are supported. Leading zeros are not allowed.

```glyph
max_attempts = 3
offset = 2
```

Signed numeric literals (`-1`, `-0.5`) are deferred beyond MVP. The tokenizer rejects a leading `-` on a numeric literal today; a unary `-` prefix at parse time is planned in a future issue. See `language-surface.md` §3.4 for the matching deferral note on `const` RHS.

### Floats

Floats are supported. Digits are required on both sides of the decimal point.

```glyph
threshold = 0.8
ratio = 3.14
```

`0.5` is valid. `.5` and `3.` are not.

Scientific notation (`1e10`) is deferred beyond MVP.

### Numeric Coercion

The source parser accepts all numeric literals without distinguishing int from float at parse time. Type enforcement is the IR's responsibility.

At call boundaries, the compiler performs lossless coercion automatically. `3.0` passed where an integer is expected coerces to `3`. `3` passed where a float is expected coerces to `3.0`. Lossy conversions such as `3.7` where an integer is expected produce a compile error.

## Booleans

Booleans are the keywords `true` and `false`.

```glyph
strict = true
verbose = false
```

Source is case-insensitive: `true`, `True`, and `TRUE` are all accepted. The IR normalizes to lowercase `true` and `false`.

`true` and `false` are reserved keywords and cannot be used as identifiers.

## None

`none` is a reserved keyword and value representing the absence of a value.

```glyph
effects: none
return none
result = none
```

Source is case-insensitive: `none`, `None`, and `NONE` are all accepted. The IR normalizes to lowercase `none`.

`none` is usable anywhere a value is expected.

## Identifiers

### Allowed Characters

Identifiers match `[a-zA-Z_][a-zA-Z0-9_]*`. They must start with a letter or underscore and may contain letters, digits, and underscores. Hyphens are not allowed in identifiers.

### Dot Access

Dots are reserved for module-qualified access (`repo_tools.inspect_repo`). Control-flow adds single-level property dot access for bound values (e.g. `ctx.has_tests`); see `data-flow.md` for full rules and disambiguation.

### Case Normalization

The compiler normalizes all identifiers to a canonical form for resolution. `makePlan`, `make_plan`, `MakePlan`, and `MAKE_PLAN` all resolve to the same name. The source preserves what the author wrote; the IR stores the canonical form.

If two declarations in different files or scopes use different casings for the same normalized name and both are visible, the compiler emits a collision diagnostic so the author can settle on one spelling.

### Convention

`snake_case` is recommended by convention but not enforced by the compiler.

### Reserved Words

The following are reserved keywords and cannot be used as identifiers:

`skill`, `block`, `export`, `import`, `const`, `flow`, `call`, `if`, `elif`, `else`, `return`, `true`, `false`, `none`, `effects`, `constraints`, `inputs`, `outputs`, `when_to_use`, `description`, `as`, `generated`, `input`, `output`, `must`, `require`, `avoid`, `context`, `and`, `or`, `not`.

This list grows with the language. New keywords should be added conservatively.

## Name Resolution

### What A Bare Identifier Can Resolve To

A bare identifier such as `make_plan` may resolve to:

- a value-binding declaration (`const`) in the current file;
- a parameter of the enclosing skill or block;
- a local binding;
- an imported name;
- a standard-library entry;
- a repair-generated definition (MVP: `generated const` only).

A parenthesized form such as `make_plan()` or `make_plan(ctx)` is always a block call.

| Form | Resolves to |
|---|---|
| `make_plan` (bare) | const, parameter, local binding, import, or generated definition |
| `make_plan()` (with parens) | block call (zero arguments) |
| `make_plan(ctx)` (with arguments) | block call |

### No Shadowing

Ambiguous name resolution is a compile error, not a warning or silent fallback.

If the same normalized name is visible from multiple sources in overlapping scopes, the compiler rejects the program and requires the author to rename one of the conflicting declarations.

Examples of conflicts that produce hard errors:

- A parameter shares a name with a `const` declaration in the same file.
- A local binding shares a name with a parameter in the same block.
- An import collides with a same-file declaration.

The author's fix is always to rename one of the conflicting names. This is cheap, obvious, and permanent.

### Generated Definitions Must Be Visible

When the repair pass generates definitions for undefined bare names, the compiler must emit a warning-level diagnostic for each generated definition. This ensures the author notices when a name they thought was already defined was actually auto-generated, preventing silent misresolution.

### UFCS Name Resolution

UFCS (Uniform Function Call Syntax) is **pure syntactic sugar in a single namespace**. `x.foo(args)` desugars to `foo(x, args)` during Lower (Phase 4). There is no method namespace, no trait dispatch, and no overload resolution.

The name `foo` in `x.foo(args)` resolves through the **same name resolution rules** as a free call `foo(x, args)` — the resolution table above applies identically. If `foo` resolves to an imported `export block`, a same-file `block`, or a stdlib entry, the desugared call proceeds normally. If `foo` does not resolve, it is an undefined-call error, same as any other unresolved parens-call.

After desugaring, the receiver's type is checked against the callee's first parameter type via nominal matching (`types.md`). If the types are annotated and the names differ, the compiler emits `G::analyze::nominal-mismatch` — the same error as if the author had written `foo(x, args)` directly with a mismatched first argument.

The no-shadowing rule applies unchanged: if multiple declarations named `foo` are visible after case normalization, the compiler rejects the program regardless of whether the call was written as `x.foo()` or `foo(x)`.

UFCS desugaring happens in Lower (Phase 4), not Parse. The AST preserves the `.foo()` syntax so diagnostic spans point to what the author actually wrote. Analyze disambiguates dot syntax from qualified-call syntax (`M.foo()`) based on what the left-hand side resolves to — see `data-flow.md` §UFCS for the full disambiguation rule and worked examples.

## Enums And Symbols

The MVP does not include a dedicated enum or symbol type. Enumerated values are represented as strings.

```glyph
risk = "medium"
```

A dedicated enum type with validation and exhaustiveness checking may be added post-MVP if real authoring needs emerge.

## Open Syntax Choices

These details can still change without affecting the semantic commitments above:

- The exact normalization algorithm for identifiers (simple lowercasing, or case-insensitive plus underscore-normalized).
- Whether the reserved word list is maintained in a compiler configuration file or hardcoded.
- The precise format of collision and shadowing diagnostics.
