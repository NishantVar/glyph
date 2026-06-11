# BUG-084: boolean_literal and none_literal are lowercase-only, but the spec accepts case-insensitive True/NONE/None

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `tree-sitter-glyph/grammar.js:651-652`
**Found by:** treesitter | **Audit date:** unknown-date

## Description

The grammar defines `boolean_literal: choice("true", "false")` and `none_literal: "none"` — lowercase only. The authoritative design docs state these are case-insensitive: `design/values-and-names.md` line 107 ("Source is case-insensitive: true, True, and TRUE are all accepted"), line 121 ("none, None, and NONE are all accepted"), and `design/language-surface.md` line 292.

A valid Glyph file using `if ctx.ready == True`, `result = NONE`, or `return None` mis-tokenizes the case-variant: because it matches the `identifier` regex `[a-zA-Z_][a-zA-Z0-9_]*` and is NOT a reserved literal, `True`/`NONE`/`None` parses as an `identifier` node instead of `boolean_literal`/`none_literal`. Consequences: wrong highlighting (the value loses its `@constant.builtin` capture from `highlights.scm` and is instead treated as a `@local.reference` identifier), and the tree-sitter parse tree diverges from the compiler's accepted token set on valid input.

Note: the glyph-core compiler is unaffected — it uses its own tokenizer with `to_ascii_lowercase()` and `eq_ignore_ascii_case()` — so this is a tree-sitter editor-tooling parity gap only.

## Trigger / Reproduction

Write a `.glyph` file containing a condition like `if mode != None` or a binding like `result = True`. Open the file in an editor using nvim-treesitter (or run `tree-sitter highlight`). The `None`/`True` token receives `@local.reference` (identifier) highlighting instead of `@constant.builtin`. The corpus test at `test/corpus/value_bindings.txt:32` only tests lowercase, so this is untested.

## Evidence

```js
// tree-sitter-glyph/grammar.js lines 651-652
boolean_literal: (_) => choice("true", "false"),
none_literal: (_) => "none",

// design/values-and-names.md:107 — 'true, True, and TRUE are all accepted'
// design/values-and-names.md:121 — 'none, None, and NONE are all accepted'

// identifier rule (line 655) also matches True/NONE/None:
identifier: (_) => /[a-zA-Z_][a-zA-Z0-9_]*/,
```

## Recommended Resolution

Make the tokens case-insensitive to match the spec. Use `token(prec(1, ...))` wrappers so the regex-based rules take priority over `identifier` (string literals get automatic keyword-extraction priority, but regex tokens do not):

```js
boolean_literal: (_) => token(prec(1, /[Tt][Rr][Uu][Ee]|[Ff][Aa][Ll][Ss][Ee]/)),
none_literal: (_) => token(prec(1, /[Nn][Oo][Nn][Ee]/)),
```

Add corpus cases for `True`/`NONE`/`False`. Verify the generated parser has no ambiguity with the `identifier` rule by running the tree-sitter corpus tests after the change.

## Verification Notes

The grammar.js discrepancy is real: lines 651-652 define `boolean_literal` and `none_literal` as lowercase-only strings, while `design/values-and-names.md` lines 107 and 121 explicitly specify case-insensitive acceptance. The glyph-core compiler correctly handles booleans case-insensitively at `parse.rs` line 4678 (`to_ascii_lowercase`) and `none` via `eq_ignore_ascii_case`. The glyph-lsp also uses glyph-core's semantic_tokens module for highlighting, not the grammar's `highlights.scm`. The actual impact is confined to tree-sitter-native syntax highlighting in editors that consume the grammar directly (e.g., Neovim nvim-treesitter), where `True`/`NONE`/`False` would parse as identifier nodes and miss the `@constant.builtin` capture — a cosmetic highlighting issue. Severity is correctly low given the compiler and primary LSP path are entirely unaffected.

## Independent Agent Finding

**Verdict:** Reproduced. The tree-sitter grammar remains lowercase-only for `boolean_literal` and `none_literal`, while the Rust compiler path accepts the case variants covered by the spec.

**Reproduction/Refutation:** A temporary flow fixture with lowercase and case-variant literals side by side showed the reported tree-sitter classification gap. In `tree-sitter parse ../tmp/bug084-case-literals.glyph`, lowercase `true` parsed as `value: (boolean_literal)` and lowercase `none` parsed as `value: (none_literal)`, but `True`, `None`, and `NONE` parsed as `value: (identifier)` or condition-side `(identifier)` nodes. `tree-sitter query -c queries/highlights.scm ../tmp/bug084-case-literals.glyph` captured only lowercase `true`/`none` as `constant.builtin`; the case variants had no literal highlight capture. `tree-sitter query -c queries/locals.scm ../tmp/bug084-case-literals.glyph` captured `True`, `None`, and `NONE` as `local.reference`. The repo's existing compiler fixture `crates/glyph-cli/tests/corpus/valid/const_bool_uppercase.glyph` also reproduced a stronger tree-sitter symptom: `tree-sitter parse ../crates/glyph-cli/tests/corpus/valid/const_bool_uppercase.glyph` exited 1 with an `ERROR` node around `const verbose_mode = True`, because const RHS does not accept a plain identifier fallback.

**Evidence:** Bounded reads confirmed `tree-sitter-glyph/grammar.js` still has `boolean_literal: (_) => choice("true", "false")`, `none_literal: (_) => "none"`, and `identifier: (_) => /[a-zA-Z_][a-zA-Z0-9_]*/`. Graphify also pointed at the generated grammar nodes for `boolean_literal` and `none_literal`. The design text still says booleans and none are source case-insensitive. Compiler-side checks stayed green: `cargo run -q -p glyph-cli -- check crates/glyph-cli/tests/corpus/valid/const_bool_uppercase.glyph --format json` exited 0 with no output, `cargo test -p glyph-cli --test const_kinds const_bool_uppercase_normalizes_to_lowercase -- --exact` passed 1/1, and `cargo test -p glyph-cli --test typed_decl_missing_return typed_skill_return_capital_none_fires -- --exact` passed 1/1. These results support the report's conclusion that this is tree-sitter editor/tooling parity, not a glyph-core compiler failure.

**Resolution Input:** Preserve the existing suggested resolution. The `token(prec(1, ...))` case-insensitive regex approach should make `True`/`False`/`NONE` win over `identifier` and restore `@constant.builtin` captures. Add corpus coverage for both flow binding/condition cases and the existing valid compiler const case (`const verbose_mode = True`), because the latter currently produces a tree-sitter `ERROR` rather than just identifier misclassification.
