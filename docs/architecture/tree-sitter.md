# Tree-Sitter Architecture

The tree-sitter grammar (`tree-sitter-glyph/`) is the editor-tooling
front-end for Glyph. It parses `.glyph` source into a syntax tree
suitable for highlighting and structural queries; it is *not* the
compiler's parser and does *not* validate semantics.

The hand-rolled parser in `glyph-core` (see
[[0001-hand-rolled-parser]])
remains authoritative for everything beyond syntactic shape.

## Role and Boundaries

- **What it parses.** The full M2 surface language: declarations,
  sub-sections, constraint/context markers, control flow (`if`/
  `elif`/`else`, `==`, `and`, `or`, `not`, `.applies()`), calls,
  bindings, returns, string interpolation, triple-quoted strings,
  numeric/boolean/`none` literals, type annotations.
- **What it does not do.** No name resolution, no scope analysis, no
  effect inference, no diagnostic IDs. The grammar is intentionally
  permissive — semantic checks live in `glyph-core`.
- **Failure mode.** On malformed input the grammar degrades to partial
  trees with `ERROR` nodes rather than crashing — editors keep
  highlighting around the broken region.

## Indentation Strategy

Glyph is indentation-significant on the Python model: 4-space steps,
no braces, no `end` keyword, blank lines ignored. The grammar handles
this with an **external scanner** (`src/scanner.c`) emitting
`INDENT` / `DEDENT` / `NEWLINE` tokens — see
[[0022-tree-sitter-external-scanner]]
for the choice rationale.

Invariants the scanner enforces:

- Indent stack of integer column positions, initialised to `[0]`.
- Blank and comment-only lines do not affect indentation.
- Inside `()`, `{}`, and `"""`, indentation is not structurally
  significant: bracket depth suppresses `INDENT`/`DEDENT`/`NEWLINE`.
- At EOF, one `DEDENT` is emitted per remaining stack entry above 0.

`tree-sitter-python` is the primary reference; `tree-sitter-nim` and
`tree-sitter-yaml` were considered and rejected (Nim's `=` continuation
is unnecessary; YAML's flow/block context stack adds complexity Glyph
does not need).

## Synchronisation With The Hand-Rolled Parser

Because the grammar and `glyph-core` parse the same source, divergence
is a real risk. The grammar is intentionally **more permissive** than
the compiler. The contract is:

- Anything that parses cleanly in `glyph-core` must also parse cleanly
  in tree-sitter. The reverse does not hold.
- Where the Rust parser normalises body-level vs section-level markers
  in later phases, tree-sitter preserves both positions and lets
  highlight queries differentiate. No normalisation in the grammar.
- Implicit `flow:` (a private block with a single-string body) is a
  grammar alternative inside `declaration_body`, not a desugaring.
- Duplicate sub-section recovery is in the AST builder, not the
  grammar: the CST keeps every occurrence as a sibling, and the AST
  lifts the first occurrence and pushes later ones into the recovery
  slot ([[docs/architecture/repair]] §Phase 3a).

When the language surface changes, both parsers must be updated in the
same change set; the test corpus
(`crates/glyph-cli/tests/corpus/valid/`) is the shared acceptance
target.

## Node Taxonomy

The grammar emits these top-level node kinds (full enumeration kept
inline because the names are referenced from highlight queries):

| Node | Represents |
|------|------------|
| `source_file` | Root; sequence of top-level declarations |
| `skill_declaration` | `skill name(params)` + body |
| `block_declaration` | `block name(params)` + body |
| `export_block_declaration` | `export block name(params)` + body |
| `const_declaration` | `const name = <literal>` (kind inferred from literal) |
| `generated_const_declaration` | `generated const name = "value"` |
| `generated_block_declaration` | `generated block name()` + body |
| `import_statement` | `import "path" { ... }` or `... as alias` |
| `parameter_list`, `parameter`, `type_annotation`, `return_type` | Header components |
| `declaration_body` | Indented body of a skill/block |
| `description_section`, `context_section`, `constraints_section`, `flow_section` | Sub-sections |
| `require_marker`, `avoid_marker`, `must_marker`, `must_avoid_marker` | Constraint markers |
| `context_marker` | Context marker |
| `flow_statement` | Union: instruction, call, binding, return, if, marker |
| `inline_instruction`, `call_expression`, `applies_expression`, `variable_binding`, `return_statement` | Flow statement kinds |
| `if_statement`, `elif_clause`, `else_clause`, `condition`, `comparison` | Branching |
| `argument_list`, `argument` | Call arguments |
| `string_literal`, `block_string`, `interpolation` | String forms |
| `integer_literal`, `float_literal`, `boolean_literal`, `none_literal` | Numeric/boolean literals |
| `identifier`, `qualified_name`, `comment`, `type_identifier` | Leaves |

**Keyword tokens** (named for reliable highlighting): `skill`,
`block`, `export`, `generated`, `text`, `int`, `float`, `import`,
`as`, `description`, `context`, `constraints`, `flow`, `require`,
`avoid`, `must`, `if`, `elif`, `else`, `return`, `true`, `false`,
`none`, `and`, `or`, `not`, `applies`.

## Highlight Capture Taxonomy

Highlight queries (`queries/highlights.scm` per editor) map nodes to
the following capture names. The taxonomy is the durable contract —
editor themes pick concrete colors.

| Capture | Bound to | Intent |
|---------|----------|--------|
| `@keyword` | `skill`, `block`, `export`, `generated`, `import`, `as`, `if`, `elif`, `else`, `return`, `and`, `or`, `not` | Structural keywords |
| `@keyword.directive` | `require`, `avoid`, `must` | Constraint markers — visually distinct from `@keyword` |
| `@keyword.context` | `context` (marker keyword form) | Context marker keyword |
| `@label` | `description`, `context`, `constraints`, `flow` section headers | Section anchors — the primary visual differentiator |
| `@type`, `@type.builtin` | `type_identifier`; `text`, `int`, `float`, `none`, `true`, `false` | Types and built-in values |
| `@function` | Skill name in declaration header | Skill name |
| `@function.method` | Block / export-block name in declaration header | Block name |
| `@function.call` | Call target identifier | Call site |
| `@variable` | LHS of `variable_binding` | Local binding |
| `@variable.parameter` | Parameter name | Parameter |
| `@string`, `@string.special` | `string_literal`, `block_string`; `interpolation` | Strings; `{name}` slots highlighted distinctly |
| `@constant`, `@constant.builtin` | Named text bindings; `none_literal`, `boolean_literal` | Constants and built-in values |
| `@module` | `import_path` and import aliases | Modules |
| `@comment`, `@number` | `comment`; `integer_literal`, `float_literal` | Comments and numeric literals |
| `@property` | RHS of `qualified_name`; `applies` | Member access |
| `@punctuation.delimiter`, `@punctuation.bracket`, `@punctuation.special` | `:`, `,`; `(`, `)`, `{`, `}`; `=`, `->`, `.` | Structural punctuation |

The goal is that each construct occupies its own visual lane: section
headers, constraint markers, instruction strings with highlighted
slots, and call targets are immediately distinguishable.

## References

- Grammar source: [`tree-sitter-glyph/grammar.js`](../../tree-sitter-glyph/grammar.js)
- External scanner: `tree-sitter-glyph/src/scanner.c`
- Editor configs: `tree-sitter-glyph/editors/{nvim,vscode,zed,helix}/`
- Linguist registration: `tree-sitter-glyph/linguist/`
- Outstanding work: [[tree-sitter-todos]]
