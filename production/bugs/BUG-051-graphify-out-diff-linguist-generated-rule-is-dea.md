# BUG-051: graphify-out/** -diff/linguist-generated rule is dead: the path is fully gitignored

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `.gitattributes:6-10`
**Found by:** gap:git-ignore-attributes-interaction | **Audit date:** unknown-date

## Description

`.gitattributes` line 10 declares `graphify-out/** -diff linguist-generated=true` to keep the graphify output out of diffs and GitHub language stats. But `.gitignore` line 1 (`graphify-out/`) ignores that entire directory, so nothing under `graphify-out/` is ever tracked by git. A `.gitattributes` rule only applies to files git tracks; since no file under `graphify-out/` is tracked, this rule can never fire. The comment's stated goal (downstream reviewers like Codex see only a "Binary files differ" line, GitHub collapses the diff) is moot because those files are never in the repo or a diff to begin with. The rule is misleading dead config — a maintainer reading it would believe `graphify-out/` is committed-but-collapsed, when it is actually fully untracked.

## Trigger / Reproduction

```
git ls-files "graphify-out"       # returns empty — nothing tracked
git check-ignore -v graphify-out/graph.json
# resolves to: .gitignore:1:graphify-out/
```

The `.gitattributes` rule at line 10 therefore has no files it can ever apply to.

## Evidence

```gitattributes
# Generated artifacts — do not show in diffs or count toward language stats.
# `-diff` makes `git diff` and downstream reviewers (e.g. Codex) see only a
# "Binary files differ" line instead of the full content.
# `linguist-generated=true` collapses these in GitHub PR review UI.
graphify-out/** -diff linguist-generated=true
```

`.gitignore` line 1: `graphify-out/` — the entire directory is ignored, so nothing under it is ever tracked.

## Recommended Resolution

Either delete the `graphify-out/**` line from `.gitattributes` (it is a no-op given the gitignore), or — if the intent is actually to commit `graphify-out/` and only collapse it in diffs — un-ignore the path in `.gitignore`. The two files currently encode contradictory intents (ignore entirely vs. track-but-collapse); pick one.

## Verification Notes

`git ls-files "graphify-out"` returns empty and `git check-ignore -v graphify-out/graph.json` resolves to `.gitignore:1:graphify-out/`. Git attributes only apply to tracked files, so the rule is a confirmed no-op. Zero impact on compiler behavior, CLI output, or any functional aspect of the project — purely a maintenance/clarity issue.

## Independent Agent Finding

### Verdict

Reproduced, with one narrow qualification: the `.gitattributes` pattern does syntactically match `graphify-out/graph.json` when queried with `git check-attr`, but because `graphify-out/` is ignored and there are currently no tracked files under that path, the rule has no effect on normal repository diffs or GitHub language accounting.

### Reproduction/Refutation

The current repository state reproduces the report:

- `.gitignore` starts with `graphify-out/`.
- `.gitattributes` contains `graphify-out/** -diff linguist-generated=true`.
- `git ls-files graphify-out` returned no tracked paths.
- `git check-ignore -v graphify-out/graph.json` returned `.gitignore:1:graphify-out/	graphify-out/graph.json`.
- `git add --dry-run graphify-out/graph.json` failed with Git's ignored-path message and exit code `1`, including the hint to use `-f` to add it anyway.

The qualification is:

- `git check-attr -a -- graphify-out/graph.json` returned `diff: unset` and `linguist-generated: true`, so the attribute rule is not malformed. It is dead for the repository's current tracked-file and normal-add behavior, not because Git cannot match the path.

### Evidence

Graphify orientation found only the configured graphify MCP entry and did not identify any implementation dependency that would make `graphify-out/` a required tracked artifact. The decisive evidence is Git metadata behavior:

```text
$ git ls-files graphify-out
# no output

$ git check-ignore -v graphify-out/graph.json
.gitignore:1:graphify-out/	graphify-out/graph.json

$ git check-attr -a -- graphify-out/graph.json
graphify-out/graph.json: diff: unset
graphify-out/graph.json: linguist-generated: true

$ git add --dry-run graphify-out/graph.json
The following paths are ignored by one of your .gitignore files:
graphify-out
...
exit=1
```

### Resolution Input

Keep the existing recommended resolution: either delete the `graphify-out/**` line from `.gitattributes` if `graphify-out/` is intended to remain untracked generated output, or un-ignore the path in `.gitignore` if the intended policy is to commit `graphify-out/` while collapsing its diffs/language stats. The current files still encode contradictory maintenance intent.
