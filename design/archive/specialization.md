# Glyph Agent Specialization

Compile-time agent specialization: a reusable abstract agent exposes named slots, and concrete agents fill or extend only those slots. The compiler flattens everything before IR compilation -- there is no runtime inheritance.

## Post-MVP Status

Specialization is **not part of the MVP** declaration set. The MVP surface starts with `import`, `text`, `export block`, `block`, and `skill`. This document records a planned later extension and should not be read as adding `agent`, `abstract agent`, or `trait` to the MVP compiler.

For foundational principles that also govern specialization (no runtime dispatch, no hidden replacement, deterministic compilation), see `foundations.md`.

## Goals

Specialization should let authors:

- define a reusable abstract expert agent once;
- create concrete expert agents by changing only the parts that differ;
- preserve readability in the derived source;
- make inherited behavior visible after compilation;
- prevent accidental weakening of important inherited constraints.

## Core Model

Three public concepts:

- **`skill`** -- the public task artifact that compiles to Markdown instructions.
- **`agent`** -- a reusable behavioral identity that a skill may use.
- **`abstract agent`** -- a base agent that cannot be used directly; it exists only to be extended.

An abstract agent can expose constraints, named slots, and optional flow defaults.

```glyph
abstract agent ExpertAgent
    constraints:
        require reason_carefully
        require explain_tradeoffs

    locked constraints:
        require do_not_fabricate
        require state_uncertainty

    slots:
        domain_context
        evaluation_criteria
        failure_modes

    flow:
        understand_task()
        apply(domain_context)
        analyze_with(evaluation_criteria)
        check(failure_modes)
        return expert_answer()
```

A concrete agent extends the abstract agent and fills its slots:

```glyph
agent SecurityExpert extends ExpertAgent
    override domain_context:
        threat_modeling
        auth_boundaries
        secure_defaults

    override evaluation_criteria:
        exploitability
        blast_radius
        likelihood

    append failure_modes:
        ignoring_trust_boundaries
        treating_encryption_as_complete_security
```

The compiler expands `SecurityExpert` into a complete agent definition before IR compilation. The resulting IR carries no inheritance dependency.

An `agent` does not necessarily compile to a standalone output file. Currently it compiles to an agent IR fragment embedded into a skill when that skill selects the agent. Standalone agent artifacts may come later.

## Slots

A slot is a named extension point in a reusable definition. Slots may hold:

- instruction lists
- text blocks
- constraints (including preferred constraints)
- flow fragments
- parameter values
- domain-specific vocabularies

The base abstract agent declares which slots exist. A derived agent may only modify declared slots unless it explicitly declares new local slots.

## Slot Operations

Derived definitions must be explicit about how they change a slot.

**`override`** -- replaces the base slot entirely:

```glyph
override evaluation_criteria:
    exploitability
    blast_radius
```

**`append`** -- adds content after the base slot:

```glyph
append failure_modes:
    ignoring_trust_boundaries
```

**`prepend`** -- adds content before the base slot:

```glyph
prepend domain_context:
    user_environment
```

These operations are valid only for compatible slot kinds. Appending to a scalar slot or overriding a locked slot produces a compiler diagnostic.

## Slot Merge Order

Merging is deterministic and single-pass. For single-base specialization:

1. Flatten the base chain from oldest ancestor to immediate base.
2. Start each slot with its fully merged inherited value.
3. If the derived agent has an `override`, replace the inherited value.
4. Apply `prepend` entries before the current value (source order preserved).
5. Apply `append` entries after the current value (source order preserved).

Example:

```glyph
abstract agent ExpertAgent
    failure_modes:
        shallow_answer
        overconfidence

agent SecurityExpert extends ExpertAgent
    prepend failure_modes:
        ignoring_trust_boundaries

    append failure_modes:
        treating_encryption_as_complete_security
```

Normalized `failure_modes`:

```text
ignoring_trust_boundaries
shallow_answer
overconfidence
treating_encryption_as_complete_security
```

When a derived agent uses both `override` and `append` on the same slot, append applies after override:

```text
override evaluation_criteria: [exploitability, blast_radius]
append  evaluation_criteria: [likelihood]
  =>  [exploitability, blast_radius, likelihood]
```

Multiple `override` operations for the same slot in the same agent are rejected.

## Locked Constraints

Base agents can mark constraints as locked to prevent derived agents from weakening them.

```glyph
abstract agent ExpertAgent
    locked constraints:
        require state_uncertainty
        require do_not_fabricate
```

Derived agents may add stronger or more specific constraints around locked ones, but may not remove, replace, weaken, or contradict them.

A locked constraint is a semantic contract. It becomes a stable constraint in the normalized agent IR. Derived agents must preserve three properties:

- **Presence.** The locked constraint must still appear in the flattened agent.
- **Potency.** Strength, polarity, and scope may not be weakened (e.g. `require` cannot become `prefer`).
- **Non-contradiction.** The derived agent may not add behavior that conflicts with the locked constraint.

The compiler represents locked constraints as named constraint nodes with stable identity:

```text
LockedConstraint {
  id: "ExpertAgent.do_not_fabricate",
  strength: required,
  polarity: require,
  definition: do_not_fabricate,
  source_agent: ExpertAgent
}
```

For list-like slots, derived agents may add compatible constraints before or after locked ones. For order-sensitive slots (workflow fragments), the base's locked relative order must be preserved unless an explicit extension point exists.

Some contradictions are detectable deterministically (overriding a locked slot, weakening strength, flipping polarity). More semantic contradictions may need a validation diagnostic.

## Base Agent Versioning

Inherited behavior is not written out in derived source, so derived agents need a stable story for base changes.

Each exported abstract agent exposes:

- a **contract version** (authored);
- a **contract fingerprint** (compiler-computed).

The contract covers: slot names and kinds, allowed operations per slot, locked constraint identities, required parameters and defaults, exported agent name and visibility.

The compiler may also compute a **behavior fingerprint** over inherited instruction content, answering "did inherited behavior change enough to warrant review?"

Derived agents record the base version they target:

```glyph
agent SecurityExpert extends ExpertAgent@^1
    override domain_context:
        threat_modeling
        auth_boundaries
```

Version rules:

- Compatible contract changes compile but surface inherited behavior changes for review.
- Incompatible contract changes fail with a migration diagnostic.
- Repair may update a version pin only when the agent still compiles and the change is clearly compatible.
- Repair must not silently adapt to removed slots, changed slot kinds, or weakened locked constraints.

Breaking changes include: removing/renaming a slot, changing a slot kind, making an overrideable slot locked, removing/renaming a locked constraint identity, or changing required parameters incompatibly.

## Traits

Traits provide reusable cross-cutting behavior without forcing an inheritance chain.

```glyph
trait EvidenceFirst
    behavior:
        distinguish_observation_from_inference
        cite_basis_for_claims

agent SecurityExpert extends ExpertAgent with EvidenceFirst
    override domain_context:
        threat_modeling
        auth_boundaries
```

Traits are flattened at compile time. If two traits modify the same slot, conflict handling must be explicit. The first version of Glyph may defer traits until single-base specialization is stable.

Trait merge order: traits merge after the base agent and before the derived agent's local operations, in explicit source order. Unresolvable conflicts require a local override or produce a diagnostic.

## Conflict Handling

The compiler rejects ambiguous specialization. Diagnostics for:

- overriding a slot the base did not declare
- modifying a locked slot
- operation incompatible with slot kind
- two traits with conflicting content in the same slot
- inheritance cycles
- exceeding configured inheritance-depth limit

## IR Normalization

Specialization compiles away before the main IR contract. A source `agent SecurityExpert extends ExpertAgent` with overrides normalizes into a flat `Agent { name, source_base, constraints, slots, flow }` node where all inherited values are fully expanded. Downstream compiler passes see a complete, explicit agent definition with no inheritance edges.

## Relationship To Skills And Blocks

`agent` does not replace `skill` or `block`:

- `agent` / `abstract agent` -- reusable behavioral identity, role, stance, defaults, constraints, extension points.
- `skill` -- public unit of reusable task behavior.
- `block` -- internal unit of structure, reuse, and testing.

A skill may use an agent:

```glyph
skill answer_security_question(question, agent = SecurityExpert)
    flow:
        context = prepare_context(question)
        return agent.answer(context)
```

Agent calls follow the same data-flow and calls design: explicit values, analyzable call nodes.

## Repair Behavior

The LLM repair pass may fix specialization issues by adding minimal syntax:

- changing an implicit slot modification into `override`/`append`/`prepend` when diagnostics make intent clear;
- adding a missing slot declaration to a base if the source clearly defines and uses it;
- adding an explicit trait conflict diagnostic instead of guessing a merge.

Repair must not silently change inherited behavior or flatten an agent by expanding everything into prose.

## Open Questions

- Final keywords for `agent`, `abstract agent`, `extends`, locked constraint keyword (`locked`/`final`/`sealed`)
- Trait syntax (`with`, `uses`, or import-like) and whether traits ship with first specialization release
- Slot kind catalog, declaration syntax, and per-kind operation compatibility
- Locked constraint identity declaration/inference rules
- Base agent version/fingerprint source syntax
- Depth and visibility limits for cross-module agent extension
- Whether `agent.answer(context)` is a callable surface, compiler shorthand, or later runtime concept
