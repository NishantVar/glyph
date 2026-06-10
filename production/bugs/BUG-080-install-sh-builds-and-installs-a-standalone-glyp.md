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
