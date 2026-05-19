# Glyph — Project Index

Glyph is a human-readable, visualizable DSL for authoring reusable agent skills that compiles into explicit, task-specific instructions for coding agents. See the top-level [README.md](README.md) for the public project description.

## Documentation Layout

Documentation is split by audience. Pick the folder that matches what you're writing or reading.

- [design/](design/) — **language and product design** for Glyph. Principles, boundaries, language core, semantics, types & effects, constraints, IR shape from a designer's perspective. Audience: language/product designers.
- [docs/reference/](docs/reference/) — **stable contracts** for users, tools, agents, and downstream integrations (CLI surface, compiled-output shape, diagnostic IDs, IR JSON contract, MVP acceptance).
- [docs/architecture/](docs/architecture/) — **maintainer-facing architecture** and invariants for the compiler, repair pass, expand pass, IR schema/semantics, LSP, tree-sitter grammar, agent-skill companion, walking-skeleton example.
- [docs/adr/](docs/adr/) — **architecture decision records** (numbered, short, immutable rationale for non-obvious internal choices).
- [todo/](todo/) — **bugs, implementation TODOs, migration chores, test gaps**. In a normal repo these would be GitHub issues; the owner asked for in-tree tracking instead.

If you're about to add docs, see each folder's own `AGENTS.md` for the rules on what belongs there.

## Research

- [research/](research/) — tiered research wiki

## Build
- Requires **Rust / Cargo** (`cargo build`, `cargo test`). Workspace crates: `glyph-core`, `glyph-cli`, `glyph-lsp`.

## End-User Language Guide
- [GLYPH_LANGUAGE_GUIDE.md](GLYPH_LANGUAGE_GUIDE.md) — single document for skill authors: file shape, declarations, sub-sections, flow statements, values, names, imports, stdlib, and the compilation contract. Read this before authoring or editing `.glyph` files.

## Codebase Exploration

A graphify knowledge graph is pre-built at `graphify-out/graph.json` and exposed via MCP (configured in `.mcp.json`). **Use the graphify MCP tools instead of reading source files** to understand code structure:

- `query_graph` — search by concept or keyword
- `get_neighbors` — what a node connects to
- `shortest_path` — how two concepts relate
- `get_node` — details on a specific symbol
- `god_nodes` — highest-connectivity entry points (start here when unfamiliar with the codebase)

Only read source files when you need exact implementation details (e.g. to write a fix).

## Agent Conventions for glyph

### Bounded reads (default)
- Never read a file sequentially unless explicitly required. Default to
  structural skeletons via `ast-grep` or `documentSymbol`: signatures only,
  bodies opt-in.
- Never read a file over 200 lines entirely. Request signatures first; pull
  function bodies only for the symbols you intend to modify.

### Escape hatch
- If the AST view is insufficient (broken state, complex imperative algorithm),
  invoke a raw read capped at 300 lines per request.

### Bounded edits
- Apply edits via `ast-grep` replace patterns or LSP workspace edits with
  exact text-to-be-removed. Raw unified diffs and whole-file replacements
  are prohibited.
- If the original text isn't found, the harness rejects the edit
  automatically.

### Verification scales with blast radius
- Trivial / single-file private logic:  cargo fmt + cargo check + targeted
  cargo-nextest on the modified module.
- Public API or cross-crate change:     workspace cargo check +
  workspace cargo nextest.
- Touches `unsafe` or security-sensitive code: human-in-the-loop approval
  required before edit; MIRI run before merge.

### Discovery order
1. Graphify   — owning concept, crate, module, design boundary
2. SCIP / LSP — symbol resolution, definitions, references, types
3. ast-grep   — structural match for the exact node to edit
4. Edit → cargo fmt → cargo check → cargo nextest
