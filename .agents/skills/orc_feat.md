---
name: orc_feat
description: 'Orchestrate end-to-end implementation of a PRD (or a single issue treated as a PRD-of-one) by delegating to a planner subagent, while the team lead handles only setup, mid-flow implementer lifecycle, and finalization.'
---

## Parameters

- **prd**. Required.

## Constraints

- **Must avoid:** Sacred context: never load implementation, planning, or review content into your own context. The orchestrator does only setup, mid-flow implementer spawn or kill or escalation relay, and finalization. Every other concern — including investigation, planning, coding, reviewing, and judgment calls — is delegated to a subagent. If something surfaces that is not on the allowed list, delegate it.
- **Must avoid:** The planner is the only interlocutor for implementation and review. Never SendMessage the implementer or codex directly. Never read the codex review output or the implementer's diff. If you need to influence either, route the request through the planner.
- **Require:** Before creating any branch, worktree, team, or teammate, confirm the PRD is accessible, every linked issue is readable, and the dependency order between issues is explicit. If any of these is missing, raise the specific gap to the human and stop. Do not begin orchestration with incomplete inputs.
- **Require:** Trust the chain. The implementer runs its own build and tests; the planner drives codex review through the iteration cap. Do not re-verify their work. Re-verifying wastes context and contradicts the delegation model.

## Context

- **orchestrator-role**

  You are the team lead. Your responsibilities are exclusively: validate inputs, create the isolated branch and worktree, spawn the team and planner, respond to planner requests for implementer spawn or kill and escalation relay, and finalize with a squash plus a pull request to main. You never plan, implement, or review work yourself.

- **comms-topology**

  Topology:
  - Team lead (you) talks to: planner only.
  - Planner talks to: team lead, codex (via the p2p skill over a forked terminal), implementer (via SendMessage).
  - Implementer talks to: planner only — it never reaches the team lead directly.
  - Codex talks to: planner only.
  At every spawn, supply each teammate the identifiers it needs to communicate with the others it is allowed to reach.

- **worktree-isolation-rule**

  All teammate work happens inside one worktree branched from main. Spawn each teammate with the worktree as its working directory so it never needs to cd. Branch and worktree naming are your judgment — keep them deterministic and derivable from the PRD identifier.

- **single-issue-as-prd**

  A single issue is treated as a one-issue PRD. Do not synthesize a separate PRD document around it — the issue itself contains all the detail a PRD would. The planner names the design-change log file with the `issue` prefix in that case.

## Steps

1. Verify the PRD or single-issue input at {prd} can be read in full from disk via Read or from GitHub via the gh CLI. If it cannot, tell the human exactly what is missing and stop without creating any branch, worktree, or team.
2. Enumerate every issue linked from the PRD at {prd} and confirm each can be fetched and read from the same source. On any failure, tell the human which issue could not be reached and stop without creating any branch, worktree, or team. A single-issue input satisfies this trivially.
3. Confirm the PRD at {prd} declares an explicit dependency order between its issues. If the ordering is missing, tell the human and stop without creating any branch, worktree, or team. A single-issue input satisfies this trivially.
4. Create a deterministically-named branch from main keyed off the PRD identifier, lay down a sibling worktree on that branch, and capture its absolute path. Refer to this result as worktree.
5. Create a new team that will host the planner and every implementer for this PRD. Refer to this result as team.
6. Spawn one planner teammate inside team with the worktree from step 4 as its working directory, initialized from the planner agent definition at .claude/agents/planner.md; in its spawn prompt give it the worktree path, the PRD reference, and your own team-lead teammate identifier so the planner can SendMessage you back, then record its teammate identifier. Refer to this result as planner.
7. Follow the coordinate-with-planner procedure below to dispatch the planner's mid-flow requests until the PRD-complete signal arrives or an escalation aborts the run.
8. Squash the issue-by-issue commits inside the worktree from step 4 into a small meaningful set, not necessarily one. Use your judgment for the final count and commit messages; the goal is a readable git history, not noise.
9. Push the branch from the worktree from step 4 to the remote, open a single pull request to main that bundles every change for the PRD, and tell the human the PR is ready with its URL — noting that worktree cleanup is the human's call after the merge.

### Procedure: coordinate-with-planner

1. Wait for the next SendMessage from the planner you spawned in step 6.
2. Decide which of the following applies and follow only that path:
   If the planner is asking the team lead to spawn an implementer for a specific issue:
   a. Spawn a new implementer teammate inside the team from step 5 with the worktree from step 4 as its working directory, initialized from the implementer agent definition at .claude/agents/implementer.md; in its spawn prompt give it the full issue text the planner supplied, the worktree path, and the planner's teammate identifier so it can SendMessage the planner with updates; then SendMessage the new implementer's teammate identifier back to the planner so the planner can address it.
   If the planner is asking the team lead to kill a named implementer teammate:
   a. Terminate the implementer teammate the planner has named in the request. Do not kill any other teammate.
   If the planner has reported an escalation that requires human attention:
   a. Surface the planner's escalation verbatim to the human, including the issue, the planner's stated reason, and the iteration count or other context the planner sent, and stop the orchestration without squashing or opening a pull request.
   If the planner has signaled that the PRD is complete with every issue signed off and committed:
   a. Exit the coordination loop and proceed to finalization.
   Otherwise:
   a. Ask the planner to clarify any request that does not match a known dispatch case. Never act on an unknown request.
3. Repeat the planner dispatch above for every subsequent message from the planner until either the PRD-complete signal arrives or an escalation has been relayed and the orchestration aborted.
