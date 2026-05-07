---
name: glyph
description: Entry point for the Glyph toolkit. Use when the user wants to author, compile, decompile, or learn the Glyph DSL. Routes the request to the matching `/glyph:*` slash command.
---

## Instructions

### Steps

1. Decide which of the following applies and follow only that path:
   If User wants to author or edit a `.glyph` source file, or wants the Glyph DSL syntax taught or explained. Triggers include phrases like `write a glyph skill`, `teach me glyph`, `how does glyph syntax work`, or any request to create or edit a `.glyph` file:
   a. Hand off to the `/glyph:teach` slash command. Pass through the target `.glyph` path the user mentioned, or ask for it if absent. Do not run any authoring steps in this router — `/glyph:teach` owns the full procedure.
   If User wants to compile an existing `.glyph` source file or directory through the full Glyph pipeline (compile, fmt, repair loop, constraint scan, prose reshape, validate-output) and surface the agent-facing `.md` output:
   a. Hand off to the `/glyph:compile` slash command. Pass through the source `.glyph` file or directory path. Do not run `glyph compile` directly from this router — `/glyph:compile` owns the full pipeline.
   If User wants to convert an existing compiled-form skill (a `.md` file, a `SKILL.md`, an Anthropic-style skill, or any other Markdown skill) back into a Glyph source file (`.glyph`):
   a. Hand off to the `/glyph:decompile` slash command. Pass through the source `.md` path and the target `.glyph` path. Do not perform any reverse-mapping in this router — `/glyph:decompile` owns the full procedure.
   Otherwise:
   a. Ask the user which Glyph capability they need: `teach` (author or learn the DSL), `compile` (build a `.glyph` into agent-facing Markdown), or `decompile` (reverse a `.md` skill back into `.glyph`). Once they answer, re-enter the matching branch.

