; ── Section headers ─────────────────────────────────────────────
(description_section "description" @label)
(context_section "context" @label)
(flow_section "flow" @label)

; ── Declaration keywords ───────────────────────────────────────
"skill" @keyword
"text" @type.builtin

; ── Constraint markers ─────────────────────────────────────────
(require_marker "require" @keyword.directive)
(avoid_marker "avoid" @keyword.directive)

; ── Context marker ─────────────────────────────────────────────
(context_marker "context" @keyword.context)

; ── Skill name ─────────────────────────────────────────────────
(skill_declaration name: (identifier) @function)

; ── Text binding name ──────────────────────────────────────────
(text_declaration name: (identifier) @constant)

; ── Parameters ─────────────────────────────────────────────────
(parameter name: (identifier) @variable.parameter)

; ── Strings and interpolation ──────────────────────────────────
(string_literal) @string
(interpolation) @string.special

; ── Constants ──────────────────────────────────────────────────
(none_literal) @constant.builtin

; ── Punctuation ────────────────────────────────────────────────
":" @punctuation.delimiter
"," @punctuation.delimiter
"(" @punctuation.bracket
")" @punctuation.bracket
"=" @punctuation.special

; ── Comments ───────────────────────────────────────────────────
(comment) @comment
