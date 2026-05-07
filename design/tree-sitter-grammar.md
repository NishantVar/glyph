# Tree-Sitter Grammar Design for Glyph

This document specifies the tree-sitter grammar design for `.glyph` source files. It is the blueprint for implementing `grammar.js` and the external scanner. A separate implementer agent should be able to pick this up and start writing code without re-reading the corpus or design docs.

## 1. Syntax Survey

Every construct below was observed in the test corpus (`crates/glyph-cli/tests/corpus/valid/` and `repairable/`) or is specified in the canonical design docs (`language-surface.md`, `ir-and-semantics.md`). Corpus file citations use short names and retain the current `.glyph` extension in paths — the project is migrating source files to `.glyph`; that rename is handled separately and does not affect the grammar.

### 1.1 Top-Level Declarations

| Construct | Example | Corpus files |
|-----------|---------|--------------|
| `skill` with empty params | `skill review_code()` | `body_context`, `constraint_only`, `explicit_blocks` |
| `skill` with default params | `skill fix_bug(scope = ".")` | `with_context`, `imports/fix_bug` |
| `skill` with multiple params | `skill summarize_dir(scope = ".", target)` | `with_params` |
| `skill` with return type | `skill name() -> Type` | design docs only (not in corpus) |
| `block` with empty params | `block small_helper()` | `explicit_blocks` |
| `block` with params | `block fast_mode()` | `branching` |
| `export block` with params | `export block inspect_repo(scope = ".")` | `imports/repo_tools` |
| `const` binding | `const name = "value"` | `with_context`, `constraint_only`, `update_docs` |
| `export const` binding | `export const name = "value"` | `imports/prefs` |
| `const` numeric binding | `const max_attempts = 3` (int inferred from literal) | design docs only |
| `const` numeric binding | `const threshold = 0.8` (float inferred from literal) | design docs only |
| `generated const` binding | `generated const name = "value"` | design docs only |
| `generated block` | `generated block name()` | design docs only |
| `import` selective | `import "./path" { name1, name2 }` | `imports/fix_bug`, `imports/unused_import` |
| `import` with alias | `import "./path" as alias` | design docs only |
| `import` selective with alias | `import "./path" { name as alias }` | design docs only |

### 1.2 Sub-Section Headers (Colon-Terminated)

| Header | Forms | Corpus files |
|--------|-------|--------------|
| `description:` | Short form: `description: "text"` | all skills with descriptions |
| `context:` | Long form (indented body) | `with_context` |
| `flow:` | Long form (indented body) | nearly all files |
| `constraints:` | Long form (indented body) | design docs (body-level markers used in corpus instead) |

### 1.3 Constraint and Context Markers

| Marker | Position | Corpus files |
|--------|----------|--------------|
| `require <name>` | body-level | `constraint_only`, `update_docs`, `imports/fix_bug` |
| `avoid <name>` | body-level | `constraint_only`, `update_docs` |
| `must <name>` | any | design docs only |
| `must avoid <name>` | any | design docs only |
| `context <name>` | body-level | `body_context` |
| `context "string"` | body-level | `body_context` |
| `context <name>` | inside `flow:` | `flow_context` |
| `context "string"` | inside `flow:` | `flow_context` |

### 1.4 Flow Statements

| Construct | Example | Corpus files |
|-----------|---------|--------------|
| Inline string instruction | `"Do something."` | all files with flow |
| Block call (no args) | `review_code()` | `explicit_blocks` |
| Block call (with args) | `inspect_repo(scope)` | `imports/fix_bug` |
| Variable binding from call | `ctx = inspect_repo(scope)` | `imports/fix_bug` |
| `return` expression | `return ctx` | `imports/fix_bug`, `imports/repo_tools` |
| `return` call | `return failure_report()` | design docs only |
| `if` / `elif` / `else` | `if mode == "fast"` | `branching` |
| String interpolation slot | `"Inspect at {scope}."` | `with_params`, `imports/repo_tools` |
| `.applies()` predicate | `block_x.applies()` | design docs only |

### 1.5 Other Constructs

| Construct | Example | Corpus files |
|-----------|---------|--------------|
| Comments | `// comment` | design docs only (not in corpus files) |
| Triple-quoted strings | `"""multi-line"""` | design docs only |
| Return type annotation | `-> ReturnType` | `imports/repo_tools` (design docs) |
| Bare name (ambiguous role) | `accuracy` at body level | `repairable/ambiguous_role` |
| Bare text name in flow | `validate_style` in flow | `repairable/bare_name_in_flow` |
| Tab indentation (error) | tabs instead of spaces | `repairable/tab_indent` |
| Slot in non-instruction string | `{scope}` in description/context | `repairable/slot_in_context`, `repairable/slot_in_description` |

### 1.6 Parameter Syntax

Parameters appear inside `()` on callable declarations. Four forms from design docs:

```
name                          // required, untyped
name = "default"              // optional, untyped, with default
name: Type                    // required, typed
name: Type = default_value    // optional, typed, with default
```

Defaults can be string literals, integer literals, float literals, booleans (`true`/`false`), `none`, or bare-name references to value bindings.

## 2. Node Taxonomy

### 2.1 Node Name Table

| Node name | What it represents | Leaf/Branch |
|-----------|--------------------|-------------|
| `source_file` | Root node; sequence of top-level declarations | Branch |
| `skill_declaration` | `skill name(params)` + body | Branch |
| `block_declaration` | `block name(params)` + body | Branch |
| `export_block_declaration` | `export block name(params)` + body | Branch |
| `const_declaration` | `const name = <literal>` (kind inferred per `language-surface.md` §3.6) | Branch |
| `generated_const_declaration` | `generated const name = "value"` | Branch |
| `generated_block_declaration` | `generated block name(params)` + body | Branch |
| `import_statement` | `import "path" { ... }` or `import "path" as alias` | Branch |
| `import_path` | The quoted path string in an import | Leaf |
| `import_list` | `{ name, name as alias }` | Branch |
| `import_specifier` | Single name (with optional alias) inside import list | Branch |
| `parameter_list` | `(param, param, ...)` on declarations | Branch |
| `parameter` | Single parameter with optional type and default | Branch |
| `type_annotation` | `: Type` on a parameter | Branch |
| `return_type` | `-> Type` on a declaration header | Branch |
| `type_identifier` | Type name (e.g., `Plan`, `String`, `None`) | Leaf |
| `declaration_body` | Indented body of a skill/block | Branch |
| `description_section` | `description:` + content | Branch |
| `context_section` | `context:` + indented entries | Branch |
| `constraints_section` | `constraints:` + indented markers | Branch |
| `flow_section` | `flow:` + indented statements | Branch |
| `require_marker` | `require <name-or-string>` | Branch |
| `avoid_marker` | `avoid <name-or-string>` | Branch |
| `must_marker` | `must <name-or-string>` | Branch |
| `must_avoid_marker` | `must avoid <name-or-string>` | Branch |
| `context_marker` | `context <name-or-string>` | Branch |
| `flow_statement` | Union: instruction, call, binding, return, if, marker | Branch |
| `inline_instruction` | `"Quoted instruction string."` | Branch |
| `call_expression` | `name(args)` | Branch |
| `applies_expression` | `name.applies()` | Branch |
| `argument_list` | `(arg, arg, ...)` in a call | Branch |
| `argument` | Single argument (positional or named) | Branch |
| `variable_binding` | `name = expression` in flow | Branch |
| `return_statement` | `return expression` | Branch |
| `if_statement` | `if` / `elif` / `else` block | Branch |
| `elif_clause` | `elif condition` + body | Branch |
| `else_clause` | `else` + body | Branch |
| `condition` | Expression after `if`/`elif` | Branch |
| `comparison` | `name == "value"` | Branch |
| `string_literal` | `"..."` inline string | Leaf |
| `block_string` | `"""..."""` triple-quoted string | Leaf |
| `interpolation` | `{name}` inside a string | Branch |
| `integer_literal` | `42`, `3` | Leaf |
| `float_literal` | `0.8`, `0.7` | Leaf |
| `boolean_literal` | `true`, `false` | Leaf |
| `none_literal` | `none` | Leaf |
| `identifier` | Bare name (variable, binding, type, etc.) | Leaf |
| `comment` | `// text` | Leaf |
| `qualified_name` | `module.member` | Branch |

**Duplicate sub-section recovery.** The grammar does not enforce singleton-ness on `description_section`, `context_section`, `constraints_section`, `effects_section`, or `flow_section` — a `declaration_body` may parse with multiple occurrences of the same sub-section kind. The CST therefore preserves every occurrence as a sibling node in source order. The AST builder lifts the **first** occurrence of each kind into the canonical singleton field on the declaration AST node and pushes every **later** occurrence into the additive recovery slot `extra_subsections: Vec<DuplicateSubsection>` (see `language-surface.md` §2.5). Each `DuplicateSubsection` retains the sub-section kind, its full body span, and its associated comment trivia so Phase 3a's deterministic merge (`repair.md` §4.11) can splice bodies and comments without reparsing.

### 2.2 Keyword Tokens

These are named keyword tokens (not anonymous string literals) for reliable highlighting:

```
"skill" "block" "export" "generated" "text" "int" "float" "import" "as"
"description" "context" "constraints" "flow"
"require" "avoid" "must"
"if" "elif" "else"
"return"
"true" "false" "none"
"and" "or" "not"
"applies"
```

## 3. Indentation Strategy

### 3.1 Decision: External Scanner with INDENT / DEDENT / NEWLINE

**Chosen approach: (a) External scanner emitting INDENT, DEDENT, and NEWLINE tokens** — the Python-style approach.

### 3.2 Rationale

Glyph's indentation semantics are directly modeled on Python: blocks open when indentation increases by a fixed unit (4 spaces), and close when indentation returns to the previous level. Blank lines are ignored. There are no braces, no `end` keywords, and no optional semicolons. This is structurally identical to what tree-sitter-python solves.

**Why not option (b) — whitespace-significant context-free encoding?** This approach (used by some simpler grammars) encodes indentation levels directly in the grammar rules via `$.indent_N` tokens. It works for fixed-depth grammars but breaks when nesting is unbounded (e.g., `if` inside `if` inside `flow:`). Glyph supports arbitrary nesting depth for branching, so this approach is insufficient.

**Why not option (c) — YAML-style approach?** tree-sitter-yaml uses a complex external scanner that tracks context stacks for flow vs block mode. Glyph has no flow/block mode distinction — it is uniformly indentation-sensitive. The YAML approach adds complexity without benefit.

### 3.3 Reference Grammars

| Grammar | Relevance to Glyph |
|---------|-------------------|
| **tree-sitter-python** | **Primary reference.** Same indentation model (fixed unit, INDENT/DEDENT/NEWLINE). Python's external scanner (`scanner.c`) maintains an indent stack, emits INDENT on depth increase, DEDENT on decrease, and NEWLINE at line boundaries. Glyph's scanner will be structurally identical but simpler (no `\` line continuation, no `f-string` nesting). |
| **tree-sitter-nim** | Nim also uses significant indentation with an external scanner. Its approach is similar to Python's but handles Nim-specific constructs (like `=` continuation). Less directly applicable. |
| **tree-sitter-yaml** | Overly complex for Glyph's needs. YAML has context-dependent indentation (mapping vs sequence vs scalar), which Glyph does not. |

### 3.4 Scanner Design

The external scanner (written in C, compiled as `src/scanner.c`) maintains:

- **Indent stack:** A stack of integer column positions. Initialized with `[0]`.
- **Pending dedents:** A counter for queued DEDENT tokens when multiple levels close at once.

**Token emission logic** (runs at each line boundary):

1. Skip blank lines and comment-only lines entirely (they do not affect indentation).
2. Measure the leading whitespace of the current non-blank line.
3. If indent > stack top: push indent, emit `INDENT`.
4. If indent == stack top: emit `NEWLINE`.
5. If indent < stack top: pop stack until top matches indent (or error if no match), emit one `DEDENT` per pop, then emit `NEWLINE`.
6. At EOF: emit `DEDENT` for every remaining stack entry above 0, then `NEWLINE`.

**Tab handling:** The scanner should reject tabs (emit an `ERROR` node) since Glyph requires 4-space indentation. However, for error recovery, the scanner can treat a tab as 4 spaces and let the grammar produce a valid (but marked-error) tree.

**Paired delimiters:** Inside `()`, `{}`, and `"""`, indentation is not structurally significant (per language-surface.md §2.7). The scanner must track bracket depth and suppress INDENT/DEDENT/NEWLINE emission when bracket depth > 0.

## 4. Highlight Capture Plan

### 4.1 Capture Table

| Capture name | Node(s) | Semantic meaning | Visual effect (typical theme) |
|-------------|---------|------------------|------------------------------|
| `@keyword` | `skill`, `block`, `export`, `generated`, `import`, `as`, `if`, `elif`, `else`, `return`, `and`, `or`, `not` | Language keywords | Bold / purple |
| `@keyword.directive` | `require`, `avoid`, `must` | Constraint markers — distinct from structural keywords | Red / orange |
| `@keyword.context` | `context` (when used as marker keyword) | Context marker keyword | Teal / cyan |
| `@type` | `type_identifier`, return type in `-> Type` | Type names | Yellow / green |
| `@type.builtin` | `text`, `int`, `float`, `none`, `true`, `false` | Built-in type/value keywords | Yellow / green (dimmer) |
| `@function` | `identifier` in `skill_declaration` header | Skill name | Blue / bold |
| `@function.method` | `identifier` in `block_declaration` / `export_block_declaration` header | Block name | Blue |
| `@function.call` | `identifier` in `call_expression` | Call target | Blue (unbold) |
| `@variable` | `identifier` in `variable_binding` (LHS) | Local variable | White / foreground |
| `@variable.parameter` | `identifier` in `parameter` | Parameter name | Italic / orange |
| `@string` | `string_literal`, `block_string` | String content | Green |
| `@string.special` | `interpolation` (`{name}` inside strings) | Slot reference in instruction strings | Green + bold, or distinct color |
| `@label` | `description`, `context`, `constraints`, `flow` (when used as section headers) | Section header keywords | Cyan / teal — **the key differentiator** |
| `@punctuation.delimiter` | `:` after section headers, `,` in lists | Structural punctuation | Grey |
| `@punctuation.bracket` | `(`, `)`, `{`, `}` | Brackets | Grey |
| `@punctuation.special` | `=` in bindings and defaults, `->` in return types, `.` in qualified names | Operators | Grey / white |
| `@constant` | `identifier` in `text_declaration` name position | Named constants | Magenta / coral |
| `@constant.builtin` | `none_literal`, `boolean_literal` | Built-in constant values | Magenta |
| `@module` | `import_path`, `identifier` in `import ... as <alias>` | Module path/alias | Yellow / orange |
| `@comment` | `comment` | Line comments | Grey / italic |
| `@number` | `integer_literal`, `float_literal` | Numeric literals | Orange / yellow |
| `@property` | `identifier` after `.` in `qualified_name`, `applies` keyword | Member access | Blue / italic |

### 4.2 Making Context vs Flow vs Description Visually Distinct

The user's primary goal is that "different parts like context [are] linked differently than instructions." Here is how the capture plan achieves this:

1. **Section headers are `@label`** — `context:`, `flow:`, `description:`, `constraints:` all render in a distinct color (cyan/teal). This immediately tells the reader which section they're in.

2. **Constraint markers are `@keyword.directive`** — `require`, `avoid`, `must` render in red/orange, visually distinct from structural `@keyword` (purple). This makes behavioral rules pop.

3. **Context markers are `@keyword.context`** — when `context` is used as a marker keyword (not a section header), it renders in teal/cyan, connecting it visually to the `context:` section header.

4. **String instructions in `flow:` are `@string`** — green, the standard string color. Combined with the `@label` section header above them, the reader knows these are instructions.

5. **Slot references `{name}` are `@string.special`** — visually distinct within instruction strings, so parameter slots are immediately identifiable.

6. **Named constants (text bindings) are `@constant`** — when a bare name references a `text` binding in `context:` or `constraints:`, it renders differently from call targets (`@function.call`) and variables (`@variable`).

The net effect: a reader scanning a skill sees purple keywords, cyan section headers, red constraint markers, green instruction strings with highlighted slots, and blue call targets — each construct occupies its own visual lane.

### 4.3 Editor-Specific Query Files

Each editor needs a `highlights.scm` file mapping tree-sitter nodes to captures. Example queries (representative, not exhaustive):

```scheme
; Section headers — the visual anchors
(description_section "description" @label)
(context_section "context" @label)
(flow_section "flow" @label)
(constraints_section "constraints" @label)

; Declaration keywords
"skill" @keyword
"block" @keyword
"export" @keyword
"import" @keyword
"return" @keyword
"if" @keyword
"elif" @keyword
"else" @keyword

; Constraint markers — visually distinct from keywords
(require_marker "require" @keyword.directive)
(avoid_marker "avoid" @keyword.directive)
(must_marker "must" @keyword.directive)
(must_avoid_marker "must" @keyword.directive)
(must_avoid_marker "avoid" @keyword.directive)

; Context marker
(context_marker "context" @keyword.context)

; Skill/block names in headers
(skill_declaration name: (identifier) @function)
(block_declaration name: (identifier) @function.method)
(export_block_declaration name: (identifier) @function.method)

; Calls
(call_expression function: (identifier) @function.call)

; Strings and interpolation
(string_literal) @string
(block_string) @string
(interpolation) @string.special

; Types
(type_identifier) @type
"text" @type.builtin
"int" @type.builtin
"float" @type.builtin

; Parameters
(parameter name: (identifier) @variable.parameter)

; Variables
(variable_binding name: (identifier) @variable)

; Constants
(text_declaration name: (identifier) @constant)

; Comments
(comment) @comment
```

## 5. Phased Implementation Plan

### M1: Minimum Viable Grammar

**Scope:** Parse the core skill structure — enough to highlight `with_context.glyph` and `update_docs.glyph` fully.

**Deliverables:**
- `grammar.js` with rules for: `source_file`, `skill_declaration`, `text_declaration`, `parameter_list`, `parameter` (with defaults), `declaration_body`, `description_section`, `context_section`, `flow_section`, `inline_instruction`, `string_literal`, `interpolation`, `identifier`, `comment`
- External scanner (`scanner.c`) emitting `INDENT`, `DEDENT`, `NEWLINE` with indent stack, blank-line skipping, and bracket-depth tracking for `()`
- `highlights.scm` with captures for all M1 node types
- `package.json` with tree-sitter configuration
- Test corpus (`test/corpus/*.txt`) covering M1 constructs

**Constructs covered:**
- `skill name(params)` declarations
- `description:` (short and long form)
- `context:` with bare names and inline strings
- `flow:` with inline string instructions
- `text name = "value"` bindings
- `// comments`
- Parameter defaults (`name = "value"`)
- String interpolation `{name}`

**Exit criteria:**
- `tree-sitter generate` succeeds without conflicts
- `tree-sitter test` passes for all M1 test cases
- `tree-sitter highlight` produces correct captures for `with_context.glyph` and `update_docs.glyph`
- External scanner correctly handles: 0->4->8 indent, dedent across multiple levels, blank lines inside blocks, bracket-depth suppression in parameter lists

### M2: Full Language Coverage

**Scope:** All remaining Glyph constructs — the grammar parses every file in the valid corpus.

**Deliverables:**
- Grammar rules for: `block_declaration`, `export_block_declaration`, `const_declaration`, `generated_const_declaration`, `generated_block_declaration`, `import_statement`, `import_list`, `import_specifier`, `constraints_section`, `require_marker`, `avoid_marker`, `must_marker`, `must_avoid_marker`, `context_marker`, `call_expression`, `argument_list`, `variable_binding`, `return_statement`, `if_statement`, `elif_clause`, `else_clause`, `condition`, `comparison`, `applies_expression`, `qualified_name`, `block_string`, `return_type`, `type_annotation`, `type_identifier`, `integer_literal`, `float_literal`, `boolean_literal`, `none_literal`
- Scanner update: bracket-depth tracking for `{}` (import lists, interpolation)
- Extended `highlights.scm` with all capture names from §4
- Test corpus covering every valid corpus file and key repairable patterns

**Constructs added:**
- `block` and `export block` declarations
- `import` (selective and whole-module)
- `const` and `export const` bindings (kind inferred from literal)
- `generated const`, `generated block` declarations
- `require`, `avoid`, `must`, `must avoid` markers (body-level and in-flow)
- `context` marker (body-level and in-flow)
- `constraints:` section
- `if`/`elif`/`else` branching with conditions
- `return` statements
- Variable binding from calls
- Block calls with arguments
- `.applies()` predicate
- Qualified names (`module.name`)
- Triple-quoted block strings
- Numeric and boolean literals
- Return type annotations (`-> Type`)
- Type annotations on parameters (`: Type`)

**Exit criteria:**
- Every file in `tests/corpus/valid/` (including `imports/`) parses without errors
- `tree-sitter test` passes for all M2 test cases
- `tree-sitter highlight` produces correct captures for `branching.glyph`, `explicit_blocks.glyph`, `imports/fix_bug.glyph`
- No grammar conflicts reported by `tree-sitter generate`

### M3: Error Recovery and Ecosystem Integration

**Scope:** Robustness and editor integration.

**Deliverables:**
- Error recovery rules in `grammar.js` (using tree-sitter's `$.ERROR` and `prec` for graceful degradation)
- Grammar handles all repairable corpus files without crashing (produces partial trees with `ERROR` nodes for invalid constructs)
- `locals.scm` for scope-aware highlighting (parameter scope, text-binding references)
- `injections.scm` (empty — Glyph files contain no embedded languages)
- GitHub Linguist integration: register `.glyph` extension in a `languages.yml` PR to github-linguist
- Editor plugin scaffolding: nvim (queries in `queries/`), VS Code (`syntaxes/` TextMate fallback + tree-sitter config), Zed (`languages/`), Helix (`runtime/queries/`)
- Documentation: README, installation instructions per editor

**Error recovery targets:**
- Missing `description:` -> rest of skill parses correctly
- Tab indentation -> scanner treats tabs as errors but continues parsing
- Bare name in ambiguous position -> parsed as `identifier` (not crash)
- Duplicate imports -> both parse, semantic error left to the compiler
- Slot in non-instruction string -> parsed normally (semantic check is compiler's job)

**Exit criteria:**
- All repairable corpus files produce partial trees (no scanner panics, no infinite loops)
- `tree-sitter highlight` degrades gracefully on malformed input (unhighlighted regions, not garbled output)
- At least one editor (nvim or VS Code) can load the grammar and highlight a `.glyph` file end-to-end
- `.glyph` extension registered with GitHub Linguist (PR submitted or `.gitattributes` fallback documented)

## 6. Open Questions and Risks

### 6.1 File Extension

The project is migrating to a `.glyph` extension. The grammar assumes this exclusively. Obsidian rendering is no longer a concern for the grammar — if Obsidian support is wanted later, that's a separate plugin track. GitHub Linguist gets a clean extension association via `languages.yml`, no column-0 heuristic needed.

### 6.2 Migration Handoff

The compiler and corpus rename from `.glyph` to `.glyph` is out of scope for this design and is being handled separately by the user. The grammar implementation should target `.glyph` from the start; any remaining `.glyph` references in the corpus are historical and will be updated independently.

### 6.3 Indentation Edge Cases

The Glyph parser in `glyph-core` enforces exactly 4-space indentation increments. The tree-sitter external scanner should match this — but some edge cases need decisions:

- **Partial indentation (e.g., 2 spaces, 6 spaces):** The Glyph compiler rejects these. The tree-sitter scanner should still produce a parse tree (for highlighting) but can emit an `ERROR` node or a special `bad_indent` token.
- **Mixed tabs and spaces:** Reject with `ERROR`. The scanner should not silently convert.
- **Trailing whitespace:** Ignored (does not affect indentation measurement).

**Risk level:** Low. Python's tree-sitter scanner handles all these cases; we can adapt its approach directly.

### 6.4 Existing Parser Divergence

The Rust parser in `glyph-core` is hand-rolled and may handle edge cases differently from a tree-sitter grammar. Specific risks:

- **Single-string shorthand** (block body without `flow:`): The Rust parser treats a bare string as the block body. The tree-sitter grammar needs to handle this as an alternative to `flow_section` inside `declaration_body`. This is a grammar design choice, not a scanner issue.
- **Body-level constraint/context markers vs section-level:** The Rust parser normalizes these in later phases. The tree-sitter grammar should parse both positions and let the highlight queries differentiate. No normalization needed — tree-sitter is for syntax, not semantics.
- **Implicit `flow:` in blocks:** When a block has a single string body, there's no `flow:` header. The grammar needs a rule like `declaration_body -> (section | inline_instruction | marker)*` that accepts both forms.

**Risk level:** Low. Tree-sitter grammars are intentionally more permissive than language compilers. The goal is correct highlighting, not semantic validation.

### 6.5 Condition Expressions in `if`/`elif`

The corpus shows `if mode == "fast"` — simple equality comparison. The design docs also mention `and`, `or`, `not`, and `.applies()` compositions. The condition grammar needs to be expressive enough for:

```
if mode == "fast"
if block_x.applies()
if block_x.applies() and not is_dry_run
if mode == "fast" or mode == "turbo"
```

This is a standard expression grammar (binary operators, unary `not`, comparisons, method calls). No special scanner support needed — just grammar rules with appropriate precedence.

**Risk level:** Low.

### 6.6 String Interpolation Nesting

`{name}` slots in strings are simple identifiers only — no nested expressions, no format specifiers. This makes the scanner/grammar simpler than Python f-strings or Rust format strings. The scanner can handle `{` and `}` as delimiters within string contexts without needing a mode stack.

**Risk level:** Very low.
