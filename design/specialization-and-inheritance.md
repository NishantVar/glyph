# Glyph Agent Specialization And Inheritance

This document defines how Glyph should support inheritance-like reuse for agents without adopting unrestricted object-oriented inheritance. The initial model is compile-time agent specialization: a reusable abstract agent exposes named slots, and concrete agents replace or extend only those slots.

## MVP Status

Specialization is not part of the MVP top-level declaration set. The MVP source language starts with `import`, `text`, `export block`, `block`, and `skill`. This document records a likely later extension point and should not be read as adding `agent`, `abstract agent`, or `trait` to the MVP compiler surface.

## Goals

Specialization should let authors:

- define a reusable abstract expert agent once;
- create specific concrete expert agents by changing only the parts that differ;
- preserve readability in the derived source;
- make inherited agent behavior visible after compilation;
- prevent accidental weakening of important inherited constraints.

## Non-Goals

Glyph should not start with general-purpose class inheritance.

The specialization model should not allow:

- arbitrary hidden replacement of inherited behavior;
- deep inheritance chains that are hard to inspect;
- runtime method dispatch;
- implicit changes to behavior outside declared extension points;
- multiple inheritance with ambiguous conflict resolution.

Inheritance-like reuse is an authoring convenience. The compiler should flatten it before IR compilation.

## Core Model

The public concepts are `skill` and `agent`.

- A `skill` is the public task artifact that compiles to Markdown instructions.
- An `agent` is a reusable behavioral identity that a skill may use.
- An `abstract agent` is a reusable base agent that cannot be used directly as the selected agent for a skill. It exists only to be extended by concrete agents.

An abstract expert agent can expose constraints, named slots, and optional flow defaults.

Example:

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

A specific concrete agent extends the abstract agent and changes named slots:

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

The compiler expands `SecurityExpert` into a complete agent definition before IR compilation. The resulting IR should not depend on runtime inheritance.

An `agent` does not necessarily compile to its own standalone output file. In the current direction, it compiles to an agent IR fragment that is embedded into a skill when the skill selects or defaults to that agent. A later target may emit standalone agent artifacts, but that is not required for the MVP.

## Slots

A slot is a named extension point in a reusable definition.

Slots may hold:

- instruction lists;
- text blocks;
- constraints, including preferred constraints;
- flow fragments;
- parameter values;
- domain-specific vocabularies.

The base abstract agent should declare which slots exist. A derived agent may only modify declared slots unless it explicitly declares new local slots.

## Override, Append, And Prepend

Derived definitions should be explicit about how they change a slot.

`override` replaces the base slot:

```glyph
override evaluation_criteria:
    exploitability
    blast_radius
```

`append` adds content after the base slot:

```glyph
append failure_modes:
    ignoring_trust_boundaries
```

`prepend` adds content before the base slot:

```glyph
prepend domain_context:
    user_environment
```

These operations should be valid only for compatible slot kinds. Appending to a scalar slot or overriding a locked slot should produce a compiler diagnostic.

## Slot Merge Order

Slot merging must be deterministic and easy to explain. For the first version, single-base specialization should use this order:

1. Flatten the base chain from oldest ancestor to immediate base.
2. Start each slot with the fully merged inherited value.
3. If the derived agent has an `override` for that slot, replace the inherited value with the override value.
4. Apply all derived `prepend` entries for that slot before the current value, preserving their source order.
5. Apply all derived `append` entries for that slot after the current value, preserving their source order.

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

If a derived agent uses both `override` and `append`, the append applies after the override:

```glyph
agent SecurityExpert extends ExpertAgent
    override evaluation_criteria:
        exploitability
        blast_radius

    append evaluation_criteria:
        likelihood
```

Normalized `evaluation_criteria`:

```text
exploitability
blast_radius
likelihood
```

The first version should reject multiple `override` operations for the same slot in the same agent. Traits, if added later, should merge after the base agent and before the derived agent's local operations, in the explicit source order listed by the agent. Any trait conflict that cannot be represented by this order should require an explicit local override or diagnostic.

## Locked Constraints

Base abstract agents should be able to mark constraints as locked when derived agents must not weaken them.

Example:

```glyph
abstract agent ExpertAgent
    locked constraints:
        require state_uncertainty
        require do_not_fabricate
```

A derived agent may add stronger or more specific constraints around locked constraints, but may not remove, replace, weaken, or contradict them.

Locked constraints are useful for safety, honesty, validation, and project-wide guarantees.

Locked constraints are a semantic contract, not just a label. A locked constraint becomes a stable constraint in the normalized agent IR. Derived agents must preserve three properties:

- **Presence.** The locked constraint identity from the base must still appear in the flattened derived agent.
- **Potency.** The constraint's role, strength, polarity, and scope may not be weakened. For example, a base `require do_not_fabricate` cannot become a `prefer do_not_fabricate`.
- **Non-contradiction.** The derived agent may not add behavior that conflicts with the locked constraint.

The compiler should represent locked constraints as named constraint nodes, not as anonymous prose. A locked constraint should have a stable identity, normalized text or generated definition reference, role, strength, polarity, scope, and source agent.

Example normalized shape:

```text
LockedConstraint {
  id: "ExpertAgent.do_not_fabricate",
  role: Constraint,
  strength: required,
  polarity: require,
  definition: do_not_fabricate,
  source_agent: ExpertAgent
}
```

For list-like instruction slots, derived agents may add compatible constraints before or after locked constraints. For order-sensitive slots such as workflow fragments, the base's locked relative order must be preserved unless the base explicitly declares an extension point.

Some contradictions can be detected deterministically, such as overriding a locked slot, weakening constraint strength, flipping polarity, or requiring the opposite of a named locked constraint. More semantic contradictions may need a validation diagnostic rather than silent repair.

## Base Agent Versioning

Derived agents need a stable story for base changes because inherited behavior is not written out in the derived source.

Each exported base abstract agent should expose a public contract version and a compiler-computed contract fingerprint. The contract includes:

- slot names and slot kinds;
- allowed operations for each slot;
- locked constraint identities;
- required parameters and defaults that affect specialization;
- exported agent name and visibility.

The compiler may also compute a behavior fingerprint that includes inherited instruction content. The contract fingerprint answers "does this derived agent still type-check against the same base surface?" The behavior fingerprint answers "did the inherited behavior change enough that a human may want to skim it?"

A derived agent should record or resolve the base version it was written against:

```glyph
agent SecurityExpert extends ExpertAgent@^1
    override domain_context:
        threat_modeling
        auth_boundaries
```

The exact syntax is open, but the rule is:

- compatible base contract changes may compile, but should surface inherited behavior changes for review;
- incompatible contract changes should fail with a migration diagnostic;
- repair may update a version pin or fingerprint only when the agent still compiles and the change is clearly compatible;
- repair must not silently adapt to removed slots, changed slot kinds, or weakened locked constraints.

Breaking base changes include removing or renaming a slot, changing a slot kind, making an overrideable slot locked, removing a locked constraint, renaming a locked constraint identity, or changing required parameters in a way the derived agent does not satisfy.

## Traits

Traits can provide reusable cross-cutting behavior without forcing an inheritance chain.

Example:

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

Traits should be flattened at compile time. If two traits modify the same slot, conflict handling must be explicit. The first version of Glyph may defer traits until single-base specialization is stable.

## Conflict Handling

The compiler should reject ambiguous specialization.

Diagnostics should be produced for:

- overriding a slot that the base did not declare;
- modifying a locked slot;
- applying an operation incompatible with the slot kind;
- importing two traits that write conflicting content into the same slot;
- creating inheritance cycles;
- exceeding any configured inheritance-depth limit.

## IR Normalization

Specialization should compile away before the main IR contract.

Source:

```glyph
agent SecurityExpert extends ExpertAgent
    override domain_context:
        threat_modeling
```

Normalized form:

```text
Agent {
  name: SecurityExpert,
  source_base: ExpertAgent,
  constraints: [...flattened constraints...],
  slots: {
    domain_context: [threat_modeling],
    evaluation_criteria: [...inherited...],
    failure_modes: [...inherited...]
  },
  flow: [...flattened flow...]
}
```

The exact IR syntax is open. The semantic requirement is that downstream compiler passes see a complete, explicit agent definition.

## Relationship To Skills And Blocks

`agent` is not a replacement for `skill` or `block`.

- `agent` describes reusable agent behavior, role, stance, defaults, constraints, and extension points.
- `abstract agent` describes a reusable base that must be extended before use.
- `skill` remains the public unit of reusable task behavior.
- `block` remains the internal unit of structure, reuse, and testing.

A skill may use an agent:

```glyph
skill answer_security_question(question, agent = SecurityExpert)
    flow:
        context = prepare_context(question)
        return agent.answer(context)
```

The data-flow and calls design still applies: agent calls should pass explicit values and compile into analyzable call nodes.

## Repair Behavior

The LLM repair pass may fix specialization issues by adding minimal syntax.

Allowed repairs include:

- changing an implicit slot modification into `override`, `append`, or `prepend` when diagnostics make the intent clear;
- adding a missing slot declaration to a base abstract agent if the source clearly defines and uses it;
- adding an explicit trait conflict diagnostic instead of guessing a merge.

Repair should not silently change inherited behavior or flatten an agent by expanding everything into prose.

## Important Remaining Gaps

The specialization design still needs decisions in these areas:

- **Slot kinds and compatibility.** Define which slot kinds exist, which operations each supports, and how the compiler validates `override`, `append`, and `prepend`.
- **Depth and visibility limits.** Decide whether agents can extend abstract agents across modules, how much inheritance depth is allowed, and how flattened inherited behavior is displayed.
- **Trait conflict policy.** Decide whether traits are deferred, and if not, how conflicts are detected and resolved without implicit ordering surprises.
- **Agent-call contract.** Define whether `agent.answer(context)` is a real callable surface, a compiler shorthand, or a later runtime concept.

## Open Syntax Choices

The following details remain open:

- whether `agent` and `abstract agent` are the final keywords;
- whether `extends` is the right keyword for single-base agent specialization;
- whether traits use `with`, `uses`, or import-like syntax;
- how slot kinds are declared;
- whether locked constraints use `locked`, `final`, `sealed`, or another keyword;
- how locked constraint identities are declared or inferred;
- how base agent versions and fingerprints are written in source;
- whether traits are included in the first version or deferred.
