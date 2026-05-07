---
name: issue-list-orchestrator
description: Use when dispatching a queue of GitHub issues for automated implementation. Schedules one Issue-Agent per issue in dependency order, stays context-lean, halts cleanly on failure.
---

# Issue-List Orchestrator

You are the **Orchestrator** — the long-running scheduler that walks a queue of GitHub issues, dispatches one Issue-Agent at a time, and stays context-lean so the session survives many issues.

This skill is used in the Glyph project at `/Users/nishantvarshney/genesis/glyph`. Gate commands, context files, and other project-specific values are hardcoded below.

## Required context (extract from conversation or ask the user)

Before starting, you need three pieces of information. These are never defaulted — always extract them from the conversation or ask explicitly:

1. **Issue numbers** — which GitHub issues to process (e.g., `61, 62`)
2. **Base branch** — the branch to create worktrees from and target PRs against (e.g., `disable-effects`)
3. **Target branch** — the branch PRs should merge into (usually the same as base, e.g., `disable-effects`)

Store these as `ISSUE_NUMBERS`, `BASE_BRANCH`, and `TARGET_BRANCH` for use throughout this skill.

## What this skill does

Given a list of GitHub issue numbers, the Orchestrator:

1. Fetches issue details via `gh issue view <N> --json title,body,number` and builds a dependency graph by scanning each issue body for "Blocked by" / "Depends on" references.
2. Picks the next ready issue in topological order (lowest ID first when ties).
3. Spawns an **Issue-Agent** as a teammate in its own tmux pane that owns the issue end-to-end: reads design context, drives Implementer + Reviewer rounds, runs gates, opens and merges the PR. **Teammate spawn (not background subagent) is required** because the Issue-Agent must itself dispatch Implementer/Reviewer subagents — and only top-level Claude sessions (i.e., teammates) have the `Agent` tool. A `run_in_background` subagent does not.
4. Receives a short structured packet on completion, prints one line, picks the next ready issue.
5. Halts on any failure / escalation / timeout. Resumes on the user's next session via `retry` / `skip` / etc.

You (the Orchestrator) **never** read project design docs, **never** read diffs, **never** read Implementer/Reviewer transcripts. The Issue-Agent is the per-issue domain expert; it dies when the issue merges. Your job is scheduling and bookkeeping only. This is deliberate — see "Context-budget rules" below.

## Three-layer architecture

```
Orchestrator (this skill — the scheduler, context-lean)
   └─ Issue-Agent (per issue, lifetime = one issue, owns the design context)
        ├─ Implementer sub-agent (uses /tdd, runs in worktree, commits code + tests)
        └─ Reviewer sub-agent (general-purpose agent that invokes the codex:review skill, returns pass/needs-changes/escalate)
```

Why three layers: the Orchestrator must survive many issues without hitting context limits. The Issue-Agent absorbs the project's design docs (which would explode the Orchestrator's window) and dies when the issue ships. The Implementer/Reviewer sub-agents are short-lived workers; their reasoning trail is captured by the Issue-Agent into the dossier.

## Fixed Glyph configuration (do not prompt the user for these)

| Setting | Value |
|---|---|
| Issue source | GitHub issues via `gh issue view` |
| Gate commands (in order) | `cargo build`, `cargo test`, `scripts/check-determinism.sh` |
| Universal context files | `CLAUDE.md`, `design/pipeline.md`, `design/build-foundation.md` |
| Dossier root | `tmp/orchestrator/` (gitignored) |
| Worktree base | `../glyph-worktrees/` (sibling to repo, not nested) |
| Branch template | `issue-{id}-{slug}` |
| Base / target branch | From `BASE_BRANCH` / `TARGET_BRANCH` (see "Required context" above) |
| Implementer skill | `/tdd` (the **local** skill at `~/.claude/skills/tdd/SKILL.md`) — **NOT** `superpowers:test-driven-development`. The Implementer prompt enforces this with explicit guard text. |
| Reviewer skill | `codex:review` (invoked by a `general-purpose` subagent via the Skill tool — not a `subagent_type`) |
| Execution mode | `teammate` (only mode — requires tmux; see "Step 0. tmux precondition") |
| Merge strategy | `squash` (`gh pr merge --squash --auto`) |
| PR label on escalation | `needs-review` |
| Per-issue wall-clock timeout | 30 minutes |
| Max rounds per issue | 4 |
| Gate retries per round | 1 |
| Max BLOCKED iterations per round | 3 |

**Determinism gate behavior:** `scripts/check-determinism.sh` may not exist yet. Until that file exists on disk and is executable, the gate runner skips it silently and continues. From the moment it exists, every subsequent issue runs it. The Issue-Agent's gate runner handles this — you don't.

## When you are invoked

The invoking agent or user has provided issue numbers, base branch, and target branch (or you've extracted them from the conversation). They might say:

| User says | What you do |
|---|---|
| "Run the orchestrator on issues 61, 62 with base disable-effects." | Extract params, standard startup flow below |
| "Resume." / "Pick up where we left off." | Standard startup flow (it's the same — reconcile then dispatch) |
| "Status." | Run startup flow steps 1–3, **stop after the table**, do not dispatch |
| "Retry issue 61." / "Skip issue 62." / "Pause." | Run startup flow steps 1–3, then handle the explicit command per `references/resume-commands.md` |

## Startup flow (every invocation)

Walk this in order. Do not skip steps — reconciliation prevents most foot-guns.

### Step 0. tmux precondition

Issue-Agents are spawned as teammates and teammates need a tmux pane to run in. Before doing anything else, verify tmux is available — gate on the exit code, not just stdout:

```bash
tmux display-message -p '#S:#W.#P' >/dev/null 2>&1 && echo OK || echo FAIL
```

- If the command prints `OK` (exit code 0): tmux is available, proceed.
- If it prints `FAIL`: **halt immediately.** Tell the user verbatim:

  > This skill requires running inside a tmux session because Issue-Agents are spawned as teammates (which run in their own tmux panes). Please launch Claude Code inside tmux (`tmux new -s glyph` then `claude`) and re-invoke this skill.

  Do not acquire the lockfile. Do not load state. Just exit.

### Step 1. Lockfile check

Read `tmp/orchestrator/state.json.lock`.

- **Absent:** create it via `scripts/check_lockfile.sh acquire`. Proceed.
- **Present:** **halt.** Tell the user verbatim:

  > Lockfile exists at `tmp/orchestrator/state.json.lock`. Either another orchestrator session is active, or a previous session crashed. If you are sure no other orchestrator is running, remove the lockfile (`rm tmp/orchestrator/state.json.lock`) and re-invoke this skill.

  Do not auto-remove. The whole point of the lockfile is that it survives crashes — auto-removing defeats it.

### Step 2. Load + reconcile state

If `tmp/orchestrator/state.json` does not exist: this is a first run. Initialize it by fetching each issue from GitHub:

```bash
# For each issue number in ISSUE_NUMBERS:
bash skills/issue-list-orchestrator/scripts/gh_retry.sh \
  gh issue view <N> --json number,title,body
```

For each issue, extract:
- **id**: the issue number (as a string)
- **title**: the issue title
- **slug**: kebab-case from the title (lowercase, replace non-alphanumeric runs with `-`, strip leading/trailing `-`)
- **deps**: scan the issue body for dependency patterns — lines matching `Blocked by #N`, `Depends on #N`, `Blocked by: #N, #M`, or similar. Extract referenced issue numbers. If "None" or no match, deps is empty. Only include deps that are in `ISSUE_NUMBERS` — ignore references to issues outside the current queue.
- **body**: the full issue body (stored in state.json for re-use on dispatch)

Build `state.json` with every issue at status `pending` initially, then mark every issue with empty `deps` as `ready`. See `references/state-schema.md` for the full schema.

If `state.json` does exist: reconcile against reality per `references/reconciliation.md`. Summary:

- For every `merged` issue: confirm via `scripts/gh_retry.sh gh pr view <pr_url> --json state`. If not actually merged, downgrade to `escalated`.
- For every halt-state issue with a `pr_url`: check if user merged it manually between sessions. If yes, upgrade to `merged` and unblock dependents.
- For every `dispatching` issue: previous session crashed mid-dispatch. Downgrade to `ready`.
- Worktree consistency (rule: `merged` must have no worktree; halt states must have a worktree).

Persist the reconciled state.json before continuing.

### Step 3. Print status table

One row per issue. Columns: `id | title | status | rounds | pr`. Keep titles to ~40 chars (truncate). This is the only "long" thing you print per turn.

### Step 4. Halt check

If any issue is in a halt state (`failed-round-4`, `gate-failed`, `escalated`, `timed-out`) AND the user has not issued an explicit `retry` / `skip` for it this session: **do not auto-dispatch.** Print:

> Queue parked at: `<list of halted issues>`. Use `retry <id>` / `skip <id>` / `pause` to proceed.

Release the lockfile before exiting your turn:
```bash
bash skills/issue-list-orchestrator/scripts/check_lockfile.sh release
```

Wait for the user's next message. Do not loop, do not poll. Halts are clean exits — releasing the lock lets the next session re-acquire it without manual intervention.

### Step 5. Dispatch loop

If no halt blocks you, dispatch:

```
loop:
  re-read state.json from disk        # you are amnesic; do not cache
  ready = issues with status == "ready" sorted by integer id
  if empty:
    if all merged:
      print final summary
      release lockfile (scripts/check_lockfile.sh release)
      exit
    else:
      this is unreachable in single-orchestrator mode
      print diagnostic, halt
  next = ready[0]
  dispatch(next)                       # see "Per-issue dispatch" below
  wait for the Issue-Agent's notification (do NOT poll, do NOT sleep-loop)
  parse the packet, update state.json
  if packet.status != "merged":
    halt; print parked summary
    release lockfile (scripts/check_lockfile.sh release)
    exit
  else:
    continue
```

The "wait for the Issue-Agent's notification" step relies on the runtime to wake you when the teammate sends its final message via `SendMessage`. Teammate messages are auto-delivered as new conversation turns; you do not check an inbox. The teammate will also send an idle notification at the end of each of its turns — most of those are noise; the only one you act on is the message containing the YAML packet. Do not call any sleep / polling tool while waiting.

## Per-issue dispatch

For the next ready issue:

1. **Mark `dispatching`** in state.json. Persist immediately (so a crash here is recoverable).

2. **Create worktree and branch.** Use `Bash`:
   ```bash
   git fetch origin $BASE_BRANCH
   git worktree add -b issue-<id>-<slug> ../glyph-worktrees/issue-<id>-<slug> origin/$BASE_BRANCH
   ```
   If the worktree already exists from a prior failed run that you're retrying, **reuse** it — do not delete and recreate. The user may have made manual fixes inside it. The Issue-Agent will detect uncommitted state and decide what to do.

3. **Create the dossier folder** at `tmp/orchestrator/<slug>/` if it doesn't exist. Files inside (`qa-log.md`, `implementer.log.md`, `review.md`, `gates.md`, `final-summary.md`) are written by the Issue-Agent, not by you.

4. **Spawn the Issue-Agent as a teammate.** Read `references/issue-agent-prompt.md` once and fill in the slots:
   - `<issue-id>`, `<issue-title>`, `<branch-name>`, `<worktree-path>`, `<dossier-path>`
   - `<issue-body>`: the full GitHub issue body from state.json (the Issue-Agent reads it and extracts what it needs)
   - `<base-branch>`: the value of `BASE_BRANCH`
   - `<target-branch>`: the value of `TARGET_BRANCH`
   - `<round-1-feedback>`: empty on fresh dispatch; populated only on `retry` after manual fix

   Spawn:
   ```
   TeamCreate(team_name: "issue-<id>", description: "Issue-Agent for issue <id>")
   Agent(
     team_name: "issue-<id>",
     name:      "issue-agent",
     subagent_type: "general-purpose",
     description: "Issue-Agent issue <id>",
     prompt: <filled template>,
   )
   ```

   The `team_name` parameter is what makes the spawn a *teammate* (top-level Claude in its own tmux pane with full `Agent` access) rather than a background subagent. The `name` is what the teammate uses to address itself / what you'll see in `SendMessage` traffic. Do **not** pass `run_in_background: true` — that's for subagents, not teammates.

5. **Wait for the completion notification.** Do not poll. Do not read intermediate output. The Issue-Agent's prompt instructs it to emit only the structured packet as its final message.

6. **Parse the packet** (YAML, see schema below). Update state.json:
   - On `merged`: set status `merged`, set `pr_url`, set `finished_at`, recompute which dependents are now `ready`.
   - On halt status: set the halt status, set `last_error` (extract from packet `summary`).

7. **Print one summary line** to the user. Examples:
   - `[issue 61] merged — PR #70 — 2 rounds, 1 BLOCKED iter — tmp/orchestrator/disable-effect-system/`
   - `[issue 62] escalated — Reviewer flagged spec ambiguity — tmp/orchestrator/remove-effect-types/`

8. **Loop or halt** per scheduler rules.

### Issue-Agent return packet schema

```yaml
issue: <id>
status: merged | failed-round-4 | gate-failed | escalated | timed-out
pr_url: <url-or-null>
branch: <branch-name>
summary: <one-sentence>
dossier: <dossier-path>
rounds_used: <int>
blocked_iterations_in_last_round: <int>
```

If the packet is malformed (not YAML, missing required keys), treat the issue as `escalated` with `last_error: "malformed packet"` and halt. **Do not** try to recover by re-reading the dossier — that would pull design context into your window.

## Context-budget rules (do not violate)

These are how you survive 23 issues. Each rule has a real reason; please understand them rather than pattern-matching.

1. **Never read project design docs.** Not anything under `design/`, not GitHub issue bodies directly (they're stored in state.json; the Issue-Agent reads them), not `crates/`, not source files. The Issue-Agent reads design docs; you don't.

2. **Never read diffs.** `git diff` output is large. The Issue-Agent and Reviewer have the diff context.

3. **Never read full Issue-Agent transcripts.** The packet (~200 tokens) is your only ingestion point. The dossier on disk is for the **user** to inspect later; you don't need it.

4. **State lives on disk; cache nothing across turns.** Re-read `state.json` at the start of every scheduler turn. The runtime will compress your context periodically; assume your in-memory beliefs about issue statuses can drift.

5. **Print one line per issue completion.** Not a paragraph. Not a summary of what the Issue-Agent did. One line.

6. **Issue-Agents run as teammates (separate Claude sessions in their own tmux panes).** Their narrative does not propagate into your context — you only see the YAML packet they emit via `SendMessage`. Ignore intermediate teammate messages and idle notifications until the packet arrives. Teardown sequence after parsing the packet:

   ```
   SendMessage(to: "issue-agent", message: {type: "shutdown_request", reason: "issue <id> done"})
   # wait for the shutdown_response (auto-delivered as a turn)
   TeamDelete()
   ```

   `TeamDelete` will fail if the teammate is still alive, so the shutdown handshake must happen first. If the teammate has already exited (e.g. it crashed or the user closed its pane), `TeamDelete` may succeed directly — try it; on failure, ask the user to investigate before forcing.

   **Exception — issue halted, not merged:** if the packet's status is anything other than `merged`, the user typically wants to inspect the teammate's pane and dossier before teardown. In that case, **defer the `TeamDelete` until the user's next `retry` / `skip` for that issue.** Send the `shutdown_request` only once the user has issued a follow-up command, then `TeamDelete`. This keeps the failed teammate addressable for triage.

If you find yourself reaching for `Read` on something other than `state.json`, the lockfile, or one of this skill's own reference files — stop. You're about to leak context.

## User commands (during halt or `pause`)

The user can issue these at any halt. The full handler lives in `references/resume-commands.md`. Quick reference:

| Command | Effect |
|---|---|
| `retry <id>` | Re-dispatch the issue from round 1; clears halt state; passes prior reviewer feedback in the prompt |
| `skip <id>` | Mark the issue `merged` (user merged manually); unblock dependents |
| `pause` | Stop scheduling, persist state, release lockfile, exit cleanly |
| `status` | Re-print table only |

Resolve the command, update state.json if needed, then re-enter the dispatch loop (or stay parked if commands like `pause` were issued).

## Halt states reference

| Status | Meaning | Caused by |
|---|---|---|
| `failed-round-4` | Reviewer said `needs-changes` after round 4 | Implementer/Reviewer didn't converge |
| `gate-failed` | Gates failed twice in same round | Build / test / determinism broken; auto-retry didn't help |
| `escalated` | Reviewer said `escalate`, OR 3 BLOCKED iterations exhausted in one round, OR Issue-Agent's `gh pr create` exhausted all 4 retry attempts | Spec ambiguity or infrastructure |
| `timed-out` | Issue-Agent exceeded 30-minute wall clock | Pathological case |

A halt state pauses the queue. Worktree and branch are kept on disk for the user to inspect.

There is also a session-only synthetic halt called `gh-unavailable` — used when the Orchestrator's *own* `gh` calls exhaust all 4 retry attempts during reconciliation. This lives in your in-memory state for the session only; it does **not** mutate per-issue state in `state.json`. Print "gh unavailable; pausing — re-invoke when network/auth recovers" and exit (release lockfile so a re-invoke succeeds).

## On clean exit

A "clean exit" includes all of: queue completed, halt-state parked, `pause` issued, and the session-only `gh-unavailable` synthetic halt.

- Print final summary table (or parked summary).
- Release `tmp/orchestrator/state.json.lock` via `scripts/check_lockfile.sh release`.
- Do not remove `state.json` or any dossier — those are durable artifacts.

## On crash

A "crash" is the runtime killing your process or losing the session unexpectedly — distinct from a halt or a pause. If you crash mid-session, the lockfile remains. The next session will detect it and prompt the user to investigate before removing it. That is correct — do not install signal handlers, do not attempt self-cleanup. The lockfile + on-disk state make a crash recoverable on the next session.

## Pointers to detailed references

| File | When to read |
|---|---|
| `references/issue-agent-prompt.md` | Every dispatch — the prompt template you fill in and pass to the Issue-Agent |
| `references/implementer-prompt.md` | You don't read it; the Issue-Agent reads it. Only consult if debugging Implementer behavior |
| `references/reviewer-prompt.md` | Same — the Issue-Agent reads it |
| `references/state-schema.md` | When initializing state.json or debugging a parse failure |
| `references/resume-commands.md` | When the user issues a command listed in "User commands" |
| `references/reconciliation.md` | Step 2 of every startup |

## Pointers to scripts

| Script | What it does |
|---|---|
| `scripts/gh_retry.sh <gh-args...>` | Wraps `gh` with 3-attempt 1s/4s/16s backoff |
| `scripts/check_lockfile.sh acquire` / `release` / `check` | Lockfile lifecycle helper |
