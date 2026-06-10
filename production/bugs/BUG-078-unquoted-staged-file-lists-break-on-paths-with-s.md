# BUG-078: Unquoted staged-file lists break on paths with spaces; cargo fmt --all + git add re-stages whole files

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `scripts/hooks/pre-commit:4-11`
**Found by:** scripts | **Audit date:** unknown-date

## Description

`staged_rs` is word-split unquoted at `git add $staged_rs` (line 10), and `staged_glyph` is iterated via `for f in $staged_glyph` (line 32). Both rely on IFS word-splitting of newline-separated `git diff --name-only` output, so any staged `.rs`/`.glyph` path containing a space (or glob character) is split into multiple bogus pathspecs and the `git add`/loop operate on the wrong files or fail. Separately, `cargo fmt --all` (line 9) formats the entire working tree and then `git add $staged_rs` re-stages the full working-tree content of those files; if a file was partially staged (`git add -p`), unstaged hunks get silently committed. The space-splitting is the concrete shell defect (`set -e` turns a failed `git add` into an aborted commit).

## Trigger / Reproduction

Stage a `.rs` file whose path contains a space (e.g. `src/my module/lib.rs`) and attempt a commit. `git add $staged_rs` splits the path into `src/my` and `module/lib.rs`, causing `git add` to fail or operate on wrong files. With `set -e`, the commit aborts. Alternatively: use `git add -p` to partially stage a `.rs` file, then commit — `cargo fmt --all` + `git add $staged_rs` re-stages the full file content including the unstaged hunks, silently including them in the commit.

## Evidence

```bash
# scripts/hooks/pre-commit lines 4-11
staged_rs=$(git diff --cached --name-only --diff-filter=ACM | grep '\.rs$' || true)
staged_glyph=$(git diff --cached --name-only --diff-filter=ACM | grep '\.glyph$' || true)

if [ -n "$staged_rs" ]; then
    echo "pre-commit: running cargo fmt..."
    cargo fmt --all
    git add $staged_rs   # unquoted — word-splits on spaces/globs in paths

# ...
for f in $staged_glyph; do   # line 32 — unquoted, same issue
for f in $check_glyph; do    # line 46 — unquoted, same issue
```

## Recommended Resolution

Use NUL-delimited iteration: `git diff --cached --name-only -z --diff-filter=ACM` piped into `while IFS= read -r -d '' f; do ...`, or at minimum quote and avoid re-adding whole files (use `git add -- "$f"` per file). For the partial-staging issue, format only staged files rather than `cargo fmt --all`.

## Verification Notes

Lines 10, 32, and 46 all use unquoted variable expansion, confirmed by direct inspection. The path-with-spaces scenario is theoretical for a Rust/Glyph project — no source file in this repo has a space in its name. The partial-staging issue requires deliberate `git add -p` usage. This affects only the contributor pre-commit workflow, not compiler output or user-facing behavior. Severity is low.
