# GitHub Linguist Integration

GitHub Linguist drives the language label on every file view, the
language breakdown on every repository's landing page, and the
syntax-highlighting choice for files rendered on github.com.

To get `.glyph` recognised as **Glyph** (not as
`Plain Text`/`Markdown`), the `Glyph` entry in
[`github-linguist/linguist`](https://github.com/github-linguist/linguist)'s
`lib/linguist/languages.yml` needs to land. Until that PR merges,
repositories can fall back to `.gitattributes` overrides.

## 1. Upstream PR — `languages.yml` entry

The text below is intended to be inserted into Linguist's
`lib/linguist/languages.yml`, alphabetically between `Glsl` and
`Glyph Bitmap Distribution Format`. Copy `languages.yml.entry` from
this directory verbatim into a Linguist branch and submit a PR.

When opening the PR, follow the
[Linguist contribution checklist](https://github.com/github-linguist/linguist/blob/main/CONTRIBUTING.md):

1. Provide samples — copy 5–10 representative `.glyph` files from
   `crates/glyph-cli/tests/corpus/valid/` and the multi-file
   examples into `samples/Glyph/` in the Linguist repo.
2. Reference this tree-sitter grammar from the PR description.
   The entry uses `tm_scope: none` because Glyph does not ship a
   TextMate grammar — Linguist will fall back to the tree-sitter
   grammar repo URL passed in the PR body. Replace the URL
   placeholder in `languages.yml.patch` with the canonical
   tree-sitter-glyph repo before submission.
3. Pick a `language_id`. The placeholder `999100001` in
   `languages.yml.entry` is unlikely to collide; the Linguist
   maintainers will assign a final value at review time.
4. The chosen `color` (`#1FA9A0`, teal) matches the `@label`
   accent in this grammar's `queries/highlights.scm` — section
   headers (`description:`, `context:`, `constraints:`, `flow:`)
   highlight teal, so the language-distribution bar reads as the
   same color as a `.glyph` file's anchor headers. Linguist's
   current palette has no other teal entries near
   `#1FA9A0`, so the color is unambiguous.

## 2. Repository-level fallback (`.gitattributes`)

If the upstream PR is still in flight, add this to a Glyph
repository's `.gitattributes` at the root:

```gitattributes
*.glyph linguist-detectable=true
*.glyph linguist-language=Markdown
```

`linguist-language=Markdown` is the closest analog Linguist
already supports — it prevents Glyph files from being counted as
plain text or stripped from the language breakdown. Once the
real `Glyph` entry lands, drop `linguist-language=Markdown` and
let Linguist resolve via extension.

## Files in this directory

- [`languages.yml.entry`](languages.yml.entry) — exact YAML to
  insert into Linguist's `languages.yml`.
- [`languages.yml.patch`](languages.yml.patch) — context-rich
  diff version for reviewers who want to see the insertion site.
