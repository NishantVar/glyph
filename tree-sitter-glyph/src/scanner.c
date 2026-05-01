#include "tree_sitter/parser.h"
#include <stdbool.h>
#include <stdlib.h>

// Token types — must match the order of `externals` in grammar.js.
enum TokenType {
  TOKEN_INDENT,
  TOKEN_DEDENT,
  TOKEN_NEWLINE,
};

#define MAX_INDENT_DEPTH 64

typedef struct {
  uint16_t indent_stack[MAX_INDENT_DEPTH];
  uint8_t  stack_size;

  // When the scanner crosses a line boundary it may need to emit several
  // tokens at the same source position: a NEWLINE for the line just ended,
  // followed by an INDENT or N DEDENTs. Each scanner invocation emits at
  // most one token, so we queue the rest.
  bool     pending_newline;
  bool     pending_indent;
  uint16_t pending_indent_value;
  uint8_t  pending_dedents;

  uint8_t  paren_depth;       // reserved; unused in M1 (single-line parens)
  bool     eof_done;          // EOF token sequence has been fully emitted
} Scanner;

// ─── Serialization (ABI 14: serialize returns length) ───────────

unsigned tree_sitter_glyph_external_scanner_serialize(void *payload, char *buffer) {
  Scanner *s = (Scanner *)payload;
  unsigned i = 0;

  buffer[i++] = (char)s->stack_size;
  buffer[i++] = (char)(s->pending_newline ? 1 : 0);
  buffer[i++] = (char)(s->pending_indent ? 1 : 0);
  buffer[i++] = (char)(s->pending_indent_value & 0xFF);
  buffer[i++] = (char)((s->pending_indent_value >> 8) & 0xFF);
  buffer[i++] = (char)s->pending_dedents;
  buffer[i++] = (char)s->paren_depth;
  buffer[i++] = (char)(s->eof_done ? 1 : 0);

  for (uint8_t j = 0; j < s->stack_size && i + 1 < TREE_SITTER_SERIALIZATION_BUFFER_SIZE; j++) {
    buffer[i++] = (char)(s->indent_stack[j] & 0xFF);
    buffer[i++] = (char)((s->indent_stack[j] >> 8) & 0xFF);
  }

  return i;
}

void tree_sitter_glyph_external_scanner_deserialize(void *payload,
                                                     const char *buffer,
                                                     unsigned length) {
  Scanner *s = (Scanner *)payload;
  s->stack_size = 1;
  s->indent_stack[0] = 0;
  s->pending_newline = false;
  s->pending_indent = false;
  s->pending_indent_value = 0;
  s->pending_dedents = 0;
  s->paren_depth = 0;
  s->eof_done = false;

  if (length == 0) return;

  unsigned i = 0;
  s->stack_size = (uint8_t)buffer[i++];
  if (i < length) s->pending_newline = buffer[i++] != 0;
  if (i < length) s->pending_indent = buffer[i++] != 0;
  if (i + 1 < length) {
    s->pending_indent_value = (uint16_t)((uint8_t)buffer[i] | ((uint8_t)buffer[i + 1] << 8));
    i += 2;
  }
  if (i < length) s->pending_dedents = (uint8_t)buffer[i++];
  if (i < length) s->paren_depth = (uint8_t)buffer[i++];
  if (i < length) s->eof_done = buffer[i++] != 0;

  for (uint8_t j = 0; j < s->stack_size && i + 1 < length; j++) {
    s->indent_stack[j] = (uint16_t)((uint8_t)buffer[i] | ((uint8_t)buffer[i + 1] << 8));
    i += 2;
  }
}

// ─── Lifecycle ──────────────────────────────────────────────────

void *tree_sitter_glyph_external_scanner_create(void) {
  Scanner *s = calloc(1, sizeof(Scanner));
  s->stack_size = 1;
  s->indent_stack[0] = 0;
  return s;
}

void tree_sitter_glyph_external_scanner_destroy(void *payload) {
  free(payload);
}

// ─── Helpers ────────────────────────────────────────────────────

static inline uint16_t stack_top(Scanner *s) {
  return s->indent_stack[s->stack_size - 1];
}

static inline void skip_char(TSLexer *lexer) {
  lexer->advance(lexer, true);
}

// Try to emit one queued token. Returns true if a token was emitted.
static bool drain_pending(Scanner *s, TSLexer *lexer, const bool *valid) {
  // Order matters: NEWLINE comes before INDENT/DEDENT in the token stream.
  if (s->pending_newline && valid[TOKEN_NEWLINE]) {
    s->pending_newline = false;
    lexer->result_symbol = TOKEN_NEWLINE;
    return true;
  }
  if (s->pending_indent && valid[TOKEN_INDENT]) {
    s->pending_indent = false;
    if (s->stack_size < MAX_INDENT_DEPTH) {
      s->indent_stack[s->stack_size++] = s->pending_indent_value;
    }
    lexer->result_symbol = TOKEN_INDENT;
    return true;
  }
  if (s->pending_dedents > 0 && valid[TOKEN_DEDENT]) {
    s->pending_dedents--;
    lexer->result_symbol = TOKEN_DEDENT;
    return true;
  }
  return false;
}

// ─── Scan ───────────────────────────────────────────────────────

bool tree_sitter_glyph_external_scanner_scan(void *payload,
                                              TSLexer *lexer,
                                              const bool *valid_symbols) {
  Scanner *s = (Scanner *)payload;

  // Step 1: drain any tokens already queued at this source position.
  if (drain_pending(s, lexer, valid_symbols)) {
    return true;
  }

  // If none of our external tokens are valid in the current parse state,
  // do nothing. This is how multi-line constructs (`(...)`, `{...}`, `"""..."""`)
  // suppress INDENT/DEDENT/NEWLINE: inside those bracket regions, the grammar
  // rules don't reference any of our externals, so we return false and the
  // newline/whitespace is absorbed by the grammar's `extras`. Without this
  // guard, we would advance past the `\n` and mutate state, only to have
  // `drain_pending` find no valid token to emit — silently corrupting the
  // indent stack.
  if (!valid_symbols[TOKEN_NEWLINE] &&
      !valid_symbols[TOKEN_INDENT] &&
      !valid_symbols[TOKEN_DEDENT]) {
    return false;
  }

  bool at_eof = lexer->eof(lexer);
  bool at_newline = !at_eof && lexer->lookahead == '\n';

  if (!at_newline && !at_eof) {
    return false;
  }

  // Step 2: EOF without a trailing newline. Queue NEWLINE + DEDENTs once.
  if (at_eof && !at_newline) {
    if (s->eof_done) {
      return false;
    }
    s->eof_done = true;
    s->pending_newline = true;
    if (s->stack_size > 1) {
      s->pending_dedents = (uint8_t)(s->stack_size - 1);
      s->stack_size = 1;
    }
    return drain_pending(s, lexer, valid_symbols);
  }

  // Step 3: At a newline. Consume it + blank lines + leading whitespace.
  uint16_t indent = 0;
  while (true) {
    if (lexer->lookahead == '\n') {
      indent = 0;
      skip_char(lexer);
    } else if (lexer->lookahead == ' ') {
      indent++;
      skip_char(lexer);
    } else if (lexer->lookahead == '\t') {
      // Treat tabs as 4 spaces (the parser flags tabs separately).
      indent += 4;
      skip_char(lexer);
    } else {
      break;
    }
  }

  // Mark token end at the start of the next content line (or at EOF).
  lexer->mark_end(lexer);

  bool now_eof = lexer->eof(lexer);
  uint16_t current = stack_top(s);

  // Every line boundary produces a NEWLINE.
  s->pending_newline = true;

  if (now_eof) {
    // File ended after newline(s). Drain stack to base level.
    if (s->stack_size > 1) {
      s->pending_dedents = (uint8_t)(s->stack_size - 1);
      s->stack_size = 1;
    }
    s->eof_done = true;
  } else if (indent > current) {
    s->pending_indent = true;
    s->pending_indent_value = indent;
  } else if (indent < current) {
    uint8_t dedent_count = 0;
    while (s->stack_size > 1 && stack_top(s) > indent) {
      s->stack_size--;
      dedent_count++;
    }
    s->pending_dedents = dedent_count;
  }
  // indent == current → only NEWLINE is queued.

  return drain_pending(s, lexer, valid_symbols);
}
