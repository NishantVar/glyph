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

## Independent Agent Finding

**Verdict:** Reproduced. The production `acquire` path is a real non-atomic TOCTOU lock acquisition.

**Reproduction/Refutation:** I ran the production script by absolute path from an isolated scratch working directory, so its relative `LOCK="tmp/orchestrator/state.json.lock"` wrote only to `tmp/bug-089-repro/tmp/orchestrator/state.json.lock`. With an initially absent lock, 64 parallel `acquire` invocations were launched against the same lock path. On the first attempt, 5 invocations exited 0, meaning multiple callers independently believed they acquired the same lock.

**Evidence:**
- Graphify query for `issue-list-orchestrator check_lockfile lockfile acquire state.json orchestrator` located the orchestrator lock path and `scripts/check_lockfile.sh (acquire|release|check)`, and showed the Orchestrator calls the helper.
- Bounded source read confirmed `.agents/skills/issue-list-orchestrator/scripts/check_lockfile.sh:24-31` performs `[[ -e "$LOCK" ]]` before a separate `printf ... > "$LOCK"` write.
- Documentation read confirmed `.agents/skills/issue-list-orchestrator/references/state-schema.md` says the lockfile ensures only one Orchestrator session writes to `state.json` at a time.
- Targeted reproduction command summary: from `tmp/bug-089-repro`, run 50 attempts of `seq 1 64 | xargs -P64 ... bash "$script" acquire`, deleting the scratch-local lock before each attempt and stopping on multiple successes.
- Observed output: `reproduced iteration=1 successes=5`. The final scratch lock contained one overwritten writer record: `claude-orchestrator`, `acquired_at=2026-06-10T17:48:19Z`, `pid=10708`.

**Resolution Input:** Preserve the existing suggested resolution. Replace the check-then-write sequence with an atomic create-or-fail primitive, either a directory lock via `mkdir "$LOCK"` with metadata inside it, or a file lock using `set -C`/noclobber so creation maps to `O_CREAT|O_EXCL` semantics.
