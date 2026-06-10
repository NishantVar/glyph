# BUG-012: Compiled .glyph source has the same wrong Windows triple x86_64-pc-windows-gnu

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `.agents/skills/install/SKILL.glyph:38`
**Found by:** gap:install-skill-asset-parity | **Audit date:** unknown-date

## Description

The paired Glyph source for the install skill carries the identical incorrect Windows target triple. The block `install_cli_binary` docstring maps `Windows / x86_64 (MSYS/Git Bash) -> x86_64-pc-windows-gnu` (line 38) and downloads `glyph-<tag>-<target>.tar.gz (or .zip on Windows)` (line 41), resolving to `glyph-<tag>-x86_64-pc-windows-gnu.zip`. The release workflow never publishes that asset — it builds and uploads `x86_64-pc-windows-msvc`. Because this is the source that compiles to `SKILL.md`, the bug will reappear on recompile unless fixed here too. The other three platform triples on lines 35-37 match the workflow correctly.

## Trigger / Reproduction

Recompile `SKILL.glyph` with `glyph compile`. The regenerated `SKILL.md` will again contain `x86_64-pc-windows-gnu`, propagating the 404 bug for Windows users.

## Evidence

```glyph
   - Darwin / arm64  -> aarch64-apple-darwin
   - Darwin / x86_64 -> x86_64-apple-darwin
   - Linux  / x86_64 -> x86_64-unknown-linux-gnu
   - Windows / x86_64 (MSYS/Git Bash) -> x86_64-pc-windows-gnu
```

## Recommended Resolution

Update line 38 to `Windows / x86_64 (MSYS/Git Bash) -> x86_64-pc-windows-msvc` and recompile so `SKILL.md` stays in sync. This keeps the source-of-truth correct and ensures future compiles do not re-introduce the wrong triple in the compiled output.

## Verification Notes

The release workflow at `.github/workflows/release.yml` explicitly builds and uploads Windows artifacts with target `x86_64-pc-windows-msvc`. The `SKILL.glyph` source at line 38 maps "Windows / x86_64 (MSYS/Git Bash)" to `x86_64-pc-windows-gnu`, and `SKILL.md` (the compiled output, line 52) contains the same wrong triple. An agent following these instructions would attempt to download `glyph-<tag>-x86_64-pc-windows-gnu.zip`, which is never published, causing the install to fail and fall through to the source-build fallback. The fix (change `gnu` to `msvc` in `SKILL.glyph` and recompile) is correct and complete.
