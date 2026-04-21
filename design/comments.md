# Glyph Comments

This document defines comment syntax for Glyph source files. It covers the MVP comment form and its interactions with Markdown rendering, compiled output, the LLM repair pass, and block structure.

## MVP Decision

Glyph uses `//` (double slash) for line comments. Block comments and doc-comments are deferred beyond the MVP.

```glyph
// This is a line comment.

skill fix_bug(scope)
    avoid unrelated_edits   // trailing comment on a code line
    require preserve_existing_patterns

    // Workflow starts here.
    flow:
        inspect_failure(scope)
        identify_root_cause()
        patch_minimally()
        validate_before_success
```

## Syntax

### Line Comments

A `//` token starts a comment that runs to the end of the line. Everything from `//` to the line terminator is comment text and is ignored by the compiler.

- `//` may appear at the start of a line (whole-line comment) or after code on the same line (trailing comment).
- `//` inside a string literal (`"..."` or `"""..."""`) is not a comment — it is part of the string content.

### No Block Comments In MVP

There is no `/* ... */` or equivalent multi-line comment form. Authors who need multi-line comments use consecutive `//` lines:

```glyph
// This explanation spans
// multiple lines. Each line
// starts with //.
```

**Justification:** Keeping the comment grammar to a single form simplifies the parser and aligns with principle 7 (keep the core language intentionally small). Block comments may be added post-MVP if demand warrants.

### No Doc-Comments In MVP

There is no `///`, `##`, or other doc-comment convention. Doc-comments are a tooling and documentation concern rather than a compilation concern. They can be added in a later tier when tooling needs (hover docs, generated API references) are clearer.

## Candidate Analysis

The following candidates were evaluated. The primary selection criteria were: collision with Markdown rendering, familiarity to the target audience (Python/TypeScript/Go developers), distinctness from string literals, and syntax-highlighting support.

| Syntax | Familiar to | Markdown collision | Verdict |
|--------|------------|--------------------|---------|
| `#` | Python, Ruby, YAML | **Yes — renders as heading.** Dealbreaker for `.glyph.md` files viewed in Obsidian or any Markdown renderer. | Rejected |
| `//` | JS, TS, C, Java, Go, Rust | None | **Selected** |
| `--` | SQL, Lua, Haskell | Minor (`---` is a horizontal rule) | Viable but less familiar to target audience |
| `;` | Lisp, assembly | None | Alien to Python/TS/Go developers |

### Why Not `#`

Glyph source files use the `.glyph.md` extension and may be rendered as Markdown in editors like Obsidian (boundary 5, principle 15). `#` is Markdown heading syntax. A line like `# this is a comment` would render as an H1 heading, corrupting the visual layout. This collision is a dealbreaker.

Principle 4 calls for "Python-like readability and duck-typed ergonomics, not Python runtime semantics." The readability goal does not require adopting Python's comment character when it conflicts with the file format.

### Why `//`

`//` has no meaning in Markdown and renders as plain text. It is the most widely recognized comment syntax across languages familiar to Glyph's target audience (JavaScript, TypeScript, Go, Rust, C, Java). Every major editor and syntax highlighter supports `//` comments natively. It is visually distinct from string delimiters (`"`, `"""`), reserved keywords, and all other Glyph tokens.

## Compiled Output

Comments are stripped during compilation. They do not appear in the compiled `.md` file.

This follows from `compiled-output.md`: "Authoring constructs compile away completely" and "No provenance markers." Comments are authoring constructs that serve the skill author, not the consuming agent.

## LLM Repair Pass

The repair pass must preserve comments. This is explicitly required by `llm-repair-pass.md` repair rule 1, which lists "comments" among the source elements that repair must not remove or alter (alongside names, ordering, section structure, indentation style, inline text, and imports).

Repair may insert new code around comments but must not delete, move, or rewrite comment text.

## Block-Structure Interaction

Comment syntax interacts with indentation-based block structure as follows:

- **Comment-only lines are invisible to the indentation parser.** A line containing only a comment does not open, close, or affect any block, regardless of its indentation level. This matches Python's treatment of comment lines.
- **Trailing comments do not affect indentation measurement.** The indentation of a code line is determined by its leading whitespace before the first non-whitespace token, not by anything after `//`.
- **Blank lines around comments do not close blocks.** Blank lines are freely allowed inside blocks; comments surrounded by blank lines do not accidentally terminate a block.

## Open Questions (Deferred)

- **Doc-comments.** Whether a `///` or similar convention should exist for declarations, and what tooling it would support (hover docs, generated references, IDE integration).
- **Block comments.** Whether `/* ... */` should be added post-MVP for commenting out large sections of code.
- **Structured annotations.** Whether comments can carry structured metadata (e.g., `// @deprecated`, `// @todo`) that tooling can extract.
