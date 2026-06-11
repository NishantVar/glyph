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

## Independent Agent Finding

### Verdict

Reproduced at the configuration-shape level. The repo file declares no top-level `[[grammar]]` entry; TOML parsing places `name` and `source` under `language[0].grammar` instead. I could not run the exact `hx --grammar fetch && hx --grammar build` repro locally because `hx` is not installed on this machine.

### Reproduction/Refutation

Parsed `tree-sitter-glyph/editors/helix/languages.toml` with Python's standard `tomllib` and confirmed the top-level table set contains only `language`; `grammar` exists only as a nested table under the single `[[language]]` entry. As a control, replacing only `[language.grammar]` with `[[grammar]]` makes the parsed document contain one top-level `grammar` array entry named `glyph` and removes the nested language `grammar` table.

### Evidence

- `rg -n "language\\.grammar|\\[\\[grammar\\]\\]|\\[\\[language\\]\\]|source =" tree-sitter-glyph/editors/helix/languages.toml`
  - Output summary: line 4 is `[[language]]`; line 12 is `[language.grammar]`; no `[[grammar]]` entry is present.
- `command -v hx && hx --version`
  - Output summary: exit 1 with no output, so direct Helix CLI reproduction is unavailable in this environment.
- `python3` / `tomllib` parse of the current file:
  - Output summary: `top-level keys: ['language']`, `top-level grammar entries: 0`, `language name: glyph`, `language grammar value type: dict`, `language grammar keys: ['name', 'source']`.
- `python3` / `tomllib` parse after replacing `[language.grammar]` with `[[grammar]]` in-memory:
  - Output summary: `top-level keys: ['grammar', 'language']`, `top-level grammar entries: 1`, `grammar name: glyph`, `language grammar key present: False`.
- The Helix language README in this repo instructs users to append this snippet and run `hx --grammar fetch` / `hx --grammar build`, and separately notes that the current block uses a placeholder URL.
- Current Helix docs state that a language's tree-sitter grammar source is specified in a top-level `[[grammar]]` section in `languages.toml`: https://docs.helix-editor.com/languages.html

### Resolution Input

Keep the existing suggested resolution: change `[language.grammar]` to a separate top-level `[[grammar]]` array entry while preserving the existing `name` and `source` keys. This converts the file from a nested language sub-table into the shape Helix documents for grammar fetch/build configuration.
