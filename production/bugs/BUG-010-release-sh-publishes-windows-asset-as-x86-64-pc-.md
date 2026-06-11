# BUG-010: release.sh publishes Windows asset as 'x86_64-pc-windows-gnu' while CI publishes 'x86_64-pc-windows-msvc' for the same tag

**Severity:** high | **Confidence:** high | **Status:** confirmed
**Location:** `scripts/release.sh:70-71`
**Found by:** gap:release-dist-scripts | **Audit date:** unknown-date

## Description

The two release paths produce differently-named Windows artifacts for the same version tag. `release.sh` builds `x86_64-pc-windows-gnu` and names the archive `glyph-$VERSION-x86_64-pc-windows-gnu.zip`. The CI workflow (`.github/workflows/release.yml` line 27) builds `x86_64-pc-windows-msvc` and names it `glyph-${github.ref_name}-x86_64-pc-windows-msvc.zip`. The documented installer (`.agents/skills/install/SKILL.md` install-cli-binary procedure and `SKILL.glyph` line 38) maps Windows hosts to `x86_64-pc-windows-gnu` and downloads `glyph-<tag>-x86_64-pc-windows-gnu.zip`. Concrete failure: a normal release is made by pushing a tag (which triggers ONLY the CI), so GitHub gets the `...-windows-msvc.zip` asset. A Windows user running the documented install flow requests `...-windows-gnu.zip`, gets a 404, and is silently forced into the source-build fallback. The two are also not ABI-compatible (gnu vs msvc toolchains), so even the artifact contents differ.

## Trigger / Reproduction

Push a version tag to trigger the CI release workflow. The CI publishes `glyph-<tag>-x86_64-pc-windows-msvc.zip`. A Windows user then runs the install skill, which constructs the URL for `glyph-<tag>-x86_64-pc-windows-gnu.zip` — an asset that was never uploaded — and receives a 404.

## Evidence

```sh
# scripts/release.sh lines 70-71
cross build --release --target x86_64-pc-windows-gnu --workspace
zip -j dist/glyph-$VERSION-x86_64-pc-windows-gnu.zip target/x86_64-pc-windows-gnu/release/glyph.exe

# .github/workflows/release.yml line 27 (CI, the canonical release path):
# target: x86_64-pc-windows-msvc
# -> artifact: glyph-${github.ref_name}-x86_64-pc-windows-msvc.zip

# .agents/skills/install/SKILL.glyph line 38 (install skill):
# Windows / x86_64 (MSYS/Git Bash) -> x86_64-pc-windows-gnu
# -> downloads: glyph-<tag>-x86_64-pc-windows-gnu.zip  (never published by CI)
```

## Recommended Resolution

Pick one Windows target and use it everywhere. Since the CI is the canonical automated release path, change `release.sh` to build/name `x86_64-pc-windows-msvc` (and update the install skill to download the msvc asset), OR change `release.yml` to use gnu — but make all three (`release.sh`, `release.yml`, install skill) agree on a single triple. The msvc path is preferred as it matches the CI baseline.

## Verification Notes

All three code paths are confirmed exactly as described. `scripts/release.sh` lines 70-71 build and name the Windows artifact with `x86_64-pc-windows-gnu`. `.github/workflows/release.yml` line 27 sets the Windows target to `x86_64-pc-windows-msvc`, producing `glyph-${github.ref_name}-x86_64-pc-windows-msvc.zip`. The install skill maps Windows/x86_64 to `x86_64-pc-windows-gnu` and downloads `glyph-<tag>-x86_64-pc-windows-gnu.zip`. The canonical release path is a tag push triggering release.yml (CI), which publishes the msvc artifact. The gnu-suffixed asset is never produced during a normal release, causing a 404 for every Windows user of the install skill.

## Independent Agent Finding

### Verdict

Partially reproduced. The source-level mismatch is real: the CI workflow would publish `glyph-<vtag>-x86_64-pc-windows-msvc.zip` for a v-prefixed release tag, while the install skill requests `glyph-<tag>-x86_64-pc-windows-gnu.zip`. The current live latest release checked on 2026-06-10 does not reproduce the reported 404 symptom: GitHub release `0.1.0` contains `glyph-0.1.0-x86_64-pc-windows-gnu.zip`, and no Release workflow runs were returned by `gh run list --workflow Release`.

### Reproduction/Refutation

Reproduced the mismatch without building by deriving artifact names from the current files. The derived CI asset was `glyph-vBUG010-repro-x86_64-pc-windows-msvc.zip`; both `scripts/release.sh` and the install skill derived `glyph-vBUG010-repro-x86_64-pc-windows-gnu.zip`; the comparison exited non-zero with `MISMATCH: CI does not publish the installer-requested asset`.

Refuted the current-production 404 for the latest release by querying GitHub: `gh release view --repo NishantVar/glyph --json tagName,assets` returned `tag=0.1.0` and the Windows asset `glyph-0.1.0-x86_64-pc-windows-gnu.zip`. The workflow trigger is only `v[0-9]+.[0-9]+.[0-9]+*`, so the unprefixed `0.1.0` release is not evidence of the current CI path publishing assets.

### Evidence

Commands run and summarized:

```sh
# Graphify MCP query: release.sh Windows asset target triple x86_64-pc publishing asset packaging release script bug report
# Summary: surfaced the release documentation cluster; exact script details required bounded file reads.

rg -n "x86_64-pc-windows|windows-msvc|windows-gnu|github\\.ref_name|target:|zip" .github/workflows/release.yml .agents/skills/install/SKILL.glyph .agents/skills/install/SKILL.md scripts/release.sh
# Summary: current workflow line 27 uses x86_64-pc-windows-msvc; release.sh lines 70-71 use x86_64-pc-windows-gnu; install skill maps Windows/x86_64 to x86_64-pc-windows-gnu.

VERSION=vBUG010-repro; ci_target=$(awk '/build: windows/{in_windows=1} in_windows && /target:/{print $2; exit}' .github/workflows/release.yml); release_sh_target=$(rg -o 'x86_64-pc-windows-[a-z]+' scripts/release.sh | head -n 1); install_target=$(rg -o 'x86_64-pc-windows-[a-z]+' .agents/skills/install/SKILL.glyph | head -n 1)
# Followed by comparing glyph-${VERSION}-${ci_target}.zip against glyph-${VERSION}-${install_target}.zip.
# Summary: exited 1; CI asset was msvc, installer asset was gnu.

gh release view --repo NishantVar/glyph --json tagName,assets -q '.tagName as $tag | "tag=" + $tag, (.assets[].name | select(test("windows|x86_64-pc")))'
# Summary: tag=0.1.0; glyph-0.1.0-x86_64-pc-windows-gnu.zip.

git show 0.1.0:.github/workflows/release.yml | sed -n '15,30p;44,62p'
# Summary: tag 0.1.0 workflow also names the Windows CI target x86_64-pc-windows-msvc.

gh run list --repo NishantVar/glyph --workflow Release --limit 20
# Summary: returned no Release workflow runs.
```

### Resolution Input

Keep the existing recommendation to make `release.sh`, `.github/workflows/release.yml`, and the install skill agree on one Windows triple. Also decide whether release tags are meant to be prefixed (`v0.1.0`) or unprefixed (`0.1.0`), because the current workflow only triggers for v-prefixed tags while the published latest release is unprefixed.
