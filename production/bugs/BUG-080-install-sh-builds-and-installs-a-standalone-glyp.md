# BUG-080: install.sh builds and installs a standalone 'glyph-lsp' binary that no documented client launches

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `scripts/install.sh:17-32`
**Found by:** gap:release-dist-scripts | **Audit date:** unknown-date

## Description

`install.sh` is the only place in the repo that builds and installs the standalone `glyph-lsp` binary (`cargo build -p glyph-lsp` then `install target/release/glyph-lsp`). No documented client uses it: the VS Code extension launches `<serverPath> lsp` (default `glyph lsp`) and its binary-resolution scan only ever looks for `glyph`, never `glyph-lsp` (`editors/vscode/src/extension.ts` lines 29-63, 71-80); the Neovim config in `crates/glyph-lsp/README.md` uses `cmd = { "glyph", "lsp" }`; the install skill refers to `glyph` as the binary that "doubles as the LSP server"; and the CI `release.yml` ships only `glyph`/`glyph.exe`. ADR 0024 explicitly rejected a separate `glyph-lsp` binary ("more distribution surface", "one cargo install") and locked in the `glyph lsp` subcommand. So `install.sh` diverges from the documented architecture: it doubles release-build time, clutters `$PREFIX` with an unused binary, and its final message ("Restart your editor's language server to pick up the new glyph-lsp", line 32) misleads the user into thinking their editor runs `glyph-lsp` when every documented client actually runs `glyph lsp`. A user who upgrades only the `glyph` binary through CI/install skill but ran `install.sh` once will have a stale `glyph-lsp` on PATH that nothing invokes.

## Trigger / Reproduction

Run `./scripts/install.sh`. Observe that `glyph-lsp` is built and installed to `$PREFIX/glyph-lsp`. Open VS Code or Neovim — neither client ever invokes `glyph-lsp`; both use `glyph lsp`. The installed `glyph-lsp` binary sits on PATH unused, and the final message misleads the user about which binary their editor runs.

## Evidence

```bash
# scripts/install.sh lines 17-32
echo "==> Building glyph-cli and glyph-lsp (release)"
cargo build --release -p glyph-cli -p glyph-lsp  # builds unused standalone binary

echo "==> Installing into $PREFIX"
install -m 0755 target/release/glyph     "$PREFIX/glyph"
install -m 0755 target/release/glyph-lsp "$PREFIX/glyph-lsp"  # installed but never invoked

echo "==> Restart your editor's language server to pick up the new glyph-lsp."
# ^ misleading: VS Code extension uses cmd: serverPath + args: ["lsp"]
#               Neovim config: cmd = { "glyph", "lsp" }
#               ADR 0024: standalone glyph-lsp binary explicitly rejected
```

## Recommended Resolution

Drop `-p glyph-lsp` from the build line and remove the `glyph-lsp` install line; install only `glyph` (which provides `glyph lsp`), matching ADR 0024, the editor clients, the install skill, and CI. Reword the final message to reference `glyph lsp`:

```bash
cargo build --release -p glyph-cli
install -m 0755 target/release/glyph "$PREFIX/glyph"
echo "==> Restart your editor's language server to pick up the new glyph."
```

## Verification Notes

`extension.ts` lines 70-80 always resolve to the `glyph` binary with `args: ["lsp"]`; the candidate scan (lines 29-62) only searches for `glyph`, never `glyph-lsp`. ADR 0024 explicitly rejected a standalone binary. CI `release.yml` ships only `glyph`/`glyph.exe`. No user data is lost or corrupted; compiled output is unaffected. The bug causes wasted build time, an unused binary on PATH, and a misleading final message — an operational/documentation inconsistency. Severity is low.

## Independent Agent Finding

**Verdict:** Reproduced / confirmed, with one nuance. The current installer does build and install a standalone `glyph-lsp` binary and tells users to restart for `glyph-lsp`, while the documented editor and release paths use `glyph lsp`. The nuance is that `crates/glyph-lsp/README.md` and `crates/glyph-lsp/src/main.rs` do acknowledge the standalone binary exists for users who explicitly want a focused LSP binary; that does not refute the bug because the checked editor configs and release artifact path still do not launch or ship that standalone binary by default.

**Reproduction/Refutation:** I reproduced the installer behavior in a tracked snapshot under `tmp/bug080-repro.ifug8x/` with `PREFIX` redirected to the same scratch area, so the active working tree was not built into or installed from. The command was `PREFIX="$PWD/../prefix" ./scripts/install.sh` from the scratch copy. It completed successfully and installed both `prefix/glyph` and `prefix/glyph-lsp`, then printed `==> Restart your editor's language server to pick up the new glyph-lsp.` A follow-up `tmp/bug080-repro.ifug8x/prefix/glyph --help | rg -n "lsp|Language|Commands|Usage"` showed the CLI already exposes `lsp` as `Run the Glyph Language Server over stdio`.

**Evidence:** `scripts/install.sh` still contains `cargo build --release -p glyph-cli -p glyph-lsp`, installs both `target/release/glyph` and `target/release/glyph-lsp`, lists both installed paths, and ends with the `glyph-lsp` restart message. `editors/vscode/src/extension.ts` resolves only the `glyph` binary when using the default server path and launches it with `args: ["lsp"]`; `crates/glyph-lsp/README.md` documents `cargo install --path crates/glyph-cli`, says the LSP is invoked as `glyph lsp`, and the Neovim snippet uses `cmd = { "glyph", "lsp" }`. `docs/adr/0024-lsp-shares-glyph-core.md` says the subcommand keeps the install path to one `cargo install` and that a separate `glyph-lsp` binary was rejected for distribution surface. `.github/workflows/release.yml` builds the workspace but archives only `glyph`/`glyph.exe` into release artifacts.

**Resolution Input:** Preserve the existing recommended resolution: drop `-p glyph-lsp` from `scripts/install.sh`, remove the `glyph-lsp` install/listing path, install only `glyph`, and reword the final message to refer to the `glyph`/`glyph lsp` path. If maintainers intentionally want to keep supporting the standalone binary for direct manual editor configuration, that should be an explicit opt-in path outside the default installer rather than the default install behavior and restart guidance.
