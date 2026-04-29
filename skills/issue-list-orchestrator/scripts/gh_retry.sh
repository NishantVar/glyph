#!/usr/bin/env bash
# gh_retry.sh — wrap a `gh` invocation with retry-with-backoff (1s, 4s, 16s).
#
# Usage:
#   bash gh_retry.sh gh pr view <url> --json state
#   bash gh_retry.sh gh pr create --base main --head <branch> --title ... --body ...
#   bash gh_retry.sh gh pr merge <url> --squash --auto
#
# Behavior:
# - Up to 4 attempts.
# - Sleeps 1s after attempt 1, 4s after attempt 2, 16s after attempt 3
#   (no sleep after attempt 4).
# - Exit code: 0 on first successful attempt; non-zero on permanent failure.
# - All gh stdout/stderr is forwarded as-is so the caller can parse it.
# - On retry, prints a one-line warning to stderr (prefixed `[gh_retry]`).
#
# This wrapper does NOT introspect gh's output — any non-zero exit is a retry
# trigger. That's deliberate: gh failures are usually transient (network,
# rate-limit, auth refresh) and benefit from a blind retry. If the underlying
# error is permanent (e.g., bad command), all attempts will fail equivalently
# and the caller sees the same error.

set -u

if [[ $# -lt 1 ]]; then
    echo "usage: gh_retry.sh <command> [args...]" >&2
    exit 2
fi

attempt=1
max_attempts=4
delays=(1 4 16)  # delay before attempt 2, 3, and 4 respectively

while true; do
    "$@"
    rc=$?
    if [[ $rc -eq 0 ]]; then
        exit 0
    fi
    if [[ $attempt -ge $max_attempts ]]; then
        echo "[gh_retry] gave up after $max_attempts attempts (last rc=$rc): $*" >&2
        exit "$rc"
    fi
    delay="${delays[$((attempt - 1))]}"
    echo "[gh_retry] attempt $attempt failed (rc=$rc); sleeping ${delay}s then retrying" >&2
    sleep "$delay"
    attempt=$((attempt + 1))
done
