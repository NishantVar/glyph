/// <reference types="tree-sitter-cli/dsl" />

module.exports = grammar({
  name: "glyph",

  externals: ($) => [$._indent, $._dedent, $._newline],

  // `\n` is in extras so it can be absorbed inside bracketed regions
  // (`(...)`, `{...}`, `"""..."""`) where the grammar does not reference
  // `_newline`. The external scanner emits `_newline` only when one of
  // its three tokens is in `valid_symbols`; otherwise it returns false
  // and the `\n` falls through to extras. Comments are extras so they
  // can appear anywhere — between sections, inside parameter lists,
  // trailing a flow statement — without the grammar enumerating each
  // position.
  extras: ($) => [/[ \t]/, /\r?\n/, $.comment],

  word: ($) => $.identifier,

  rules: {
    source_file: ($) => repeat($._top_level_declaration),

    _top_level_declaration: ($) =>
      choice(
        $.import_statement,
        $.skill_declaration,
        $.block_declaration,
        $.export_block_declaration,
        $.const_declaration,
        $.generated_const_declaration,
        $.generated_block_declaration,
        $.type_decl,
      ),

    // ── imports ────────────────────────────────────────────────────
    import_statement: ($) =>
      seq(
        "import",
        field("path", $.import_path),
        choice(
          seq("as", field("alias", $.identifier)),
          field("list", $.import_list),
        ),
        $._newline,
      ),

    import_path: ($) => $.string_literal,

    import_list: ($) =>
      seq("{", optional(commaSep1($.import_specifier)), "}"),

    import_specifier: ($) =>
      seq(
        field("name", $.identifier),
        optional(seq("as", field("alias", $.identifier))),
      ),

    // ── skill declaration ──────────────────────────────────────────
    skill_declaration: ($) =>
      seq(
        "skill",
        field("name", $.identifier),
        $.parameter_list,
        optional($.return_type),
        $._newline,
        $._indent,
        $.declaration_body,
        $._dedent,
      ),

    // ── block declaration (private) ────────────────────────────────
    // Plain `block` uses the permissive `declaration_body`, which
    // accepts the single-string shorthand naturally: a body with
    // exactly one bare `inline_instruction_stmt` IS the shorthand.
    block_declaration: ($) =>
      seq(
        "block",
        field("name", $.identifier),
        $.parameter_list,
        optional($.return_type),
        $._newline,
        $._indent,
        $.declaration_body,
        $._dedent,
      ),

    // ── export block declaration ───────────────────────────────────
    // `export block` requires a section-bearing body. Single-string
    // shorthand is NOT permitted (the export contract must be
    // explicit). `export_block_body` excludes bare
    // `inline_instruction_stmt` to enforce this at the grammar level.
    export_block_declaration: ($) =>
      seq(
        "export",
        "block",
        field("name", $.identifier),
        $.parameter_list,
        optional($.return_type),
        $._newline,
        $._indent,
        $.export_block_body,
        $._dedent,
      ),

    // Strict body: section-bearing, no bare inline-instruction lines.
    // Includes the `_effects_stub` for the same parse-stability
    // reason described on `declaration_body`.
    export_block_body: ($) =>
      repeat1(
        choice(
          $.description_section,
          $.context_section,
          $.constraints_section,
          $.flow_section,
          $._effects_stub,
          seq($.require_marker, $._newline),
          seq($.avoid_marker, $._newline),
          seq($.must_avoid_marker, $._newline),
          seq($.must_marker, $._newline),
          seq($.context_marker, $._newline),
        ),
      ),

    // The single-instruction body used by `generated block`. Exactly
    // one bare instruction string, no sections.
    _shorthand_body: ($) => seq($.inline_instruction, $._newline),

    // ── value bindings ─────────────────────────────────────────────
    const_declaration: ($) =>
      seq(
        optional("export"),
        "const",
        field("name", $.identifier),
        "=",
        field("value", $._const_rhs),
        $._newline,
      ),

    // Per #81: const RHS is a literal — string, block string, integer,
    // float, or boolean. Bare-name and qualified-name RHS are out of
    // scope (see crates/glyph-core/src/parse.rs `parse_const_literal_rhs`).
    _const_rhs: ($) => choice(
      $.string_literal,
      $.block_string,
      $.integer_literal,
      $.float_literal,
      $.boolean_literal,
    ),

    generated_const_declaration: ($) =>
      seq(
        "generated",
        "const",
        field("name", $.identifier),
        "=",
        field("value", choice($.string_literal, $.block_string)),
        $._newline,
      ),

    generated_block_declaration: ($) =>
      seq(
        "generated",
        "block",
        field("name", $.identifier),
        $.parameter_list,
        $._newline,
        $._indent,
        $._shorthand_body,
        $._dedent,
      ),

    // ── type declarations ──────────────────────────────────────────
    type_decl: ($) =>
      seq(
        optional("export"),
        "type",
        field("name", $.identifier),
        "=",
        field("description", $.param_description_form),
        $._newline,
      ),

    // ── parameters ─────────────────────────────────────────────────
    parameter_list: ($) =>
      seq("(", optional(commaSep1($.parameter)), ")"),

    parameter: ($) =>
      seq(
        field("name", $.identifier),
        optional($.type_annotation),
        optional(seq("=", field("default", $._parameter_default))),
      ),

    type_annotation: ($) =>
      seq(":", field("type", $.type_identifier)),

    return_type: ($) =>
      seq("->", field("type", $.type_identifier)),

    // Type identifier: a name token in a type position. Reuses the
    // identifier regex to keep the lexer simple. Highlight queries
    // distinguish via the parent rule.
    type_identifier: (_) => /[A-Za-z_][A-Za-z0-9_]*/,

    _parameter_default: ($) =>
      choice(
        $.param_description_form,
        seq($.string_literal, $.param_description_form),
        seq($.block_string, $.param_description_form),
        $.string_literal,
        $.block_string,
        $.integer_literal,
        $.float_literal,
        $.boolean_literal,
        $.none_literal,
        $.qualified_name,
        $.identifier,
      ),

    // ── declaration body ───────────────────────────────────────────
    // Body-level constructs each consume their own trailing newline
    // (no wrapper rule, matching M1's flat shape).
    //
    // `_effects_stub` is a deliberate parse-stability concession.
    // `effects:` is out of the MVP language entirely (per the M2
    // brief and the canonical design doc), but three corpus files
    // still contain `effects:` lines pending a separate cleanup. If
    // the grammar treats those lines as plain syntax errors, the
    // recovery cascade can swallow adjacent lines (notably in
    // `repo_tools.glyph` where `effects:` appears as the first
    // body line, polluting the inline-instruction string parse and
    // breaking `tree-sitter highlight`). We therefore recognise the
    // line structurally (`effects:` followed by a comma-separated
    // identifier list) but emit no highlight captures and assign no
    // semantics. Once the corpus is cleaned, this rule and its
    // declaration-body / export-block-body entries can be removed.
    declaration_body: ($) =>
      repeat1(
        choice(
          $.description_section,
          $.context_section,
          $.constraints_section,
          $.flow_section,
          $._effects_stub,
          seq($.require_marker, $._newline),
          seq($.avoid_marker, $._newline),
          seq($.must_avoid_marker, $._newline),
          seq($.must_marker, $._newline),
          seq($.context_marker, $._newline),
          seq($.inline_instruction, $._newline),
        ),
      ),

    // Stub: parses `effects: a, b, c\n` as a single non-AST node.
    // Hidden via the leading underscore so it doesn't appear in
    // the parse tree.
    _effects_stub: ($) =>
      seq("effects", ":", commaSep1($.identifier), $._newline),

    // ── description section ────────────────────────────────────────
    description_section: ($) =>
      seq(
        "description",
        ":",
        choice(
          // short form: description: "text"
          seq(field("content", choice($.string_literal, $.block_string, $.identifier)), $._newline),
          // long form: description:\n    INDENT body DEDENT
          seq(
            $._newline,
            $._indent,
            field("content", choice($.string_literal, $.block_string, $.identifier)),
            $._newline,
            $._dedent,
          ),
        ),
      ),

    // ── context section ────────────────────────────────────────────
    context_section: ($) =>
      seq(
        "context",
        ":",
        choice(
          // short form: context: name-or-string
          seq(field("entry", $._context_entry), $._newline),
          // long form
          seq(
            $._newline,
            $._indent,
            repeat1(seq($._context_entry, $._newline)),
            $._dedent,
          ),
        ),
      ),

    _context_entry: ($) => choice($.string_literal, $.block_string, $.identifier),

    // ── constraints section ────────────────────────────────────────
    constraints_section: ($) =>
      seq(
        "constraints",
        ":",
        $._newline,
        $._indent,
        repeat1(seq($._constraint_marker, $._newline)),
        $._dedent,
      ),

    _constraint_marker: ($) =>
      choice(
        $.require_marker,
        $.avoid_marker,
        $.must_avoid_marker,
        $.must_marker,
      ),

    // ── flow section ───────────────────────────────────────────────
    flow_section: ($) =>
      seq(
        "flow",
        ":",
        $._newline,
        $._indent,
        repeat1($.flow_statement),
        $._dedent,
      ),

    flow_statement: ($) =>
      choice(
        $.if_statement,
        seq($.return_statement, $._newline),
        seq($.variable_binding, $._newline),
        seq($.context_marker, $._newline),
        seq($.require_marker, $._newline),
        seq($.avoid_marker, $._newline),
        seq($.must_avoid_marker, $._newline),
        seq($.must_marker, $._newline),
        seq($._flow_expression, $._newline),
      ),

    // A flow expression is anything that can stand alone as a step:
    // a call, an applies-predicate, an inline string instruction, or
    // a bare identifier (resolved to a block during repair).
    _flow_expression: ($) =>
      choice(
        $.applies_expression,
        $.call_expression,
        $.inline_instruction,
        $.qualified_name,
        $.identifier,
      ),

    // ── markers ────────────────────────────────────────────────────
    // Markers are bare (no trailing newline) so they can be reused in
    // both body positions (wrapped by `*_marker_stmt`) and inside
    // `flow:` statements (wrapped by `flow_statement` with its own
    // newline).
    require_marker: ($) =>
      seq("require", $._marker_target),

    avoid_marker: ($) =>
      seq("avoid", $._marker_target),

    must_marker: ($) =>
      seq("must", $._marker_target),

    // `must avoid <name-or-string>` — two-keyword form. Higher
    // precedence than `must_marker` so the lexer/parser commits to
    // the two-word prefix when present.
    must_avoid_marker: ($) =>
      prec(1, seq("must", "avoid", $._marker_target)),

    context_marker: ($) =>
      seq("context", $._marker_target),

    _marker_target: ($) =>
      choice($.string_literal, $.block_string, $.qualified_name, $.identifier),

    // ── inline instruction ─────────────────────────────────────────
    inline_instruction: ($) => choice($.string_literal, $.block_string),

    // ── control flow ───────────────────────────────────────────────
    if_statement: ($) =>
      seq(
        "if",
        field("condition", $.condition),
        $._newline,
        $._indent,
        repeat1($.flow_statement),
        $._dedent,
        repeat($.elif_clause),
        optional($.else_clause),
      ),

    elif_clause: ($) =>
      seq(
        "elif",
        field("condition", $.condition),
        $._newline,
        $._indent,
        repeat1($.flow_statement),
        $._dedent,
      ),

    else_clause: ($) =>
      seq(
        "else",
        $._newline,
        $._indent,
        repeat1($.flow_statement),
        $._dedent,
      ),

    // Condition is an expression with `and`/`or`/`not` and `==`.
    // Precedence (highest to lowest): comparison > not > and > or.
    condition: ($) => $._condition_expr,

    _condition_expr: ($) =>
      choice(
        $._or_expr,
      ),

    _or_expr: ($) =>
      choice(
        prec.left(1, seq($._or_expr, "or", $._and_expr)),
        $._and_expr,
      ),

    _and_expr: ($) =>
      choice(
        prec.left(2, seq($._and_expr, "and", $._not_expr)),
        $._not_expr,
      ),

    _not_expr: ($) =>
      choice(
        prec(3, seq("not", $._not_expr)),
        $._comparison_expr,
      ),

    _comparison_expr: ($) =>
      choice(
        $.comparison,
        $._condition_atom,
      ),

    comparison: ($) =>
      prec(4, seq($._condition_atom, choice("==", "!="), $._condition_atom)),

    _condition_atom: ($) =>
      choice(
        $.applies_expression,
        $.call_expression,
        $.qualified_name,
        $.identifier,
        $.string_literal,
        $.integer_literal,
        $.float_literal,
        $.boolean_literal,
        $.none_literal,
      ),

    // ── return ─────────────────────────────────────────────────────
    return_statement: ($) =>
      seq("return", optional(field("value", $._return_value))),

    _return_value: ($) =>
      choice(
        $.applies_expression,
        $.call_expression,
        $.qualified_name,
        $.identifier,
        $.string_literal,
        $.block_string,
        $.integer_literal,
        $.float_literal,
        $.boolean_literal,
        $.none_literal,
        $.output_target_identifier,
        $.output_target_description,
      ),

    // Output-target return forms — `<name>` or `<"description">`.
    // Used to mark a returned value as a named/described output target
    // for the surrounding skill or export block.
    output_target_identifier: ($) =>
      seq("<", $.identifier, ">"),

    output_target_description: ($) =>
      seq("<", $.string_literal, ">"),

    param_description_form: ($) =>
      seq("<", choice($.string_literal, $.block_string), ">"),

    // ── variable binding ───────────────────────────────────────────
    variable_binding: ($) =>
      seq(
        field("name", $.identifier),
        "=",
        field("value", $._binding_value),
      ),

    _binding_value: ($) =>
      choice(
        $.applies_expression,
        $.call_expression,
        $.qualified_name,
        $.identifier,
        $.string_literal,
        $.block_string,
        $.integer_literal,
        $.float_literal,
        $.boolean_literal,
        $.none_literal,
      ),

    // ── calls and arguments ────────────────────────────────────────
    call_expression: ($) =>
      prec(2,
        seq(
          field("function", choice($.qualified_name, $.identifier)),
          $.argument_list,
        ),
      ),

    applies_expression: ($) =>
      prec(3,
        seq(
          field("receiver", choice($.qualified_name, $.identifier)),
          ".",
          "applies",
          "(",
          ")",
        ),
      ),

    argument_list: ($) =>
      seq("(", optional(commaSep1($.argument)), ")"),

    argument: ($) =>
      choice(
        // Named argument: `name = value`. Uses precedence so it wins
        // over the bare-expression case when an `=` follows the name.
        prec(1, seq(field("name", $.identifier), "=", field("value", $._argument_value))),
        $._argument_value,
      ),

    _argument_value: ($) =>
      choice(
        $.applies_expression,
        $.call_expression,
        $.qualified_name,
        $.identifier,
        $.string_literal,
        $.block_string,
        $.integer_literal,
        $.float_literal,
        $.boolean_literal,
        $.none_literal,
      ),

    // ── qualified name ─────────────────────────────────────────────
    qualified_name: ($) =>
      prec(1,
        seq(
          field("module", $.identifier),
          ".",
          field("member", $.identifier),
        ),
      ),

    // ── string literal with interpolation ──────────────────────────
    string_literal: ($) =>
      seq(
        '"',
        repeat(choice($.interpolation, $.string_content)),
        '"',
      ),

    string_content: (_) => token.immediate(prec(1, /[^"\\{]+/)),

    // Triple-quoted block string. Inside, `"""` ends, `{name}`
    // interpolates. Newlines inside `"""..."""` are absorbed by the
    // grammar's `\n` extras — the closing `"""` is recognized first
    // because the scanner short-circuits when no externals are valid.
    block_string: ($) =>
      seq(
        '"""',
        repeat(choice($.interpolation, $.block_string_content, $._embedded_quotes)),
        '"""',
      ),

    // Block-string content: runs of safe characters, escape sequences,
    // and embedded quotes followed by a non-special trailing char. The
    // patterns deliberately require a trailing non-special char on the
    // single- and double-quote forms so the lexer prefers the 3-char
    // closing `"""` literal (longest match) when three consecutive
    // quotes appear at the current position. Tree-sitter's RE2 regex
    // has no lookahead, so the trailing-char trick is how we encode
    // "not followed by a third quote" without consuming it later.
    //
    // Edge case: `""{name}` (two literal quotes immediately before an
    // interpolation) cannot match any of these patterns — `{` is
    // special and not eligible for the trailing slot. The
    // `_embedded_quotes` alternative on `block_string`'s repeat covers
    // exactly that case: two or one bare quotes with negative token
    // precedence so the closer always wins, and the `{` is left for
    // the next iteration's `interpolation` rule.
    block_string_content: (_) =>
      token.immediate(
        prec(1,
          choice(
            /[^"{\\]+/,
            /\\./,
            /"[^"{\\]/,
            /""[^"{\\]/,
          ),
        ),
      ),

    // Bare embedded quotes used only when the longer content patterns
    // and the closing `"""` literal both fail (e.g. `""` immediately
    // before `{` or `\`). Negative precedence keeps the closer
    // dominant.
    _embedded_quotes: (_) =>
      token.immediate(prec(-1, choice('""', '"'))),

    interpolation: ($) =>
      seq(
        token.immediate("{"),
        $.identifier,
        "}",
      ),

    // ── literals ───────────────────────────────────────────────────
    integer_literal: (_) => /-?[0-9]+/,
    float_literal: (_) => /-?[0-9]+\.[0-9]+/,
    boolean_literal: (_) => choice("true", "false"),
    none_literal: (_) => "none",

    // ── identifier ─────────────────────────────────────────────────
    identifier: (_) => /[a-zA-Z_][a-zA-Z0-9_]*/,

    // ── comment ────────────────────────────────────────────────────
    comment: (_) => token(seq("//", /[^\n]*/)),
  },

});

/**
 * Comma-separated list with at least one element, optional trailing comma.
 */
function commaSep1(rule) {
  return seq(rule, repeat(seq(",", rule)), optional(","));
}
