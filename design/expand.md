# Expand — Author-Facing Contract

This document describes what the **Expand** pass changes in the compiled output that an author should understand. The mechanics — scaffold spans, the LLM reshape sub-pass, the Phase 6b validation gate, retry policy, and the diagnostic catalog — live in [[docs/architecture/expand]].

## What Expand Does

Expand is the compiler step that turns the resolved, validated IR into the body of the compiled Markdown — the `## Parameters`, `## Context`, `## Steps`, and `## Constraints` sections an author sees in `foo.md`. It is the only pass that may apply natural-language judgement to author content:

- `with` modifiers are folded into the prose of the Step they specialize.
- Scoped constraints declared on a called `block` are woven into the inlined Step's prose, not promoted to the top-level `## Constraints` list (see [[data-flow]] §Constraint Scoping).
- Local-binding references (e.g., `{diagnosis}` where `diagnosis` is a local, not a parameter) are resolved into natural-language cross-references in the prose.
- The `OutputContract` (from `return <name>` or `return <"description">`) is folded into the wording of the final Step.

The frontmatter (`name`, `description`, `effects`) is **not** assembled by Expand — it is emitted by Phase 7 (Emit) from skill-level IR metadata ([[design/compiled-output]]).

## What Authors Can Rely On

Expand is **structurally constrained**. The deterministic Phase 6b validation gate runs after the reshape and rejects any output that does not project the IR 1-to-1. That gate is the contract an author can build on:

- **Role preservation (1-to-1).** Every top-level `Step` node, every top-level `Call`/`InlineInstruction`/`InstructionRef` that projects to a Step, and every top-level `Branch` produces exactly one numbered item under `## Steps`, in IR order. Every `Constraint` node produces exactly one bulleted item under `## Constraints`. Every `Branch` arm produces lettered sub-steps (`a.`, `b.`, …) in the same count and order as the arm's IR body. The `Return` expression folds into the final Step rather than producing a separate item.
- **No invented content.** Expand may reshape wording but must not add new steps, new constraints, new sub-steps, new sections, or commentary. It must not invent `{param}` references that are not declared parameters.
- **Parameter references survive.** `{param}` references for declared parameters pass through unchanged.
- **No authoring artifacts in output.** `with` modifier text, `generated` markers, import paths, and local-binding `{name}` tokens are all gone from the compiled Markdown.

If Expand cannot produce an output that satisfies the 1-to-1 contract within its retry budget, the compiler aborts with the specific Phase 6b diagnostic on stderr and does not write the `.md` file. The failure is loud, not silent.

## What Authors Cannot Rely On

Expand is **not idempotent**. Two compilations of the same skill — even with no source changes — may produce two `.md` files whose prose differs word-for-word. This is honest about the LLM in the pipeline; structural shape is stable, prose is not.

Practical consequences for authors:

- **Diff-based CI** that compares compiled `.md` across commits will see prose churn even when the source is unchanged. Cache compiled outputs alongside source, or skip diffing compiled artifacts entirely.
- **Snapshot tests** should assert structural shape (step counts, ordering, section presence, frontmatter) rather than byte-identical prose. The Phase 6b checks are themselves the authoritative structural contract — assert on the same shape Phase 6b enforces.
- **Semantic faithfulness of wording is not verified.** Phase 6b is a structural gate, not a semantic one. The mitigations are the single-string rule for generated bodies ([[design/repair]] §5), the resolved-body text flowing through Expand unchanged, and the role-preservation 1-to-1 check. Authors who want strong semantic stability should prefer explicit, concrete inline strings over loose phrasing that the reshape pass might smooth differently each run.

## Cross-References

- **Compiled output shape** — [[design/compiled-output]] (the H2 catalogue, constraint rendering, return-fold suffix, procedure projection tiers).
- **Repair contrast** — [[design/repair]] (Repair is source-to-source and idempotent; Expand is IR-to-Markdown and is not).
- **IR roles and effects** — [[ir-and-semantics]] (the role vocabulary the 1-to-1 contract is keyed on).
- **Maintainer-facing mechanics** — [[docs/architecture/expand]] (scaffold + spans, Step 2 internals, Phase 6b algorithm and diagnostic catalog, retry policy).
- **CLI surface for the validator** — [[docs/reference/cli]] §`glyph validate-output` (exit codes, IO contract for the external Phase 6b runner).
