# ADR 0021 — Closed Five-Role IR

## Status

Accepted (MVP).

## Context

The IR needs to classify every author-written instruction by intent. There
are several reasonable taxonomies:

- **Output-shape driven** — roles match Markdown sections (`Step`,
  `Constraint`, `Parameter`, etc.). Pro: simple mental model. Con:
  Markdown layout is target-specific; the taxonomy should be source-driven.
- **Granular per-feature** — separate roles for `Precondition`,
  `Postcondition`, `FailurePolicy`, `Activation`, etc. Pro: expressive. Con:
  the set grows without obvious bound and most distinctions are not
  observable to the consuming agent.
- **Open / extensible roles** — let imports introduce new roles. Pro:
  flexibility. Con: kills cross-skill visualization and validation.

## Decision

The MVP instruction role set is **closed** to exactly five roles:

- `InputContract` — what must be provided at invocation.
- `Step` — an ordered action in the workflow.
- `Constraint` — a behavioral rule (with strength/polarity attributes).
- `OutputContract` — what the result should contain or satisfy.
- `Context` — non-normative informational framing.

Activation, preconditions, failure policies, and effects are **not** MVP
roles. They are either separate IR structures (effects) or deferred design
areas.

## Consequences

- Role inference and repair have a small target set, which keeps the
  evidence ordering tractable.
- Visualization and IR-JSON consumers can rely on a fixed enum.
- Constraint variants (`require`/`avoid`, `soft`/`hard`) collapse into one
  role with two attributes rather than four roles — see ADR 0019.
- Effects stay separate from roles: a step in a flow is `Step` with effect
  annotations, not simultaneously an `Effect` role. Conflating them would
  force calls like `inspect_repo(scope)` to be both `Step` and `Effect`.

## Alternatives Considered

- **Open role set.** Rejected: destroys cross-skill tooling and validation.
- **Output-driven role set.** Rejected: ties the source taxonomy to a
  specific target.
- **Larger role set with `Precondition`, `Activation`, etc.** Deferred:
  these may earn their own roles post-MVP if usage shows they cannot be
  expressed adequately as `InputContract` / `Constraint` / routing
  metadata.
