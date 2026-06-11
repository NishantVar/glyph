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

## Independent Agent Finding

**Verdict:** Reproduced / confirmed.

**Reproduction/Refutation:** Parsed `tree-sitter-glyph/package.json` and inspected its legacy `tree-sitter[0]` registration entry. It registers `scope`, `file-types`, and `highlights` only; `locals` and `injections` are absent from the manifest entry even though `queries/highlights.scm`, `queries/locals.scm`, and `queries/injections.scm` all exist under `tree-sitter-glyph/`. This reproduces the reported package-manifest gap for any consumer that configures query loading from `package.json` rather than scanning the `queries/` directory.

**Evidence:** Graphify was queried first for the relevant tree-sitter package/query context, then exact bounded checks were run. `nl -ba tree-sitter-glyph/package.json | sed -n '1,40p'` showed the `tree-sitter` block at lines 14-20 with only `"highlights": "queries/highlights.scm"`. `jq -r '."tree-sitter"[0] | keys_unsorted[]' tree-sitter-glyph/package.json` output `scope`, `file-types`, `highlights`. A targeted Node reproduction printed `registered=scope,file-types,highlights`, `missing=locals,injections`, and `files=queries/highlights.scm:true,queries/locals.scm:true,queries/injections.scm:true`. `rg -n '@(local\.|scope|definition|reference)' tree-sitter-glyph/queries/locals.scm` confirmed real locals captures such as `@local.scope`, `@local.definition.constant`, `@local.definition.function`, `@local.definition.import`, `@local.definition.parameter`, `@local.definition.var`, and `@local.reference`. `nl -ba tree-sitter-glyph/queries/injections.scm | sed -n '1,80p'` showed the injections query file is comment-only and intentionally declares no injections.

**Resolution Input:** Preserve the existing recommended resolution: add `"locals": "queries/locals.scm"` and `"injections": "queries/injections.scm"` to the `package.json` `tree-sitter` block so the legacy manifest matches the shipped query files. The current report's functional assessment remains low severity; the only nuance found independently is that `injections.scm` is not zero-byte empty, but is semantically empty because it contains only explanatory comments.
