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

## Independent Agent Finding

**Verdict:** Reproduced. The report is valid as written.

**Reproduction/Refutation:** From the repository root, the Zed manifest at `tree-sitter-glyph/editors/zed/extension.toml` still ships literal placeholder values for the extension repository, grammar repository, and grammar commit. The Zed README does describe the directory as a scaffold and explains symlink/packaging steps, but it does not mention `PLACEHOLDER`, `repository`, `commit`, `replace`, or `grammar fetch`. The Helix README does include the missing replacement warning for its analogous placeholder grammar source. An external fetch of the Zed grammar placeholder URL fails before any editor-specific packaging step.

**Evidence:**

- `mcp__graphify.query_graph` for the Zed manifest/README placeholder terms did not surface the Zed scaffold directly, so exact bounded reads were used for the editor files.
- `nl -ba tree-sitter-glyph/editors/zed/extension.toml` shows `repository = "https://github.com/PLACEHOLDER/glyph"` on line 7, `repository = "https://github.com/PLACEHOLDER/tree-sitter-glyph"` on line 10, and `commit = "PLACEHOLDER_COMMIT_SHA"` on line 11.
- `rg -n "PLACEHOLDER|PLACEHOLDER_COMMIT_SHA|replace|repository|commit|grammar fetch|published git|git URL" tree-sitter-glyph/editors/zed/README.md` exited 1 with no output, confirming the README has no explicit replacement instruction for these placeholders.
- `rg -n "PLACEHOLDER|PLACEHOLDER_COMMIT_SHA|replace|repository|commit|grammar fetch|published git|git URL" tree-sitter-glyph/editors/helix/README.md tree-sitter-glyph/editors/zed/extension.toml tree-sitter-glyph/editors/helix/languages.toml` reports the Zed placeholder lines and the Helix README warning at lines 42-45: Helix grammar fetch expects a published git URL/tagged commit and the placeholder URL must be replaced before `hx --grammar fetch`.
- `env GIT_TERMINAL_PROMPT=0 git ls-remote https://github.com/PLACEHOLDER/tree-sitter-glyph` exited 128 with `remote: Repository not found.` and `fatal: repository 'https://github.com/PLACEHOLDER/tree-sitter-glyph/' not found`.

**Resolution Input:** Keep the existing recommended resolution. Add the same kind of explicit note to `tree-sitter-glyph/editors/zed/README.md` as the Helix README already has, and optionally add a nearby comment in `extension.toml`, stating that the placeholder `repository` values and `commit` SHA must be replaced with the real published grammar repository and commit/tag before the extension can resolve the grammar.
