---
name: glyph
description: 'Entry point for the Glyph toolkit. Use when the user wants to author, compile, audit, decompile, or learn the Glyph DSL. Routes the request to the matching `/glyph:*` slash command.'
---

## Context

- **toolkit-slash-commands**

  User-facing slash commands: `/glyph:teach` (author or edit a `.glyph` source file), `/glyph:compile` (run a `.glyph` file or directory through the full pipeline), `/glyph:icompile` (apply a small targeted change to both the `.glyph` source and its compiled `.md` in tandem, bypassing the full pipeline), `/glyph:audit` (run semantic checks against a `.glyph` source or directory — does this skill actually make sense), `/glyph:decompile` (reverse-engineer an existing `.md` skill back into `.glyph`).

- **toolkit-internal-skills**

  Internal skills loaded by Glyph slash commands when the pipeline reaches their stage: `repair` (Phase 3b LLM source-rewrite, loaded by `/glyph:compile` when the compiler exits 2 with repairable diagnostics), `semantic_validation` (LLM semantic check on a declaration's resolved constraint set, loaded by `/glyph:audit` for every declaration with 2 or more constraints; currently runs the constraint-conflict scan), and `expand` (Phase 4 LLM span-fill, loaded by `/glyph:compile` to fill the marked prose spans in the scaffolded compiled `.md` against its resolved IR side-map).

## Steps

1. Decide which of the following applies and follow only that path:
   If User wants to author or edit a `.glyph` source file, or wants the Glyph DSL syntax taught or explained. Triggers include phrases like `write a glyph skill`, `teach me glyph`, `how does glyph syntax work`, or any request to create or edit a `.glyph` file:
   a. Hand off to the `/glyph:teach` slash command. Pass through the target `.glyph` path the user mentioned, or ask for it if absent. Do not run any authoring steps in this router — `/glyph:teach` owns the full procedure.
   If User wants to compile an existing `.glyph` source file or directory through the full Glyph pipeline (compile, fmt, repair loop, prose reshape, validate-output) and surface the agent-facing `.md` output:
   a. Hand off to the `/glyph:compile` slash command. Pass through the source `.glyph` file or directory path. Do not run `glyph compile` directly from this router — `/glyph:compile` owns the full pipeline.
   If User wants to apply a small, targeted change to both a `.glyph` source file and its sibling compiled `.md` in tandem, bypassing the full pipeline so unrelated prose is preserved. Triggers include phrases like `icompile`, `incremental compile`, or `patch both files`:
   a. Hand off to the `/glyph:icompile` slash command. Pass through the source `.glyph` path and the plain-language description of the change. Do not perform any edits in this router — `/glyph:icompile` owns the full procedure.
   If User wants to run semantic checks against a `.glyph` source file or directory without recompiling — e.g. check that a skill's constraints make sense together. Triggers include phrases like `audit`, `check semantics`, `does this skill make sense`, or `look for constraint conflicts`:
   a. Hand off to the `/glyph:audit` slash command. Pass through the source `.glyph` file or directory path. Do not perform any semantic analysis in this router — `/glyph:audit` owns the full procedure.
   If User wants to convert an existing compiled-form skill (a `.md` file, a `SKILL.md`, an Anthropic-style skill, or any other Markdown skill) back into a Glyph source file (`.glyph`):
   a. Hand off to the `/glyph:decompile` slash command. Pass through the source `.md` path and the target `.glyph` path. Do not perform any reverse-mapping in this router — `/glyph:decompile` owns the full procedure.
   Otherwise:
   a. Ask the user which Glyph capability they need: `teach` (author or learn the DSL), `compile` (build a `.glyph` into agent-facing Markdown), `icompile` (apply a small targeted change to both the `.glyph` source and its compiled `.md` in tandem), `audit` (run semantic checks against a `.glyph` source), or `decompile` (reverse a `.md` skill back into `.glyph`). Once they answer, re-enter the matching branch.

