#!/usr/bin/env bash
# check_lockfile.sh — lockfile lifecycle helper for the orchestrator.
#
# Usage:
#   bash check_lockfile.sh acquire   # create the lockfile if absent; exit 0
#                                    # exit 1 if already present
#   bash check_lockfile.sh release   # remove the lockfile if present; exit 0
#   bash check_lockfile.sh check     # exit 0 if absent, exit 1 if present
#
# The lockfile contains the current ISO-8601 timestamp + the literal
# "claude-orchestrator" so a human inspecting it can tell what created it.
#
# The lockfile is intentionally NOT auto-removed on stale detection — the user
# must manually `rm` it after confirming no other orchestrator is running. This
# is the design contract (see SKILL.md "Lockfile check").

set -u

LOCK="tmp/orchestrator/state.json.lock"

cmd="${1:-}"

case "$cmd" in
    acquire)
        mkdir -p "$(dirname "$LOCK")"
        if [[ -e "$LOCK" ]]; then
            echo "lockfile already exists at $LOCK" >&2
            exit 1
        fi
        printf "claude-orchestrator\nacquired_at=%s\npid=%s\n" \
            "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$$" > "$LOCK"
        exit 0
        ;;
    release)
        if [[ -e "$LOCK" ]]; then
            rm -f "$LOCK"
        fi
        exit 0
        ;;
    check)
        if [[ -e "$LOCK" ]]; then
            cat "$LOCK"
            exit 1
        fi
        exit 0
        ;;
    *)
        echo "usage: check_lockfile.sh acquire|release|check" >&2
        exit 2
        ;;
esac
