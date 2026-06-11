# BUG-076: Next Steps print './release.sh' and './upload.sh' but the scripts live in scripts/

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `scripts/bump_version.sh:55-56`
**Found by:** gap:release-dist-scripts | **Audit date:** unknown-date

## Description

The printed Next Steps tell the user to run `./release.sh v$NEW_VERSION` and `./upload.sh v$NEW_VERSION`. Both scripts actually live in `scripts/`, and `bump_version.sh` itself cd's to the repo root (line 4). Step 1 (`git push origin main --tags`) is also run from the repo root, so the user is at the root when they reach steps 2-3. Running `./release.sh v0.2.0` from the repo root fails with "no such file or directory". The scripts' own usage strings confirm the correct invocation is `./scripts/release.sh` / `./scripts/upload.sh` (`release.sh` line 33, `upload.sh` line 9).

## Trigger / Reproduction

Run `./scripts/bump_version.sh <new-version>` and follow the printed Next Steps. After step 1 (`git push origin main --tags`), run `./release.sh v<new-version>` from the repo root. The command fails with "no such file or directory" because the script only exists at `scripts/release.sh`.

## Evidence

```bash
# scripts/bump_version.sh lines 54-56
echo "Next Steps:"
echo "  1. Push the commit and tag to GitHub:  git push origin main --tags"
echo "  2. Build the release binaries:         ./release.sh v$NEW_VERSION"
echo "  3. Upload to GitHub Releases:          ./upload.sh v$NEW_VERSION"
# Both scripts live in scripts/, not at the repo root.
# release.sh line 33 and upload.sh line 9 both show ./scripts/release.sh
# and ./scripts/upload.sh as the correct invocations.
```

## Recommended Resolution

Change the printed paths to `./scripts/release.sh v$NEW_VERSION` and `./scripts/upload.sh v$NEW_VERSION` to match the actual script locations and their own usage strings.

## Verification Notes

`bump_version.sh` line 4 changes to the repo root. Lines 55-56 print `./release.sh` and `./upload.sh`. Both scripts live exclusively in `scripts/` and do not exist at the repo root. The usage strings in both scripts explicitly show the `./scripts/` prefix. Only affects the instructional text printed to the developer — the actual scripts work fine when invoked correctly. No compiled output, no data loss, no crash. Severity is low.

## Independent Agent Finding

**Verdict:** Reproduced. The report is valid.

**Reproduction/Refutation:** I did not run `./scripts/bump_version.sh <new-version>` because that path mutates version files, updates lockfiles, commits, and tags. Instead, I used the lower-risk equivalent checks for the reported failure path: verified the exact printed Next Steps in `scripts/bump_version.sh`, verified the script locations, and executed the printed root-relative commands from the repository root. Both printed commands fail because there is no `release.sh` or `upload.sh` at the repo root.

**Evidence:** `rg --files | rg '(^|/)(bump_version|release|upload)\.sh$'` returned only `scripts/release.sh`, `scripts/upload.sh`, and `scripts/bump_version.sh`. Bounded reads showed `scripts/bump_version.sh:4` changes to the repo root and `scripts/bump_version.sh:55-56` print `./release.sh v$NEW_VERSION` and `./upload.sh v$NEW_VERSION`. `scripts/release.sh:33` and `scripts/upload.sh:9-10` advertise `./scripts/...` usage. Running `./release.sh v0.2.0` from the repo root exited 127 with `zsh:1: no such file or directory: ./release.sh`; running `./upload.sh v0.2.0` exited 127 with the same missing-file error for `./upload.sh`.

**Resolution Input:** Keep the existing recommended resolution: change the printed Next Steps in `scripts/bump_version.sh` to `./scripts/release.sh v$NEW_VERSION` and `./scripts/upload.sh v$NEW_VERSION`.
