# Glyph — Work Tracking

Bugs, implementation TODOs, deferred follow-ups, migration chores, and test gaps. This is **not** a long-lived spec — entries here either get done (and deleted) or get promoted to a real design/reference/architecture doc.

In a normal repo these would be GitHub issues. They live here for now because the project owner asked for in-tree tracking instead.

## Documents

- [[bugs]] — known bug inventory (was `design/todo_bugs.md`)
- [[general-todo]] — implementation TODOs extracted from the former [[todo]]
- [[user-facing-todos]] — author-visible gaps and deferred parser support extracted from the former [[user-facing-todo]]
- [[mvp-acceptance-checklist]] — MVP exit-criteria checklist (test corpus layout, per-fixture tables, diagnostic-ID coverage matrix, insta snapshot workflow)
- [[build-foundation-todos]] — implementation residue from `design/build-foundation.md` (dependency inventory, tokenizer plan, emit module table, agent workflow diagram)

### Per-Subsystem

- [[repair-todos]] — cross-file repair, constraint canonical-form rewrite, type description coherence, open repair questions
- [[expand-todos]] — full Markdown-parser well-formedness, no-embedded-HTML scan, verbatim-framing for Branch projection
- [[diagnostics-todos]] — known gaps and retired/deferred diagnostic-ID slots
- [[ir-semantics-todos]] — `--enable-effects` gate, freeform-sections wiring, deferred per-call effect annotations
- [[lsp-todos]] — milestone plan and risk list for the LSP
- [[tree-sitter-todos]] — milestone plan and open grammar questions
- [[agent-skill-todos]] — dogfooding the skill in `.glyph`, packaging/installer

## Rule

When an entry is fixed, **delete it**. When an entry stabilizes into durable architecture, **promote** it to [`../docs/architecture/`](../docs/architecture/) or write an ADR. When an entry stabilizes into a public contract, **promote** it to [`../docs/reference/`](../docs/reference/). Do not let this folder turn into a design archive.
