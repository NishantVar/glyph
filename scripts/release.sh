#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."


VERSION=$1
if [ -z "$VERSION" ]; then
    echo "Usage: ./scripts/release.sh <version> (e.g. ./scripts/release.sh v0.1.0)"
    exit 1
fi

echo "Ensuring required Rust targets for macOS are installed..."
rustup target add aarch64-apple-darwin x86_64-apple-darwin

if ! command -v cross &> /dev/null; then
    echo "The 'cross' tool is required for Linux and Windows builds."
    echo "Please install it with: cargo install cross --git https://github.com/cross-rs/cross"
    echo "(Note: cross requires Docker or Podman to be running)"
    exit 1
fi

echo "Building release for version $VERSION..."
mkdir -p dist

# 1. macOS (Apple Silicon) - Natively supported on M1/M2/M3
echo "==> Building for macOS (aarch64)..."
cargo build --release --target aarch64-apple-darwin --workspace
tar -czf dist/glyph-$VERSION-aarch64-apple-darwin.tar.gz -C target/aarch64-apple-darwin/release glyph

# 2. macOS (Intel)
echo "==> Building for macOS (x86_64)..."
cargo build --release --target x86_64-apple-darwin --workspace
tar -czf dist/glyph-$VERSION-x86_64-apple-darwin.tar.gz -C target/x86_64-apple-darwin/release glyph

# 3. Linux (requires cross)
echo "==> Building for Linux (x86_64)..."
cross build --release --target x86_64-unknown-linux-gnu --workspace
tar -czf dist/glyph-$VERSION-x86_64-unknown-linux-gnu.tar.gz -C target/x86_64-unknown-linux-gnu/release glyph

# 4. Windows (requires cross)
echo "==> Building for Windows (x86_64)..."
cross build --release --target x86_64-pc-windows-gnu --workspace
zip -j dist/glyph-$VERSION-x86_64-pc-windows-gnu.zip target/x86_64-pc-windows-gnu/release/glyph.exe

echo "Done! Release artifacts are in the ./dist folder."
echo "You can now go to GitHub -> Releases -> Draft a new release, and upload these files."