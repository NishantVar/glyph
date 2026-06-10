# Rejected Bug Candidates

Candidate findings that adversarial verification refuted. Kept so future audits do not re-litigate them.

---

## `_effects_stub` cannot parse the documented `effects: none` form

**Location:** `tree-sitter-glyph/grammar.js` lines 257-258

**Claim:** The transitional `_effects_stub` rule is `seq("effects", ":", commaSep1($.identifier), $._newline)` — it only accepts a comma-separated list of `identifier` nodes. But GLYPH_LANGUAGE_GUIDE.md line 944 documents `effects: none` as valid syntax.

**Refutation:** Split verdict settled by tiebreaker: reproducing directly with the known-good corpus file `repo_tools.glyph` (which parses cleanly with `effects: reads_files`) and swapping in `effects: none`, `tree-sitter parse` succeeds with exit 0 and zero ERROR nodes; the value `none` is lexed as `(identifier [1,13]-[1,17])`, not as `none_literal`. The bug's premise — that tree-sitter keyword extraction fails — is false.

---

## Declared `engines.vscode ^1.75.0` is incompatible with `vscode-languageclient@^9` (requires `^1.82.0`)

**Location:** `editors/vscode/package.json` lines 7-9

**Claim:** The shipping extension declares `"engines": { "vscode": "^1.75.0" }` (and `"@types/vscode": "^1.75.0"`), but its only runtime dependency, `"vscode-languageclient": "^9.0.1"`, declares a higher minimum vscode engine, causing `vsce package` to abort.

**Refutation:** Split verdict settled by tiebreaker: The bug's central trigger — that `vsce package` aborts because it validates a runtime dependency's `engines.vscode` constraint against the extension's own engines field — is false. Tracing the actual installed `@vscode/vsce` 3.9.1 code, `validateManifestForPackaging` (out/package.js:936-1004) performs no such cross-check; the only engine validations are against the extension manifest itself, not its dependencies.

---

## Semantic tokens emit byte offsets/lengths instead of UTF-16 code units

**Location:** `crates/glyph-core/src/semantic_tokens.rs` lines 1039-1092

**Claim:** `push_span` builds `RawSemToken` `start` and `length` from byte columns/lengths, not UTF-16 code units. `scol` comes from `LineIndex::line_col` (a byte column) and the values are passed through without conversion.

**Refutation:** While `push_span` itself uses byte columns from `line_col` and byte lengths, this is not the end of the pipeline. `collect_semantic_tokens` (the sole caller of all token-collection passes) calls `to_utf16_columns(&mut tokens, source)` immediately after `sort_and_dedup`, before returning — performing the required byte-to-UTF-16 conversion at the collection boundary.

---

## Grammar accepts leading-zero integers (`03`) that the compiler rejects

**Location:** `tree-sitter-glyph/grammar.js` lines 649-650

**Claim:** The tokenizer rejects multi-digit numeric literals whose integer part starts with `0` (tokenize.rs lines 354-358, `LeadingZeroNumeric`), per `design/values-and-names.md` §Integers, but the grammar regex `/-?[0-9]+/` accepts them, creating a parse/compile mismatch that is a layered-defense gap rather than a user-visible bug.

**Refutation:** Split verdict settled by tiebreaker: Both verifiers agree on the mechanics — grammar.js accepts `03`/`01.5` while glyph-core's tokenizer rejects them via `LeadingZeroNumeric`. The dispute is only whether this gap constitutes a bug; the tiebreaker ruled it does not, as the compiler-layer rejection is the intended enforcement point and the grammar-layer is a known permissive front-end.

---

## Global `*.ir.json` ignore is intended (IR is a regenerated build artifact), not a packaging bug

**Location:** `.gitignore` line 18

**Claim:** The `*.ir.json` ignore on line 18 means no IR sidecar is tracked, which was reported as a potential packaging or distribution gap.

**Refutation:** The claimed bug is self-described as "not a bug" in its own verdict. The `.gitignore` `*.ir.json` entry is intentional: IR sidecars are regenerated build artifacts emitted by `glyph compile`, confirmed by three independent sources — `audit.md` gracefully stops and prompts the user to run `/glyph:compile` first when a sidecar is absent, and `icompile.md` reads the sidecar only when present.
