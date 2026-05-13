# 0004. Plain `Span` struct over packed `u64`

## Status

Accepted.

## Context

Every AST and IR node carries a span for diagnostic rendering. A common
optimisation is to pack `(file_id, start, end)` into a single `u64` with
bit fields, saving 4 bytes per span at the cost of bit manipulation on
every access and an artificial cap on file size.

Glyph source files are kilobytes, not megabytes. ASTs have hundreds to
low thousands of nodes. The savings from packing are immaterial.

## Decision

`Span` is a plain `#[derive(Clone, Copy)]` struct with `file_id: u32`,
`start: u32`, `end: u32` (half-open byte range, Rust convention).

`Spanned<T>` wraps every AST/IR node with `{ node: T, span: Span }`.

A `LineIndex` per file converts byte offsets to 1-indexed `{line, col}`
on demand, queried only when rendering diagnostics. This avoids paying
for line/column lookup until output time.

`codespan-reporting` consumes spans for pretty stderr rendering; the
`Span` -> `Range<usize>` conversion happens at the rendering boundary.

## Consequences

- Readable, debuggable, no artificial file-size cap.
- 4 bytes per span is the only cost vs the packed encoding; trivial at
  Glyph's scale.
- Half-open ranges align with `Range<T>`, but diagnostic JSON uses
  inclusive `{line, col}` — the conversion is centralised in
  `LineIndex::to_source_span` and is the only span boundary to maintain.
- If profiling ever shows span size matters, packing remains an internal
  optimisation that does not affect callers.
