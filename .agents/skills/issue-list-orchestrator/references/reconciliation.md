# Startup Reconciliation

Run this on every invocation, after the lockfile is acquired and before printing the status table. The goal: make `state.json` match reality before scheduling.

## Why reconcile?

Between sessions, things drift:

- The user may have manually merged a PR that was halted last session.
- A previous session may have crashed mid-dispatch, leaving an issue stuck at `dispatching`.
- Worktrees may have been deleted or moved by hand.
- A `merged` PR might actually be reverted or unmerged.

If the Orchestrator trusts stale state, it may dispatch on bad assumptions or fail to unblock dependents. Reconciliation makes the next session's first decisions correct.

## Rules (apply in order)

### Rule 1 — Confirm `merged` issues are actually merged

For every issue with `status: "merged"` and a non-null `pr_url`:

```bash
bash skills/issue-list-orchestrator/scripts/gh_retry.sh \
  gh pr view <pr_url> --json state,mergedAt
```

Expected: `state == "MERGED"`. If the PR is anything else (`OPEN`, `CLOSED`):

- **Downgrade** the issue to `escalated`.
- Set `last_error` to e.g. `"PR was unmerged or closed since last run"`.
- **Cascade-recompute** the dependent set, because un-merging an ancestor invalidates `ready` flags downstream:
  - For every issue J with `status` in `{ready, pending}`: if any dep `D ∈ J.deps` now has `status != "merged"`, set `J.status = "pending"`. If all deps are merged and J is currently `pending`, promote to `ready`.
  - For issues already `merged`: their own PR is still merged, even if a transitive ancestor is not. Leave their status alone, but flag them in the print summary as "transitively unmerged" so the user can decide whether to revert.
  - Halt-state issues (`failed-round-4`, `gate-failed`, `escalated`, `timed-out`) are unaffected — the user resolves them via `retry`/`skip`.

### Rule 2 — Detect manual-merge upgrade

For every issue in `escalated`, `failed-round-4`, `gate-failed`, or `timed-out` with a non-null `pr_url` (the Issue-Agent opens a PR on `escalated`; the others have null `pr_url` unless the user opened one manually):

```bash
bash skills/issue-list-orchestrator/scripts/gh_retry.sh \
  gh pr view <pr_url> --json state
```

If `state == "MERGED"`, **upgrade** to `merged`:

- Set `status: "merged"`.
- Set `finished_at` to the merge timestamp from `gh pr view --json mergedAt`.
- Recompute `ready` set (any dependent whose deps are now all merged).

For halt-state issues with **null** `pr_url` (user might have opened a PR for them manually outside the orchestrator), additionally try to find a PR by branch:

```bash
bash skills/issue-list-orchestrator/scripts/gh_retry.sh \
  gh pr list --base $TARGET_BRANCH --head <branch> --state merged --json url,mergedAt
```

If a merged PR exists for the branch, upgrade to `merged` and capture the URL.

### Rule 3 — Recover `dispatching` stuck state

Any issue in `dispatching` means the previous session crashed or was killed mid-dispatch. The Issue-Agent is gone; the spawn was never completed. Downgrade to `ready` for retry.

Also clear `started_at` (it's stale) and reset `rounds_used: 0`, `blocked_iterations_in_last_round: 0`. Leave the worktree alone — the Issue-Agent on next dispatch will adapt to whatever state it's in.

### Rule 4 — Worktree consistency

For each issue, check `<worktree-path>` exists on disk:

| state.status | worktree exists | What to do |
|---|---|---|
| `merged` | yes | Remove the worktree, then delete the local branch (idempotent — tolerate either being already gone): `bash` `if git worktree list | grep -q "<worktree-path>"; then git worktree remove --force "<worktree-path>"; fi; if git rev-parse --verify --quiet "<branch>" >/dev/null; then git branch -D "<branch>" 2>/dev/null || true; fi`. PR is squash-merged; branch is dead. |
| `merged` | no | Fine — already cleaned up. Continue. |
| halt state (`failed-round-4`/`gate-failed`/`escalated`/`timed-out`) | yes | Keep. The user inspects it. |
| halt state | no | Flag inconsistency. Print a warning row in the table: "issue <id>: halt-state but worktree missing". Do not auto-create. The user must `retry <id>` to redo, or `skip <id>` if they handled it manually. |
| `ready` / `pending` | yes | Stale worktree from a prior run that didn't get cleaned up. Remove: `git worktree remove --force <worktree-path>`. The next dispatch will create a fresh one. |
| `ready` / `pending` | no | Fine — nothing to clean. |
| `dispatching` | (already downgraded to `ready` by Rule 3 — re-evaluate as `ready`) | |

### Rule 5 — Branch consistency (lightweight)

Run `git branch --list 'issue-*'` once. For each branch: it should correspond to either a halt-state issue (worktree exists) or a recently-merged issue not yet cleaned up.

For branches that don't match any issue in state.json: warn but don't auto-delete. They might be the user's own work.

## When `gh` itself is unavailable

If `gh` calls exhaust all retry attempts in `scripts/gh_retry.sh` during reconciliation:

- Halt with the synthetic session-only halt called `gh-unavailable`.
- Print: `gh unavailable; pausing — re-invoke when network/auth recovers.`
- Release the lockfile (so next invocation can succeed).
- Exit.

This synthetic halt is **not** persisted to state.json — it's session-only. The next invocation will retry reconciliation from scratch.

## After reconciliation: persist

Write the updated state.json once, after all rules have been applied. Then move on to step 3 of the SKILL.md startup flow (print status table).
