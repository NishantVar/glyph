#!/usr/bin/env bash
# Build the Glyph CLI and LSP server in release mode and install them
# into a local bin directory on PATH.
#
# Usage:
#   ./scripts/install.sh              # installs to $PREFIX (default: ~/.local/bin)
#   PREFIX=~/bin ./scripts/install.sh # override install location

set -euo pipefail

cd "$(dirname "$0")/.."

PREFIX="${PREFIX:-$HOME/.local/bin}"

mkdir -p "$PREFIX"

echo "==> Building glyph-cli and glyph-lsp (release)"
cargo build --release -p glyph-cli -p glyph-lsp

echo "==> Installing into $PREFIX"
install -m 0755 target/release/glyph     "$PREFIX/glyph"
install -m 0755 target/release/glyph-lsp "$PREFIX/glyph-lsp"

echo "==> Installed:"
ls -l "$PREFIX/glyph" "$PREFIX/glyph-lsp"

case ":$PATH:" in
    *":$PREFIX:"*) ;;
    *) echo "WARNING: $PREFIX is not on your PATH. Add it to your shell profile." ;;
esac

echo "==> Restart your editor's language server to pick up the new glyph-lsp."
