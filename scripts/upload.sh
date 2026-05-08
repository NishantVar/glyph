#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."


VERSION=$1
if [ -z "$VERSION" ]; then
    echo "Usage: ./scripts/upload.sh <version> (e.g. ./scripts/upload.sh v0.1.0)"
    echo "Make sure you have run ./scripts/release.sh <version> first!"
    exit 1
fi

if ! command -v gh &> /dev/null; then
    echo "The 'gh' (GitHub CLI) tool is required. Install it via https://cli.github.com/"
    exit 1
fi

if [ ! -d "dist" ] || [ -z "$(ls -A dist)" ]; then
    echo "Error: The ./dist directory is empty or does not exist."
    echo "Please run ./release.sh $VERSION first to generate the binaries."
    exit 1
fi

echo "Creating GitHub Release $VERSION and uploading artifacts from ./dist..."

# Create a release with auto-generated notes and attach all files in the dist directory
gh release create "$VERSION" dist/* --title "Release $VERSION" --generate-notes

echo "✅ Release $VERSION created and uploaded successfully!"