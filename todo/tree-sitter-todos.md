# Tree-Sitter Grammar — Outstanding Work

Work tracking extracted from the original `design/tree-sitter-grammar.md`
when that file was promoted to durable architecture
([[tree-sitter]]). Durable rationale lives in the
architecture doc; this file is a TODO/risk register.

## Milestone Plan

### M1 — Minimum Viable Grammar

Parse the core skill structure — enough to highlight `with_context.glyph`
and `update_docs.glyph` fully.

Deliverables:

- `grammar.js` with rules for: `source_file`, `skill_declaration`,
  `text_declaration`, `parameter_list`, `parameter` (with defaults),
  `declaration_body`, `description_section`, `context_section`,
  `flow_section`, `inline_instruction`, `string_literal`,
  `interpolation`, `identifier`, `comment`.
- External scanner (`scanner.c`) emitting `INDENT`/`DEDENT`/`NEWLINE`
  with indent stack, blank-line skipping, and bracket-depth tracking
  for `()`.
- `highlights.scm` with captures for all M1 node types.
- `package.json` with tree-sitter configuration.
- Test corpus (`test/corpus/*.txt`) covering M1 constructs.

Constructs covered: skill with default params, `description:` short
and long form, `context:` with bare names and inline strings, `flow:`
with inline string instructions, `text` bindings, `// comments`,
parameter defaults, `{name}` interpolation.

Exit criteria:

- `tree-sitter generate` succeeds without conflicts.
- `tree-sitter test` passes for all M1 cases.
- `tree-sitter highlight` produces correct captures for
  `with_context.glyph` and `update_docs.glyph`.
- External scanner correctly handles 0->4->8 indent, dedent across
  multiple levels, blank lines inside blocks, bracket-depth
  suppression in parameter lists.

### M2 — Full Language Coverage

Grammar parses every file in the valid corpus.

Deliverables: grammar rules for `block_declaration`,
`export_block_declaration`, `const_declaration`,
`generated_const_declaration`, `generated_block_declaration`,
`import_statement`, `import_list`, `import_specifier`,
`constraints_section`, `require_marker`, `avoid_marker`,
`must_marker`, `must_avoid_marker`, `context_marker`,
`call_expression`, `argument_list`, `variable_binding`,
`return_statement`, `if_statement`, `elif_clause`, `else_clause`,
`condition`, `comparison`, `applies_expression`, `qualified_name`,
`block_string`, `return_type`, `type_annotation`, `type_identifier`,
`integer_literal`, `float_literal`, `boolean_literal`, `none_literal`.
Scanner update: bracket-depth tracking for `{}` (import lists,
interpolation). Extended `highlights.scm` with all capture names.
Test corpus covering every valid corpus file and key repairable
patterns.

Exit criteria:

- Every file in `tests/corpus/valid/` (including `imports/`) parses
  without errors.
- `tree-sitter test` passes for all M2 cases.
- `tree-sitter highlight` produces correct captures for
  `branching.glyph`, `explicit_blocks.glyph`, `imports/fix_bug.glyph`.
- No grammar conflicts reported by `tree-sitter generate`.

### M3 — Error Recovery and Ecosystem Integration

Robustness and editor integration.

Deliverables: error recovery rules in `grammar.js` (using
`$.ERROR` and `prec`); grammar handles all repairable corpus files
without crashing (partial trees with `ERROR` nodes for invalid
constructs); `locals.scm` for scope-aware highlighting; `injections.scm`
(empty — Glyph files contain no embedded languages); GitHub Linguist
integration via `languages.yml` PR; editor plugin scaffolding for
nvim, VS Code, Zed, Helix; per-editor installation docs.

Error recovery targets:

- Missing `description:` -> rest of skill parses correctly.
- Tab indentation -> scanner marks error but continues parsing.
- Bare name in ambiguous position -> parsed as `identifier`.
- Duplicate imports -> both parse, semantic error left to the compiler.
- Slot in non-instruction string -> parsed normally (semantic check is
  the compiler's job).

Exit criteria:

- All repairable corpus files produce partial trees (no scanner
  panics, no infinite loops).
- `tree-sitter highlight` degrades gracefully on malformed input.
- At least one editor (nvim or VS Code) can load the grammar and
  highlight a `.glyph` file end-to-end.
- `.glyph` extension registered with GitHub Linguist (PR submitted or
  `.gitattributes` fallback documented).

## Open Questions and Risks

### File Extension

The project migrated to `.glyph`. The grammar assumes this exclusively.
GitHub Linguist gets a clean extension association via `languages.yml`;
no column-0 heuristic needed.

### Indentation Edge Cases

`glyph-core` enforces exactly 4-space indentation increments. The
tree-sitter scanner should match this; remaining edge cases:

- **Partial indentation (2 spaces, 6 spaces).** `glyph-core` rejects.
  The tree-sitter scanner should still produce a parse tree (for
  highlighting) but emit an `ERROR` node or `bad_indent` token.
- **Mixed tabs and spaces.** Reject with `ERROR`. The scanner must not
  silently convert.
- **Trailing whitespace.** Ignored.

Risk: low — Python's scanner handles all these cases.

### Existing Parser Divergence

The Rust parser may handle edge cases differently from the grammar.

- **Single-string shorthand** (block body without `flow:`). The Rust
  parser treats a bare string as the block body. The tree-sitter
  grammar handles this as an alternative inside `declaration_body`.
- **Body-level constraint/context markers vs section-level.** Rust
  parser normalises these later; tree-sitter parses both positions and
  lets highlight queries differentiate.
- **Implicit `flow:` in blocks.** When a block has a single string
  body, no `flow:` header — the grammar must accept either form.

Risk: low — tree-sitter is intentionally more permissive.

### Condition Expressions

The corpus shows `if mode == "fast"`; design docs also mention `and`,
`or`, `not`, and `.applies()` compositions. The condition grammar must
support:

```
if mode == "fast"
if block_x.applies()
if block_x.applies() and not is_dry_run
if mode == "fast" or mode == "turbo"
```

Standard expression grammar with binary operators, unary `not`,
comparisons, and method calls. No special scanner support needed.

Risk: low.

### String Interpolation Nesting

`{name}` slots in strings are simple identifiers only — no nested
expressions, no format specifiers. Simpler than Python f-strings or
Rust format strings.

Risk: very low.
