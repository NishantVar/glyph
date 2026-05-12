#!/usr/bin/env bash
set -e

# This script manages symlinking Glyph skills and commands into agent configurations.
# Usage: ./install_agent_skills.sh <agent_dir> <commands_option>
# <agent_dir> is the root of the agent config (e.g., ~/.gemini)
# <commands_option> is one of: "with_desc", "no_desc", "none"

AGENT_DIR="$1"
COMMANDS_OPT="$2"

if [ -z "$AGENT_DIR" ] || [ -z "$COMMANDS_OPT" ]; then
    echo "Usage: $0 <agent_dir> <commands_option>"
    exit 1
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Resolve absolute path for AGENT_DIR to handle ~ properly if passed as a string
AGENT_DIR="${AGENT_DIR/#\~/$HOME}"

# Define agent paths
AGENT_SKILLS_DIR="$AGENT_DIR/skills"
AGENT_COMMANDS_DIR="$AGENT_DIR/commands"

# Ensure target directories exist
mkdir -p "$AGENT_SKILLS_DIR"
mkdir -p "$AGENT_COMMANDS_DIR"

# Symlink core skill
ln -sfn "$REPO_ROOT/.agents/skills/glyph" "$AGENT_SKILLS_DIR/glyph"
echo "Symlinked core skill to $AGENT_SKILLS_DIR/glyph"

# Handle commands
if [ "$COMMANDS_OPT" = "with_desc" ]; then
    ln -sfn "$REPO_ROOT/.agents/commands/glyph" "$AGENT_COMMANDS_DIR/glyph"
    echo "Symlinked commands (with descriptions) to $AGENT_COMMANDS_DIR/glyph"
elif [ "$COMMANDS_OPT" = "no_desc" ]; then
    ln -sfn "$REPO_ROOT/.agents/commands_no_desc/glyph" "$AGENT_COMMANDS_DIR/glyph"
    echo "Symlinked commands (no descriptions) to $AGENT_COMMANDS_DIR/glyph"
elif [ "$COMMANDS_OPT" = "none" ]; then
    # Remove symlink if it exists
    if [ -L "$AGENT_COMMANDS_DIR/glyph" ]; then
        rm -f "$AGENT_COMMANDS_DIR/glyph"
        echo "Removed existing commands symlink."
    else
        echo "Skipped optional commands."
    fi
else
    echo "Error: Invalid commands option '$COMMANDS_OPT'. Must be 'with_desc', 'no_desc', or 'none'."
    exit 1
fi
