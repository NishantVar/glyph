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

## Independent Agent Finding

### Verdict

Partially refuted for current production, confirmed as a repo consistency risk. The checked-in `.agents/skills/install/SKILL.glyph` and compiled `.agents/skills/install/SKILL.md` both map Windows/x86_64 to `x86_64-pc-windows-gnu`, while `.github/workflows/release.yml` maps the Windows build to `x86_64-pc-windows-msvc`. However, the current latest GitHub release checked on 2026-06-10 is `0.1.0`, and that production release does publish `glyph-0.1.0-x86_64-pc-windows-gnu.zip` and does not publish the `msvc` zip. The reported current-production 404 does not reproduce today; the forward-looking source/workflow mismatch does.

### Reproduction/Refutation

- `rg -n "x86_64-pc-windows-(gnu|msvc)|windows|target" .github/workflows/release.yml .agents/skills/install/SKILL.glyph .agents/skills/install/SKILL.md` found `SKILL.glyph:38` and `SKILL.md:52` using `x86_64-pc-windows-gnu`, and `release.yml:27` using `x86_64-pc-windows-msvc`.
- `glyph compile --output tmp/bug-012/install-SKILL.md .agents/skills/install/SKILL.glyph` exited 1 before writing output with `G::expand::llm-required-for-call` and `G::expand::llm-required-for-param-description`; this local deterministic compiler path cannot independently regenerate this skill without the external LLM expand flow.
- `gh release view --repo NishantVar/glyph --json tagName,assets -q '{tag: .tagName, assets: [.assets[].name]}'` returned tag `0.1.0` with `glyph-0.1.0-x86_64-pc-windows-gnu.zip` present and no `glyph-0.1.0-x86_64-pc-windows-msvc.zip`.
- `curl -fsSI -L -o /dev/null -w '%{http_code} ...' https://github.com/NishantVar/glyph/releases/download/0.1.0/glyph-0.1.0-x86_64-pc-windows-gnu.zip` returned HTTP 200.
- `curl -fsSI -L -o /dev/null -w '%{http_code} ...' https://github.com/NishantVar/glyph/releases/download/0.1.0/glyph-0.1.0-x86_64-pc-windows-msvc.zip` returned HTTP 404.

### Evidence

The workflow matrix and archive naming currently imply a future Windows asset named `glyph-<tag>-x86_64-pc-windows-msvc.zip`:

```yaml
target: x86_64-pc-windows-msvc
7z a "../glyph-${{ github.ref_name }}-${{ matrix.target }}.zip" "glyph.exe"
```

The install source and compiled skill currently point at the production asset name that exists in release `0.1.0`:

```glyph
- Windows / x86_64 (MSYS/Git Bash) -> x86_64-pc-windows-gnu
```

### Resolution Input

Do not treat `x86_64-pc-windows-gnu` as currently 404ing against the latest production release. Before applying the existing suggested resolution, decide which artifact name is canonical. If the workflow is authoritative, changing the install skill to `x86_64-pc-windows-msvc` is the right direction for future releases, but it must be paired with a matching published release asset or it will break installs against the current latest `0.1.0`. If the current production asset is authoritative, the workflow or release process should be changed to keep publishing `x86_64-pc-windows-gnu`.
