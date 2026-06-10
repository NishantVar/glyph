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
