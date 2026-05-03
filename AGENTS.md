# Glyph — Project Index

Glyph is a human-readable, visualizable DSL for authoring reusable agent skills that compiles into explicit, task-specific instructions for coding agents. See the top-level [README.md](README.md) for the public project description.

## Design
- [design/](design/) — **main design docs** (flat, this is the top-level design for Glyph): principles, boundaries, language core, semantics, types & effects, constraints, IR, compiler pipeline, output format, validation strategy, and a gap checklist

## Build
- Requires **Rust / Cargo** (`cargo build`, `cargo test`). Workspace crates: `glyph-core`, `glyph-cli`, `glyph-lsp`.

## End-User Language Guide
- [GLYPH_LANGUAGE_GUIDE.md](GLYPH_LANGUAGE_GUIDE.md) — single document for skill authors: file shape, declarations, sub-sections, flow statements, values, names, imports, stdlib, and the compilation contract. Read this before authoring or editing `.glyph.md` files.
