# ADR 0019 — Four-Form Constraint Model From Three Keywords

## Status

Accepted (MVP).

## Context

Glyph needs to express behavioral rules with two independent axes:

- **Strength** — is this advisory or absolute? (`soft` / `hard`)
- **Polarity** — is this a positive obligation or a prohibition?
  (`require` / `avoid`)

A naive design would expose four source-level keywords (e.g. `should`,
`should_not`, `must`, `must_not`) or a single keyword with attributes
(`constraint(strength=hard, polarity=avoid) X`).

Both options have problems. Four keywords inflate the surface vocabulary and
make the relationship between them implicit. The single-keyword form pushes
attribute syntax into authoring, which is verbose and unfriendly to the
"natural-language-like" feel of Glyph.

## Decision

Three source keywords compose into four IR forms:

| Source marker | IR mapping |
|---------------|------------|
| `require` | `Constraint(strength: soft, polarity: require)` |
| `avoid` | `Constraint(strength: soft, polarity: avoid)` |
| `must` | `Constraint(strength: hard, polarity: require)` |
| `must avoid` | `Constraint(strength: hard, polarity: avoid)` |

`must` is a strength modifier — standalone `must X` is shorthand for
`must require X`. `avoid` flips polarity. The IR carries one role
(`Constraint`) with two attributes (`strength`, `polarity`); the surface
keeps only three lexical items.

## Consequences

- Authors learn three words instead of four; the meaning of each composition
  is mechanical.
- The IR keeps a single `Constraint` role rather than a separate role per
  combination, which simplifies role inference, repair, and visualization.
- Rendering is deterministic: the `(strength, polarity)` tuple selects
  exactly one of four locked templates (see [[docs/reference/compiled-output]]).
- `must` stays rare by convention — it is not just a more emphatic
  `require`. Repair infers `must` only when the source already carries
  hard-strength intent (trusted metadata, strong wording like `must_*`,
  `never_*`, `must_never_*`).

## Alternatives Considered

- **Four separate keywords (`should`/`should_not`/`must`/`must_not`).**
  Rejected: more vocabulary, no compositional insight.
- **Single keyword with explicit attributes.** Rejected: verbose and
  inconsistent with the rest of the source surface.
- **N strength levels.** Rejected: two is enough to drive distinct rendering;
  more would invite spurious distinctions.
