# BUG-077: Next Steps tell user to both push the tag (triggers CI release) AND run the manual release/upload scripts, which conflict

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `scripts/bump_version.sh:54-56`
**Found by:** gap:release-dist-scripts | **Audit date:** unknown-date

## Description

Step 1 pushes the version tag (`git push origin main --tags`). The tag matches the CI trigger `v[0-9]+.[0-9]+.[0-9]+*` (`.github/workflows/release.yml` lines 4-6), so pushing it automatically builds artifacts and creates a GitHub Release via `softprops/action-gh-release`. Steps 2-3 then instruct the user to run `release.sh` + `upload.sh` for the same tag. `upload.sh` line 28 runs `gh release create "$VERSION" dist/* ...`, which errors when a release for that tag already exists (the CI just created it). So following the printed steps in order either races the CI or fails outright. Even if it succeeds, it publishes a second, differently-named Windows asset (gnu vs the CI's msvc) under the same tag. The manual and automated release paths are mutually exclusive but the Next Steps present them as a single sequential flow.

## Trigger / Reproduction

Run `./scripts/bump_version.sh <new-version>` and follow all three printed Next Steps in order. Step 1 pushes the tag, triggering CI to create a GitHub Release. By the time steps 2-3 complete, `upload.sh`'s `gh release create "$VERSION"` fails because the release already exists under that tag.

## Evidence

```bash
# scripts/bump_version.sh lines 54-56
echo "  1. Push the commit and tag to GitHub:  git push origin main --tags"
# ^ triggers .github/workflows/release.yml via: on: push: tags: 'v[0-9]+.[0-9]+.[0-9]+*'
# CI runs softprops/action-gh-release — creates GitHub Release automatically

echo "  2. Build the release binaries:         ./release.sh v$NEW_VERSION"
echo "  3. Upload to GitHub Releases:          ./upload.sh v$NEW_VERSION"
# upload.sh line 28: gh release create "$VERSION" dist/* ...
# ^ fails: release already exists (CI created it from the tag push in step 1)
```

## Recommended Resolution

Decide on one release path. If CI is canonical, drop steps 2-3 (the tag push is sufficient). If the manual path is canonical, either remove the tag-push trigger from `release.yml` or have `upload.sh` use `gh release create ... || gh release upload` to be idempotent against an existing release.

## Verification Notes

All three parts of the conflict are confirmed: `release.yml` triggers on the tag pattern, uses `softprops/action-gh-release@v2` to create the release, and `upload.sh` unconditionally calls `gh release create`. There is also a secondary divergence: `release.sh` builds the Windows target as `x86_64-pc-windows-gnu` while CI builds `x86_64-pc-windows-msvc`. This affects only the developer releasing the project — not end users, compiler correctness, or compiled output. Severity is low: the failure mode is an obvious `gh` CLI error rather than silent data loss.

## Independent Agent Finding

**Verdict:** Reproduced / confirmed.

**Reproduction/Refutation:** I reproduced the conflicting release-path behavior without touching the real repo release state or GitHub. A scratch copy of `scripts/bump_version.sh` with stubbed `git`, `cargo`, and `npm` printed the same three Next Steps: push commit/tag, build release binaries, then upload to GitHub Releases. A scratch copy of `scripts/upload.sh` with a fake `dist/` artifact and stubbed `gh` invoked `gh release create v9.9.9 dist/fake-artifact.txt --title "Release v9.9.9" --generate-notes`; when the stub returned a "release already exists" failure, `upload.sh` exited nonzero because it uses `set -e`. This reproduces the reported failure shape after CI has already created the release for the pushed tag.

**Evidence:** Bounded source checks show `scripts/bump_version.sh:54-56` still prints `git push origin main --tags`, `./release.sh v$NEW_VERSION`, and `./upload.sh v$NEW_VERSION` as one sequence. `.github/workflows/release.yml:3-6` triggers on pushed `v[0-9]+.[0-9]+.[0-9]+*` tags, and `.github/workflows/release.yml:101-105` uses `softprops/action-gh-release@v2` with `files: artifacts/*`, creating the release from CI. `scripts/upload.sh:28` unconditionally runs `gh release create "$VERSION" dist/* --title "Release $VERSION" --generate-notes`, with no fallback to `gh release upload`.

Summarized commands/output:

```bash
rg -n "git push origin main --tags|release.sh|upload.sh|gh release create|action-gh-release|tags:" \
  scripts/bump_version.sh scripts/upload.sh .github/workflows/release.yml
# confirmed the printed Next Steps, tag trigger, action-gh-release usage, and unconditional gh release create

# scratch harness under tmp/bug-077-repro, using copied scripts and stubbed commands
./scripts/upload.sh v9.9.9
# stdout included:
# STUB_GH_ARGS: <release> <create> <v9.9.9> <dist/fake-artifact.txt> <--title> <Release v9.9.9> <--generate-notes>
# stderr: simulated gh failure: release already exists
# exit status: 1

./scripts/bump_version.sh 9.9.9
# printed:
# 1. Push the commit and tag to GitHub:  git push origin main --tags
# 2. Build the release binaries:         ./release.sh v9.9.9
# 3. Upload to GitHub Releases:          ./upload.sh v9.9.9
```

Additional observation: the printed paths are root-relative `./release.sh` and `./upload.sh`, while the actual scripts in this repo are `scripts/release.sh` and `scripts/upload.sh`. That path mismatch can make the printed manual path fail even before the release-create conflict, but it does not refute the core report: the intended manual upload path still conflicts with the tag-triggered CI release path.

**Resolution Input:** Preserve the existing suggested resolution. The project should choose one canonical release path. If CI is canonical, drop the manual build/upload steps from the post-bump instructions or rewrite them as optional local-only artifact checks. If the manual path remains supported, remove or narrow the tag-push CI release trigger, or make `upload.sh` idempotent by uploading to an existing release instead of always creating one.
