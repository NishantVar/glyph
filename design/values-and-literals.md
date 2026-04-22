# Glyph Values And Literals

This document records the MVP decisions for Glyph's primitive value surface: strings, numbers, booleans, `none`, identifiers, and name resolution rules.

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
text preserve_existing_patterns = """
    Prefer the repository's existing patterns, helper APIs, naming,
    and file organization before introducing a new abstraction or style.
"""
```

Common leading indentation is stripped, similar to Python's `textwrap.dedent`. Authors may indent block strings naturally inside declarations without introducing unwanted whitespace in the compiled output.

### No Interpolation

Strings are opaque instruction text. There is no interpolation syntax (`${...}`, `{...}`, or equivalent). Values flow through parameters and call arguments, not string splicing.

This follows boundary 1 (Glyph is not a prompt template system), boundary 6 (text reuse is not prompt templating), and the maintenance rule against ad hoc string concatenation.

## Numbers

### Integers

Standard decimal integers are supported. Leading zeros are not allowed.

```glyph
max_attempts = 3
offset = -1
```

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

Identifiers match `[a-zA-Z_][a-zA-Z0-9_]*`. They must start with a letter or underscore and may contain letters, digits, and underscores. Hyphens are not allowed in identifiers. Dots are reserved for module-qualified access (`repo_tools.inspect_repo`).

### Case Normalization

The compiler normalizes all identifiers to a canonical form for resolution. `makePlan`, `make_plan`, `MakePlan`, and `MAKE_PLAN` all resolve to the same name. The source preserves what the author wrote; the IR stores the canonical form.

If two declarations in different files or scopes use different casings for the same normalized name and both are visible, the compiler emits a collision diagnostic so the author can settle on one spelling.

### Convention

`snake_case` is recommended by convention but not enforced by the compiler.

### Reserved Words

The following are reserved keywords and cannot be used as identifiers:

`skill`, `block`, `export`, `import`, `text`, `flow`, `call`, `if`, `elif`, `else`, `return`, `true`, `false`, `none`, `effects`, `as`, `generated`, `input`, `output`, `always`, `require`, `avoid`, `prefer`, `context`, `and`, `or`, `not`.

This list grows with the language. New keywords should be added conservatively.

## Name Resolution

### What A Bare Identifier Can Resolve To

A bare identifier such as `make_plan` may resolve to:

- a `text` declaration (named instruction content);
- a parameter of the enclosing skill or block;
- a local binding;
- an imported name;
- a standard-library entry;
- a repair-generated definition.

A parenthesized form such as `make_plan()` or `make_plan(ctx)` is always a block call.

| Form | Resolves to |
|---|---|
| `make_plan` (bare) | text, parameter, local binding, import, or generated definition |
| `make_plan()` (with parens) | block call (zero arguments) |
| `make_plan(ctx)` (with arguments) | block call |

### No Shadowing

Ambiguous name resolution is a compile error, not a warning or silent fallback.

If the same normalized name is visible from multiple sources in overlapping scopes, the compiler rejects the program and requires the author to rename one of the conflicting declarations.

Examples of conflicts that produce hard errors:

- A parameter shares a name with a `text` declaration in the same file.
- A local binding shares a name with a parameter in the same block.
- An import collides with a same-file declaration.

The author's fix is always to rename one of the conflicting names. This is cheap, obvious, and permanent.

### Generated Definitions Must Be Visible

When the repair pass generates definitions for undefined bare names, the compiler must emit a warning-level diagnostic for each generated definition. This ensures the author notices when a name they thought was already defined was actually auto-generated, preventing silent misresolution.

## Enums And Symbols

The MVP does not include a dedicated enum or symbol type. Enumerated values are represented as strings.

```glyph
risk = "medium"
```

A dedicated enum type with validation and exhaustiveness checking may be added post-MVP if real authoring needs emerge.

## Open Syntax Choices

The semantic commitments above are stronger than the exact syntax. These details can still change:

- The exact normalization algorithm for identifiers (simple lowercasing, or case-insensitive plus underscore-normalized).
- Whether the reserved word list is maintained in a compiler configuration file or hardcoded.
- The precise format of collision and shadowing diagnostics.
