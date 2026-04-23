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

## Calls & Parameters

- **Method-style call syntax (`receiver.foo(args)`).** Post-MVP. MVP allows only bare calls (`foo(x)`) and single-level qualified callees (`Alias.foo(x)`). Revisit method-style as pure sugar for `foo(receiver, args)` once there's a clear need; requires a rule to disambiguate from the qualified-callee form.
- **Deeper qualified callee nesting (`a.b.c`).** MVP allows only single-level qualified callees (`Alias.name`). Revisit for multi-level namespace access once the import model is richer.
- **Spread / splat arguments.** Post-MVP. Allow passing a collection as positional args (e.g. `foo(*items)`). Blocked on introducing a collection type.
- **Variadic parameters.** Post-MVP. Declaration-side form for blocks that accept a variable number of arguments (e.g. `block foo(*items: String)`). Not allowed in MVP. Blocked on collection types and on a parameter-grammar extension (optional / rest / keyword-only).

## Preferences

- **Pref value override mechanism.** MVP defines pref values inline in the declaring file (the literal on the right of `=` is the final value). Post-MVP: allow overriding defaults from a project config file (e.g. `glyph.config.yaml`), CLI flags, or environment variables. Decide precedence and whether overrides can add new prefs or only override declared ones.
- **Standard prefs file shipped with the compiler.** The compiler should ship a default `prefs.glyph.md` at a known path so any project can `import { tone, verbosity, ... } from "@glyph/prefs"` (or equivalent) and get a baseline pref set without defining their own. Decide the import scheme (`@glyph/...` namespace, well-known relative path, or other), what the standard pref set is, and how it composes with user-defined prefs.

## Compiler & Runtime

- **Runtime compilation and mutations.** Explore whether skills (or parts of skills) can be compiled and mutated at runtime — e.g. dynamic specialization based on agent state, live parameter injection, or hot-swapping compiled output mid-session. Consider what invariants must hold across a mutation boundary and whether a versioned IR helps.
- **Deduplication of shared imports across multi-skill composition.** When multiple included/imported skills pull in the same dependency (e.g. the same shared block or text), the compiled output currently may repeat that content. Design a merge/dedup pass so identical imported fragments are emitted once, with a clear ownership and conflict-resolution model for near-identical variants.
