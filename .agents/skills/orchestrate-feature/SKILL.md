---
name: orchestrate-feature
description: Orchestrate end-to-end implementation of a multi-issue GitHub feature using a `dev-team` (designer + implementer + per-issue planner). Use when the user has a PRD issue with sibling issues sharing a `[feature-tag]` prefix and wants the team to land each issue as its own PR into an integration branch, with a final PR to `main` for human review.
---

# orchestrate-feature

## Inputs

- **PRD issue URL** (required). The PRD title carries a `[feature-tag]` prefix that identifies the feature; the same prefix on sibling issues defines the implementation slate.
- **Integration branch.** User-provided, or branch a new one from `main`. PR'd to `main` at end of slate.
- **Worktrees root.** Sibling of the repo (e.g. `../<repo>-worktrees/`).
- **Feature short-name** for artifact-dir naming. Derived from the tag; user may shorten.
- **Decision log** (optional). User opts in.

## Discovery + planning

1. Fetch the PRD issue. Extract the `[feature-tag]` prefix.
2. Find sibling issues with the same prefix → that's the slate.
3. Confirm slate, sequential order (dependency-aware), and PR strategy with the user before spawning anything.

## Worktree topology

- **The current/main checkout is not used for code work.** Detach its HEAD if necessary so the integration branch can be put in a worktree.
- Integration branch lives in its own worktree at `<worktrees-root>/integration/`. Per-issue branches merge here.
- Each issue gets its own worktree at `<worktrees-root>/issue-<NN>-<slug>/` branched from the integration branch.
- **Orchestrator creates the per-issue worktree** (one-time bootstrap) *before* spawning planner-NN, so planner-NN can be spawned with `cwd` = the worktree from the start. This is the only git op the orchestrator owns; planner-NN takes over for branch/status/add/commit/push from that point on.
- Untracked files in the main checkout are left alone.

## Working directory discipline

**The Bash tool resets cwd to the teammate's *spawn cwd* between commands**, and shell state (env vars, PATH) likewise does not persist. Spawn cwd is captured at `Agent`/`TeamCreate` time from the `cwd` field in `~/.claude/teams/<team>/config.json` and held in-memory afterward — editing the config for a running teammate has no effect. The runtime explicitly resets ("Shell cwd was reset to ...") after every command.

The only way to avoid `cd` prefixes is to **spawn each teammate with `cwd` set to the directory they'll be working in.** That's what the runtime resets to, so plain `git`, `cargo`, and `gh` Just Work from there.

**How to set spawn cwd.** The `Agent` tool doesn't expose a `cwd` parameter — spawned teammates inherit the orchestrator's *current session cwd*. So before spawning, the orchestrator switches its session into the target directory via `EnterWorktree(path: <abs path of registered worktree>)`, spawns the teammate(s), then returns via `ExitWorktree(action: "keep")`. The worktree must already be registered (i.e. `git -C <repo> worktree add ...` has been run); `EnterWorktree` only enters; it does not create. Result: every spawn under that EnterWorktree window inherits the target cwd; ExitWorktree is a no-op for those agents (they keep their cwd in their own configs) and just brings the orchestrator back. This is the mechanism behind every "spawn implementer/planner-NN with cwd = per-issue worktree" instruction below.

Per-role spawn `cwd`:

- **planner-NN** (per-issue, spawned after worktree creation): `cwd: "<per-issue-worktree-abs-path>"`. Plain `git status`, `git add`, `git commit`, `gh pr create` work with no flags.
- **implementer**: `cwd: "<per-issue-worktree-abs-path>"`. Plain `cargo build`, `cargo test`, `cargo clippy` work. If cargo isn't on the runtime's default PATH, call it as `~/.cargo/bin/cargo` (the path is invariant; don't try to persist `PATH`).
- **designer** (persistent across slate, reads from integration worktree's `design/`): `cwd: "<integration-worktree-abs-path>"`. Designer rarely uses Bash; for in-issue design edits, planner gives an absolute path to the per-issue worktree's `design/` file and Edit/Write absolute-path calls don't depend on cwd anyway.

Implementer "persistence across the slate" means *identity + charter persistence*, not literal in-memory persistence — its spawn cwd has to change per issue, so re-spawn implementer at every issue boundary with the new per-issue worktree as `cwd`. The spawn prompt carries the charter and any cross-issue context (e.g. `/tdd` contract), so the new shell picks up clean.

**Fallbacks** (only when respawn isn't possible — e.g. mid-issue worktree change):

- Git from any cwd: `git -C <abs path> <subcmd>`
- Cargo from any cwd: `cargo build --manifest-path <abs>/Cargo.toml -p <crate>` (also `test`, `clippy`)
- Read/Edit/Write: absolute paths always work regardless of cwd.

Bake the spawn-cwd discipline into each `Agent` call and into the per-issue planner briefing. The fallbacks are documented for completeness only — the default is spawn-with-correct-cwd, period.

## Artifacts

- All session artifacts live in `<main-checkout>/tmp/orchestrator/<feature-short-name>-orchestrator/`.
- Per-session: `session.md` (slate, status, PR URLs) — orchestrator-owned.
- Per-issue subdir `issue-NN/` — planner-owned:
  - `plan.md` — **thin** reference. Chunk list with per-chunk acceptance criteria + status checklist + a short cached-design-facts list (one-liner per fact with citation, e.g. *"`-> None` rejected uniformly across kinds — design/types.md §none-value, lines 81–96"*). Do NOT re-type the issue body or paste verbatim acceptance criteria — the issue is one `gh issue view` away. Do NOT quote design excerpts; the citation is enough for any teammate to look it up via designer.
  - `decisions.md` — **one rule: only log what is not obvious and cannot be figured out from the repo or git history.** If a future reader could recover the decision by reading the code, the commit message, the issue spec, the design docs, or `gh pr view`, it does not belong here. What survives the test: scope changes (pull-forwards, deferrals to follow-up issues), escalations to team-lead, cross-team conflicts that needed a call, `/tdd` exemption rationales, AC reinterpretations that diverge from the issue's literal wording. What fails the test: interface shapes, diagnostic wording, plumbing details, helper signatures, design-fact-check answers, "we reused existing ID X" — all recoverable from the repo. Restating the issue spec also fails the test. NOT a transcript: routine chunk handoffs are NOT logged.
  - `pr.txt` — final PR URL
  - `summary.md` — final readback against the issue's user stories

Logging discipline: keep planner artifacts thin. Plan + decisions + readback. Commits carry the narrative. If a section is verbatim from the issue or is just ratifying the issue spec, it doesn't belong in any of these files.

## Team: `dev-team`

Three roles. Each must know who the others are; introduce them on spawn.

### designer (persistent across the slate)
- The **only** team member that reads `design/**`. Answers questions; cites file + section.
- Edits files under `design/**` when an issue's implementation reveals the design needs updating — only on planner's request, only in the per-issue worktree. Edits land in the same PR as the code. Never edits source code.
- Never reads source code. Never runs builds/tests/git/gh. Never initiates contact with the orchestrator.
- **Reads design files just-in-time, never up-front.** On spawn, designer goes straight to idle — no "build a mental map" pre-read of `design/**` or the PRD body. When a planner-NN question arrives, designer reads (or greps) only the file(s) needed to answer that specific question, then cites. Pre-reading wastes tokens on paths that may never be queried, dilutes attention across the slate, and gets compacted away anyway. The orchestrator's spawn prompt for designer must NOT include a reading list — it should be: role + boundaries + "go idle, read JIT when asked." Same applies to the PRD body: designer fetches it via `gh issue view` only when a question references it.

### implementer (re-spawned per issue, persistent identity + charter)
- The **only** team member that reads or writes source code. Runs targeted tests/builds to verify their own changes work.
- Re-spawned at each issue boundary so its `cwd` matches the new per-issue worktree (see Working directory discipline). The spawn prompt carries the charter and cross-issue contracts (e.g. `/tdd`), so re-spawn is identity-preserving.
- Never reads `design/**` — asks designer for any design fact.
- Never touches git in any form (no `add`/`commit`/`push`/`diff`/`status`/`branch`/`worktree`/`stash`).
- Never initiates contact with the orchestrator.
- **TDD via `/tdd` skill, per chunk.** When planner-NN delegates a chunk that introduces or changes behavior, invoke the `/tdd` skill at the start of that chunk and follow it through. `/tdd`'s planning step is **self-driven** — pick the interface and behaviors-to-test list yourself, then run vertical-slice red→green→refactor (one test → one impl → repeat — never horizontal slicing). Don't route the TDD plan through planner-NN for approval; the chunk brief is the approval. Report chunk done after refactor + targeted-test verification.
- **When to ping planner-NN mid-chunk.** Only when something *deviates* from the chunk brief or surfaces a real conflict. Specifically: (a) a design fact you need that the brief didn't cover and you can't recover from designer's earlier answers; (b) the brief's scope or AC interpretation doesn't match what the code actually requires (genuine spec-vs-reality conflict); (c) you'd need to touch a file outside the brief's allowed list; (d) a pre-existing bug surfaces that's relevant to the chunk; (e) you want a `/tdd` exemption confirmed for a chunk that's borderline doc-only / pure-deletion. **Not** for: which exact helper signature to pick, what wording to put in a diagnostic, which span to attach, where to hook a sweep, plumbing-struct field names. Those are implementer's call — make the choice, write the test, move on. If a planner-NN reply isn't blocking, don't wait for one.
- **Exempt from `/tdd`:** pure-deletion chunks, doc-only chunks (no `crates/**` source change), and chunks that only edit `design/**` (those go to designer anyway).
- **No process residue in the code.** Comments, doc-comments, test names, error messages, and design-doc edits never reference the process that produced the change: no codex / review pass / pass-1 / pass-2 / finding / round / PR number / issue number / chunk / TDD round / "addresses review feedback" / "per planner brief". Those references rot the moment the review artifact is gone — and the artifact is always gone. Comments are for non-obvious WHY (hidden constraints, subtle invariants, surprising behavior); if the WHY is "because review said so," that's not a WHY, that's process noise — drop it. Same rule applies to `design/**` edits implementer prompts designer to make: the design doc describes the system, not how the system got there. When fixing a review finding, change the code so it stands on its own; the diff and commit message are the record of the fix.
- **Code navigation: graphify first, Read second.** If the repo has a pre-built graphify knowledge graph exposed via MCP (typical tools: `query_graph`, `get_neighbors`, `god_nodes`, `get_node`, `shortest_path`), use it to locate symbols, trace call sites, and understand structure — these return targeted facts, not file contents. Only Read source files when you need exact implementation details (a specific function's body to edit, a line you're about to change). Don't Read entire crates / packages or grep broadly when graphify can answer — that's a major token sink across an 8-issue slate. The repo's `CLAUDE.md` / `AGENTS.md` typically calls this out; treat it as the default code-navigation contract, not a suggestion.

### planner-NN (re-spawned per issue, unique name per issue)
- Orchestrates one GitHub issue end-to-end. **Manager / decision-maker** — operates like a team lead: gets information from designer or implementer when they need it; never reads code or design files directly.
- **Sole authority on deviations from the issue spec or PRD.** When designer or implementer spots a spec-vs-reality conflict, they surface it to planner; planner decides (or escalates to orchestrator if it's beyond their scope). Designer and implementer never silently deviate.
- Routes design questions through designer; routes any code question (read, structure, behaviour, diff content) through implementer.
- **Does not pre-approve TDD plans.** The chunk brief is the contract; once delegated, implementer runs `/tdd` self-driven. Planner-NN responds to implementer only when implementer escalates a genuine deviation (scope conflict, design fact, file-list expansion, surfaced bug, exemption request). Don't ask implementer for status, don't ask for the TDD plan, don't push back on implementation choices that fall inside the brief.
- **Design intake discipline:** before asking designer anything, scan the issue body and PRD. For each candidate question, locate the passage in the issue/PRD that already answers it and drop those — don't ask designer to ratify what the issue already states. Only send the residue: questions the issue genuinely doesn't cover (typically design-doc spec-vs-reality scans, naming conventions, registration mechanics, cross-doc rules, deferral notes that need adding).
- Owns **all** ongoing git operations: branch, status, add, commit, push (orchestrator does the one-time worktree creation so planner-NN can be spawned with `cwd` already set to the worktree).
- Runs the codex review cycle (minimum 3 passes) before opening the PR; triages output via implementer / designer; logs anything unresolved. See "Code review cycle" below.
- Opens the PR for its issue (target = integration branch).
- Talks to the orchestrator only at done (with PR URL) or hard-block.

### orchestrator (you, top-level)
- Spawns each planner with `cwd` = the per-issue worktree. Re-spawns implementer at each issue boundary with `cwd` = the new per-issue worktree (identity-preserving via spawn prompt). Designer is spawned once with `cwd` = integration worktree and stays alive across the slate.
- Maintains `session.md`. Merges PRs. Opens the end-of-slate PR to `main`.
- Talks only to the planner during normal flow.
- **Does not poll the planner.** After dispatching, wait silently. Planner-NN messages back only at done (PR URL) or hard-block — the runtime delivers that as a notification. Don't read planner artifacts (`plan.md`, `decisions.md`) mid-flight to "check progress"; don't send "how's it going?" messages; don't `TaskOutput` the planner. Re-reading `session.md` (which orchestrator owns) before a merge is fine; reading planner-owned files just to peek isn't.
- **Does not re-verify on done.** When planner-NN reports done with a PR URL, that report is trusted — they already ran `cargo build`, `cargo test`, and the full ≥3-pass codex review cycle before sending it. Don't re-run build/tests from the orchestrator; don't pull the diff and re-read it; don't ask implementer to re-confirm anything. The orchestrator's verification is exactly: `gh pr view <N>` to confirm the PR exists and is mergeable, then `gh pr merge`. Anything more burns tokens and distrusts the contract that makes the team work.

## PR + merge flow

- Each issue → its own PR `issue-NN-<slug>` → integration branch. Planner opens it; orchestrator merges it.
- After all issues merge into the integration branch → orchestrator opens the end-of-slate PR `<integration-branch>` → `main` for human review.

## Code review cycle

After all implementation chunks land (and before PR open), planner runs a review-fix cycle **a minimum of three times**:

1. `/codex:review --background --base <integration-branch> --scope branch --cwd <per-issue-worktree>` — codex dispatches a background agent and returns a list of findings.
2. Planner triages each finding: code fix via implementer, design-doc fix via designer, or marked out-of-scope with reason. Each fix round = its own clean commit.
3. Re-run the review.

Three passes is the floor even if an earlier pass comes back clean — codex can miss things on a single run.

After the final pass, planner triages remaining findings:

- **Minor / nitpicky / scope-deferred** — logged in `summary.md` (or a dedicated `review-log.md`) with reason (out of scope, deferred to follow-up issue, design ambiguity, etc.), then push + open the PR.
- **Non-trivial / potentially blocking** (correctness, security, real bugs, design contradictions — anything you'd hold a teammate's PR for) — planner does NOT open the PR. Escalate to orchestrator. Orchestrator **stops the slate** and surfaces to the human for input; resume only after human direction.

Pass `--cwd <per-issue-worktree>` explicitly even though planner's spawn cwd is the worktree — it makes the review's scope unambiguous in command history. `--background` is the default; `--wait` only for trivial diffs.

Orchestrator passes the integration-branch name and per-issue worktree path in each planner's briefing so the planner doesn't have to derive them.

## Hard rules

- **No teammate is shut down without explicit user permission.** Per-issue planners coexist with prior planners — give each a unique name (`planner-NN`).
- Designer never reads or edits source code; edits `design/**` only when planner requests an in-issue update. Implementer never reads `design/**` and never runs git. Planner never reads `design/**` or source code, and never edits.
- No `--no-verify`, `--force`, amend-pushed-commits, or rebase-onto-upstream by anyone.
- Orchestrator does not talk to designer/implementer directly during normal flow.
- Orchestrator does not poll planner-NN. The planner-only-at-done-or-blocked contract is load-bearing for token economy; the orchestrator must trust it.
- Orchestrator does not re-verify planner's done report. Build, tests, and codex passes have already happened inside the per-issue worktree; orchestrator's job is purely PR confirmation + merge.

## Done criteria (per issue)

- TBD — refine with the user before the first planner runs. As a placeholder: build clean, tests pass for affected crates, issue acceptance criteria addressed, no out-of-scope changes.

## Notes

This skill captures the non-trivial structural decisions only. Exact prompts, charter wording, commit-message style, chunk granularity, and similar tactical choices are left to the agent running this skill.
