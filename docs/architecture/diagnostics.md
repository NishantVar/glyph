# Diagnostics — Architecture & Rationale

This document captures the durable rationale behind Glyph's diagnostic system. The user-facing contract (shape, classification tiers, full ID catalog) is in [[docs/reference/diagnostics]].

## Why a Three-Tier Classification

Glyph's compiler distinguishes `error`, `repairable`, and `warning` because each tier defines a different *handling protocol*, not just a severity level:

- `error` is the terminal signal: deterministic passes know they cannot make progress and that the LLM repair pass is not allowed to try. This guards against unbounded repair attempts on fundamentally malformed input (e.g., contradictory effect declarations, structural impossibilities like `flow:` inside `flow:`).
- `repairable` is the handoff signal between deterministic analysis and the bounded LLM repair loop. It carries the implicit claim: "this failure has a plausible mechanical fix, either a text rewrite or a generated definition." The repair pass is fed exactly the `repairable` (and surrounding `warning`) records.
- `warning` is the non-blocking observation tier. It exists so the compiler can surface useful information (e.g., `generated-const` notifications, over-declared effects, inconsistent type spellings) without halting a build the author already considers correct.

A binary `error`/`warning` model would collapse repair-eligible failures into either "fatal" (defeats the repair pass) or "ignored" (defeats correctness). The three-tier split is the smallest model that lets deterministic phases hand off cleanly to the LLM repair pass while still expressing non-fatal observations.

### Why classification is fixed per ID

Each diagnostic ID maps to exactly one classification, never context-sensitive. This is a deliberate constraint: it means tooling, IDE integration, and downstream agents can build static lookup tables (ID → handling protocol) without re-running the compiler. A diagnostic that "is sometimes repairable" would force every consumer to re-derive that judgment, undoing the contract.

The one apparent exception — `G::analyze::applies-on-undescribed-block` documented as `repairable / error` — is actually two cases with different IDs collapsed into the table for narrative clarity: same-file blocks are repairable (Repair can edit the source); imported blocks are not (Repair is single-file). Future cleanup may split this into two IDs.

## Why the `G::<phase>::<name>` ID Scheme

### Stability is a public commitment

Once an ID is published, it never changes meaning. Tools key off IDs to drive behavior — IDE quick-fixes, agent retry policies, CI rules. Renaming or repurposing an ID silently breaks every consumer. The "deprecate and replace" rule (old ID kept with old meaning; new ID added for new meaning) is the only safe evolution path.

### Phase namespace is for human readability, not routing

The `G::<phase>::*` namespacing is not a runtime routing key — the compiler does not pattern-match on the prefix. It exists because diagnostic IDs are read by humans (authors, maintainers, agent prompt authors) far more often than they are mechanically parsed. Knowing that `G::analyze::*` comes from Analyze gives a reader an immediate mental anchor for what phase failed and what fix surface to look at.

The `G::imports::*` namespace is logically a subset of Analyze but kept distinct because import resolution is a self-contained problem (paths, cycles, exports) separable from name/role/effect analysis.

### The `::` separator is a deliberate choice

`::` was chosen because it avoids collision with:

- `.` — module/field access in Glyph source
- `/` — file paths (which appear in spans and import paths)
- `-` — used inside the `<name>` portion (`G::parse::tab-indent`)

A diagnostic ID that included `.` or `/` would be harder to safely embed in tool output, log lines, or grep patterns alongside actual code references. `::` is unambiguous in every context the IDs appear.

### Why `G::` prefix at all

The `G::` (for Glyph) prefix exists for the case where Glyph diagnostics are aggregated alongside diagnostics from other tools — language servers, linters, downstream compilers. Without a vendor prefix, `parse::tab-indent` is ambiguous across toolchains. `G::parse::tab-indent` is not.

## The Repair vs Error Boundary

The line between `repairable` and `error` is not just "could a human fix it." Every error is technically fixable by editing source. The boundary is about *what the repair pass is allowed to attempt without losing the author's intent*:

- `repairable` is reserved for failures where the fix is either purely mechanical (Repair's deterministic strata — duplicate-subsection merge, none-as-return-type strip, inferred-effects insertion) or where the author's intent is locally inferable from the surrounding source (undefined names → generate definition, missing-description → generate description from name + body, placeholder-string-return → bifurcate into output-target forms).
- `error` is reserved for failures where any fix would require *guessing across the author's contract boundary*. `typed-decl-missing-return` is the canonical example: the header declares `-> SomeType`, the body has no `return`, and synthesizing a return value would invent semantics the author did not write. `name-collision`, `circular-import`, `recursive-call`, and `closure-violation` follow the same pattern — fixing them requires structural decisions only the author can make.

This boundary is what keeps the LLM repair pass safe to run unsupervised. Repair never touches `error` diagnostics, so it can never silently overwrite an authored contract.

### Why some "detectable in Parse" issues are still errors, not repairable

`G::parse::none-with-effects` is detected during parsing (the token sequence `effects: none, reads_files` is unambiguous), but classified `error` rather than `repairable`. The reason: `none` exclusivity is a hard semantic rule (`none` means "no effects"), so mixing it with other keywords is a contradiction at the author's intent layer, not a syntactic slip. Repairing it would require choosing which keyword to drop — that's a contract decision.

Contrast with `G::parse::tab-indent`: tabs are illegal, but the fix is mechanical (replace tabs with spaces at the same indent depth) and the author's intent is unambiguous. Mechanical fix, intent preserved → `repairable`.

The general principle: parse-detectable doesn't imply parse-repairable. Detection happens where the token sequence is unambiguous; repairability depends on whether the fix preserves authored intent.

## The Synthetic-Diagnostic Fallback

Some diagnostics arise from phases that operate post-Lower or post-Repair, where the precise authored location may not survive into the IR. The reference doc lists the three-step fallback (authored construct → enclosing declaration → file root); the rationale is:

- **Never emit a diagnostic without a span.** A diagnostic without `file:` cannot be surfaced in IDE or CLI output usefully.
- **Prefer narrow over wide.** A span pointing at the exact parameter that lowered into the offending node is more actionable than a span over the whole declaration, even if both correctly localize the failure.
- **The file-root fallback is reserved.** It is intended for diagnostics whose provenance is the file as a whole (e.g., `G::analyze::no-exports-in-library` — there is no specific authored location for "this file has no exports").

The Lower/Repair phase invariants are designed to guarantee enough provenance survives to reach the first or second fallback option for every diagnostic emitted in the MVP. Hitting the file-root fallback for any other diagnostic is a bug to fix in the provenance pipeline, not an acceptable outcome.

## Catalog Completeness Rule (Rationale)

The reference doc states the rule ("every check that can fail MUST have exactly one diagnostic ID"). The rationale:

- **No silent failures.** A check that emits a bare exception or unstructured string is invisible to downstream tooling. The repair pass cannot consume it; an IDE cannot localize it; an agent cannot retry it. The structured-diagnostic requirement is what makes the compiler safely automatable.
- **No diagnostic explosion.** "Exactly one ID per check" prevents the catalog from growing by accumulating near-duplicate IDs for variations of the same failure. When multiple shapes of the same failure exist (e.g., `malformed-output-target` covering both identifier-form and descriptive-form failures), they share an ID and disambiguate via the `message:` field.

## Internal Phase Distinctions Are Not Part of the Contract

The Repair pass internally distinguishes deterministic auto-fixes (text-level rewrites that need no LLM) from LLM-assisted fixes (semantic inference). That distinction matters for implementation strategy and operator cost, but it is intentionally *not* exposed through the diagnostic classification. From the author's and tooling's perspective, all `repairable` diagnostics are handled the same way: the compiler will attempt repair, and after repair the diagnostic either disappears (success) or remains (failure → loop exits with `G::repair::no-convergence`).

Exposing the internal 3a/3b/3c distinction in the diagnostic shape would couple the public contract to the repair-pass implementation. If a future repair strategy collapses 3a and 3b, or adds a 3d stratum, the public IDs must remain unchanged.

## By-Construction Satisfaction (Defense in Depth)

Many Phase 6b structural diagnostics are listed as "by construction" satisfied — meaning the deterministic emitter scaffold cannot produce output that would trip them, because the scaffold owns the section structure and list cardinality. So why keep them in the catalog?

- **Defense in depth.** The diagnostics remain enforced for hand-written or regenerated output (e.g., a retry that reads and rewrites `foo.md` directly rather than re-filling scaffolded spans).
- **Future-proofing.** Any future migration where the LLM is given a wider surface than scaffolded spans (e.g., to support richer freeform sections) inherits these checks automatically. Removing them would force re-implementation when the wider surface lands.
- **Compiler audit.** They remain a check against bugs in the deterministic emitter itself — if a scaffold bug ever produces a structural mismatch, the diagnostic surfaces it instead of letting bad output ship.

The by-construction set is documented so reviewers can understand which diagnostics are dead-code-for-now versus actively triggered, without removing them from the spec.

## Cross-References

- Compiler pipeline architecture: [[compiler-pipeline]] (when present)
- Repair pass architecture: [[docs/architecture/repair]] (when present)
- Diagnostic catalog: [[docs/reference/diagnostics]]
