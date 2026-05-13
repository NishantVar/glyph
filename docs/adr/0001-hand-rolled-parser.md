# 0001. Hand-rolled parser, not a parser generator

## Status

Accepted.

## Context

Glyph's grammar has three properties that fight parser generators (`pest`,
`lalrpop`, `nom`, `winnow`, `chumsky`):

- **Indentation significance.** 4-space units determine nesting. Grammar-based
  parsers need synthetic INDENT/DEDENT tokens — an extra layer for no gain.
- **Context-sensitive keywords.** `require`, `avoid`, `must` are constraint
  markers in some positions and bare names in others. Sub-section headers
  (`flow:`, `description:`, etc.) are only headers in specific contexts.
- **Rich spans.** Diagnostics demand precise byte spans on every node, with
  control over what gets attached where. Hand-rolling keeps full control over
  span emission.

## Decision

Implement a hand-rolled recursive-descent parser on top of a hand-rolled
two-phase tokenizer (line-oriented pre-processing, then token-level scanning
within lines). Zero parsing dependencies beyond `std`.

Declaration-boundary recovery only: on a parse failure, emit a diagnostic and
skip to the next top-level declaration. No fine-grained recovery inside
`flow:` bodies — the agent repair loop owns that.

No incremental parsing in MVP.

## Consequences

- Tokenizer and parser code are project-owned, ~1500 LOC, easy to evolve as
  the grammar shifts.
- Context-sensitive keywords are handled naturally by parser state.
- Adopting a parser generator later remains possible but is unlikely to pay
  for itself.
- Multi-error parse diagnostics inside one declaration are deferred to the
  agent repair loop; the parser bails at the first error inside a
  declaration body.
