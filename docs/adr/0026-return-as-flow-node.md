# ADR 0026 — Return as a Flow Node, Rendered Deterministically as `Output:`

## Status

Proposed.

## Context

The OutputContract — the `return <"...">` or `return <identifier>` line at
the end of a Glyph skill — was lifted out of `flow` and stored as a separate
`skill.output_contract` field on the IR. The compiled-Markdown emitter laid
down no slot for it; the Expand pass was responsible for paraphrasing the
description into a sentence and **folding it into the final Step's prose**
(Expand Step 4).

This worked while every skill had a single linear final Step. It breaks the
moment the final flow node is a branch: the fold lands inside one arm, which
silently asymmetrizes the deliverable — the consuming agent reads as if one
arm produces the deliverable and the other does not. The asymmetry passes
`validate-output` because both arms structurally contain a Step body.

Two adjacent design pressures push toward a different shape:

- Arm-local `return` is a likely post-MVP extension (`if x: return <a>` /
  `else: return <b>`). A fold mechanism rooted at "the final Step" has no
  natural place to put per-arm returns.
- The Expand pass owns prose, not structure. Return rendering is mechanical:
  one fixed sentence shape per variant, no judgement required. Giving it to
  the LLM costs latency and creates the asymmetry above.

## Decision

A `return` becomes a **node in the `flow` array** at its source position,
the same way `Call` and `Branch` nodes are. The IR introduces a `Return`
flow node with two variants:

- `{ kind: "return", form: "description", description: "...", ty: ... }`
- `{ kind: "return", form: "identifier", local_ref: "<name>",
    producer_node_id: "<node_id>", ty: ... }`

For the identifier variant the producer's `NodeId` is resolved at lower
time from the binding's producing flow node and stored explicitly on the
`Return` node. The deterministic emitter consumes `producer_node_id`
directly when rendering `<name> from step <M>` — it does not re-walk the
flow with name resolution at emit time. `producer_node_id` is omitted for
the description form and for identifier returns that don't resolve to a
flow-local producer (e.g. when the identifier shadows a parameter).

`skill.output_contract` may remain as a top-level metadata view (type, form,
description text) for type-checking and tooling, but the **renderable
position** of the return is its slot in `flow`. Arm-local returns are just
Return nodes inside a branch arm's `then_body` / `else_body`.

The deterministic emitter renders a Return node as:

- Top-level: `<N>. Output: <description>.`
- Inside a branch arm: `a. Output: <description>.`
- Identifier variant: `<N>. Output: <name> from step <M>.` where `M` is
  the step number of the producing flow node referenced by the Return
  node's `producer_node_id`.

Expand Step 4 (the OutputContract fold) is removed. The "OutputContract
Identifier return-fold suffix" preservation note in Expand's constraint list
is removed. Local-ref resolution for `return <name>` becomes a deterministic
emitter responsibility, not an LLM one.

`glyph validate-output` updates its structural check: it walks the IR's
`flow` array in order and verifies each Return node has a matching
`Output: …` line at its expected position in the compiled Markdown.

## Consequences

- Branch-arm returns become trivially representable, unblocking the
  post-MVP `if/else return` form without further structural work.
- The compiled Markdown for skill-scope `return <"...">` is symmetric across
  branch arms: a single trailing `Output:` step, not a fold inside one arm.
- Expand loses a responsibility (the fold) and a preservation rule (the
  return-fold suffix), shrinking the LLM contract surface.
- One LLM responsibility per skill is eliminated, since return rendering
  becomes fully deterministic.
- The Identifier variant's cross-reference phrasing becomes a single fixed
  form (`<name> from step <M>`) instead of LLM-chosen. This narrows
  expressiveness but removes a per-call variance source.
- Existing compiled `.md` artifacts must be regenerated. Source files are
  unchanged; the change is downstream of parsing.

## Alternatives Considered

- **Keep the fold, require a non-branch final step.** Rejected: forces
  authors to write a trailing no-op step purely to host the fold. Doesn't
  help arm-local returns.
- **Emit a top-level `## Output` section instead of a flow step.**
  Rejected: doesn't generalize to arm-local returns and doesn't preserve
  flow position.
- **Append a trailing `Produce:` line at the end of `## Steps`.** Rejected
  for the same reason: hard-codes a single trailing return position.
