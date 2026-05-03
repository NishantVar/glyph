; locals.scm — scope-aware highlighting for Glyph.
;
; Glyph's scope rules are simple: top-level declarations are visible
; everywhere in the file (no shadowing, per design/values-and-names.md);
; parameters of a `skill`/`block` are visible within that declaration's
; body; flow-local `name = expr` bindings are visible from their point
; of definition onward inside the same flow scope.
;
; A `@local.scope` capture introduces a fresh lexical environment.
; `@local.definition.<kind>` captures introduce names. `@local.reference`
; captures resolve to the nearest enclosing definition with a matching
; name. The compiler still owns semantic name resolution — these queries
; only support editor features (rename, jump-to-definition, dimmed unused
; locals) and never override the highlight queries' coloring.

; ── Scopes ─────────────────────────────────────────────────────
; The whole file is one scope so file-level definitions are visible
; everywhere. Each callable adds an inner scope so its parameters and
; local variables don't leak.

(source_file) @local.scope

(skill_declaration) @local.scope
(block_declaration) @local.scope
(export_block_declaration) @local.scope
(generated_block_declaration) @local.scope

; ── File-level definitions (constants and callables) ──────────

(text_declaration name: (identifier) @local.definition.constant)
(int_declaration name: (identifier) @local.definition.constant)
(float_declaration name: (identifier) @local.definition.constant)
(generated_text_declaration name: (identifier) @local.definition.constant)

(skill_declaration name: (identifier) @local.definition.function)
(block_declaration name: (identifier) @local.definition.function)
(export_block_declaration name: (identifier) @local.definition.function)
(generated_block_declaration name: (identifier) @local.definition.function)

; ── Imports ────────────────────────────────────────────────────
; `import "x.glyph" as foo` — `foo` is bound at file scope.
; `import "x.glyph" { a, b as c }` — `a` (no alias) and `c` (the
; alias of `b`) are the visible names. The unaliased original
; `b` is captured for rename support but is not the binding name.

(import_statement alias: (identifier) @local.definition.import)
(import_specifier name: (identifier) @local.definition.import)
(import_specifier alias: (identifier) @local.definition.import)

; ── Parameters ─────────────────────────────────────────────────

(parameter name: (identifier) @local.definition.parameter)

; ── Flow-local bindings ────────────────────────────────────────
; `result = compute(scope)` introduces `result` for the rest of
; the enclosing `flow:` scope.

(variable_binding name: (identifier) @local.definition.var)

; ── References ─────────────────────────────────────────────────
; Every identifier that isn't a definition site (the patterns above
; consume those) is a reference. The locals engine attempts to
; resolve each reference to the nearest enclosing definition.

(identifier) @local.reference
