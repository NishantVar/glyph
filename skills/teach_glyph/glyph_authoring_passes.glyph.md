// glyph_authoring_passes.glyph.md
//
// Shared post-write passes for any skill that authors or edits Glyph source.
// teach_glyph and convert_md_to_glyph both import these.

export block factor_long_instructions_and_texts()
    flow:
        "Scan every instruction string in `flow:` bodies. For any string longer than 10 words, extract it into a named `block` (or `export block` if it must be reachable from another file) and replace the inline string with a call to that block. Pick a verb-phrase name that describes the step's intent."
        "Scan every inline string used as a marker body (`require`/`avoid`/`must`/`must avoid`/`context`) or as a `context:` entry. For any string longer than 10 words, extract it into a named `const` constant (or `export const` if another file imports it) and replace the inline string with a bare-name reference. Skip `description:` strings — leave them inline."
        return none

export block sort_declarations()
    flow:
        "Reorder top-level declarations in the file so that the single `skill` declaration appears first, every `block` and `export block` follows it, and every `const` and `export const` constant comes last. Preserve `import` statements at the very top of the file, above the `skill` declaration."
        return none

export block compile_and_iterate(target = ".")
    flow:
        "Run the Glyph compiler on {target} and read the diagnostics."
        "If the compiler exits with repairable diagnostics (exit 2), run `glyph fmt` on {target}."
        "If `glyph fmt` changes the file, re-invoke the compiler and re-evaluate the diagnostics."
        "Treat errors as required fixes. If repairable diagnostics remain after `glyph fmt`, treat them as informational — the LLM repair pass will rewrite the source. Treat warnings as advisory."
        "Review the source diff after the LLM repair pass. If repair inserted `generated text` or `generated block` definitions, decide whether each is acceptable as-is or should be promoted to hand-authored by renaming `generated text` to `text` and `generated block` to `block`."
        "Iterate on remaining diagnostics until the file compiles cleanly with the intended structure."
        return none
