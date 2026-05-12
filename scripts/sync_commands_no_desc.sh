#!/usr/bin/env bash
set -e

# This script duplicates .agents/commands/glyph to .agents/commands_no_desc/glyph,
# stripping `description:` lines and prepending an AUTO-GENERATED banner.
#
# Banner placement:
#   - .glyph files: leading `//` comments (Glyph parser strips line comments).
#   - .md   files: `#` comments inside the YAML frontmatter, right after the
#                  opening `---`. The frontmatter parser drops these comments,
#                  so the banner does NOT appear in the skill body that loaders
#                  feed into agents at runtime — it is only visible to anyone
#                  who reads the raw file (i.e. someone about to edit it).

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC_DIR="$REPO_ROOT/.agents/commands/glyph"
DEST_DIR="$REPO_ROOT/.agents/commands_no_desc/glyph"

if [ ! -d "$SRC_DIR" ]; then
    echo "Source directory $SRC_DIR does not exist."
    exit 1
fi

# Clean and recreate destination
rm -rf "$DEST_DIR"
mkdir -p "$DEST_DIR"

shopt -s nullglob
for file in "$SRC_DIR"/*; do
    [ -f "$file" ] || continue
    filename=$(basename "$file")
    dest_file="$DEST_DIR/$filename"
    rel_src=".agents/commands/glyph/$filename"
    ext="${filename##*.}"

    case "$ext" in
        glyph)
            awk -v src="$rel_src" '
            BEGIN {
                print "// AUTO-GENERATED FILE -- DO NOT EDIT"
                print "// Source: " src
                print "// Regenerate: scripts/sync_commands_no_desc.sh"
            }
            /^[[:space:]]*description:/ { next }
            { print }
            ' "$file" > "$dest_file"
            ;;
        md)
            awk -v src="$rel_src" '
            NR==1 {
                print
                print "# AUTO-GENERATED FILE -- DO NOT EDIT"
                print "# Source: " src
                print "# Regenerate: scripts/sync_commands_no_desc.sh"
                next
            }
            /^[[:space:]]*description:/ { next }
            { print }
            ' "$file" > "$dest_file"
            ;;
        *)
            # Unknown extension: copy verbatim (no banner, no strip).
            cp "$file" "$dest_file"
            ;;
    esac
done

echo "Successfully synced $DEST_DIR (descriptions stripped, banners added)."
