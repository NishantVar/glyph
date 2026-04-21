# Glyph Design — Open TODOs

Items deferred from MVP decisions that should be revisited in future tiers.

## Values & Literals

- **Extended escape sequences in inline strings.** MVP supports only `\"` and `\\`. Consider adding `\n`, `\t`, and Unicode escapes (`\uXXXX`) post-MVP if real authoring needs emerge.
- **Enums/symbols.** MVP uses strings for enumerated values (`risk = "medium"`). Consider a dedicated enum type post-MVP if validation or exhaustiveness checking becomes valuable.
- **Scientific notation for numbers.** MVP supports integer and float literals only. Add `1e10` form if needed.

## Authoring Surface

- **Comments in source files.** MVP has no comment syntax. Add a comment form (e.g. `# ...` line comments) so authors can annotate skills without affecting compiled output. Decide whether block/inline comment variants are needed and how comments interact with the LLM repair pass.
- **Preconditions.** Revisit whether `InputContract` should split into required inputs and state preconditions. For MVP, invocation requirements belong under `InputContract`; later design may need a distinct construct for "only valid after X is true" or "before running this, establish Y."
- **Failure policy.** Add a post-MVP construct for what to do when assumptions fail, inputs are missing, validation fails, or tool calls cannot run. Until then, simple cases should be represented with workflow structure or constraints.
- **Activation contract.** Add a post-MVP routing/trigger construct for when a skill should be selected. Keep it separate from execution roles.
- **Constraint compilation treatment.** Design how constraint strength and polarity affect target-specific compilation, including prominence, repetition, wording, demotion protection, and specialization behavior for invariant constraints.

## Compiler & Runtime

- **Runtime compilation and mutations.** Explore whether skills (or parts of skills) can be compiled and mutated at runtime — e.g. dynamic specialization based on agent state, live parameter injection, or hot-swapping compiled output mid-session. Consider what invariants must hold across a mutation boundary and whether a versioned IR helps.
- **Deduplication of shared imports across multi-skill composition.** When multiple included/imported skills pull in the same dependency (e.g. the same shared block or text), the compiled output currently may repeat that content. Design a merge/dedup pass so identical imported fragments are emitted once, with a clear ownership and conflict-resolution model for near-identical variants.
