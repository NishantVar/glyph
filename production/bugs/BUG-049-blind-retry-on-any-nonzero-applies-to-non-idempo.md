# BUG-049: Blind retry-on-any-nonzero applies to non-idempotent gh pr create / pr merge in documented usage

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `.agents/skills/issue-list-orchestrator/scripts/gh_retry.sh:17-35`
**Found by:** gap:bundled-skill-tooling-scripts | **Audit date:** unknown-date

## Description

The wrapper retries on ANY non-zero exit (lines 34-48) and its documented usage (header lines 5-7) explicitly includes `gh pr create` and `gh pr merge`. These are not idempotent. Concrete trigger: `gh pr create` succeeds server-side (PR is opened) but the client returns non-zero due to a transient network drop while reading the response; the wrapper then retries, and the retry either opens a duplicate PR or fails with `a pull request already exists`, which is reported as a permanent failure to the caller (exit non-zero) even though a PR was successfully created. Same hazard for `pr merge`. The header acknowledges blind retry as deliberate for transient failures, but it is unsafe for these write operations as documented.

## Trigger / Reproduction

1. Call `bash gh_retry.sh gh pr create --base main --head <branch> --title ... --body ...`.
2. GitHub creates the PR server-side and returns a 201, but the network drops before the client finishes reading the response body, so `gh` exits non-zero.
3. The wrapper sleeps 1s and retries `gh pr create` with identical arguments.
4. GitHub now returns an error ("a pull request already exists for this branch"), which is also non-zero.
5. After max attempts, the wrapper exits non-zero — the caller sees failure even though the PR was created.

## Evidence

```bash
# Usage header (lines 5-7):
#   bash gh_retry.sh gh pr view <url> --json state
#   bash gh_retry.sh gh pr create --base main --head <branch> --title ... --body ...
#   bash gh_retry.sh gh pr merge <url> --squash --auto

# Retry loop (lines 34-48):
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
```

## Recommended Resolution

Either restrict the wrapper's documented usage to idempotent read/query commands (e.g. `gh pr view`, `gh issue view`), or make the wrapper idempotency-aware for create/merge (on a retry, first query whether the PR already exists / is already merged and treat that as success). At minimum, drop `gh pr create`/`gh pr merge` from the usage examples so callers don't wrap non-idempotent writes with blind retry.

## Verification Notes

The script at lines 5-7 explicitly documents `gh pr create` and `gh pr merge` as valid usage, and `issue-agent-prompt.md` lines 257-296 instructs agents to call both commands through `gh_retry.sh`. The retry loop (lines 34-48) blindly retries any non-zero exit. In the documented scenario — `gh pr create` succeeds server-side but the client gets a transient network error reading the response — the retry will attempt `gh pr create` again, returning exit 1 ("a pull request already exists"). The wrapper then reports permanent failure even though the PR exists. Severity is low because the trigger condition (network drop specifically between server write and client response read) is uncommon.
