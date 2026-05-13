# 0006. JSON output is byte-stable across runs

## Status

Accepted.

## Context

The MVP exit criteria require that `.md`, `.ir.json`, and diagnostic JSON
output be byte-identical across runs over identical input. This is
trivial for the deterministic compiler itself, but two common Rust
patterns silently break byte-stability of JSON:

- `HashMap`'s iteration order is randomised per process.
- Diagnostic emission order can depend on traversal order, which is
  stable in isolation but can drift as phases are reordered or files
  are listed in a different order.

Snapshot tests in `insta` are byte-comparisons; any non-determinism
turns the test suite into a coin flip.

## Decision

Two rules apply to every JSON output reachable from a CLI output path:

1. **Map-shaped JSON uses `BTreeMap`** (or any equivalent sorted-keys
   serialization). `HashMap` is forbidden in any type whose
   `Serialize` impl is reachable from a CLI output path. This covers
   IR node fields, parameter sets, effect sets recorded as maps, and
   any future map-shaped diagnostic field.
2. **Diagnostic arrays are sorted by `(file, span.start.byte, id)`** in
   that lexicographic order. This is the canonical ordering shared by
   pretty-printed stderr output and the NDJSON stdout stream. In a
   multi-file build, files are emitted in topological compile order;
   within each file the diagnostic array is sorted internally.

## Consequences

- Snapshot tests on `.ir.json` and diagnostic JSON are reliable.
- A `HashMap` introduced anywhere on the serialization path is a bug
  caught by snapshot diff, not silent corruption of an external
  contract.
- The sort key for diagnostics is part of the public diagnostic
  ordering contract; consumers of the NDJSON stream may rely on it.
