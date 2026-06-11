# BUG-044: git add of editors/vscode/package.json aborts the bump when the VS Code dir is absent

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `scripts/bump_version.sh:41-48`
**Found by:** scripts | **Audit date:** unknown-date

## Description

Lines 32-38 treat `editors/vscode` as optional (the whole npm update is guarded by `if [ -d "editors/vscode" ]`), but line 41 unconditionally runs `git add Cargo.toml Cargo.lock editors/vscode/package.json`. With `set -e` active, `git add` on a missing pathspec exits 128, so on any checkout where `editors/vscode/package.json` does not exist the script dies AFTER it has already rewritten `Cargo.toml` and `Cargo.lock` — leaving a half-applied version bump with no commit and no tag, and an error message that doesn't explain the partial state.

## Trigger / Reproduction

Run `scripts/bump_version.sh <new-version>` in any worktree or checkout that omits the `editors/vscode/` tree. The script will:
1. Rewrite `Cargo.toml` with the new version (line ~21-23).
2. Update `Cargo.lock` (line 28).
3. Die at line 41 with `git add` exit 128 — no commit, no tag, dirty working tree.

## Evidence

```bash
# scripts/bump_version.sh

set -e   # line 2 — any non-zero exit aborts

# lines 32-38: vscode update correctly guarded
if [ -d "editors/vscode" ]; then
    cd editors/vscode
    npm version "$NEW_VERSION" --no-git-tag-version --allow-same-version > /dev/null 2>&1
    cd ../..
    echo "✅ Updated editors/vscode/package.json"
fi

# line 41: unconditional — dies under set -e if editors/vscode/package.json is absent
git add Cargo.toml Cargo.lock editors/vscode/package.json

# line 43-45: package-lock.json already guarded (inconsistent with above)
if [ -f "editors/vscode/package-lock.json" ]; then
    git add editors/vscode/package-lock.json
fi

git commit -m "chore: bump version to v$NEW_VERSION"
git tag "v$NEW_VERSION"
```

## Recommended Resolution

Guard the `git add` for the vscode file the same way the npm update is guarded, mirroring the existing `package-lock.json` pattern:

```bash
git add Cargo.toml Cargo.lock
if [ -f "editors/vscode/package.json" ]; then
    git add editors/vscode/package.json
fi
```

## Verification Notes

`git add` on a missing pathspec exits 128. The `if [ -d "editors/vscode" ]` guard on the npm update block was added precisely because the directory may not always be present, making this an inconsistency introduced when the guard was added. The `package-lock.json` add (lines 43-45) is already correctly guarded, making the unguarded `package.json` add on line 41 the sole defect.

## Independent Agent Finding

**Verdict:** Reproduced. The report is valid.

**Reproduction/Refutation:** I first queried Graphify for `scripts/bump_version.sh` / VS Code package context, then performed a bounded read of the 56-line `scripts/bump_version.sh`. The script still has `set -e`, guards the npm update with `if [ -d "editors/vscode" ]`, and unconditionally runs `git add Cargo.toml Cargo.lock editors/vscode/package.json`. I reproduced the failure in an isolated scratch Git repo under `tmp/` by copying the real bump script, omitting `editors/vscode/`, and shadowing only `cargo update --workspace` so the run reached the exact staging step without modifying the real checkout.

**Evidence:** Targeted run summary:

```text
PATH="$PWD/fake-bin:$PATH" bash scripts/bump_version.sh 0.2.0
exit_code=128

Bumping version to 0.2.0...
✅ Updated Cargo.toml
✅ Updated Cargo.lock
fatal: pathspec 'editors/vscode/package.json' did not match any files

git status --short
 M Cargo.lock
 M Cargo.toml
?? run.out

git tag --list
# no tags

git log --oneline -1
32cb362 baseline

rg -n "^version = " Cargo.toml Cargo.lock
Cargo.lock:6:version = "0.2.0"
Cargo.toml:3:version = "0.2.0"
```

The scratch fixture was removed after the run. `git diff -- Cargo.toml Cargo.lock` in the real checkout was empty after cleanup.

**Resolution Input:** Preserve the existing recommended resolution. Stage `Cargo.toml` and `Cargo.lock` unconditionally, then guard `editors/vscode/package.json` with `[ -f "editors/vscode/package.json" ]`, matching the already-guarded `package-lock.json` add.
