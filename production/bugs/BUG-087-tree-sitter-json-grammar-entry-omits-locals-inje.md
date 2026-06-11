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

## Independent Agent Finding

**Verdict:** Refuted for the current local `tree-sitter` CLI path. The static manifest omission is real, but I could not reproduce the claimed behavior that `tree-sitter highlight` ignores `queries/locals.scm` or `queries/injections.scm` when those keys are absent from `tree-sitter.json`. With `tree-sitter 0.26.8`, the CLI loads the conventional `queries/locals.scm` and `queries/injections.scm` files from the grammar directory.

**Reproduction/Refutation:**

- Confirmed `tree-sitter-glyph/tree-sitter.json` and `tree-sitter-glyph/package.json` only declare `"highlights": "queries/highlights.scm"`, while both `queries/locals.scm` and `queries/injections.scm` exist.
- Baseline from `tree-sitter-glyph`: `tree-sitter highlight --quiet ../.agents/commands/glyph/compile.glyph` returned `rc=0`, with highlighted output and empty stderr.
- Scratch grammar copy with unchanged manifest but malformed `queries/locals.scm`: `tree-sitter highlight --quiet ../../../.agents/commands/glyph/compile.glyph` returned `rc=1` and reported a query parse error at line 175, immediately after the 174-line `highlights.scm`. If the CLI ignored `locals.scm`, this would have succeeded.
- Controls: moving `queries/locals.scm` away returned `rc=0`; malformed `queries/injections.scm` returned `rc=1`; malformed `queries/notloaded.scm` returned `rc=0`. This indicates the CLI auto-loads known conventional query filenames (`locals.scm`, `injections.scm`) rather than every `.scm` file.

**Evidence:**

```text
tree-sitter --version
tree-sitter 0.26.8

tree-sitter-glyph/tree-sitter.json:
  "highlights": "queries/highlights.scm"

tree-sitter-glyph/package.json tree-sitter block:
  "highlights": "queries/highlights.scm"

bad locals scratch run:
  rc=1
  Error in query file queries/highlights.scm:
  Query error at 175:1. Invalid syntax:
  Unexpected EOF

controls:
  no locals file: rc=0
  malformed injections.scm: rc=1
  malformed notloaded.scm: rc=0
```

**Resolution Input:** Preserve the existing suggested resolution as a harmless manifest-completeness improvement, but do not keep this report confirmed as written for `tree-sitter highlight` on CLI 0.26.8. If the production concern is a specific `tree-sitter-loader` consumer or older CLI version that reads only explicit manifest keys, reproduce against that exact tool/version and record the command output; otherwise close or downgrade this to a packaging completeness issue rather than a confirmed CLI loader bug.
