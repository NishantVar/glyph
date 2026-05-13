# ADR 0015: Repair Idempotence Anchored On Name Resolution

## Status

Accepted (MVP).

## Context

Repair is the primary content-generation mechanism for novice authors. It may materialize `generated const` and `generated block` declarations on the first compile, then must **not** regenerate, rename, or rewrite those declarations on subsequent compiles unless something the author would recognize as an input change occurred (source edited, imports changed, language rules changed).

Several detection strategies were considered:

- **Fingerprinting / content hashing.** Compute a hash of the generated body and embed it in source (e.g. `// generated-hash: abc123`). On re-compile, re-derive what would be generated and compare hashes.
- **Versioning by compiler / model release.** Tag each generated declaration with `// generated-by: glyph 0.5, model claude-sonnet-4.7` and re-generate when the tag changes.
- **Name resolution.** If the name already resolves to *any* declaration in scope — `const`, `generated const`, `block`, `generated block`, import, parameter, local binding — Repair skips the generation step entirely. No tag, no hash.

Fingerprinting and versioning each add a piece of compiler-managed metadata to source that the author must understand and not edit, plus a mechanism for the compiler to invalidate that metadata. They also create a wedge for re-generation: a model version change would silently rewrite the author's `generated const` bodies the next time Repair runs.

## Decision

**Repair idempotence is anchored on name resolution, not on content.** The rule is exactly: "does this name resolve to something?" If yes, do not regenerate. Repair never inspects the generated body to decide whether it is "still good" — it only asks whether the name has a declaration.

This applies uniformly to:

- Names the author hand-wrote as `const`, `block`, `export block`, or imports.
- Names Repair previously materialized as `generated const` or `generated block`.
- Names provided by parameters or local bindings.

The `generated` keyword is a source-level marker for the author and for the no-shadowing rule (an author-written declaration with the same name supersedes and deletes its `generated` counterpart). It is **not** consulted by the idempotence check.

## Consequences

**Positive.**

- No compiler-managed metadata in source. The author can read a `generated` declaration as ordinary source code and edit its body without triggering regeneration.
- Authors may edit a `generated block` body freely; Repair sees the name resolves and skips it. The declaration stays `generated` until the author chooses to promote it by deleting the keyword.
- Model version drift cannot silently rewrite previously-generated bodies. A new compiler / model release does not re-derive existing declarations — they exist, they resolve, Repair leaves them alone.
- The contract is simple to explain: "Repair fills in undefined names. If a name is defined, it's defined."

**Negative.**

- If the language rules change in a way that would make an existing `generated const` invalid (e.g. a string form is disallowed), Repair will not detect this — the standard validation chain (Phases 2, 4, 5) will. The error appears as an ordinary diagnostic on the existing declaration, and the author manually deletes or rewrites it.
- There is no automated migration path for "regenerate everything against the new model version." The author must explicitly delete the declaration to opt into regeneration. This is treated as a feature, not a defect: the author retains ownership of their `generated` declarations and decides when to refresh.
- Two different unresolved use sites that would produce the same `generated` name hard-fail with `G::analyze::name-collision`. The LLM cannot infer which definition the author intended, so the safest rule is to require manual disambiguation. Idempotence by name forces this choice on the author rather than picking silently.

## Alternatives Considered

- **Content fingerprinting.** Rejected: adds compiler-managed metadata in source, creates a wedge for silent regeneration, and complicates the "edit the body" promotion path.
- **Per-declaration model-version tags.** Rejected: makes generated declarations brittle to model upgrades, and the migration semantics (auto-rewrite vs. flag-only vs. ignore) become a separate policy debate.
- **Per-call regeneration (re-derive on every compile, only write back if different).** Rejected: every compile becomes non-deterministic across machines and pays an LLM cost. The "commit post-repair source" workflow loses meaning.

## See Also

- [[design/repair]] §5 (idempotence as a contract) and §7 (no-shadowing).
- [[docs/architecture/repair]] §3.2 (Phase 3b).
- [[0014-deterministic-compiler-with-bounded-llm-repair]].
