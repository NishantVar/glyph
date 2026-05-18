# IR Semantics — Architecture

Maintainer-facing companion to [[ir-and-semantics]]. The design file
states what an author writes and what each construct means. This file documents
the compiler-internal mechanics: how roles are computed, how markers are
hoisted, how the inference / validation algorithms work, and what cross-phase
invariants must be preserved.

For the IR data shape and JSON encoding, see [[ir-schema]]
and [[ir-json]]. For the role/effect/section *semantics* an
author needs, see [[ir-and-semantics]].

## 1. Role Computation Internals

Some forms have fixed roles assigned by grammar — no inference step is involved:

- Bare calls inside `flow:` are definitionally `Step`.
- A bare string as a block body shorthand (omitting `flow:`, per
  [[language-surface]] §3.2) is definitionally `Step`.
- Content inside `description:` carries no instruction role — it is context
  metadata.

For genuinely ambiguous forms, the compiler applies the following evidence
order during role / strength / polarity inference:

1. Explicit marker in source.
2. Metadata from same-file `const` or block declarations.
3. Metadata from imported or standard-library declarations.
4. Position and structure (e.g., inside `flow:` or as a bare block-body string
   using the single-string shorthand implies `Step`; inside `description:` is
   context metadata and carries no instruction role).
5. Compound-name cues (`avoid_*`, `must_*`, `never_*`, `must_never_*`) — used
   as evidence for role/polarity inference; no forced splitting.
6. LLM repair-generated definitions.
7. Diagnostic if role, strength, or polarity remains ambiguous.

`require` and `avoid` may be inferred during repair when evidence is clear.
`must` should be inferred conservatively — only when the source already
carries hard-strength intent (trusted metadata, strong wording like `must_*`,
`never_*`, `must_never_*`). A plain `avoid_*` cue repairs to soft avoidance,
not hard. `must` stays rare; it is not just a more emphatic `require`.

### Projection Mechanics

Projection from IR to compiled Markdown is target-specific. MVP projection
produces YAML frontmatter, a conditional `## Parameters` section, and
peer-level body H2s — `## Context`, `## Steps`, `## Constraints` — sitting
at the same level as `## Parameters`; no `## Instructions` wrapper is
emitted (see [[docs/reference/compiled-output]]):

- `Step` → numbered list items under `## Steps`. Parameters carried by the
  step appear as `{param}` references in the compiled prose, resolved by the
  consuming LLM at runtime.
- `Context` → bulleted items under `## Context`, before `## Steps`. Passive
  informational text — no strength/polarity wording.
- `Constraint` → bulleted items under `## Constraints`. Strength
  (`soft`/`hard`) and polarity (`require`/`avoid`) influence wording and
  prominence.
- `InputContract` → projected into the `## Parameters` section of compiled
  output (names, descriptions, and either a default value or a `(required)`
  marker per parameter; see [[docs/reference/compiled-output]] §`## Parameters`). Parameters
  appear as `{param}` references in Step and Constraint prose, resolved by the
  consuming LLM at runtime.
- `OutputContract` → folded into the final `Step`. The `return` expression
  becomes the closing sentence of the last numbered step. No dedicated
  compiled section in MVP.
- Effects → YAML frontmatter `effects` list, not a prose section.

**Invariant.** The IR preserves role, strength (`soft`/`hard`), polarity
(`require`/`avoid`), and the full `InputContract` / `OutputContract`
structure even though MVP compiled output does not project them as separate
sections.

## 2. Constraint Hoisting And Branch Scoping

### Body-Level Constraint Normalization

Body-level markers stay where the author wrote them in source; the compiler's
Lower pass (Phase 4) synthesizes a `## Constraints` section at canonical slot
3 by hoisting body-level constraint AST nodes into the declaration's
`constraints` list at IR level — concretely, `IrSkill.constraints` for a
`skill` and `IrExportBlock.constraints` for an `export block` (the two
declaration kinds that emit peer-level H2 sections). `glyph fmt` preserves
source order and marker position — it does not rewrite markers into a
`constraints:` sub-section. Authors may write either form; both produce
identical IR.

**Block projection note.** Private `block` declarations have no peer-level
H2 sections (only the enclosing skill emits H2s). Body-level constraint
markers on a private block therefore land in `IrBlock.constraints` rather
than being hoisted into any H2 list. At emit time, when the block is
promoted to Tier 2 (its presence alone is one of the Tier 2 triggers — see
[[docs/reference/compiled-output]] §Three-Tier Block Projection), those
constraints render as the procedure preamble described in
[[docs/reference/compiled-output]] §Procedure Preamble (Tier 2 and Tier 3) —
**not** as a `## Constraints` H2 inside the block. For `export block`
declarations, body-level constraints continue to hoist into
`IrExportBlock.constraints` for the standalone procedure file's
`## Constraints` H2, **and** the same `body_constraints` AST list is also
read by the Tier 3 emitter to render the same procedure preamble in the
standalone `.md` (per the byte-identical Tier 2 / Tier 3 contract in ADR
0025).

### Flow-Level Constraint Markers

Constraint markers (`require`/`avoid`/`must`/`must avoid`) are legal as flow
statements inside `flow:`, including inside `if`/`elif`/`else` branch bodies.
The IR represents them as `Constraint` nodes admissible in the `FlowNode`
union ([[ir-schema]] §Flow Nodes). `Context` markers (via the `context`
keyword) are similarly admissible as flow nodes, following the same
hoisting/branch-scoping rules. Lower (Phase 4) splits them by location:

- **Flow top-level** — a constraint marker at the top level of `flow:` (not
  inside a branch) is **hoisted** out of the flow and appended to the
  enclosing declaration's `constraints` list, deduplicated against existing
  entries by canonical text + polarity + strength. Phase 4 (Lower) performs
  this hoisting at IR level; `glyph fmt` does not rewrite markers in source.
  After hoisting it renders in `## Constraints` like any other top-level
  constraint.
- **Branch-scoped** — a constraint marker inside an `if`/`elif`/`else` branch
  body **stays inline** in that branch. Expand renders it as part of the
  conditional Step prose so the consuming LLM sees that the constraint applies
  only when that branch is taken (e.g., "If the change touches public APIs, do
  not break backwards compatibility."). It does not appear in
  `## Constraints`. See [[docs/reference/compiled-output]] §Constraint Rendering.

By the time Lower completes, all unconditional constraints — whether
originally in a `constraints:` section, at body level, or at flow top-level —
reside in the declaration's `constraints` list. Lower's IR-level hoisting is
the single mechanism that produces this result; `glyph fmt` preserves source
as written. Branch-scoped markers are the only constraints that remain inside
the flow.

### Body-Level And Flow-Level Context Markers

Body-level `context` markers stay where the author wrote them in source; the
compiler's Lower pass (Phase 4) synthesizes a `## Context` section at
canonical slot 4 by hoisting body-level `context` AST nodes into the
declaration's `context` list at IR level — concretely, `IrSkill.context`
for a `skill` and `IrExportBlock.context` for an `export block`. `glyph fmt`
preserves source order and marker position — it does not rewrite markers
into a `context:` sub-section.

**Block projection note.** Private `block` declarations have no peer-level
H2 sections, so body-level `context` markers on a private block land in
`IrBlock.context` rather than being hoisted into any H2 list. At emit time,
when the block is promoted to Tier 2 (the presence of `IrBlock.context`
entries is itself a Tier 2 trigger — see
[[docs/reference/compiled-output]] §Three-Tier Block Projection), those
entries render as part of the procedure preamble described in
[[docs/reference/compiled-output]] §Procedure Preamble (Tier 2 and Tier 3),
using the locked label forms (`**<kebab-name>:** <text>` for name-ref
operands; `**Context:** <text>` for inline-string operands) defined in
[[0025-context-preamble-format]]. For `export block` declarations,
body-level `context` markers continue to hoist into `IrExportBlock.context`
for the standalone procedure file's `## Context` H2, **and** the same
`body_context` AST list is also read by the Tier 3 emitter to render the
same procedure preamble in the standalone `.md`.

`context` markers are also legal as flow statements inside `flow:`. The IR
represents them as `Context` nodes admissible in the `FlowNode` union
(alongside `Constraint` nodes — see [[ir-schema]] §Flow Nodes). Lower (Phase
4) splits them by location:

- **Flow top-level** — a `context` marker at the top level of `flow:` (not
  inside a branch) is **hoisted** out of the flow and appended to the
  enclosing declaration's `context` list, deduplicated against existing
  entries by canonical text. Phase 4 (Lower) performs this hoisting at IR
  level; `glyph fmt` does not rewrite markers in source. After hoisting it
  renders in `## Context` like any other top-level context entry.
- **Branch-scoped** — a `context` marker inside an `if`/`elif`/`else` branch
  body **stays inline** in that branch. Expand renders it as part of the
  conditional Step prose so the consuming LLM sees that the context applies
  only when that branch is taken. It does not appear in `## Context`.

By the time Lower completes, all unconditional context — whether originally
in a `context:` section, at body level, or at flow top-level — resides in the
declaration's `context` list. Branch-scoped markers are the only context
entries that remain inside the flow.

## 3. Effect Inference And Validation Algorithms

### Propagation Rules

The compiler infers effects by walking the call graph using a
**transitive-eager, single-compilation-unit** algorithm. Three propagation
rules cover every call, applied unconditionally to every reachable callee —
including calls inside `if`/`elif`/`else` branch arms and calls modified by
`with`. There is no per-arm reachability analysis; every reachable call
contributes.

- **Stdlib-direct.** A call to a standard-library entry contributes that
  entry's documented effects (see [[stdlib]]).
- **Local-transitive.** A call to a same-file `block` contributes the
  callee's **inferred** effect set, computed transitively through that
  callee's own call graph. Same-file `export block` calls follow this rule
  too — locally we have full visibility.
- **Import-by-declaration.** A call to a callee imported from another file
  contributes the imported `export block`'s **declared** effect set (the
  import contract). The importer never re-derives the imported callee's
  inferred set; it trusts the dependency's declaration as validated by that
  file's own Validate pass (per [[compiler-pipeline]] §Multi-File Compilation Order
  and [[data-flow]] §Effect Propagation).

A block's inferred effect set is the **union** of its own direct effects and
the contributions from every reachable call. Skills, exported blocks, and
private blocks all participate in inference. There is no effect subtraction
or masking in the MVP. Effect sets are unordered; the compiler may sort them
alphabetically or by declaration order.

### Projection Tier And Effect Propagation

Projection tier (inline, same-file procedure, external file — see
[[docs/reference/compiled-output]] §Three-Tier Block Projection) is a Phase 6 output-layout
decision. Effect propagation is resolved in Phases 2 and 5, before projection
tiers are assigned. Therefore, **projection tier does not affect effect
semantics**: a callee's effect set propagates identically regardless of which
tier the compiler later selects for compiled output.

The one addendum: when the compiler selects Tier 3 (external file) for an
imported block in Phase 6 Step 1, the compiled output directs the consuming
agent to load an external file — a runtime `reads_files` action. If the
skill's effect set (resolved in Phases 2/5) does not already include
`reads_files`, Phase 6 Step 1 emits an error requiring the author to add it.
This is a **post-Phase-5 validation check** specific to Tier 3 selection, not
a propagation-time contribution. In practice, most skills that call imported
blocks already carry `reads_files` from the callee's own declared effects;
the check catches the rare case where the callee has no file-reading effects
but the tier selection introduces one. See [[docs/reference/compiled-output]] §External
Procedure Files.

### Author Declaration And Validation

**Infer-when-omitted, validate-when-declared.** The compiler computes an
**inferred** effect set by walking the call graph and unioning every callee's
effect contribution (user-defined blocks per their declared `effects:`, and
stdlib calls per their synthetic-body projection — see [[stdlib]]
§Projection Model: Uniform Synthetic Body and §Propagation). How the
compiler uses that inferred set depends on whether the author wrote an
`effects:` line:

- **Omitted entirely.** The author did not write `effects:` at all. Phase 2
  (Analyze) emits `G::analyze::missing-effects` (repairable). Phase 3a
  (deterministic repair) auto-adds an `effects:` sub-section with the
  inferred set and emits `G::repair::inferred-effects` (warning,
  informational) so the author knows what was added. This applies uniformly
  to skills, blocks, and export blocks — there is no declaration-type
  asymmetry. If the inferred set is empty, no `effects:` line is added and
  no diagnostic fires (the declaration genuinely has no effects).
- **Declared by the author.** The compiler validates that the **declared set
  is a superset of the inferred set**. If the declared set is smaller than
  inferred, that is a compile error (`G::analyze::effects-under-declared`) —
  the declaration is lying about what the block does. Writing
  `effects: none` explicitly when the inferred set is non-empty is the same
  error: `none` is a strict subset claim that contradicts inference. This is
  not repairable — the author made a deliberate declaration and the compiler
  will not silently overwrite it.

If the declared set is **larger** than inferred (e.g.
`effects: reads_files, runs_commands` when only `reads_files` is inferred),
the compiler emits a `warning`-tier diagnostic
(`G::analyze::effects-over-declared`). Compilation proceeds. Over-declaration
is legitimate (forward-compat, intentional widening of a public contract),
so it is not an error; the warning lets the author remove the extra keyword
if they are confident it is no longer needed. Repair never narrows a
declared effect set, since that would silently break import contracts.

**Across imports and inlines.** A caller's declared `effects:` must
additionally be a superset of every imported callee's declared effects and
every inlined private callee's inferred effects. This is enforced in Phase 5
(Validate) — see [[compiler-pipeline]] and [[data-flow]] §Effect Propagation.
Effects propagate by *declaration*, not by transitive analysis: the importer
sees only the imported callee's declared contract, so the callee's own
Validate pass must have already produced a complete declared set.

Import contracts are satisfied through the compiler's output: the IR and
compiled Markdown always contain the full inferred effect set regardless of
whether the author wrote `effects:`.

## 4. Freeform Section IR Plumbing

Freeform colon-keyword sections lower to dedicated IR kinds defined in
[[ir-schema]] §Freeform sections; see also the design specification §4.1.4a
for the canonical content-item shape. This section documents the **Expand
Step 1 algorithm** that plumbs freeform sections through the IR — the
schema-side shape (container/content fields, marker-clause fields, host-list
mechanic) lives in [[ir-schema]].

**Population order.** Expand Step 1 visits each compilation unit (`Skill`,
`Block`, `ExportBlock`) in source order. For every freeform section attached
to a unit, Step 1:

1. Resolves each item's text — inline string literals pass through verbatim,
   bare-name references inline the resolved `const` / `export const` body,
   and marker clauses resolve their operand the same way `constraints:` /
   `context:` operands resolve.
2. Derives `strength` / `polarity` from `marker_word` for `require` / `avoid`
   / `must` / `must avoid` markers; leaves both `None` for plain
   string-literal / name-ref items and for the `context` marker.
3. Appends the resolved `FreeformContent` to the section's `items` list in
   source order.
4. Appends the populated `FreeformSection` to the host decl's
   `freeform_sections` list, preserving the source-declaration order of
   sections on that host.

**Hoisting-scope rule.** Authors may use the same `require` / `avoid` /
`must` / `must avoid` / `context` marker keywords inside a freeform section
as in `constraints:` / `context:`. Markers inside a freeform section do
**not** hoist into the enclosing decl's `constraints` / `context` lists —
they stay scoped to their section so the emitter renders the section as
authored under its own `## Heading`. The `marker_word` + `strength` +
`polarity` fields on `FreeformContent` preserve marker semantics within the
section so emit can still produce strength / polarity badges or context
lead-ins per the freeform-section design. The hoisting rule described in §2
therefore applies only to constraints/context entries written *outside* a
freeform section.

## 5. Predicate Key Resolution In Expand Step 1

Branch conditions in `if` / `elif` arms may use predicate forms — see
[[ir-and-semantics]] §Predicates for the three syntactic variants
and [[ir-schema]] §Resolved IR for the `resolved_predicates` side-map
shape. Expand Step 1 populates the side-map by walking each Branch's
condition string and, for every predicate token it identifies, adding one
entry to the Branch's `resolved_predicates` map keyed by the predicate
token's spelling in the condition string:

- **Block trigger predicate (`.applies()` form).** The token
  `block_name.applies()` (or `module_alias.block_name.applies()` for a
  qualified callee) is keyed as `"block_name.applies()"` in the map. The
  value is the resolved `description:` string of the referenced
  `block` / `export block` — read from the same-file declaration or, for
  imports, from the declared description on the imported export block.
- **String-const predicate.** A bare identifier resolving to a
  string-kinded `const` / `export const` is keyed as `"const_name"`. The
  value is the resolved string body of that constant.
- **Inline literal predicate.** A quoted string literal in condition
  position is **not** stored in `resolved_predicates`; the literal already
  sits in the condition string and Step 2 reads it directly there.

If no condition arm in the Branch uses a predicate form,
`resolved_predicates` stays `null`. Step 2 reads both the verbatim
condition string and the side-map to render predicate-driven prose (see
[[design/compiled-output]] §Predicate-Driven Branch Projection).
