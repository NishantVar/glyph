# `state.json` Schema

Location: `tmp/orchestrator/state.json` (gitignored).

This file is the durable source of truth for the Orchestrator. The Orchestrator re-reads it at the start of every scheduler turn (it is amnesic between turns by design).

## Top-level shape

```json
{
  "schema_version": 1,
  "issues": {
    "<issue-id>": { ... per-issue object ... },
    ...
  }
}
```

Issue IDs are stringified integers (`"1"`, `"2"`, etc.) for deterministic JSON serialization. The Orchestrator sorts by integer value when picking the next ready issue.

## Per-issue object

```json
{
  "title": "Workspace bootstrap & walking skeleton",
  "slug": "workspace-bootstrap-and-walking-skeleton",
  "status": "merged",
  "deps": ["<dep-id-1>", "<dep-id-2>"],
  "branch": "slice-1-workspace-bootstrap-and-walking-skeleton",
  "worktree": "../glyph-worktrees/slice-1-workspace-bootstrap-and-walking-skeleton",
  "pr_url": "https://github.com/NishantVar/glyph/pull/34",
  "rounds_used": 1,
  "blocked_iterations_in_last_round": 0,
  "started_at": "2026-04-28T22:10:00Z",
  "finished_at": "2026-04-28T22:54:00Z",
  "last_error": null,
  "execution_mode": "background"
}
```

### Fields

| Field | Type | Required | Notes |
|---|---|---|---|
| `title` | string | yes | From parsed slice header |
| `slug` | string | yes | kebab-case from title |
| `status` | enum | yes | See "Status enum" below |
| `deps` | string[] | yes | Issue IDs this slice is blocked by |
| `branch` | string | yes | Always `slice-{id}-{slug}` |
| `worktree` | string | yes | Path under `../glyph-worktrees/` |
| `pr_url` | string \| null | yes | Set when Issue-Agent opens a PR; null otherwise |
| `rounds_used` | int | yes | 0 before first dispatch; 1–4 thereafter |
| `blocked_iterations_in_last_round` | int | yes | 0–3; useful for diagnosis |
| `started_at` | ISO 8601 \| null | yes | Set on first dispatch |
| `finished_at` | ISO 8601 \| null | yes | Set on Issue-Agent return |
| `last_error` | string \| null | yes | One-sentence error from packet `summary`, on halt |
| `execution_mode` | enum | yes | `background` or `teammate` |

### Status enum

| Value | Meaning | Unblocks dependents? |
|---|---|---|
| `pending` | Some dependency isn't `merged` yet | — |
| `ready` | All deps merged, not yet dispatched | — |
| `dispatching` | Issue-Agent is currently in flight | No |
| `merged` | PR merged on `main` | **Yes** |
| `failed-round-4` | Reviewer wouldn't pass after round 4 | No (halt) |
| `gate-failed` | Gates failed twice in same round | No (halt) |
| `escalated` | Reviewer escalated, OR 3 BLOCKED iters in one round, OR `gh pr create` failed permanently | No (halt) |
| `timed-out` | 30-min wall clock exceeded | No (halt) |

## Initialization (first run)

Run the parser:

```bash
python skills/issue-list-orchestrator/scripts/parse_issues.py mvp-issues.md
```

The script outputs JSON of shape:

```json
{
  "issues": [
    {
      "id": "1",
      "title": "Workspace bootstrap & walking skeleton",
      "slug": "workspace-bootstrap-and-walking-skeleton",
      "deps": [],
      "acceptance": ["...", "..."],
      "prose": "Two-crate Cargo workspace...",
      "context_files": ["design/mvp-acceptance.md", "..."]
    },
    ...
  ]
}
```

Build `state.json` by mapping each parser issue to the per-issue object, with:

- `status: "ready"` if `deps` is empty, else `"pending"`
- `branch: "slice-{id}-{slug}"`
- `worktree: "../glyph-worktrees/slice-{id}-{slug}"`
- everything else `null` / `0`

The parser fields `acceptance`, `prose`, and `context_files` are **not** persisted to state.json — they're re-extracted from the parser on each dispatch (cheap, deterministic). state.json is a pure runtime-state file; the slice spec lives in `mvp-issues.md`.

## Validation rules (enforce on every read)

If state.json fails any of these on read, halt and tell the user (do not auto-repair):

1. `schema_version` must equal 1.
2. Every issue's `deps` must reference existing issue IDs.
3. The dependency graph must be acyclic.
4. Every issue's `status` must be one of the enum values.
5. `rounds_used` ∈ [0, 4]; `blocked_iterations_in_last_round` ∈ [0, 3].
6. If `status == "merged"`, `pr_url` must be non-null.
7. If `status == "merged"`, `finished_at` must be non-null.

## When to write state.json

Persist immediately on:

- Initial creation (first run).
- Status change for any issue.
- After reconciliation upgrades/downgrades.
- After a packet is parsed (status update + `pr_url` + `finished_at` etc.).

Do **not** persist mid-dispatch (e.g., during a round). The Issue-Agent's progress lives in the dossier, not in state.json. state.json is per-issue terminal-state only.

## Concurrency

The lockfile (`tmp/orchestrator/state.json.lock`) ensures only one Orchestrator session writes to `state.json` at a time. The Issue-Agent never writes `state.json` — only the Orchestrator does, on packet receipt.
