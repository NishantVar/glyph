# Glyph — Project Index

Glyph is a human-readable, visualizable DSL for authoring reusable agent skills that compiles into explicit, task-specific instructions for coding agents. See the top-level [README.md](README.md) for the public project description.

## Design
- [design/](design/) — **main design docs** (flat, this is the top-level design for Glyph): principles, boundaries, language core, semantics, types & effects, constraints, IR, compiler pipeline, output format, validation strategy, and a gap checklist

## Build
- Requires **Rust / Cargo** (`cargo build`, `cargo test`). Workspace crates: `glyph-core`, `glyph-cli`, `glyph-lsp`.

## End-User Language Guide
- [GLYPH_LANGUAGE_GUIDE.md](GLYPH_LANGUAGE_GUIDE.md) — single document for skill authors: file shape, declarations, sub-sections, flow statements, values, names, imports, stdlib, and the compilation contract. Read this before authoring or editing `.glyph.md` files.

## Codebase Exploration

A graphify knowledge graph is pre-built at `graphify-out/graph.json` and exposed via MCP (configured in `.mcp.json`). **Use the graphify MCP tools instead of reading source files** to understand code structure:

- `query_graph` — search by concept or keyword
- `get_neighbors` — what a node connects to
- `shortest_path` — how two concepts relate
- `get_node` — details on a specific symbol
- `god_nodes` — highest-connectivity entry points (start here when unfamiliar with the codebase)

Only read source files when you need exact implementation details (e.g. to write a fix).
