; ── Section headers (the visual anchors) ──────────────────────
; The `@glyph.section.*` captures are LSP-aligned (see PRD #93);
; the `@label` capture is the standard fallback.
(description_section "description" @glyph.section.description @label)
(context_section "context" @glyph.section.context @label)
(constraints_section "constraints" @glyph.section.constraints @label)
(flow_section "flow" @glyph.section.flow @label)

; ── Declaration keywords ───────────────────────────────────────
"skill" @keyword
"block" @keyword
"export" @keyword
"import" @keyword
"as" @keyword
"return" @keyword
"if" @keyword
"elif" @keyword
"else" @keyword

; ── Logical operators ──────────────────────────────────────────
"and" @keyword
"or" @keyword
"not" @keyword

; ── Built-in type keywords ─────────────────────────────────────
"const" @type.builtin
"generated" @type.builtin

; ── Constraint markers — visually distinct from keywords ───────
(require_marker "require" @keyword.directive)
(avoid_marker "avoid" @keyword.directive)
(must_marker "must" @keyword.directive)
(must_avoid_marker "must" @keyword.directive)
(must_avoid_marker "avoid" @keyword.directive)

; ── Context marker ─────────────────────────────────────────────
(context_marker "context" @keyword.context)

; ── Skill / block names in headers ─────────────────────────────
(skill_declaration name: (identifier) @function)
(block_declaration name: (identifier) @function.method)
(export_block_declaration name: (identifier) @function.method)
(generated_block_declaration name: (identifier) @function.method)

; ── Calls ──────────────────────────────────────────────────────
; `@glyph.flow.call` is LSP-aligned; `@function.call` is fallback.
; Tree-sitter cannot distinguish block calls from stdlib calls without
; a symbol table — the LSP layer further refines block-resolved calls
; into `GlyphBlockCall`. Per team-lead, all calls map to flow.call
; here; `@glyph.block.call` is reserved for the LSP side.
(call_expression function: (identifier) @glyph.flow.call @function.call)
(call_expression
  function: (qualified_name member: (identifier) @glyph.flow.call @function.call))

; ── Member access ──────────────────────────────────────────────
(applies_expression "applies" @property)
(qualified_name member: (identifier) @property)

; ── Strings and interpolation ──────────────────────────────────
; Generic string captures stay as the fallback for any string outside
; an `inline_instruction` / `context_section` / `context_marker`
; (e.g. `description:` content, `const` declaration RHS). The
; section-aware Glyph-prefixed captures are below.
(string_literal) @string
(block_string) @string
; `@glyph.interpolation` is LSP-aligned; `@string.special` is fallback.
; Applied globally — interpolations only appear inside strings, and the
; loud-slot styling reads correctly in any string context.
(interpolation) @glyph.interpolation @string.special

; ── Types ──────────────────────────────────────────────────────
(type_identifier) @type

; ── Parameters ─────────────────────────────────────────────────
(parameter name: (identifier) @variable.parameter)

; ── Variables (LHS of `name = expr` in flow) ───────────────────
(variable_binding name: (identifier) @variable)

; ── Argument names (named arguments: `name = value`) ───────────
(argument name: (identifier) @variable.parameter)

; ── Value-binding names (const, generated const) ──────────────
(const_declaration name: (identifier) @constant)
(generated_const_declaration name: (identifier) @constant)

; ── Module / import names ──────────────────────────────────────
(import_path) @string.special
(import_statement alias: (identifier) @module)
(import_specifier alias: (identifier) @module)

; ── Built-in constant values ───────────────────────────────────
(none_literal) @constant.builtin
(boolean_literal) @constant.builtin

; ── Numeric literals ───────────────────────────────────────────
(integer_literal) @number
(float_literal) @number

; ── Punctuation ────────────────────────────────────────────────
":" @punctuation.delimiter
"," @punctuation.delimiter
"(" @punctuation.bracket
")" @punctuation.bracket
"{" @punctuation.bracket
"}" @punctuation.bracket
"=" @punctuation.special
"->" @punctuation.special
"." @punctuation.special
"==" @punctuation.special

; ── Comments ───────────────────────────────────────────────────
(comment) @comment

; ─────────────────────────────────────────────────────────────────────
; Glyph-prefixed captures — section-aware string + name_ref additions
; ─────────────────────────────────────────────────────────────────────
;
; The remaining Glyph-prefixed captures (`@glyph.section.*`,
; `@glyph.flow.call`, `@glyph.interpolation`) live alongside their
; standard fallbacks above; only the section-aware patterns below
; cannot be folded into existing lines because they require an outer
; `context_section` / `context_marker` / `inline_instruction` context.
;
; All capture names mirror the LSP `SemTokenType` vocabulary
; (`SemTokenType::legend()` in crates/glyph-core/src/semantic_tokens.rs).
; Adding a new Glyph-prefixed capture here MUST stay in lockstep with
; a matching `SemTokenType` variant (see PRD #93 / issue #95).
;
; Note: skill / block declaration names and value-binding names
; (const / int / float / generated_const) intentionally do NOT receive
; Glyph-prefixed captures — they keep the standard `@function` /
; `@function.method` / `@constant` captures from above. The locked
; LSP vocabulary classifies those via existing token types and
; modifiers; only the ten new primitives get Glyph-prefixed captures.

; Context strings — in `context:` body and in `context <string>` markers
; (which appear inside `flow:` per the canonical `flow_context.glyph.md`
; example). Visually muted to read as "background knowledge".
(context_section (string_literal) @glyph.context.string @string)
(context_section (block_string) @glyph.context.string @string)
(context_marker (string_literal) @glyph.context.string @string)
(context_marker (block_string) @glyph.context.string @string)

; Flow strings — every `inline_instruction`. Covers both `flow:` body
; and the implicit single-string body shorthand for private blocks
; (`block foo: "do thing"`). Visually loudest: this is the
; instructional payload the agent will execute.
(inline_instruction (string_literal) @glyph.flow.string @string)
(inline_instruction (block_string) @glyph.flow.string @string)

; Context bare-name reference — an identifier in `context:` body or in
; a `context <name>` marker, pointing back to a `const` declaration.
; PRD §Visual Hierarchy: "context bare-name references: same color as
; const declaration names so the link between reference and definition
; is obvious".
(context_section (identifier) @glyph.context.name_ref @variable)
(context_marker (identifier) @glyph.context.name_ref @variable)

; ─────────────────────────────────────────────────────────────────────
; Output-target return forms — `return <name>` / `return <"description">`
; ─────────────────────────────────────────────────────────────────────
;
; The angle-bracket form marks a returned value as a named/described
; output target. Identifier form gets a type-like fallback so the name
; reads as a referent; description form gets `@string.special` so the
; quoted text reads as a label rather than a payload string. The
; brackets themselves are punctuation.
(output_target_identifier (identifier) @glyph.return.target.ident @type)
(output_target_description (string_literal) @glyph.return.target.description @string.special)
(output_target_identifier "<" @punctuation.bracket)
(output_target_identifier ">" @punctuation.bracket)
(output_target_description "<" @punctuation.bracket)
(output_target_description ">" @punctuation.bracket)
