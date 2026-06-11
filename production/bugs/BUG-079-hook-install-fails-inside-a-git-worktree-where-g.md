# BUG-079: Hook install fails inside a git worktree where .git is a file, not a directory

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `scripts/install-hooks.sh:15-16`
**Found by:** scripts | **Audit date:** unknown-date

## Description

The script hard-codes `cp scripts/hooks/pre-commit .git/hooks/pre-commit`. In a linked worktree (created via `git worktree add`), `.git` is a regular file containing `gitdir: ...`, not a directory, so `.git/hooks/` does not exist and the `cp` fails. With `set -e` the install aborts with a confusing error. The repo's own `.worktrees/` convention (with active worktrees including `B04-fix`, `ari-work`, etc.) makes this a plausible and reachable environment.

## Trigger / Reproduction

From a linked worktree (e.g. `cd .worktrees/B04-fix`), run `../../scripts/install-hooks.sh`. The `cp` at line 15 fails because `.git` is a plain text file (`gitdir: /Users/.../glyph/.git/worktrees/B04-fix`) rather than a directory, and `.git/hooks/` does not exist. With `set -e`, the script exits immediately with a "Not a directory" error.

## Evidence

```bash
# scripts/install-hooks.sh lines 14-16
# .git is a file in worktrees, not a directory — .git/hooks/ does not exist
cp scripts/hooks/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit

# In a linked worktree, .git contains:
#   gitdir: /Users/nishantvarshney/genesis/glyph/.git/worktrees/B04-fix
# So .git/hooks/ does not exist → cp fails → set -e aborts install.
#
# git rev-parse --git-path hooks → resolves correctly to the central hooks dir
# for both the main repo and all linked worktrees.
```

## Recommended Resolution

Resolve the real hooks directory portably:

```bash
HOOKS_DIR="$(git rev-parse --git-path hooks)"
mkdir -p "$HOOKS_DIR"
cp scripts/hooks/pre-commit "$HOOKS_DIR/pre-commit"
chmod +x "$HOOKS_DIR/pre-commit"
```

## Verification Notes

The repo has many active linked worktrees (e.g. `.worktrees/B04-fix`). In each, `.git` is confirmed to be a plain text file with a `gitdir:` pointer. `git rev-parse --git-path hooks` correctly resolves to the centralized hooks path (`/Users/nishantvarshney/genesis/glyph/.git/hooks`) from any worktree. The bug is reachable in the project's own documented workflow. Severity is low: only affects the contributor setup script, not compiler output or user-facing behavior.

## Independent Agent Finding

### Verdict

Reproduced. The installer still hard-codes `.git/hooks/pre-commit`, so it fails from a linked worktree where `.git` is a gitdir pointer file instead of a directory.

### Reproduction/Refutation

I reproduced this in a disposable detached worktree under `tmp/` and removed it afterward. The exact failing invocation is the worktree-local installer path, for example `./scripts/install-hooks.sh --no-graphify` from the linked worktree root. One nuance: the report's example `../../scripts/install-hooks.sh` from a nested `.worktrees/...` checkout can resolve to the main checkout's script and `cd` back to the main checkout, which does not exercise the `.git`-file failure mode. The underlying bug is still real for normal worktree-local invocation.

### Evidence

Summarized commands/output:

```bash
git worktree add --detach tmp/bug-079.XXXXXX/worktree HEAD
# created a linked worktree; cleanup removed it after the check

test -f tmp/bug-079.XXXXXX/worktree/.git
# .git is a file

head -n 1 tmp/bug-079.XXXXXX/worktree/.git
# gitdir: /Users/nishantvarshney/genesis/glyph/.git/worktrees/worktree

(cd tmp/bug-079.XXXXXX/worktree && git rev-parse --git-path hooks)
# /Users/nishantvarshney/genesis/glyph/.git/hooks

(cd tmp/bug-079.XXXXXX/worktree && ./scripts/install-hooks.sh --no-graphify)
# exit 1
# cp: .git/hooks/pre-commit: Not a directory
```

Implementation check: `scripts/install-hooks.sh` is 26 lines and still contains:

```bash
cp scripts/hooks/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

Graphify was queried first for hook-install context; the existing graph did not surface this shell script directly, so I used a bounded read of the 26-line script for exact implementation evidence.

### Resolution Input

Keep the existing recommended resolution: resolve the hook directory with `git rev-parse --git-path hooks`, create it with `mkdir -p`, then copy/chmod the hook there. That path is valid both in the main checkout and in linked worktrees.
