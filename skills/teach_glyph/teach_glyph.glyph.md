// teach_glyph.glyph.md
//
// Skill that teaches a coding agent how to author Glyph source files.
// Context texts live in teach_glyph_context.glyph.md;
// constraint texts live in teach_glyph_constraints.glyph.md.

import "./teach_glyph_context.glyph.md" { glyph_language_context }

import "./teach_glyph_constraints.glyph.md" { glyph_authoring_constraints }
import "./glyph_authoring_passes.glyph.md" {
    factor_long_instructions_and_texts,
    sort_declarations,
    compile_and_iterate,
}

skill teach_glyph(target)
    description: "Author or edit a Glyph source file (.glyph.md) at `target` for a task described by the user."

    context:
        glyph_language_context()

    constraints:
        glyph_authoring_constraints()

    flow:
        gather_intent(target)
        choose_file_shape()
        write_skill_or_block_header()
        write_description_and_sub_sections()
        write_flow_section()
        promote_repeated_content()
        factor_long_instructions_and_texts()
        sort_declarations()
        compile_and_iterate(target)
        return <"path to the authored or updated .glyph.md file">

// ─────────────────────────────────────────────────────────────────────────────
// Authoring-procedure blocks — each block is one logical phase of writing
// a Glyph source file. Bodies are private to this file.
// ─────────────────────────────────────────────────────────────────────────────

block gather_intent(target)
    flow:
        "Read the user's request and identify the skill's purpose, the runtime parameters it will need, and any task-specific constraints."
        "If {target} is an existing `.glyph.md` file, read it first and treat the task as an edit. Otherwise, plan to create a new file at {target}."

block choose_file_shape()
    "Decide the file kind: a skill file with exactly one `skill` declaration, or a library file with zero `skill` declarations and at least one `export block` or `export const`."

block write_skill_or_block_header()
    "Write the declaration header. For `skill`, parameters may have no defaults (the agent extracts them from user context at runtime). For `export block`, every parameter must have a default. For return types, use a named domain type (`Plan`, `BranchName`, `Diagnosis`) or omit `->` entirely when there is no meaningful return value — never `String`/`Int`/`Float`/`Bool`/`None`."

block write_description_and_sub_sections()
    flow:
        "Add `description:` as a single-line routing string, or as a bare-name reference to a same-file `const` constant. No `{name}` slots inside `description:`."
        "Choose constraints. Use `require` / `avoid` for soft rules and `must` / `must avoid` for genuinely non-negotiable ones. Each marker carries either a bare-name reference to a same-file `const` constant or an inline string."
        "Add `context:` entries for background facts the agent should keep in mind at runtime — bare-name references to string-valued `const` constants, inline strings, or `context`-prefixed markers."

block write_flow_section()
    flow:
        "Write `flow:` as an ordered sequence of instruction strings, bindings, calls (bare, qualified, or UFCS), and branches. Use `with \"...\"` to specialize a single call site. Use `if`/`elif`/`else` with `BLOCKNAME.applies()` for description-driven dispatch."
        "Reference parameters and local bindings inside instruction strings with `{name}` slots — only where the surrounding string is instruction-bearing."
        "End `flow:` with at most one top-level `return`, placed last (never inside a branch arm). Use `return <name>` or `return <\"description\">` only when the value is synthesized by the agent from prose; otherwise prefer a normal binding. For `export block`, the `return` must be explicit even when returning `none`."

block promote_repeated_content()
    "Promote any inline string that repeats into a `const` constant. Promote any instruction sequence that repeats into a `block` (or `export block` if another file needs it)."
