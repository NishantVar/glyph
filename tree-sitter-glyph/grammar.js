/// <reference types="tree-sitter-cli/dsl" />

module.exports = grammar({
  name: "glyph",

  externals: ($) => [$._indent, $._dedent, $._newline],

  extras: ($) => [/[ \t]/],

  word: ($) => $.identifier,

  rules: {
    source_file: ($) =>
      repeat(choice($.skill_declaration, $.text_declaration, $.comment, $._newline)),

    // ── skill declaration ──────────────────────────────────────────
    skill_declaration: ($) =>
      seq(
        "skill",
        field("name", $.identifier),
        $.parameter_list,
        $._newline,
        $._indent,
        $.declaration_body,
        $._dedent,
      ),

    // ── text binding ───────────────────────────────────────────────
    text_declaration: ($) =>
      seq(
        "text",
        field("name", $.identifier),
        "=",
        $.string_literal,
        $._newline,
      ),

    // ── parameter list ─────────────────────────────────────────────
    parameter_list: ($) =>
      seq("(", optional(commaSep1($.parameter)), ")"),

    parameter: ($) =>
      seq(
        field("name", $.identifier),
        optional(seq("=", field("default", $._parameter_default))),
      ),

    _parameter_default: ($) =>
      choice($.string_literal, $.identifier, $.none_literal),

    none_literal: (_) => "none",

    // ── declaration body ───────────────────────────────────────────
    declaration_body: ($) =>
      repeat1(
        choice(
          $.description_section,
          $.context_section,
          $.flow_section,
          $.require_marker,
          $.avoid_marker,
          $.context_marker,
          $.inline_instruction,
          $.comment,
          $._newline,
        ),
      ),

    // ── description section ────────────────────────────────────────
    description_section: ($) =>
      seq(
        "description",
        ":",
        choice(
          // short form: description: "text" on same line
          seq(field("content", choice($.string_literal, $.identifier)), $._newline),
          // long form: description:\n    INDENT "text" DEDENT
          seq(
            $._newline,
            $._indent,
            field("content", choice($.string_literal, $.identifier)),
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
          // short form
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

    _context_entry: ($) => choice($.string_literal, $.identifier),

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
        seq($.inline_instruction, $._newline),
        seq($.context_marker, $._newline),
        seq($.require_marker, $._newline),
        seq($.avoid_marker, $._newline),
        seq($.comment, $._newline),
      ),

    // ── markers ────────────────────────────────────────────────────
    require_marker: ($) =>
      seq("require", choice($.string_literal, $.identifier)),

    avoid_marker: ($) =>
      seq("avoid", choice($.string_literal, $.identifier)),

    context_marker: ($) =>
      seq("context", choice($.string_literal, $.identifier)),

    // ── inline instruction ─────────────────────────────────────────
    inline_instruction: ($) => $.string_literal,

    // ── string literal with interpolation ──────────────────────────
    string_literal: ($) =>
      seq(
        '"',
        repeat(choice($.interpolation, $.string_content)),
        '"',
      ),

    string_content: (_) => token.immediate(prec(1, /[^"\\{]+/)),

    interpolation: ($) =>
      seq(
        token.immediate("{"),
        $.identifier,
        "}",
      ),

    // ── identifier ─────────────────────────────────────────────────
    identifier: (_) => /[a-zA-Z_][a-zA-Z0-9_]*/,

    // ── comment ────────────────────────────────────────────────────
    comment: (_) => seq("//", /[^\n]*/),
  },
});

/**
 * Comma-separated list with at least one element, optional trailing comma.
 */
function commaSep1(rule) {
  return seq(rule, repeat(seq(",", rule)), optional(","));
}
