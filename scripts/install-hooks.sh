#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

GRAPHIFY=true

for arg in "$@"; do
    case $arg in
        --no-graphify) GRAPHIFY=false ;;
        *) echo "Unknown option: $arg"; echo "Usage: ./scripts/install-hooks.sh [--no-graphify]"; exit 1 ;;
    esac
done

cp scripts/hooks/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
echo "Installed pre-commit hook."

if [ "$GRAPHIFY" = true ]; then
    if command -v graphify &>/dev/null; then
        graphify hook install
        echo "Installed graphify hooks."
    else
        echo "graphify not found — skipping graphify hooks. Install graphify and re-run, or pass --no-graphify to suppress this."
    fi
fi
