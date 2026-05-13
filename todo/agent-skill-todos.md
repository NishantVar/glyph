# Agent Skill — Deferred Work

Tracked items extracted from [[agent-skill]]. Move to GitHub issues when prioritized.

## Post-MVP

- **Dogfood the agent skill in Glyph.** Today the skill is plain Markdown; authoring it as a `.glyph` file is a post-MVP goal. Requires the language to express the workflow state machine (compile/fmt/validate-output orchestration, exit-code branching, iteration budgets).

- **Packaging and installer.** For MVP the agent skill ships inside the `glyph` repo at a known path (e.g., `glyph-cli/agent/glyph.skill.md`) and the user copies it into their coding agent's skill directory manually. Add a `glyph install-skill` subcommand (or equivalent installer) so users do not hand-copy. Decide skill discovery convention per supported coding agent.
