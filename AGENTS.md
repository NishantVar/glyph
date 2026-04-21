# Glyph — Project Index

Glyph is a human-readable, visualizable DSL for authoring reusable agent skills that compiles into explicit, task-specific instructions for coding agents. See the top-level [README.md](README.md) for the public project description.

This project uses the **Athena** tiered research wiki. Research flows through `unconfirmed/` → `confirmed/` → `consolidated/` → (optionally) `design/`, where each tier is a higher trust level than the one before.

**If you're about to write, move, or promote any research or design content, load the `athena` skill first.** Athena enforces tier discipline, registry updates, and the audit log that keeps this structure coherent. Reading existing `confirmed/`, `consolidated/`, or `design/` content directly is fine; reading `unconfirmed/` content requires Athena (and a subagent when possible).

## Research
- [agent-skill-dsl](research/agent-skill-dsl/) — founding research topic: syntax, IR, compiler architecture, visualization, and how Glyph differs from existing systems (DSPy, LangGraph, prompt templates, constrained-generation DSLs, agent frameworks)

## Design
- [design/](design/) — **main design docs** (flat, this is the top-level design for Glyph): principles, boundaries, language core, semantics, types & effects, constraints, IR, compiler pipeline, output format, validation strategy, and a gap checklist

## Trust Tiers

Research follows Athena's tiered trust model:

```
unconfirmed  →  confirmed  →  consolidated  →  design
  (raw)       (verified)    (synthesised)    (decisions)
```

- Treat files under `research/<topic>/unconfirmed/` as provisional.
- Files under `confirmed/`, `consolidated/`, and `design/` are trusted.
- Always flag to the user when a recommendation leans on unconfirmed findings.
