#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."


NEW_VERSION=$1
if [ -z "$NEW_VERSION" ]; then
    echo "Usage: ./scripts/bump_version.sh <new-version> (e.g. ./scripts/bump_version.sh 0.2.0)"
    exit 1
fi

# Strip the leading 'v' if the user provides one by accident (e.g., v0.2.0 -> 0.2.0)
NEW_VERSION=${NEW_VERSION#v}

echo "Bumping version to $NEW_VERSION..."

# 1. Update Cargo.toml
# Uses cross-platform sed to update the version line specifically under [workspace.package]
if [[ "$OSTYPE" == "darwin"* ]]; then
    sed -i '' -e "s/^version = \".*\"/version = \"$NEW_VERSION\"/" Cargo.toml
else
    sed -i -e "s/^version = \".*\"/version = \"$NEW_VERSION\"/" Cargo.toml
fi
echo "✅ Updated Cargo.toml"

# 2. Update Cargo.lock by forcing cargo to reconcile the new workspace version
cargo update --workspace > /dev/null 2>&1
echo "✅ Updated Cargo.lock"

# 3. Update VS Code Extension
if [ -d "editors/vscode" ]; then
    cd editors/vscode
    # Use npm to safely update package.json without creating its own git tag
    npm version "$NEW_VERSION" --no-git-tag-version --allow-same-version > /dev/null 2>&1
    cd ../..
    echo "✅ Updated editors/vscode/package.json"
fi

# 4. Stage and Commit
git add Cargo.toml Cargo.lock editors/vscode/package.json
# Also add package-lock.json if npm updated it
if [ -f "editors/vscode/package-lock.json" ]; then
    git add editors/vscode/package-lock.json
fi

git commit -m "chore: bump version to v$NEW_VERSION"
git tag "v$NEW_VERSION"

echo ""
echo "🎉 Successfully bumped to v$NEW_VERSION, committed, and tagged!"
echo ""
echo "Next Steps:"
echo "  1. Push the commit and tag to GitHub:  git push origin main --tags"
echo "  2. Build the release binaries:         ./release.sh v$NEW_VERSION"
echo "  3. Upload to GitHub Releases:          ./upload.sh v$NEW_VERSION"
