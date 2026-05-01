; injections.scm — language injection rules for Glyph.
;
; Glyph source files do not embed any other language. Inline
; instruction strings (`"..."`), block strings (`"""..."""`),
; description text, and constraint text are all natural-language
; prose for the LLM — they are not Markdown, code, regex, JSON,
; or any other parseable sub-language.
;
; This file is intentionally empty. It exists so editors that
; expect every tree-sitter language to ship an `injections.scm`
; (Helix, some nvim-treesitter setups) load Glyph cleanly without
; falling back to "no injections" warnings.
