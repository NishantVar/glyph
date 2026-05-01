# tree-sitter-glyph

Tree-sitter grammar for the [Glyph](../README.md) agent-skill DSL.
Parses `.glyph` source files into a syntax tree suitable for editor
highlighting, scope-aware refactor tooling, and structural queries.

## What this parses

Glyph source files. The grammar covers the full M2 language as
specified in [`design/tree-sitter-grammar.md`](../design/tree-sitter-grammar.md):

- Top-level declarations: `skill`, `block`, `export block`,
  `text`, `int`, `float`, `generated text`, `generated block`,
  `import` (whole-module + selective).
- Sub-sections: `description:`, `context:`, `constraints:`,
  `flow:` (and the implicit single-string body shorthand for
  private blocks).
- Constraint markers: `require`, `avoid`, `must`, `must avoid`.
- Context markers: `context <name-or-string>`.
- Control flow: `if`/`elif`/`else` with `==`, `and`, `or`, `not`,
  and `.applies()` predicates.
- Expressions: function calls, named arguments, member access
  (`module.member`), variable bindings, `return` statements.
- Literals: strings with `{name}` interpolation, triple-quoted
  block strings (with embedded literal quotes), integers, floats,
  booleans, `none`.
- Type annotations: parameter types and return types.

The grammar is intentionally permissive — it accepts any
syntactically reasonable Glyph source so editors can highlight
in-progress files. Semantic validation (name resolution,
no-shadowing, ordering rules) is the compiler's job, not the
grammar's. Where a construct is genuinely ill-formed, the
grammar emits an `ERROR` node and recovers around it.

## Build

This grammar uses the standard tree-sitter toolchain.

```sh
# from this directory
tree-sitter generate    # generate src/parser.c from grammar.js
tree-sitter test        # run the corpus tests
tree-sitter parse <file>
tree-sitter highlight <file>
```

The `src/scanner.c` file implements an external scanner that
emits `INDENT`, `DEDENT`, and `NEWLINE` tokens to make
Python-style significant-indentation work. The scanner also
handles bracket-depth suppression: inside `(...)`, `{...}`,
and `"""..."""`, indentation tokens are not emitted, so
multi-line constructs flow naturally without breaking on
internal newlines.

## Editor integration

| Editor | Status | Path |
|---|---|---|
| Neovim | **tested end-to-end** | [`editors/nvim/`](editors/nvim/) |
| VS Code | scaffold | [`editors/vscode/`](editors/vscode/) |
| Zed | scaffold | [`editors/zed/`](editors/zed/) |
| Helix | scaffold | [`editors/helix/`](editors/helix/) |

The Neovim integration is verified: parser loads, ftdetect
maps `.glyph` to the `glyph` filetype, the highlights and
locals queries fire on real corpus files, and malformed input
degrades gracefully (errors highlighted, surrounding code
still colored). The other editors have on-disk layout and
manifests in place but were not loaded end-to-end in M3 — see
each directory's README for what's done and what's next.

## GitHub Linguist

[`linguist/`](linguist/) ships the language entry to add to
[`github-linguist/linguist`](https://github.com/github-linguist/linguist)'s
`lib/linguist/languages.yml` plus a contributor README explaining
the upstream-PR path and a `.gitattributes` fallback for
repositories that need recognition before the PR merges. The
patch is **prepared but not submitted** — opening the PR is left
for the project maintainer.

## Test corpus conventions

Tests live in [`test/corpus/`](test/corpus/). Each `.txt` file
groups related tests; one test is a header line of `=` characters,
a name, another `=` line, the source under test, a separator of
`-` characters, and the expected S-expression tree.

```
================================================================================
Skill with body-level markers and flow
================================================================================

skill update_docs(scope = ".")
    description: "Update the documentation."
    require completeness
    flow:
        "Audit the docs."

text completeness = "..."

--------------------------------------------------------------------------------

(source_file
  (skill_declaration
    name: (identifier)
    ...
```

Key conventions:

- **Use field-name prefixes** (e.g. `name: (identifier)`,
  `value: (string_literal ...)`) where the grammar declares
  fields. `tree-sitter test --show-fields` checks them.
- **Hidden rules don't appear in trees** (rules whose name starts
  with `_`). If you depend on a sub-rule's structure, the
  expected tree should reflect what the parser actually emits,
  not what the rule looks like in `grammar.js`.
- **Comments are extras** — they don't appear in the expected
  tree wrapped inside their statement; they sit as siblings at
  whatever scope the grammar is parsing.
- **`ERROR` nodes are part of the contract** for malformed
  inputs. The error-recovery corpus locks in which malformed
  cases produce clean partial trees and which produce contained
  `ERROR` nodes — both are acceptable per the M3 exit criteria,
  but a regression that swaps one for the other is a real
  change worth flagging.

## Scanner notes

The external scanner in [`src/scanner.c`](src/scanner.c) is the
trickiest part of the grammar. A few invariants to preserve:

1. **Three external tokens, in order**: `_indent`, `_dedent`,
   `_newline`. Match the order in `externals: ($) => [...]` in
   `grammar.js`. The token-type enum in the scanner mirrors that
   order; reordering one without the other corrupts the parser's
   token stream silently.
2. **Bracket-depth suppression via `valid_symbols`**: when none
   of the three externals are valid in the current parse state,
   the scanner returns `false` immediately. This is how
   `(...)`, `{...}`, and `"""..."""` absorb internal newlines —
   the grammar never references `_newline` inside those regions,
   so the scanner stays out of the way and `\n` falls through to
   the `extras` rule. Don't add an explicit bracket counter.
3. **Pending-token queue**: a single line boundary may emit a
   `NEWLINE` followed by an `INDENT` or several `DEDENT`s. The
   scanner queues those as `pending_*` flags and drains one per
   `scan` call. Serialization preserves the queue across
   incremental re-parses.
4. **Tabs are treated as 4 spaces** for indentation accounting,
   then a separate compiler pass flags tab usage as a repairable
   error. The grammar doesn't enforce 4-space-only — that's the
   compiler's job.
5. **EOF without trailing newline**: the scanner queues a final
   `NEWLINE` plus enough `DEDENT`s to drain the indent stack to
   the base level. This happens once (`eof_done` guard); a
   second EOF call returns `false`.

## Known limitations

- **`_effects_stub`**: a hidden grammar rule that recognises
  `effects: a, b, c` as a no-op line. `effects:` is out of MVP
  Glyph entirely, but a few corpus fixtures still contain
  `effects:` lines pending separate cleanup. The stub prevents
  recovery cascades that would pollute adjacent inline
  instructions. Tracked for removal in
  [`design/todo.md`](../design/todo.md).
- **VS Code scaffold is incomplete**: tree-sitter integration
  for VS Code requires a WASM-compiled parser plus a TypeScript
  host extension. The on-disk manifest is in place; the WASM
  build and host wiring are deferred.
- **Zed and Helix integrations are scaffold-only**: layout and
  manifests are in place, but neither was loaded end-to-end in
  M3.
- **Linguist PR is prepared, not submitted**.

## Contributing

When changing the grammar:

1. Edit `grammar.js`. Run `tree-sitter generate`.
2. Run `tree-sitter test`. If trees changed, decide whether the
   change is intentional (update the test) or a regression
   (fix the grammar).
3. Run `tree-sitter highlight` over the project corpus
   (`crates/glyph-cli/tests/corpus/valid/`,
   `crates/glyph-cli/tests/corpus/multi-file/`,
   `crates/glyph-cli/tests/corpus/repairable/`,
   `crates/glyph-cli/tests/corpus/invalid/`) to confirm no
   regressions on real files.
4. If editor query files change, validate against the corpus:
   `tree-sitter query queries/locals.scm <file>` and similar.
5. If the external scanner changes, also test pathological
   inputs: empty file, only-whitespace, mixed tabs and spaces,
   unclosed parentheses, unclosed triple-quoted strings.
   Scanner panics or infinite loops are regressions; ERROR
   nodes are fine.

## Related design documents

- [`../design/tree-sitter-grammar.md`](../design/tree-sitter-grammar.md)
  — the canonical grammar plan: scope, indentation strategy,
  capture plan, M1/M2/M3 phases, open questions and risks.
- [`../design/language-surface.md`](../design/language-surface.md)
  — Glyph syntax (declarations, sections, indentation rules).
- [`../design/values-and-names.md`](../design/values-and-names.md)
  — naming and scoping rules (no shadowing, file scope, etc.)
  that drive the `locals.scm` model.

## License

TBD (matches the parent project).
