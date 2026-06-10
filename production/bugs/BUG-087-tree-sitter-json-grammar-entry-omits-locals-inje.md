# BUG-087: tree-sitter.json grammar entry omits locals/injections, so CLI-based consumers never load locals.scm

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `tree-sitter-glyph/tree-sitter.json:2-11`
**Found by:** gap:editor-manifests-registration | **Audit date:** unknown-date

## Description

The `grammars[0]` entry registers only `"highlights": "queries/highlights.scm"` and never declares the `locals` or `injections` query paths. The grammar ships a non-trivial `queries/locals.scm` (scope + definition/reference captures for constants, callables, imports, parameters, and flow-local bindings) and an intentional empty `queries/injections.scm`.

The tree-sitter CLI loader (`tree-sitter highlight`, and the `tree-sitter-loader` crate used by some tooling) reads these explicit paths from `tree-sitter.json`; it does NOT auto-discover other `.scm` files in the `queries/` dir. Concrete trigger: running `tree-sitter highlight foo.glyph` applies `highlights.scm` but silently ignores `locals.scm` and `injections.scm`, so local-scope features and any locals-derived highlight precedence are lost for those consumers.

Note: Helix, Zed, and nvim-treesitter are unaffected because they load queries by scanning the runtime `queries/` directory by convention rather than via `tree-sitter.json`.

## Trigger / Reproduction

Run `tree-sitter highlight foo.glyph` (or any tool that configures query loading from `tree-sitter.json`). The tool applies `highlights.scm` but silently skips `locals.scm` and `injections.scm`. Scope-aware features (rename, jump-to-definition, unused-variable dimming) are absent even though the query files are physically present.

## Evidence

```json
{
  "grammars": [
    {
      "name": "glyph",
      "camelcase": "Glyph",
      "scope": "source.glyph",
      "path": ".",
      "file-types": ["glyph"],
      "highlights": "queries/highlights.scm"
    }
  ]
}
```

`queries/locals.scm` (64 lines, non-trivial scope/definition/reference captures) and `queries/injections.scm` (intentionally empty, present for compatibility) both exist but are not referenced.

## Recommended Resolution

Add the missing query paths to the grammar entry so CLI-based tree-sitter consumers load the same query set the editors do:

```json
"highlights": "queries/highlights.scm",
"locals": "queries/locals.scm",
"injections": "queries/injections.scm"
```

Apply the same fix to `package.json`'s `tree-sitter` block (BUG-085), which has the identical omission.

## Verification Notes

The omission is confirmed: `tree-sitter.json` (line 9) only declares `"highlights": "queries/highlights.scm"` with no `locals` or `injections` keys, while both `queries/locals.scm` (64 lines, with scope/definition/reference captures) and `queries/injections.scm` (intentionally empty, 13-line comment) exist. No ADR or design doc marks this intentional. Severity is low rather than medium: the affected consumers (raw `tree-sitter highlight` CLI invocations) are only a developer workflow convenience listed in the README. The actual editors (Helix, Zed, nvim) all scan the queries directory by convention and are unaffected. The practical impact is limited to developers running `tree-sitter highlight` directly or embedding the grammar via tree-sitter-loader, and the missing `locals.scm` only affects editor features like rename/jump-to-definition rather than correctness of highlighting output.
