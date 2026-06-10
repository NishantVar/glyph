# BUG-082: Helix grammar declared as [language.grammar] sub-table instead of top-level [[grammar]] array entry

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `tree-sitter-glyph/editors/helix/languages.toml:12-14`
**Found by:** vscode | **Audit date:** unknown-date

## Description

Helix's languages.toml schema declares grammar sources as top-level array-of-tables entries (`[[grammar]]` with `name` + `source` keys), separate from the `[[language]]` entries. This file instead nests the grammar source under `[language.grammar]`, which in TOML is a sub-table nested under the preceding `[[language]]` array entry — not a top-level `[[grammar]]` entry.

Helix's grammar fetcher does not read this form. Concrete consequence: after appending this snippet and running `hx --grammar fetch` / `hx --grammar build` (the exact steps described in the helix README), Helix finds no grammar source for "glyph" and fails to fetch/build the parser, so highlighting never loads.

The block is gated behind PLACEHOLDER url/rev so it is non-functional until edited anyway, hence low severity, but the structural form is wrong and would still fail even after the placeholders are replaced.

## Trigger / Reproduction

Append the file contents to `~/.config/helix/languages.toml`, replace the PLACEHOLDER values with a real repo URL and commit SHA, then run `hx --grammar fetch && hx --grammar build`. Helix's grammar fetcher scans for `[[grammar]]` entries; the `[language.grammar]` sub-table is not recognized and is silently ignored, so the grammar is never fetched and highlighting is never installed.

## Evidence

```toml
[[language]]
name = "glyph"
scope = "source.glyph"
file-types = ["glyph"]
comment-token = "//"
indent = { tab-width = 4, unit = "    " }
roots = []

[language.grammar]
name = "glyph"
source = { git = "https://github.com/PLACEHOLDER/tree-sitter-glyph", rev = "PLACEHOLDER_COMMIT_SHA" }
```

## Recommended Resolution

Change the table header from `[language.grammar]` to a top-level `[[grammar]]` array entry (keeping `name` + `source`) so Helix's grammar fetcher recognizes the source:

```toml
[[grammar]]
name = "glyph"
source = { git = "https://github.com/PLACEHOLDER/tree-sitter-glyph", rev = "PLACEHOLDER_COMMIT_SHA" }
```

## Verification Notes

The file at lines 12-14 uses `[language.grammar]` which in TOML is a sub-table nested under the preceding `[[language]]` array entry, not a top-level `[[grammar]]` array entry. Helix's languages.toml schema requires grammar sources to be declared as separate top-level `[[grammar]]` entries (with `name` and `source` keys), which is what the grammar fetcher reads when running `hx --grammar fetch`. The `[language.grammar]` form is not recognized by Helix's grammar fetcher and would silently be ignored, causing fetch/build to fail. The bug is real and the structural form is definitively wrong. Severity is low because the file is a user-appendable template with PLACEHOLDER values that make it non-functional regardless, and it only affects optional editor integration rather than the compiler.
