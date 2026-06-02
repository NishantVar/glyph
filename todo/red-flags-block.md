# `red_flags:` block — language feature

## Status
- **Phase 1 (freeform body):** to be implemented next. See "Phase 1 scope" below.
- **Phase 2 (rows-as-pairs syntax):** deferred. See "Phase 2 — discipline upgrade" below.

## Background

Red flags are constraints-in-context: "don't do X because Y", same polarity as `avoid` markers, but rendered as a comparison table instead of bullets. Today they live as `const red_flags = """..."""` in three SKILL.glyph files (`cmux-observability`, `p2p`, `tfork`). The compile pipeline does not emit a Red Flags section in the compiled SKILL.md — the const is dormant. Each skill's source carries a hand-written comment saying so.

This is source ↔ compiled drift that `/glyph:icompile` can't bridge: incremental edits require a corresponding compiled region to mirror to, and there is none.

## Phase 1 scope (freeform body)

Promote red flags from `const` to a first-class block, with freeform string body (mirrors how triple-quoted const bodies work today). Minimal change: parser + IR + renderer + migration of the three skills.

### Syntax
```glyph
red_flags: """
| Thought | Reality |
|---|---|
| "Add my own API client to summarize." | The skill is intentionally SDK-free. |
...
"""
```

### Compiler work
- **Grammar (`tree-sitter-glyph`):** add `red_flags` block rule alongside existing top-level skill blocks (`context:`, `constraints:`, `flow:`).
- **IR (`glyph-core`):** add `red_flags: Option<String>` to the Skill IR. Bump `ir_version` if schema is versioned.
- **Renderer:** emit `### Red Flags\n\n{body}\n` after `## Steps`. Skip when absent.
- **Tests:** unit test for parse + render; golden-file test for at least one skill round-trip.

### Migration
- `skills/cmux-observability/SKILL.glyph` — change `const red_flags = """..."""` to `red_flags: """..."""`; add a new row for the cmux read-screen gotcha (`cmux read-screen --surface foo` without `--workspace` fails with misleading "Surface is not a terminal").
- `skills/p2p/SKILL.glyph` — same syntax migration; content unchanged.
- `skills/tfork/SKILL.glyph` — same syntax migration; content unchanged.
- Recompile each; verify the new ### Red Flags section appears in the compiled SKILL.md.

### Out of scope
- `/glyph:icompile` support for editing inside `red_flags:` blocks. The block body is freeform string; existing icompile rules for triple-quoted const bodies should transfer with minimal change, but verify.

## Phase 2 — discipline upgrade (later)

Once Phase 1 is in use, consider tightening the syntax to rows-as-pairs so the renderer owns the table formatting and authors can't silently break the schema:

```glyph
red_flags:
    flag "Add my own API client" -> "The skill is intentionally SDK-free."
    flag "Call cmux directly" -> "Always go through subcommands."
```

### Tradeoff
- **Pros:** consistent rendering, lint-able, IR can carry structured `[(thought, reality)]` pairs for tooling.
- **Cons:** breaking change for Phase 1 adopters; can't easily embed multi-line reality text or markdown formatting per cell.

### Decision needed before implementing Phase 2
- Drop freeform support entirely, or support both (`red_flags: """..."""` AND `red_flags: flag ... -> ...`)?
- Allow markdown in cells (would constrain how the renderer escapes content)?

File new sub-todos when picking this up.

## Related
- Trigger for this todo: cmux-observability v1.2 sign-off conversation, 2026-05-28. The user wanted a cmux read-screen gotcha added to SKILL.md via `/glyph:icompile`, which surfaced the dormant-const drift.
- Cross-link: `/Users/nishantvarshney/.config/superpowers/worktrees/agent-skills/feat-cmux-observability/todo/cmux-observability.md` (the cmux read-screen gotcha row to be added during Phase 1 migration).
