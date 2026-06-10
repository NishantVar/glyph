# BUG-085: package.json 'tree-sitter' block registers only highlights, omitting locals/injections

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `tree-sitter-glyph/package.json:14-20`
**Found by:** gap:editor-manifests-registration | **Audit date:** unknown-date

## Description

The legacy `tree-sitter` registration block (consumed by node bindings / older Atom-style tree-sitter tooling) lists only `"highlights": "queries/highlights.scm"` and never references `queries/locals.scm` or `queries/injections.scm`, even though both files exist and `locals.scm` carries real scope-tracking captures (scope, definition, reference captures for constants, callables, imports, parameters, and flow-local bindings).

Any consumer that loads query configuration from this block (rather than scanning the `queries/` directory itself) will not apply locals or injections. This is the same root cause and consumer-divergence as the `tree-sitter.json` gap (BUG-087); both manifests are internally inconsistent with the shipped queries directory.

Note: modern editors (nvim-treesitter, Helix, Zed) scan the `queries/` directory directly and ignore these manifest keys entirely; only older Atom-style tooling and legacy node-binding consumers are affected.

## Trigger / Reproduction

Any tool that reads `package.json`'s `tree-sitter` block to configure query loading will apply `highlights.scm` but silently skip `locals.scm` and `injections.scm`. This causes scope-aware editor features (rename, jump-to-definition, unused-local dimming) to be absent for those consumers, even though the query files are physically present.

## Evidence

```json
"tree-sitter": [
  {
    "scope": "source.glyph",
    "file-types": ["glyph"],
    "highlights": "queries/highlights.scm"
  }
]
```

`queries/locals.scm` and `queries/injections.scm` both exist on disk but are not referenced.

## Recommended Resolution

Add `"locals": "queries/locals.scm"` and `"injections": "queries/injections.scm"` to the `tree-sitter` block entry to match the shipped queries directory and the sibling `tree-sitter.json` (once BUG-087 is also fixed):

```json
"tree-sitter": [
  {
    "scope": "source.glyph",
    "file-types": ["glyph"],
    "highlights": "queries/highlights.scm",
    "locals": "queries/locals.scm",
    "injections": "queries/injections.scm"
  }
]
```

## Verification Notes

The facts are confirmed: `package.json` lines 14-20 register only `"highlights": "queries/highlights.scm"` while `queries/locals.scm` (with real scope-tracking captures) and `queries/injections.scm` (intentionally empty but present for editor compatibility) both exist. The same omission exists in `tree-sitter.json`. Severity is low rather than medium: modern editors (nvim-treesitter, Helix) scan the `queries/` directory directly and ignore these manifest keys entirely; only older Atom-style tooling and legacy node-binding consumers read the `tree-sitter` block in `package.json`. Since `injections.scm` is intentionally empty, its omission from the manifest has zero functional impact regardless of consumer. The proposed fix is correct and complete.
