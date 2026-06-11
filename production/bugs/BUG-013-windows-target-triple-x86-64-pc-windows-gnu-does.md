# BUG-013: Windows target triple x86_64-pc-windows-gnu does not match the released asset (msvc); Windows download 404s

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `.agents/skills/install/SKILL.md:52`
**Found by:** gap:install-skill-asset-parity | **Audit date:** unknown-date

## Description

The `install-cli-binary` procedure maps Windows/x86_64 to the target triple `x86_64-pc-windows-gnu` and then downloads `glyph-<tag>-<target>.tar.gz (or .zip on Windows)`, i.e. `glyph-<tag>-x86_64-pc-windows-gnu.zip`. But the CI release workflow (`.github/workflows/release.yml` line 27) builds Windows with `target: x86_64-pc-windows-msvc`, and the archive step names the asset `glyph-${{ github.ref_name }}-${{ matrix.target }}.zip`, i.e. `glyph-<tag>-x86_64-pc-windows-msvc.zip`. The gnu-suffixed asset is never published. Concrete trigger: a Windows user runs the install skill under MSYS/Git Bash; the skill derives the gnu triple, builds the download URL `https://github.com/NishantVar/glyph/releases/download/<tag>/glyph-<tag>-x86_64-pc-windows-gnu.zip` (and the corresponding `gh release download --pattern`), which 404s / matches no asset, so the CLI download always fails on Windows and silently falls back to a source build (or fails entirely if Cargo is absent). The Darwin arm64/x86_64 and Linux x86_64 mappings all match the workflow correctly; only Windows is wrong.

## Trigger / Reproduction

On a Windows host (MSYS or Git Bash), run the install skill. The skill maps the platform to `x86_64-pc-windows-gnu`, constructs a download URL for `glyph-<tag>-x86_64-pc-windows-gnu.zip`, and the request fails — that asset does not exist on any tag-triggered release.

## Evidence

```
# SKILL.md line 52 (install-cli-binary procedure):
# Windows/x86_64 (MSYS or Git Bash) to `x86_64-pc-windows-gnu`
# -> downloads: glyph-<tag>-x86_64-pc-windows-gnu.zip

# .github/workflows/release.yml line 27:
# target: x86_64-pc-windows-msvc
# -> artifact: glyph-${{ github.ref_name }}-x86_64-pc-windows-msvc.zip
```

## Recommended Resolution

Change the Windows mapping in `SKILL.md` from `x86_64-pc-windows-gnu` to `x86_64-pc-windows-msvc` so the resulting asset name `glyph-<tag>-x86_64-pc-windows-msvc.zip` matches what `release.yml` uploads. Also update the source `SKILL.glyph` (see BUG-012) so the fix survives recompilation.

## Verification Notes

`SKILL.md` line 52 explicitly maps Windows/x86_64 to `x86_64-pc-windows-gnu`, while the release workflow at `.github/workflows/release.yml` line 27 uses `target: x86_64-pc-windows-msvc` and names the Windows archive `glyph-${{ github.ref_name }}-${{ matrix.target }}.zip` (line 48). The gnu-suffixed asset is never produced or uploaded. Any Windows user following the install skill would construct a download URL referencing `glyph-<tag>-x86_64-pc-windows-gnu.zip`, which does not exist in any release. The skill's fallback to source build triggers silently. The Darwin and Linux mappings in the same procedure all correctly match the workflow targets; only Windows is wrong.

## Independent Agent Finding

### Verdict

Refuted for current production. The only published release, `0.1.0`, does contain `glyph-0.1.0-x86_64-pc-windows-gnu.zip`, and the installer-style `gh release download` command succeeds for that asset. The checked-in source/workflow mismatch is real, but the reported current-production Windows `gnu` download 404 is not reproducible.

### Reproduction/Refutation

Used Graphify first for codebase orientation, then switched to targeted file and release checks because the graph did not surface the exact installer/release nodes. Local searches confirmed `.agents/skills/install/SKILL.md:52` and `.agents/skills/install/SKILL.glyph:38` map Windows/x86_64 to `x86_64-pc-windows-gnu`, while `.github/workflows/release.yml:27` currently declares `x86_64-pc-windows-msvc` and `.github/workflows/release.yml:48` names the archive from `matrix.target`.

Commands run:
- `rg -n "x86_64-pc-windows-(gnu|msvc)|github.ref_name|matrix.target|zip|release download|download_url" .agents/skills/install/SKILL.md .github/workflows/release.yml .agents/skills/install/SKILL.glyph` found the `gnu` installer mapping and `msvc` workflow target.
- `gh release list --repo NishantVar/glyph --limit 20` showed only `Release 0.1.0`, marked latest.
- `gh release view --repo NishantVar/glyph --json tagName,assets` returned tag `0.1.0` with `glyph-0.1.0-x86_64-pc-windows-gnu.zip` in the asset list.
- `curl -sI -L -o /dev/null -w "%{http_code} %{url_effective}\n" https://github.com/NishantVar/glyph/releases/download/0.1.0/glyph-0.1.0-x86_64-pc-windows-gnu.zip` returned `200`.
- `curl -sI -L -o /dev/null -w "%{http_code} %{url_effective}\n" https://github.com/NishantVar/glyph/releases/download/0.1.0/glyph-0.1.0-x86_64-pc-windows-msvc.zip` returned `404`.
- `gh release download 0.1.0 --repo NishantVar/glyph --pattern glyph-0.1.0-x86_64-pc-windows-gnu.zip --dir tmp/bug-013` exited `0` and downloaded a 3,038,564-byte zip; the scratch directory was removed afterward.
- `gh release download 0.1.0 --repo NishantVar/glyph --pattern glyph-0.1.0-x86_64-pc-windows-msvc.zip --dir tmp/bug-013-msvc` exited `1` with `no assets match the file pattern`; the scratch directory was removed afterward.

### Evidence

Production release evidence contradicts the report's "gnu asset is never published" claim: the release API lists `glyph-0.1.0-x86_64-pc-windows-gnu.zip`, the direct `gnu` URL returns `200`, and the `gh release download --pattern` path succeeds. Conversely, the `msvc` URL/pattern is the missing asset for the current latest release.

### Resolution Input

Do not change the installer mapping from `gnu` to `msvc` as a standalone production fix, because that would make the current `0.1.0` Windows download miss the only published Windows asset. The actionable issue is release/install parity: choose the intended Windows target triple, then align `.agents/skills/install/SKILL.glyph`, the compiled `.agents/skills/install/SKILL.md`, `.github/workflows/release.yml`, and any existing or backfilled release assets. If future tag-triggered releases are expected to publish `msvc`, add a compatibility fallback or publish both names before changing the installer default.
