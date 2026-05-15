# Contributing to Glyph

## Setup

After cloning, install the git hooks:

```sh
./scripts/install-hooks.sh
```

Pass `--no-graphify` to skip the graphify hooks if you don't use it:

```sh
./scripts/install-hooks.sh --no-graphify
```

This installs a `pre-commit` hook that:
- Runs `cargo fmt` on any staged `.rs` files (auto-formats and re-stages)
- Runs `glyph fmt` on any staged `.glyph` files (auto-formats and re-stages)
- Runs `glyph check` on staged `.glyph` files — commit is aborted on errors

Both `cargo` and `glyph` must be in your `PATH`. The hook skips gracefully if `glyph` is not found, but formatting won't be applied.

## Build

```sh
cargo build --workspace
cargo test --workspace
```

## Codebase Navigation (Optional)

A pre-built [graphify](https://github.com/graphify) knowledge graph lives at `graphify-out/graph.json` and is wired up as an MCP server in `.mcp.json`. If you use an MCP-aware agent (Claude Code, etc.) it will automatically use it for codebase exploration — no setup needed.

If you make large structural changes and want to regenerate the graph:

```sh
graphify .
```

This is entirely optional. The pre-built graph is committed and kept up to date by the maintainers.
