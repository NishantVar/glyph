# Glyph Design — Open TODOs

Items deferred from MVP decisions that should be revisited in future tiers.

## Values & Literals

- **Extended escape sequences in inline strings.** MVP supports only `\"` and `\\`. Consider adding `\n`, `\t`, and Unicode escapes (`\uXXXX`) post-MVP if real authoring needs emerge.
- **Enums/symbols.** MVP uses strings for enumerated values (`risk = "medium"`). Consider a dedicated enum type post-MVP if validation or exhaustiveness checking becomes valuable.
- **Scientific notation for numbers.** MVP supports integer and float literals only. Add `1e10` form if needed.

## Authoring Surface

- **Block comments and doc-comments.** MVP defines `//` line comments (see `repair.md`, section 6). Consider adding block comments (`/* ... */`) or doc-comments post-MVP if structured documentation or inline annotation needs emerge.
- **Preconditions.** Revisit whether `InputContract` should split into required inputs and state preconditions. For MVP, invocation requirements belong under `InputContract`; later design may need a distinct construct for "only valid after X is true" or "before running this, establish Y."
- **Failure policy.** Add a post-MVP construct for what to do when assumptions fail, inputs are missing, validation fails, or tool calls cannot run. Until then, simple cases should be represented with workflow structure or constraints.
- **Activation contract.** Add a post-MVP routing/trigger construct for when a skill should be selected. Keep it separate from execution roles.
- **Constraint compilation treatment.** Design how constraint strength and polarity affect target-specific compilation, including prominence, repetition, wording, demotion protection, and specialization behavior for invariant constraints.

## Compiled Output Sections (Deferred From MVP)

MVP compiled output contains only YAML frontmatter (`name`, `description`, `effects`) and one H2 section (`## Instructions` with `### Steps` and `### Constraints`). The following sections were removed from MVP but may be restored post-MVP if author or agent-consumption needs emerge:

- **`## Inputs` section.** Removed because MVP uses per-invocation compilation: parameters resolve to concrete values at expand time and are woven into Step prose, so a dedicated input section is redundant. Restoring would require a two-tier compilation model (abstract card for discovery, concrete body per call) or a convention for rendering unresolved params. Also revisit the `inputs:` source sub-section header at the same time.
- **`## Output` section.** Removed because `return` folds into the final Step. Restore if output contracts become rich enough (typed return shapes, post-conditions) that folding them into prose loses information. Also revisit the `outputs:` source sub-section header.
- **`## Effects` section.** Removed because effects now live only in YAML frontmatter. Restore if executing agents (not just selectors/tooling) need effects visible in the prose body. Human-readable expansions (e.g. "Reads files (source code, logs, test output)") would live here.
- **`## When To Use` section.** Removed because routing fully moves to frontmatter `description`. Restore if trigger guidance exceeds what fits in a one-line description, or if tooling wants a structured routing block separate from description. Also revisit the `when_to_use:` source sub-section header.

## IR Roles

- **`Context` role.** MVP closes the role set to four: `InputContract`, `Step`, `Constraint`, `OutputContract`. `Context` (non-normative informational framing) was dropped because with `## Inputs` gone there is no section for it to project into, and any genuine context can be authored as a Step, a Constraint, or a leading inline sentence in `flow:`. Restore post-MVP if authors consistently produce framing text that doesn't fit the four roles. Reserved keyword `context` stays reserved for this restoration.

## Calls & Parameters

- **Method-style call syntax (`receiver.foo(args)`).** Post-MVP. MVP allows only bare calls (`foo(x)`) and single-level qualified callees (`Alias.foo(x)`). Revisit method-style as pure sugar for `foo(receiver, args)` once there's a clear need; requires a rule to disambiguate from the qualified-callee form.
- **Deeper qualified callee nesting (`a.b.c`).** MVP allows only single-level qualified callees (`Alias.name`). Revisit for multi-level namespace access once the import model is richer.
- **Spread / splat arguments.** Post-MVP. Allow passing a collection as positional args (e.g. `foo(*items)`). Blocked on introducing a collection type.
- **Variadic parameters.** Post-MVP. Declaration-side form for blocks that accept a variable number of arguments (e.g. `block foo(*items: String)`). Not allowed in MVP. Blocked on collection types and on a parameter-grammar extension (optional / rest / keyword-only).

## Preferences

- **Pref value override mechanism.** MVP defines pref values inline in the declaring file (the literal on the right of `=` is the final value). Post-MVP: allow overriding defaults from a project config file (e.g. `glyph.config.yaml`), CLI flags, or environment variables. Decide precedence and whether overrides can add new prefs or only override declared ones.
- **Standard prefs file shipped with the compiler.** The compiler should ship a default `prefs.glyph.md` at a known path so any project can `import { tone, verbosity, ... } from "@glyph/prefs"` (or equivalent) and get a baseline pref set without defining their own. Decide the import scheme (`@glyph/...` namespace, well-known relative path, or other), what the standard pref set is, and how it composes with user-defined prefs.

## Diagnostics

- **Maybe: formalize auto vs. LLM repair distinction in diagnostic classification.** `pipeline.md` Phase 2 tags diagnostics as `error`/`repairable`/`warning`, but Phase 3 already has two sub-steps: 3a (deterministic auto-fixes like tab→spaces, unused import removal) and 3b (LLM-assisted fixes like unresolved names, ambiguous roles). Currently nothing in the diagnostic shape distinguishes these — both are just `repairable`. Consider splitting `repairable` into `repairable(auto)` and `repairable(llm)` if the difference matters for author notification or trust. May not be needed if Phase 3 handles the distinction internally.
- **Maybe: effect-expansion table for human-readable prose.** MVP puts effects in YAML frontmatter as raw keywords (`effects: [reads_files, writes_files]`). If `## Effects` is ever restored as a prose section (see Compiled Output Sections above), or if visualization tooling wants human-readable effect descriptions, define a canonical expansion table mapping each keyword to a verb phrase + parenthetical examples (e.g. `reads_files` → "Reads files (source code, logs, test output)").

## Design Coherence

- **Canonicalize the compiler pipeline.** The pipeline is described differently across README (5 passes), `language-surface.md` (9 steps), and `foundations.md` (prose summary). Settle on one canonical description and have the others reference it.
- **`context` disambiguator syntax.** `context` is a reserved keyword and available as an "author-facing disambiguator" per `ir-and-semantics.md`, but no document shows its source syntax or placement. Define how authors explicitly mark something as `Context` role, or explicitly defer it.

## Inheritance & Specialization

- **Skill-level inheritance via block override.** Post-MVP. Allow a skill to derive from an exported parent skill (e.g. `skill security_review(scope) from code_review`), inheriting the parent's flow, constraints, effects, and description. The child overrides behavior by defining local blocks that shadow the parent's exported blocks — name resolution picks the local definition, so blocks are the implicit extension points. Constraints can be appended/prepended. Locked constraints on the parent cannot be removed or weakened by the child. The compiler flattens inheritance during the Lower phase; no runtime inheritance.
- **Earlier `agent`/`abstract agent` design (archived).** An earlier heavier proposal (`archive/specialization.md`) introduced `agent` and `abstract agent` as separate declaration kinds with named slots, override/append/prepend operations, traits, and contract versioning. Key ideas worth revisiting: locked constraints (presence/potency/non-contradiction rules), deterministic single-pass merge order, trait-based cross-cutting behavior, base agent versioning with contract fingerprints. The simpler block-override model above may replace this entirely — evaluate once MVP usage patterns clarify real reuse needs.

## Compiler & Runtime

- **Runtime compilation and mutations.** Explore whether skills (or parts of skills) can be compiled and mutated at runtime — e.g. dynamic specialization based on agent state, live parameter injection, or hot-swapping compiled output mid-session. Consider what invariants must hold across a mutation boundary and whether a versioned IR helps.
- **Deduplication of shared imports across multi-skill composition.** When multiple included/imported skills pull in the same dependency (e.g. the same shared block or text), the compiled output currently may repeat that content. Design a merge/dedup pass so identical imported fragments are emitted once, with a clear ownership and conflict-resolution model for near-identical variants.
