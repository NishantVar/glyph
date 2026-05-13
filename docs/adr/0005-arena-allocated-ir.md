# 0005. Single hand-rolled arena per file for IR

## Status

Accepted.

## Context

The IR spec requires stable node IDs (`n0`, `n1`, ...) so that Validate
errors, Phase 6b retry feedback, and diagnostics can refer to nodes by ID.
IDs are per-file and reset to 0 at the start of each compilation; there
is no global uniqueness requirement.

Available designs:

- **Owned tree** (`Vec<Decl>` with nested `Vec<Stmt>`). Requires a
  separate ID assignment pass and a parallel `NodeId -> &Node` map.
- **`id-arena` crate.** Provides typed `Id<T>` that prevents mixing
  arenas of different element types.
- **Hand-rolled arena.** A `Vec<IrNode>` where `NodeId(u32)` is the
  storage index.

## Decision

Hand-rolled single arena per file:

```rust
pub struct NodeId(pub u32);  // maps to spec's n0, n1, ...

pub struct IrArena {
    nodes: Vec<IrNode>,
}
```

`IrNode` is an enum over all node types. Children are referenced by
`NodeId`. The AST (Phases 1–2) stays as owned trees; the arena is built
during Lower (Phase 4) by pre-order source-traversal allocation.

One arena per file, counter resets to 0. No `id-arena` dependency.

## Consequences

- The node ID *is* the storage index — zero bookkeeping for ID -> node
  lookup.
- `IrNode` derives `serde::Serialize` for `--emit-ir`. The JSON shape is
  a nested tree (see the IR JSON reference doc), so a custom walk over
  the arena builds the output; the arena itself is not what hits disk.
- A single `NodeId` namespace across all node kinds matches the spec.
  A typed `Id<T>` from `id-arena` would prevent mixing arenas of
  different types, but there is only one arena per file.
- Cycles are impossible by construction inside one arena; the IR is a
  tree.
