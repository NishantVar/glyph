// convert_md_to_glyph.glyph.md
//
// Skill that reverse-maps a compiled Glyph output (`.md`) back into Glyph
// source (`.glyph.md`). Reuses the language reference and authoring
// constraints bundled by the teach_glyph context and constraints skills.

import "./teach_glyph_context.glyph.md" { glyph_language_context }
import "./teach_glyph_constraints.glyph.md" { glyph_authoring_constraints }
import "./glyph_authoring_passes.glyph.md" {factor_long_instructions_and_texts, sort_declarations, compile_and_iterate}

skill convert_md_to_glyph(source_md, target_glyph)
    description: "Convert an existing compiled-form skill at `source_md` into a Glyph source file at `target_glyph`."

    context:
        glyph_language_context()

    constraints:
        glyph_authoring_constraints()

    flow:
        parse_compiled_skill(source_md)
        map_frontmatter_to_skill_header()
        extract_branches_into_procedures()
        map_context_section()
        map_constraints_section()
        recover_flow_from_steps()
        write_initial_glyph_source(target_glyph)
        factor_long_instructions_and_texts()
        sort_declarations()
        compile_and_iterate(target_glyph)
        return <"path to the produced .glyph.md file">

// ─────────────────────────────────────────────────────────────────────────────
// Conversion-procedure blocks — each block is one logical phase of reverse-
// mapping a compiled-form .md skill back into Glyph source.
// ─────────────────────────────────────────────────────────────────────────────

block find_something() -> Description
    flow:
        "check the files"
        return <"description of what's found">

block parse_compiled_skill(source_md)
    "Read the file at {source_md}. Split the YAML frontmatter from the Markdown body. Within the body, locate the `## Parameters`, `### Context`, `### Steps`, and `### Constraints` sub-sections — note which are present and which are absent."

block map_frontmatter_to_skill_header()
    "Use the frontmatter `name`, `description`, and parameter list (if present) to author the `skill <name>(params)` declaration line and its `description:` sub-section. Recover parameter names from the `## Parameters` section when the frontmatter does not list them. Do not invent parameters that are not in the source."

block extract_branches_into_procedures()
    flow:
        "Scan the steps, context bullets, and constraint bullets for conditional language — phrases like `if`, `when`, `for the case where`, `depending on whether`, or any wording that signals one path applies under one condition and a different path under another. Distinguish real control flow from hedging advice (e.g. `if you see X, consider Y` inside a single step is usually not a branch)."
        "For each branch you find, define a new `block` whose body holds the entire branch arm — its steps, its context bullets, and its constraint bullets — and replace the original branch site in the parent flow with an `if PROCNAME.applies()` chain that dispatches by description."
        "Set each new block's `description:` to the predicate prose of its arm so `.applies()` can match against runtime context."
        "For each context bullet and constraint bullet that came along inside an extracted arm, classify the wording. If it reads as a file-wide rule, hoist it to the parent skill's `context:` / `constraints:`. If it reads as scoped to this branch, leave it inside the procedure's own `context:` / `constraints:`. If the wording is ambiguous, ask the user."
        "Recurse on each newly created block: scan its body for nested branches and apply the same extraction. Stop when no block contains conditional language."

block map_context_section()
    "Map each remaining top-level context bullet to a `context:` entry on the skill. Use inline strings at this stage; the factoring pass will promote them to `text` constants where appropriate."

block map_constraints_section()
    "Map each remaining top-level constraint bullet to a `constraints:` entry on the skill. Recover polarity from the bullet's leading verb (`Always` / `Must` / `Never` / `Avoid` / `Prefer` / `Consider`) and pick the matching marker (`require` / `must` / `must avoid` / `avoid`). Use inline strings; factoring will promote them later."

block recover_flow_from_steps()
    flow:
        "Map the remaining top-level numbered steps to instruction strings inside `flow:` in their original order. Branch sites already became `if PROCNAME.applies()` calls during extraction — leave them alone here."
        "If the final step describes what the skill produces, lift that into a top-level `return <\"description\">` statement at the end of `flow:` rather than restating it as another instruction."

block write_initial_glyph_source(target_glyph)
    "Write the assembled skill, blocks, and any extracted text constants to {target_glyph}. This is a verbose first draft — factoring and sorting will tidy it up next."
