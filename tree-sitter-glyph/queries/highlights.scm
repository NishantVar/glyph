; ── Section headers (the visual anchors) ──────────────────────
(description_section "description" @label)
(context_section "context" @label)
(constraints_section "constraints" @label)
(flow_section "flow" @label)

; ── Declaration keywords ───────────────────────────────────────
"skill" @keyword
"block" @keyword
"export" @keyword
"generated" @keyword
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
"text" @type.builtin
"int" @type.builtin
"float" @type.builtin

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
(call_expression function: (identifier) @function.call)
(call_expression function: (qualified_name member: (identifier) @function.call))

; ── Member access ──────────────────────────────────────────────
(applies_expression "applies" @property)
(qualified_name member: (identifier) @property)

; ── Strings and interpolation ──────────────────────────────────
(string_literal) @string
(block_string) @string
(interpolation) @string.special

; ── Types ──────────────────────────────────────────────────────
(type_identifier) @type

; ── Parameters ─────────────────────────────────────────────────
(parameter name: (identifier) @variable.parameter)

; ── Variables (LHS of `name = expr` in flow) ───────────────────
(variable_binding name: (identifier) @variable)

; ── Argument names (named arguments: `name = value`) ───────────
(argument name: (identifier) @variable.parameter)

; ── Value-binding names (text, int, float, generated text) ────
(text_declaration name: (identifier) @constant)
(int_declaration name: (identifier) @constant)
(float_declaration name: (identifier) @constant)
(generated_text_declaration name: (identifier) @constant)

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
