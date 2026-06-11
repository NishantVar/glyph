# BUG-045: Inline string_literal cannot tokenize escape sequences (\\" and \\\\), producing ERROR nodes on valid strings

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `tree-sitter-glyph/grammar.js:586-593`
**Found by:** treesitter | **Audit date:** unknown-date

## Description

The inline `string_literal` rule is `seq('"', repeat(choice($.interpolation, $.string_content)), '"')` and `string_content` is `token.immediate(prec(1, /[^"\\{]+/))` — the character class explicitly excludes backslash, and there is NO escape-sequence alternative in the `repeat`. The Glyph spec explicitly allows escapes in inline strings: `GLYPH_LANGUAGE_GUIDE.md` line 928 says "Escapes: `\"` and `\\` only."

By contrast, the sibling `block_string_content` rule (line 627) DOES include `/\\./` to consume escape sequences.

Concrete trigger: tokenizing `"say \"hi\""` — after the opening quote, `string_content` matches `say `, then hits `\` which it cannot consume and there is no `\\.` alternative. The `"` of `\"` is taken as the closing quote (yielding string `"say "`), leaving a stray `\` and `hi\""` as garbage → ERROR node / wrong parse tree. Same failure for `"a\\b"` (literal backslash) and `"path\\to\\file"`. The editor highlighting and any tree-sitter-based tooling diverge from the compiler on valid input.

No corpus test exercises an escaped inline string (`test/corpus/multi_line_and_block_strings.txt` has none), so this is untested.

## Trigger / Reproduction

Parse a `.glyph` file containing an inline string with an escape:

```
skill test()
    description: "test"
    flow:
        "say \"hi\""
```

Tree-sitter produces an ERROR node in the flow section. The `glyph-core` compiler (which does not use tree-sitter) compiles the same input cleanly with exit 0.

## Evidence

```javascript
// tree-sitter-glyph/grammar.js lines 586-593
string_literal: ($) =>
  seq(
    '"',
    repeat(choice($.interpolation, $.string_content)),
    '"',
  ),

string_content: (_) => token.immediate(prec(1, /[^"\\{]+/)),
// excludes backslash; no escape-sequence alternative

// vs block_string_content (line 622-632) which correctly handles escapes:
block_string_content: (_) =>
  token.immediate(
    prec(1,
      choice(
        /[^"{\\]+/,
        /\\./,          // <-- escape sequences handled here
        /"[^"{\\]/,
        /""[^"{\\]/,
      ),
    ),
  ),
```

## Recommended Resolution

Add an escape alternative to the `string_literal` repeat, mirroring `block_string_content`. Either add a `string_escape` node:

```javascript
string_escape: (_) => token.immediate(/\\["\\]/),
```

to the `choice` in the `string_literal` repeat (line 589), restricting to the two spec-allowed escapes, or add `/\\./` as a sibling alternative inside `string_content`. Also add a corpus test for `"a \" b \\ c"` in `test/corpus/`.

## Verification Notes

Running `tree-sitter parse` on a file containing `"say \"hi\""` produces `ERROR` nodes — confirmed end-to-end. The `glyph-core` compiler has its own handwritten tokenizer in `tokenize.rs` and does not depend on tree-sitter at all (no tree-sitter in any workspace `Cargo.toml`). The impact is limited to editor syntax highlighting and tree-sitter-based tooling (Neovim/Helix/VS Code/Zed tree-sitter plugins) producing spurious ERROR nodes for valid `.glyph` files. The LSP (`glyph-lsp`) uses `glyph_core::check_source_with_resolutions`, not tree-sitter, so compiled output and LSP diagnostics are unaffected.

## Independent Agent Finding

**Verdict:** Reproduced.

**Reproduction/Refutation:** I reproduced the tree-sitter failure with both escape forms named in the report. A scratch fixture containing the flow step `"say \"hi\""` parsed with `tree-sitter parse -p tree-sitter-glyph tmp/bug045-inline-escape.glyph` exited 1 and produced an `ERROR [3, 8] - [3, 15]` around the inline string. A second scratch fixture containing `"a\\b"` parsed with `tree-sitter parse -p tree-sitter-glyph tmp/bug045-inline-backslash.glyph` also exited 1 and produced `ERROR [3, 10] - [3, 13]`. The same two scratch fixtures were accepted by the Rust compiler path via `cargo run -p glyph-cli -- check ...`, both exiting 0.

**Evidence:**

- Graphify lookup for `tree-sitter-glyph grammar string_literal string_content block_string_content escape sequences inline strings` pointed to the generated grammar nodes for `string_literal`, `string_content`, and `block_string_content`, matching the report's claimed implementation area.
- `tree-sitter-glyph/grammar.js` still has `string_literal` repeating only `choice($.interpolation, $.string_content)`, while `string_content` is `token.immediate(prec(1, /[^"\\{]+/))`; backslash is excluded and there is no inline escape alternative.
- The sibling `block_string_content` rule includes `/\\./`, so block strings have an escape-consuming branch that inline strings lack.
- `GLYPH_LANGUAGE_GUIDE.md` section 9.1 says inline strings allow `\"` and `\\`, so the tree-sitter grammar rejects spec-valid source.
- `rg -n "tree-sitter" Cargo.toml crates/glyph-core/Cargo.toml crates/glyph-cli/Cargo.toml crates/glyph-lsp/Cargo.toml tree-sitter-glyph/package.json` matched only `tree-sitter-glyph/package.json`, confirming the Rust workspace manifests do not depend on tree-sitter.
- `crates/glyph-core/src/tokenize.rs` handles `\"` and `\\` in the inline string scanner, which explains why `glyph check` accepts the same inputs.

**Resolution Input:** Preserve the existing suggested resolution. The preferred fix is to add an inline-only escape token such as `string_escape: (_) => token.immediate(/\\["\\]/)` and include it in the `string_literal` repeat, because that matches the language guide's two allowed inline escapes more precisely than `/\\./`. Add tree-sitter corpus coverage for both escaped quote and escaped backslash inline strings, for example `"a \" b \\ c"`, so this editor/tooling regression is caught directly.
