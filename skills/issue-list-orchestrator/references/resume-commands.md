# Resume / User Command Handler

When the Orchestrator is parked at a halt (or the user issues a command at startup), handle the user's input here. After resolving the command, re-enter the dispatch loop in `SKILL.md` (or stay parked if the command was `pause` / `status`).

## Commands

### `retry <issue-id>`

Re-dispatch the issue from round 1.

1. Look up the issue in `state.json`. It should be in a halt state (`failed-round-4`, `gate-failed`, `escalated`, `timed-out`). If not, tell the user the issue is in `<status>` and `retry` doesn't apply.
2. Set `status` back to `ready`. Reset `rounds_used: 0`, `blocked_iterations_in_last_round: 0`, `last_error: null`.
3. **Do NOT delete the worktree.** It may contain manual fixes from the user. The Issue-Agent will inspect it on dispatch.
4. **Do NOT delete the dossier folder.** Append a delimiter to each log file:
   ```
   ---
   ## Retry begins — <ISO-8601 timestamp>
   ---
   ```
   so subsequent entries show up clearly distinguished from prior runs.
5. Persist state.json.
6. Re-enter dispatch loop. The next dispatch will pick this issue up.

If the user says "retry slice 3 with a note" or similar, capture the note and inject it into the round-1 Implementer prompt's `<reviewer-feedback-or-empty>` slot. Otherwise the slot is empty.

### `skip <issue-id>`

Mark the issue `merged` artificially (the user merged manually outside the orchestrator).

1. Look up the issue in state.json.
2. Run `bash skills/issue-list-orchestrator/scripts/gh_retry.sh gh pr list --base main --head <branch>` — try to find a corresponding PR. If found and merged, capture the URL.
3. Set `status: "merged"`, `pr_url` to the captured URL (or null if no PR), `finished_at` to current time, `last_error: null`.
4. Persist state.json.
5. Recompute which issues are now `ready` (any with all deps now merged).
6. Re-enter dispatch loop.

If the user says `skip <id>` but the branch doesn't appear merged, ask before proceeding: "Branch `<branch>` does not appear merged on GitHub. Are you sure you want to mark slice <id> as merged anyway? (yes/no)"

### `pause`

Stop scheduling cleanly.

1. Persist state.json.
2. Release the lockfile: `bash skills/issue-list-orchestrator/scripts/check_lockfile.sh release`.
3. Print a short summary: how many merged, how many parked, how many remaining.
4. Exit. The next invocation re-acquires the lockfile and resumes.

### `status`

Print the table only. Do not dispatch.

1. Re-read state.json.
2. Print the same table from `SKILL.md` step 3.
3. **If `status` was the user's first/only command this session** (i.e., they invoked the skill, the table printed at startup as part of the normal flow, and they typed `status` rather than a dispatch command): treat this like `pause` — release the lockfile and exit. The user can re-invoke the skill cheaply if they want to dispatch.
4. **If `status` was issued during an active dispatch loop or after another command this session:** stay parked, hold the lockfile, wait for the next command (do not dispatch).

Rule of thumb: don't leak the lockfile across an end-of-session. If you're unsure whether the user is about to issue another command, prefer to release; re-acquiring on the next invocation is fast.

### `mode background` / `mode teammate`

Change the **session-level** execution mode for subsequent dispatches.

This persists in your in-memory state for the rest of the session (you do not write it to state.json — it's a session preference, not durable). When you next dispatch, use this mode unless overridden by `dispatch <id> --teammate` / `--background`.

If the user issues this in the middle of a parked queue, acknowledge and wait for the next command. Do not auto-dispatch from a `mode` command alone.

### `dispatch <issue-id> --teammate` / `--background`

One-off mode override. The next dispatch (and only the next) uses this mode; subsequent dispatches revert to the session-level (or project default) mode.

1. Look up the issue. It must be in `ready` state. (If it's halted, the user should `retry <id>` first.)
2. Apply the per-dispatch mode override.
3. Re-enter dispatch loop, which will pick up this issue next.

## Mode precedence

1. Per-dispatch override (`dispatch <id> --teammate`) — applies only to that one dispatch.
2. Session-level (`mode <bg|teammate>`) — persists across dispatches in the session.
3. Project default — `background`.

The Issue-Agent's contract (what it reads, what it writes, what it returns) is identical in both modes. Only the spawn mechanism differs.

## Teammate spawn (when mode is teammate)

Instead of `Agent(run_in_background: true)`, spawn the Issue-Agent as a teammate so the user can watch in its own terminal:

```
TeamCreate(team_name: "issue-<id>")
Agent(
  team_name: "issue-<id>",
  subagent_type: "general-purpose",
  description: "Issue-Agent slice <id>",
  prompt: <filled template>,
)
```

The teammate's terminal narrative is independent of your context. The Issue-Agent's prompt instructs it to emit only the structured packet as its final message — that's what you ingest. The runtime may surface incidental teammate notifications back to you; ignore non-packet content.

This is best-effort isolation, not airtight (per design doc §9 caveat). If you observe context bloat from teammate dispatches in practice, fall back to background mode and ask the user.

When the teammate completes, do **not** auto-delete the team. Leave it so the user can inspect the terminal afterward. The user disposes the team when ready.

## Unknown commands

If the user says something that's not in the table above, ask once for clarification: "Unknown command. Did you mean `retry <id>` / `skip <id>` / `pause` / `status` / `mode <bg|teammate>` / `dispatch <id> --teammate`?"

If the user is just talking conversationally ("looks good", "thanks", etc.) — acknowledge briefly and wait for the next command.
