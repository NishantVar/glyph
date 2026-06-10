# BUG-083: Zed extension.toml ships literal PLACEHOLDER grammar repo/commit with no README instruction to replace them

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `tree-sitter-glyph/editors/zed/extension.toml:7-11`
**Found by:** gap:editor-manifests-registration | **Audit date:** unknown-date

## Description

`extension.toml` declares `repository = "https://github.com/PLACEHOLDER/glyph"` and `[grammars.glyph] repository = "https://github.com/PLACEHOLDER/tree-sitter-glyph"` / `commit = "PLACEHOLDER_COMMIT_SHA"`. A Zed install built from this manifest cannot resolve the grammar (the repo host literally does not exist and the commit is not a SHA).

Unlike the helix README — whose "Why this is only a scaffold" section explicitly states "The [language.grammar] block ... uses a PLACEHOLDER URL — replace it with the real grammar repository before running hx --grammar fetch" — the Zed README (`editors/zed/README.md`) never tells the user the PLACEHOLDER `repository`/`commit` must be replaced; it only discusses symlinks and the Zed extension-index packaging.

A user who follows the Zed README will package/install a manifest that fails to fetch the grammar, with no in-repo signal that the placeholders are the blocker. This is a doc-vs-artifact divergence relative to the helix path.

## Trigger / Reproduction

Follow `editors/zed/README.md` to install the extension locally. The manifest is loaded by Zed with the PLACEHOLDER values intact. Grammar fetch fails because `https://github.com/PLACEHOLDER/tree-sitter-glyph` does not resolve and `PLACEHOLDER_COMMIT_SHA` is not a valid SHA. No README note points to the placeholders as the cause.

## Evidence

```toml
repository = "https://github.com/PLACEHOLDER/glyph"

[grammars.glyph]
repository = "https://github.com/PLACEHOLDER/tree-sitter-glyph"
commit = "PLACEHOLDER_COMMIT_SHA"
```

## Recommended Resolution

Mirror the helix README: add an explicit note in `editors/zed/README.md` (and/or a comment in `extension.toml`) that the PLACEHOLDER `repository` values and `commit` SHA must be replaced with the real published grammar repo + tagged commit before the extension can resolve the grammar.

## Verification Notes

The PLACEHOLDERs in `extension.toml` (lines 7, 10, 11) are confirmed present. The Zed README at `editors/zed/README.md` describes the scaffold status and packaging steps but never mentions that the PLACEHOLDER `repository` and `commit` values must be replaced — unlike the Helix README which contains the explicit warning. This is a real doc-vs-artifact divergence: a user following the Zed README would package a manifest that cannot resolve the grammar, with no in-repo signal pointing to the PLACEHOLDERs as the cause. Severity is low rather than medium because the README already calls the directory a "scaffold" and explicitly states CI/publishing steps have not been done, giving a reasonable (if incomplete) signal that the extension is not yet usable.
