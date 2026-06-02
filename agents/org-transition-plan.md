# Glyph Org Transition Plan

Durable approved org truth for the Glyph repo team. Source of record for the
seated roster, ownership, gates, day-one artifact contract, and bridge status.

- Run id: `glyph-maya-all7-eval-swap-2026-06-01`
- Source proposal: `<flux>/experiments/flux-maya-all7-eval-swap-2026-06-01/codex/glyph/maya-draft-03.md`
- Eval result: `<flux>/experiments/flux-maya-all7-eval-swap-2026-06-01/codex/glyph/eval-03.yaml` (`pass`, iteration 3, no blocking findings)
- Handoff: `<flux>/experiments/flux-maya-all7-eval-swap-2026-06-01/codex/glyph/ari-handoff.md`
- Materialized by: Ari (Agent Architect) from the approved Maya/human handoff.

`<flux>` is the Flux installation root, supplied by the invoker; these source
paths live in the Flux installation, not in this repo.

## Approved Roster and Display-Title Mapping

| Display title | agent_id | Codex port | Claude port |
| --- | --- | --- | --- |
| Language Designer | `language-designer-agent` | `.codex/agents/language-designer.toml` | `.claude/agents/language-designer-agent.md` |
| Compiler Engineer | `compiler-engineer-agent` | `.codex/agents/compiler-engineer.toml` | `.claude/agents/compiler-engineer-agent.md` |
| Integration Engineer | `runtime-integration-engineer-agent` | `.codex/agents/runtime-integration-engineer.toml` | `.claude/agents/runtime-integration-engineer-agent.md` |
| QA Engineer | `evaluation-engineer-agent` | `.codex/agents/evaluation-engineer.toml` | `.claude/agents/evaluation-engineer-agent.md` |
| Release Engineer | `release-engineer-agent` | `.codex/agents/release-engineer.toml` | `.claude/agents/release-engineer-agent.md` |

Technical `agent_id`s are unchanged, kebab-case, and routable; display titles are
ordinary human role names. `workflow-lead-agent`, `issue-planner-agent`,
`reviewer-agent`, and `verifier-agent` are **not** seated as project roles; they
appear only as bootstrap adapters in gate coverage. The repo's pre-existing
generic ports (`.claude/agents/planner.md`, `reviewer.md`, `implementer.md`,
`probe-*.md`, `.codex/agents/planner.toml`, `reviewer.toml`) are Glyph's own
artifacts and are left untouched.

## Installed Bootstrap Adapter Ports (mechanical, not seated)

Per the Agent Architect bootstrap step, the four lifecycle adapter ports are
copied from the Flux installation into this repo for repo-local gate dispatch.
This is a standing **mechanical install, not a roster seating**: these adapters
do not appear in the roster, ownership matrix, day-one artifact contract, or
runtime-port bridge plan, and they own no work surface. They exist only so the
dispatched gates (planning, review, verification) have repo-local coverage and so
`workflow-lead-agent`'s `can_invoke` resolves locally at dispatch.

| Adapter | Claude port | Codex port |
| --- | --- | --- |
| `workflow-lead-agent` | `.claude/agents/workflow-lead-agent.md` | `.codex/agents/workflow-lead.toml` |
| `issue-planner-agent` | `.claude/agents/issue-planner-agent.md` | `.codex/agents/issue-planner.toml` |
| `reviewer-agent` | `.claude/agents/reviewer-agent.md` | `.codex/agents/reviewer.toml` |
| `verifier-agent` | `.claude/agents/verifier-agent.md` | `.codex/agents/verifier.toml` |

`workflow-lead-agent`'s `can_invoke` (Claude) and invocation prose (Codex) were
materialized from the gate coverage matrix to reach the five seated owners plus
the fallback adapters.

**Resolved — reviewer adapter Codex port.** The Flux reviewer adapter's Codex
name is `reviewer` (file `reviewer.toml`), which originally collided with Glyph's
pre-existing `.codex/agents/reviewer.toml`. That collision was cleared by the
repo owner removing Glyph's own generic adapter configs in the working tree
(`.codex/agents/reviewer.toml`, `.codex/agents/planner.toml`, and
`.claude/agents/reviewer.md` all show as deletions), so the path was free. The
Flux reviewer adapter is now installed at `.codex/agents/reviewer.toml`,
completing all four adapters in both runtimes. Note for the Integration Engineer:
Glyph's `.codex/hooks.json` / multi-agent gate hooks that referenced the former
generic `reviewer`/`planner` ports may need a follow-up pass.

## Work-Surface Ownership Matrix

One primary owner per path pattern; adjacent roles are reviewers/consults.

| Work surface | Owner |
| --- | --- |
| `README.md`, `GLYPH_LANGUAGE_GUIDE.md`, `assets/**` | Language Designer |
| `design/**`, `docs/reference/**`, `docs/AGENTS.md`, `docs/CLAUDE.md` | Language Designer |
| `mvp-issues.md`, `specs/**` | Language Designer |
| `AGENTS.md` (canonical repo index), root `CONTEXT-MAP.md` | Language Designer |
| `crates/glyph-core/**`, `crates/glyph-cli/**`, compiler/CLI tests | Compiler Engineer |
| `tests/**`, `examples/**` | Compiler Engineer |
| `Cargo.toml`, `Cargo.lock`, crate manifests | Compiler Engineer (Release consult on release metadata) |
| `llm_expand_pass.md`, `REPAIR_PASS_SPEC.md`, compiler architecture docs/ADRs/TODOs, `docs/superpowers/**` | Compiler Engineer |
| `crates/glyph-lsp/**`, `tree-sitter-glyph/**`, `editors/vscode/**` | Integration Engineer |
| `.agents/commands/**`, `.agents/skills/glyph/**`, orchestration support skills | Integration Engineer |
| `.agents/skills/install_glyph_editor_extension.{glyph,md}` | Integration Engineer (Release consult on install-path/user-machine safety) |
| `.claude/**` and `.codex/**` runtime-port mechanics, `.mcp.json`, `scripts/sync_commands_no_desc.sh` | Integration Engineer |
| `agents/**` scaffold except `agents/quality/roundtrip/**` | Integration Engineer |
| `.agents/skills/e2e_tests/**`, `agents/quality/roundtrip/**` | QA Engineer |
| `.github/workflows/**`, release/install scripts (except `scripts/sync_commands_no_desc.sh`) | Release Engineer |
| `.agents/skills/install/**`, `.agents/skills/release/**` | Release Engineer |
| `CONTRIBUTING.md`, `.gitignore`, `.gitattributes`, `LICENSE`, `release/**`, cross-surface todo triage | Release Engineer |
| Role behavioral contract inside any `.claude/agents/**` / `.codex/agents/**` port | the corresponding role (not Integration Engineer) |
| Roadmap, prioritization, licensing, public positioning | Human |

## Gate Coverage Matrix

| Gate / surface | Primary role | Fallback adapter | Status |
| --- | --- | --- | --- |
| Planning / all surfaces | Owning surface role supplies route and constraints | `issue-planner-agent` when route unclear | intentional-fallback |
| Implementation / language docs, design, specs, reference, public assets | Language Designer | None | no gap |
| Implementation / compiler core, CLI, compiler tests, pass specs | Compiler Engineer | None | no gap |
| Implementation / LSP, tree-sitter, editor, agent/runtime artifacts | Integration Engineer | None | no gap |
| Implementation / round-trip QA harness and QA artifact policy | QA Engineer | None | no gap |
| Implementation / CI, release, install, generated/local governance | Release Engineer | None | no gap |
| Review / semantic or public-contract changes | Language Designer | `reviewer-agent` for supplemental general review | no gap |
| Review / compiler, integration, or release/ops changes | Peer owning engineer when boundary is implicated | `reviewer-agent` | intentional-fallback |
| Review / evaluation/QA harness changes | Integration Engineer (mechanics); Language/Compiler Engineer (semantic drift policy) | `reviewer-agent` | no gap |
| Verification / any nontrivial implementation | Owning role supplies commands and acceptance claims | `verifier-agent` strict atomic-claim verifier | intentional-fallback |
| Domain QA / round-trip semantic drift surface | QA Engineer | None | no gap |
| Domain QA / deterministic compiler behavior (no semantic-drift judgment) | Not separately run; strict verifier applies | None | no gap |
| Human approval / license, roadmap, high-risk policy | Human | None | no gap |

Implementation has no generic fallback: every in-scope implementation surface has
a named seated owner. QA is not a verifier duplicate — the strict verifier owns
current-run pass/fail evidence; the QA Engineer owns semantic-drift strategy,
coverage, chronic-drift policy, and qualitative findings.

## Day-One Artifact Contract

| Role | Durable writes | Artifact home / topology |
| --- | --- | --- |
| Language Designer | Language guide, design notes, reference contracts, accepted specs, context-map updates, public assets | `design/**`, `docs/reference/**`, `specs/**`, `assets/**`; root `CONTEXT-MAP.md`; org decisions in `agents/decisions/**` |
| Compiler Engineer | Rust compiler/CLI changes, compiler tests, diagnostics docs, compiler architecture updates | `crates/glyph-core/**`, `crates/glyph-cli/**`, `docs/architecture/**` (compiler), `docs/superpowers/**`, pass specs, `tests/**`, `examples/**` |
| Integration Engineer | LSP/tree-sitter/editor changes, `.agents/**` product artifacts (except QA/release-owned subtrees), runtime-port mechanics, generated command-copy updates | `crates/glyph-lsp/**`, `tree-sitter-glyph/**`, `editors/vscode/**`, `.agents/**`, `.claude/**`, `.codex/**`, `agents/**` scaffold |
| QA Engineer | Drift findings, coverage maps, chronic-drift policy, accepted-drift notes, harness updates | `.agents/skills/e2e_tests/**`; `agents/quality/roundtrip/**`; gate evidence linked from `agents/reviews/**` |
| Release Engineer | Release notes/checklists/evidence, CI/release/install changes, legal/version metadata proposals, todo triage | `.github/workflows/**`, `scripts/**`, `.agents/skills/{install,release}/**`, `CONTRIBUTING.md`, `.gitignore`, `.gitattributes`, `LICENSE`; `release/**` |
| Human | Approval decisions or requested edits | Recorded into `agents/org-transition-plan.md`, `agents/decisions/**`, or relevant docs |

## Expansion Triggers

Seat new roles or materialize deferred homes only when these triggers fire;
until then the bootstrap adapters remain the documented satisfiers.

- **Documentation Engineer** — when docs become an independent recurring product
  (roadmap, audience research, docs QA) separate from the surface owners.
- **Skill/Tooling Engineer** — when `.agents/**` ownership becomes many-to-many
  enough that served-domain owners cannot maintain it coherently. Presence of a
  skill home alone is not a trigger.
- **Research Curator** — when a tracked `research/**` wiki becomes durable
  product/research truth.
- **Frontend Engineer** — when VS Code / editor UI becomes a broader interactive
  application surface beyond language integration.
- **Roster Steward** — when role changes, drift audits, or ownership-retirement
  work becomes recurring.
- **Seated project verifier / planner** — only when a durable domain evidence
  surface or recurrent ambiguous cross-surface planning emerges. Until then
  `verifier-agent` and `issue-planner-agent` cover those gate rows.
- **Promote canonical definitions to `agents/definitions/**`** — if canonical
  role sources are migrated out of the runtime ports into an installed layout.
  Not approved now; the runtime ports plus this plan are the current role truth.

## Bridge-Plan Status

Created now (this materialization):

- Root `CONTEXT-MAP.md` (Language Designer).
- `agents/AGENTS.md`, `agents/org-transition-plan.md` (this file),
  `agents/handoffs/`, `agents/reviews/`, `agents/decisions/`,
  `agents/quality/roundtrip/`.
- `release/`.
- Claude + Codex runtime ports for all five seated `agent_id`s.

Kept:

- `.agents/**` as Glyph product/dogfood artifacts (not rehomed to `agents/**`).
- Existing Rust workspace split (`glyph-core`, `glyph-cli`, `glyph-lsp`).
- Existing documentation layout by audience.
- Round-trip QA harness path `.agents/skills/e2e_tests/**` (durable findings move
  to `agents/quality/roundtrip/**`).
- Generated no-description commands as tracked generated output.
- Pre-existing generic runtime ports as Glyph's own artifacts.

Revised:

- Root `AGENTS.md` — added pointers to `CONTEXT-MAP.md` and this plan, and a note
  that `.agents/**` is product/dogfood surface while `agents/**` is Flux scaffold.

Routed to Integration Engineer for review (not executed in this materialization):

- Absolute repo-local path references in `.agents/skills/issue-list-orchestrator/SKILL.md`,
  `.agents/skills/issue-list-orchestrator/references/issue-agent-prompt.md`,
  `.codex/config.toml`, `.codex/hooks.json`, `.mcp.json`.
- Graphify generated-output governance across `.codex/config.toml`, `.mcp.json`,
  `.gitattributes`, `.gitignore`, `AGENTS.md`, `CONTRIBUTING.md` (route together).
- `CLAUDE.md` compatibility references vs. the canonical-`AGENTS.md`/symlink
  convention (hits in `.agents/skills/**`, `docs/superpowers/**`, `specs/**`).

Needs human decision (routed by Release Engineer, not resolved here):

- License metadata mismatch: `Cargo.toml` says `MIT OR Apache-2.0`; `README.md`
  and `LICENSE` indicate Apache-2.0.

Avoided:

- Seating lifecycle adapters (`reviewer-agent`, `verifier-agent`,
  `issue-planner-agent`) as local repo roles.
- A generic implementation fallback.
- Replacing or symlinking `.agents/**` with `agents/**`.

## Approval Boundary

This plan records the approved starting state only. Human approval is still
required for any future scaffold, role, or product-scope change not listed in
this handoff — new roles, new artifact homes, tool/skill broadening beyond what
is recorded here, the license-metadata decision, and any change to product
priority or risk. Agents pause before dependent changes that require such
approval.
