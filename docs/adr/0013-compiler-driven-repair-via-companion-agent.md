# ADR 0013: Compiler-Driven Repair via a Companion Agent

## Status

Accepted (v0). Orchestrator will renumber if a sibling ADR uses this slot.

## Context

Glyph source can be invalid in two qualitatively different ways:

1. **Hard errors.** Lexically or grammatically broken; intent is not recoverable. E.g., dangling `flow:` with no body, malformed indentation that no rewrite can salvage.
2. **Repairable diagnostics.** Source is structurally close to valid, but a deterministic fix would have to guess what the author meant. E.g., bare-name reference to an undefined identifier, ambiguous role marker, missing return.

The repairable category cannot be solved by a deterministic algorithm in a way that preserves author intent — generating a `generated const some_name = "..."` body requires summarizing what `some_name` is *supposed* to mean from the surrounding flow, which is an LLM task. But running an LLM inside the compiler binds the compiler to a model provider, makes builds non-deterministic, and forces every caller (CI, editors, agents) to provision an API key.

## Decision

Split the work along a hard boundary:

- **The compiler is deterministic.** It never calls an LLM. It parses, analyzes, lowers, validates, expands (Step 1: deterministic projection), and emits. Phase 3a (`glyph fmt`) handles all deterministic auto-fixes — indentation normalization, duplicate-subsection merging, unused-import removal, stdlib auto-import, etc. Whenever the compiler hits diagnostics it cannot safely fix on its own, it stops and exits 2.
- **A companion agent skill owns the LLM-dependent phases.** Phase 3b (semantic repair: generating `generated const` / `generated block` bodies, role disambiguation, etc.), Phase 3c (constraint conflict scan), and Phase 6 Step 2 (prose reshaping of compiled Markdown) all run in the coding agent's own LLM. The skill is a plain Markdown file the agent loads; it encodes the workflow state machine, repair patterns by diagnostic ID, and Step 2 rules. The agent re-invokes `glyph compile` after each edit.
- **The compiler enforces structural invariants on the agent's output.** Phase 6b (`glyph validate-output`) is a deterministic subcommand that consumes the agent's rewritten Markdown plus the IR JSON and rejects any structural drift (extra H2s, step-count mismatches, invented `{param}` references, leaked `with` modifier strings).

The agent and compiler communicate via three contracts: exit codes (control flow), NDJSON diagnostics on stdout (per-file repair instructions), and the IR JSON envelope (`--emit-ir` produces, `validate-output` consumes).

## Consequences

- **Deterministic builds where they matter.** CI gates, `glyph check`, and editor diagnostics never call an LLM. A failing build is reproducible without API access.
- **LLM-agnostic skill.** The skill works with any coding agent — Claude Code, Copilot, Cursor — because the agent provides its own LLM. Glyph does not ship a model client.
- **Bounded LLM cost.** `glyph fmt` runs first in every repair iteration and absorbs all deterministic fixes; only genuinely ambiguous repairs reach the LLM. The repair loop is capped at 3 iterations per file.
- **Iteration accounting is per-file, owned by the agent.** The compiler is stateless across invocations; the agent maintains the iteration counter. The 3-iteration limit hard-fails on the file that cannot converge, not the whole build.
- **The agent never runs deterministic logic.** This avoids the failure mode where an agent paraphrases a diagnostic and silently degrades the contract. All structural validation (role preservation, span-shape checks, the locked four-form constraint template) lives in the compiler.
- **The skill ships as documentation, not code.** Versioning the skill is a documentation problem, not a packaging problem. Future installer work is tracked in [[agent-skill-todos]].
