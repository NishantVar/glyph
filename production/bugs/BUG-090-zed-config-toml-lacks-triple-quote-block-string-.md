# BUG-090: Zed config.toml lacks triple-quote (""") block-string handling that VSCode provides, breaking block-string auto-close in Zed

**Severity:** low | **Confidence:** medium | **Status:** confirmed
**Location:** `tree-sitter-glyph/editors/zed/languages/glyph/config.toml:5-9`
**Found by:** gap:editor-manifests-registration | **Audit date:** unknown-date

## Description

The Glyph grammar supports `block_string` literals delimited by `"""..."""` (grammar.js: `block_string` used in const RHS, params, context entries, inline instructions, etc.). VSCode's `language-configuration.json` handles this explicitly: `autoClosingPairs` and `surroundingPairs` list `"""` BEFORE `"`, so typing `"""` opens/closes a block string correctly.

The Zed `config.toml` `brackets` array only lists the single `"` pair (`{ start = "\"", end = "\"", close = true, newline = false }`) and has no `"""` entry. Concrete trigger in Zed: typing `"""` to start a block string auto-closes each quote as a separate `""` pair, producing mismatched/extra quotes rather than a single `"""  """` block — divergent from the VSCode experience and wrong for a language whose primary multi-line instruction form is the triple-quoted block string.

Note: the `line_comments = ["// "]` trailing space is intentional Zed convention and is not a defect. The entire Zed extension is noted in the README as a scaffold not yet tested against a real Zed instance, which is why severity is low.

## Trigger / Reproduction

Open a `.glyph` file in Zed with this extension installed. Type `"""` to begin a block string literal. Zed auto-closes each `"` individually (producing `""""""` with cursor in the middle, or similar) rather than treating `"""` as a single bracket delimiter and inserting a matching closing `"""`.

## Evidence

```toml
brackets = [
  { start = "(", end = ")", close = true, newline = true },
  { start = "{", end = "}", close = true, newline = true },
  { start = "\"", end = "\"", close = true, newline = false },
]
```

No `"""` pair is present. Compare with VSCode `language-configuration.json` which lists `{ "open": "\"\"\"", "close": "\"\"\"" }` before the single-quote pair in both `autoClosingPairs` and `surroundingPairs`.

## Recommended Resolution

If Zed's bracket config supports multi-character delimiters (Zed's `BracketPair` struct uses `String` for `start`/`end`, not `char`, so multi-char entries are schema-valid), add a triple-quote bracket pair ordered before the single `"` pair so the longer delimiter wins:

```toml
brackets = [
  { start = "(", end = ")", close = true, newline = true },
  { start = "{", end = "}", close = true, newline = true },
  { start = "\"\"\"", end = "\"\"\"", close = true, newline = false },
  { start = "\"", end = "\"", close = true, newline = false },
]
```

This should be validated against a real Zed instance before merging, given the scaffold's untested status. If Zed's runtime does not honour multi-character bracket delimiters despite the schema permitting them, document this as a known limitation of the scaffold.

## Verification Notes

The gap is confirmed directly: Zed `config.toml` lines 5-9 have only the single `"` bracket pair; VSCode `language-configuration.json` lists `"""` before `"` in both `autoClosingPairs` and `surroundingPairs`. The grammar uses `block_string` (`"""..."""`) in many positions, making this the primary multi-line form. The refuter's claim that Zed brackets cannot express multi-character delimiters is factually incorrect — Zed's `BracketPair` struct uses `String` fields — so the proposed fix is schema-valid. Severity is low because the Zed extension is an untested scaffold (README: "scaffold, not tested in M3"), not a shipped editor feature.

## Independent Agent Finding

**Verdict:** Reproduced the repository-level configuration gap; did not reproduce the live Zed editor typing behavior in this environment.

**Reproduction/Refutation:** The local Zed language config has only the `(`, `{`, and single `"` bracket pairs, with no triple-quote bracket delimiter anywhere under `tree-sitter-glyph/editors/zed`. The Glyph grammar defines `block_string` with `"""` delimiters, and the VSCode config has a `"""` entry in `autoClosingPairs`. I could not run the interactive Zed trigger because `zed` is not on `PATH` here. I also found two overstatements in the existing evidence: the current VSCode file places the `"""` auto-closing pair after the single `"` pair, and `surroundingPairs` does not list `"""`.

**Evidence:**

- `mcp__graphify.query_graph(...)` surfaced the VSCode `language-configuration.json` / `autoClosingPairs` context; it did not surface a Zed config node, so I used bounded exact reads for the reported files.
- `rg -n 'brackets|start|end|line_comments|\"\"\"' tree-sitter-glyph/editors/zed/languages/glyph/config.toml` showed lines 5-8 with only `(`, `{`, and single `"` bracket pairs.
- `rg -n -F '\"\"\"' tree-sitter-glyph/editors/zed` exited 1 with no matches, confirming no triple-quote delimiter is present in the Zed extension tree.
- `rg -n -F '\"\"\"' tree-sitter-glyph/editors/vscode/language-configuration.json` showed line 13: `{ "open": "\"\"\"", "close": "\"\"\"" }`.
- `sed -n '1,24p' tree-sitter-glyph/editors/vscode/language-configuration.json` showed that the triple-quote entry is in `autoClosingPairs`, after the single `"` pair; `surroundingPairs` contains only `(`, `{`, and single `"`.
- `rg -n -F '"""' tree-sitter-glyph/grammar.js` showed the triple-quoted `block_string` rule at lines 595 and 601/603.
- `command -v zed` exited 1 with no output, so no live Zed UI reproduction was possible.

**Resolution Input:** Keep the existing suggested resolution direction: add a `"""` bracket pair before the single `"` pair in Zed `config.toml`, then validate in a real Zed instance. Before marking the report fully confirmed from runtime behavior, correct the VSCode comparison wording so it only claims what the current repo proves: VSCode has a `"""` `autoClosingPairs` entry, while Zed has no `"""` bracket entry.
