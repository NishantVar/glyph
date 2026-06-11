# BUG-014: acquire reports success (exit 0) even when mkdir/lock-write fails

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `.agents/skills/issue-list-orchestrator/scripts/check_lockfile.sh:25-32`
**Found by:** gap:bundled-skill-tooling-scripts | **Audit date:** unknown-date

## Description

The script runs `set -u` only (no `set -e`) and never checks the return code of `mkdir -p "$(dirname "$LOCK")"` or of the `printf ... > "$LOCK"` write. If the parent directory cannot be created or the write fails (e.g. `tmp` exists as a regular file, read-only filesystem, disk full, permission denied), both commands fail but control still falls through to `exit 0`. Concrete trigger: a stray `tmp` file (or `tmp/orchestrator` being unwritable) — `mkdir -p` errors, the redirection errors, yet the script exits 0. The orchestrator then believes it holds the lock and proceeds to load/mutate state, while no lockfile actually exists on disk, so a second session would also acquire and the crash-recovery contract (lock survives crashes) silently breaks.

## Trigger / Reproduction

Create a plain file named `tmp` in the working directory, then call the script with `acquire`. `mkdir -p` prints "Not a directory", the `printf` redirection fails with the same error, yet the script exits 0. The orchestrator believes it holds the lock while no lockfile exists on disk.

## Evidence

```sh
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

Check failure of directory creation and the write before declaring success:

```sh
mkdir -p "$(dirname "$LOCK")" || { echo 'cannot create lock dir' >&2; exit 1; }
printf "claude-orchestrator\nacquired_at=%s\npid=%s\n" \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$$" > "$LOCK" || { echo 'cannot write lockfile' >&2; exit 1; }
```

An atomic-mkdir lock (using `mkdir` as the lock primitive, which is atomic on POSIX filesystems) would also naturally fix this since the lock creation itself is the success signal.

## Verification Notes

Confirmed empirically: running the script with a stray `tmp` file causes `mkdir -p` to fail with "Not a directory" and the `printf` redirect to also fail, yet the script exits 0 because there is no `set -e` and no error checks on either command. The lockfile does not exist on disk yet the caller receives exit 0. A second `acquire` call in the same broken state also exits 0 (the `[[ -e "$LOCK" ]]` check returns false because the path cannot be traversed), so two concurrent orchestrator sessions would both believe they hold the lock. The SKILL.md design contract explicitly states the lock must survive crashes, so silently creating a phantom lock breaks that contract.

## Independent Agent Finding

### Verdict

Reproduced. `acquire` reports success with exit 0 when `tmp` is a regular file, even though both lock-directory creation and lockfile writing fail.

### Reproduction/Refutation

Ran an isolated reproduction under `tmp/bug014-lock-repro/` so the repository lock path was not touched. A control run with no stray `tmp` file returned exit 0, created `tmp/orchestrator/state.json.lock`, and returned exit 1 on a second acquire. The broken run created a regular file at `tmp` before invoking `acquire`; both the first and second acquire returned exit 0 while no lockfile existed.

### Evidence

Commands run:

```sh
git status --short
nl -ba .agents/skills/issue-list-orchestrator/scripts/check_lockfile.sh | sed -n '1,90p'
rm -rf tmp/bug014-lock-repro && mkdir -p tmp/bug014-lock-repro/control tmp/bug014-lock-repro/broken && ...
```

Relevant output summary:

```text
CONTROL exit=0 lock_exists_test_exit=0 second_exit=1
CONTROL second stderr: lockfile already exists at tmp/orchestrator/state.json.lock|
BROKEN exit=0 lock_exists_test_exit=1 second_exit=0 second_lock_exists_test_exit=1
BROKEN stderr: mkdir: tmp: Not a directory|... line 30: tmp/orchestrator/state.json.lock: Not a directory|
BROKEN second stderr: mkdir: tmp: Not a directory|... line 30: tmp/orchestrator/state.json.lock: Not a directory|
```

Line inspection also confirmed the acquire branch runs `mkdir -p "$(dirname "$LOCK")"` and the `printf ... > "$LOCK"` redirect without checking either result before unconditional `exit 0`.

### Resolution Input

Keep the existing suggested resolution. The minimum behavior change should make any failed parent-directory creation or lockfile write return non-zero, and the lock should only be considered acquired after the success signal has been durably created.
