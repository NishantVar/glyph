# ADR 0022: Tree-Sitter External Scanner for Indentation

## Status

Accepted.

## Context

Glyph is indentation-significant on the Python model: 4-space steps,
no braces, no `end` keyword, blank lines ignored, arbitrary nesting
depth for branching. The tree-sitter grammar must produce a syntax
tree that mirrors this structure so editors can highlight it
correctly.

Three approaches were considered:

1. **External scanner emitting INDENT / DEDENT / NEWLINE tokens** —
   the Python-style approach.
2. **Whitespace-significant context-free encoding** — `$.indent_N`
   tokens encoded directly in grammar rules. Works for fixed-depth
   grammars; breaks when nesting is unbounded.
3. **YAML-style context-stack scanner** — tracks flow vs block mode.
   Glyph has no flow/block distinction.

## Decision

Use approach (1): an **external scanner** (`src/scanner.c`) that
maintains an indent stack and emits `INDENT`, `DEDENT`, and `NEWLINE`
tokens.

The scanner:

- Initialises the indent stack with `[0]`.
- Skips blank and comment-only lines entirely.
- On a non-blank line, measures the leading whitespace and emits the
  appropriate token(s).
- Suppresses `INDENT`/`DEDENT`/`NEWLINE` emission when bracket depth
  > 0 (inside `()`, `{}`, or `"""`).
- At EOF, emits one `DEDENT` per remaining stack entry above 0.

`tree-sitter-python`'s scanner is the primary reference. Glyph's
scanner is structurally identical but simpler — no `\` line
continuation, no f-string nesting.

## Consequences

- Arbitrary nesting depth is supported with constant scanner state.
- The grammar mirrors Python's well-understood model; new contributors
  who have read `tree-sitter-python` will recognise the pattern.
- A C dependency is introduced (the external scanner), which is the
  norm for indentation-sensitive tree-sitter grammars.
- Tab handling and partial-indentation handling become scanner-level
  concerns. The scanner rejects tabs with an `ERROR` node and treats
  partial indents (2 spaces, 6 spaces) as scanner errors rather than
  re-flowing the indent stack.
- If the hand-rolled `glyph-core` parser ever diverges on indentation
  policy, the scanner is the single point that needs updating on the
  tree-sitter side.
