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

If the user says "retry issue 61 with a note" or similar, capture the note and inject it into the round-1 Implementer prompt's `<reviewer-feedback-or-empty>` slot. Otherwise the slot is empty.

### `skip <issue-id>`

Mark the issue `merged` artificially (the user merged manually outside the orchestrator).

1. Look up the issue in state.json.
2. Run `bash skills/issue-list-orchestrator/scripts/gh_retry.sh gh pr list --base $TARGET_BRANCH --head <branch>` — try to find a corresponding PR. If found and merged, capture the URL.
3. Set `status: "merged"`, `pr_url` to the captured URL (or null if no PR), `finished_at` to current time, `last_error: null`.
4. Persist state.json.
5. Recompute which issues are now `ready` (any with all deps now merged).
6. Re-enter dispatch loop.

If the user says `skip <id>` but the branch doesn't appear merged, ask before proceeding: "Branch `<branch>` does not appear merged on GitHub. Are you sure you want to mark issue <id> as merged anyway? (yes/no)"

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

## Teammate spawn (always — this is the only mode)

Issue-Agents always run as teammates (top-level Claude sessions in their own tmux panes). This is required because the Issue-Agent must itself dispatch Implementer/Reviewer subagents via `Agent`, and only top-level sessions have `Agent` in their tool roster — not subagents spawned via `Agent(run_in_background: true)`.

Spawn sequence:

```
TeamCreate(team_name: "issue-<id>", description: "Issue-Agent for issue <id>")
Agent(
  team_name:     "issue-<id>",
  name:          "issue-agent",
  subagent_type: "general-purpose",
  description:   "Issue-Agent issue <id>",
  prompt:        <filled template>,
)
```

Notes:

- **`team_name` is what makes the spawn a teammate.** Without it, you'd get a regular subagent that lacks the `Agent` tool and cannot dispatch Implementer/Reviewer.
- **Do not pass `run_in_background: true`** — that's the subagent path.
- The teammate's terminal narrative does not propagate into your context. You only ingest the YAML packet it sends via `SendMessage`. Ignore intermediate messages and idle notifications.
- After the issue completes (merged or halted) and you've parsed the packet, tear the team down with the shutdown handshake described in `SKILL.md` "Context-budget rules" item 6: send a `shutdown_request` to the `issue-agent`, wait for the `shutdown_response`, then call `TeamDelete()`. This keeps `~/.claude/teams/` and `~/.claude/tasks/` from accumulating dead per-issue directories. If the issue halted (not merged), the user may want to inspect the teammate's pane first — defer the `TeamDelete` until the user issues the next `retry`/`skip` for that issue.

## Unknown commands

If the user says something that's not in the table above, ask once for clarification: "Unknown command. Did you mean `retry <id>` / `skip <id>` / `pause` / `status`?"

If the user is just talking conversationally ("looks good", "thanks", etc.) — acknowledge briefly and wait for the next command.
