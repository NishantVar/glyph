# ADR 0020 — Fixed Effect Keyword Vocabulary

## Status

Accepted (MVP). The effects subsystem is gated behind `--enable-effects`
(default off) until inference handles call-graph-less skills.

## Context

Glyph annotates skills, blocks, and export blocks with what external
capabilities they touch. Two large design choices:

1. **What is the vocabulary?** A free-form tag system, an effect algebra, or
   a fixed keyword set?
2. **How are effects determined?** Author-only declaration, full inference,
   or a hybrid?

A free-form tag system would let authors invent effects per project, but
that destroys import contracts: a caller cannot reason about a callee's
declared effects if "writes" in one codebase means something else in
another. An open-ended algebra would let authors compose effects (e.g.
`reads(/etc/...)`), but the design surface and validation rules balloon
quickly and the agent-consumer of compiled output gains little.

## Decision

**Nine fixed `verb_noun` snake_case keywords** for MVP:

`none`, `reads_files`, `reads_env`, `writes_files`, `runs_commands`,
`uses_network`, `asks_user`, `creates_artifacts`, `spawns_agent`.

The set is closed. New keywords may be added by future versions (e.g.
`reads_database`, `sends_messages`), but existing keywords are never
renamed or removed once stabilized. Old skills remain valid.

**Hybrid inference + declaration.** The compiler always computes an
inferred effect set from the call graph. If the author omits `effects:`,
deterministic repair inserts the inferred set. If the author declares one,
the declared set must be a superset of inferred (`G::analyze::effects-under-declared`
on shortfall, `G::analyze::effects-over-declared` warning on legitimate
over-declaration).

## Consequences

- Import contracts are stable: every consumer knows the exact vocabulary.
- Visualization tools can render effects from a closed enum.
- Inference removes the boilerplate burden — most authors write no
  `effects:` line at all; the compiler synthesizes it.
- Explicit `effects: none` becomes a useful author assertion that diverges
  from inference and yields a compile error if wrong.
- Extension is backwards-compatible: adding a keyword does not break old
  source.

## Alternatives Considered

- **Free-form tags.** Rejected: kills import contracts and visualization.
- **Effect algebra with parameters.** Rejected: large surface, little gain
  for the agent consumer.
- **Inference-only, no author declaration.** Rejected: authors lose the
  ability to widen the contract for forward-compatibility, and explicit
  `effects: none` assertions become impossible.
- **Declaration-only, no inference.** Rejected: pushes boilerplate onto
  every author and risks silent drift between code and declaration.
