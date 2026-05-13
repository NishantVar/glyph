# Glyph — Documentation

This folder holds material that is **not** part of the user-facing language design. Each subfolder serves a different audience.

## Subfolders

- **[reference/](reference/)** — stable contracts for users, tools, agents, and downstream integrations. State what an external consumer can rely on. No implementation rationale.
- **[architecture/](architecture/)** — durable maintainer-facing architecture and invariants for the compiler, repair pass, expand pass, IR schema, IR semantics, LSP, tree-sitter grammar, agent-skill companion, and walking-skeleton example. Explain *why* the system is shaped this way; do not duplicate code.
- **[adr/](adr/)** — small decision records for non-obvious implementation choices whose rationale would otherwise be lost. Numbered sequentially.

## When to write here vs. elsewhere

- Language and product design → [`../design/`](../design/)
- Public/external contracts → `reference/`
- Internal architecture and invariants → `architecture/`
- "Why did we choose this?" for an internal design choice → `adr/`
- Bugs, implementation TODOs, migration chores → [`../todo/`](../todo/)

Reference docs **state contract, not rationale**. Architecture docs **explain why** and call out invariants. ADRs are **short** (a page or two) and record context + decision + consequences.
