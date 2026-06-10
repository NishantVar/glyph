# BUG-089: Lockfile acquire is TOCTOU-racy (non-atomic [ -e ] + write), not real mutual exclusion

**Severity:** low | **Confidence:** medium | **Status:** confirmed
**Location:** `.agents/skills/issue-list-orchestrator/scripts/check_lockfile.sh:24-32`
**Found by:** gap:bundled-skill-tooling-scripts | **Audit date:** unknown-date

## Description

The `acquire` path tests `[[ -e "$LOCK" ]]` and, if absent, writes the lock with `printf ... > "$LOCK"`. These are two separate operations with no atomicity. The lockfile's entire documented purpose (SKILL.md Step 1: "Either another orchestrator session is active, or a previous session crashed") is to provide mutual exclusion between concurrent orchestrator sessions.

Concrete trigger: two orchestrator sessions invoke `check_lockfile.sh acquire` near-simultaneously; both pass the `-e` check while the file is still absent, both then `printf > $LOCK`, and BOTH report exit 0 (lock acquired). Two sessions then run against the same `state.json` concurrently, defeating the lock.

A proper atomic primitive (`mkdir "$LOCK"` which fails if the dir exists, `ln`, or `set -o noclobber` with `>`) is required for the guarantee the script claims. Severity is low in practice because the skill is human-driven and the race window (microseconds between stat and write) requires two orchestrators launching at the same millisecond, which is implausible in interactive use.

## Trigger / Reproduction

Two independent Claude Code sessions both invoke `bash check_lockfile.sh acquire` within the same millisecond window while no lock file currently exists. Both sessions pass the `[[ -e "$LOCK" ]]` check before either has written the file, so both proceed to `printf ... > "$LOCK"` and both exit 0, falsely reporting successful lock acquisition.

## Evidence

```bash
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
```

## Recommended Resolution

Use an atomic create-or-fail primitive. Simplest: replace the file-lock with a directory lock — `if mkdir "$LOCK" 2>/dev/null; then write metadata inside; else echo exists >&2; exit 1; fi`. Alternatively keep the file but use `set -o noclobber`: `( set -C; printf ... > "$LOCK" ) 2>/dev/null || { echo "lockfile already exists at $LOCK" >&2; exit 1; }`. Both map to atomic `O_CREAT|O_EXCL` semantics at the OS level and eliminate the TOCTOU window entirely.

## Verification Notes

The TOCTOU race between `[[ -e "$LOCK" ]]` and `printf > "$LOCK"` is structurally present at lines 26-31 and matches the described pattern exactly. Real-world severity is low rather than medium: the orchestrator's design is explicitly single-session, the lockfile's primary stated purpose is crash recovery detection (not high-throughput mutual exclusion), the race window is on the order of microseconds, and two orchestrators launching at the same millisecond is implausible in human-driven interactive use. The skill's own instructions tell users to manually verify before removing a stale lock. The proposed fix (`mkdir` atomic lock or `set -C` noclobber) is correct and cheap.
