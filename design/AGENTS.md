# Glyph — Design

This folder is the working source of truth for Glyph system design — flat because this **is** the main design (not a per-topic subfolder). The repository [README](../README.md) describes the project at a high level; design decisions are captured here.

## Document Roles

### Foundations (strongest statements)
- [principles.md](principles.md) — stable design principles that shape all downstream decisions
- [boundaries.md](boundaries.md) — hard conceptual boundaries that keep the system from drifting

### Language Shape (active decisions)
- [authoring-surface.md](authoring-surface.md) — human-facing `.glyph.md` source forms, MVP top-level declarations, one-file-to-one same-basename `.md` output, exported text/blocks, path-based imports, semantic shortcuts, inline instructions, and duck-typed authoring
- [data-flow-and-calls.md](data-flow-and-calls.md) — how skills, private blocks, exported blocks, calls, parameters, local bindings, returns, duck typing, closure, and effects pass values through Glyph
- [effects.md](effects.md) — MVP effect vocabulary (8 keywords), `effects:` clause syntax, inference and propagation semantics, `none` handling, compiled-output surfacing, and extension policy
- [ir-roles.md](ir-roles.md) — MVP instruction role taxonomy: `InputContract`, `Step`, `Constraint`, `Context`, `OutputContract`, with constraint strength and polarity attributes
- [values-and-literals.md](values-and-literals.md) — MVP primitive values: strings (inline and block, no interpolation), numbers (int and float with lossless coercion), booleans, `none`, identifier rules (case-normalized, no shadowing), and name resolution
- [declaration-headers.md](declaration-headers.md) — MVP declaration header syntax: exact keyword order, parameter lists, return type markers, and terminators for `skill`, `block`, `export block`, `text`, `export text`, `int`, `export int`, `float`, `export float`, and `import`
- [block-structure.md](block-structure.md) — MVP block structure: Python-style significant indentation, 4-space indent unit, colon-terminated sub-section headers, blank line semantics, line continuation, no Markdown passthrough in source
- [types.md](types.md) — MVP type vocabulary: primitive type names (`String`, `Int`, `Float`, `Bool`, `None`), named domain types as opaque semantic labels, nominal matching at call boundaries, no structural type definitions or checking in MVP
- [calls-and-args.md](calls-and-args.md) — MVP call-site syntax: positional-then-named arguments, comma-separated inside parens, default skipping via named args, trailing commas, nested calls with IR desugaring, bare and qualified callee resolution, and IR call-node normalization
- [section-vocabulary.md](section-vocabulary.md) — MVP sub-section headers inside declaration bodies: `effects:`, `constraints:`, `inputs:`, `outputs:`, `flow:`, `when_to_use:`, with spelling, mandatory/optional rules, source ordering, body-level constraint normalization, and source-to-compiled-output mapping
- [import-resolution.md](import-resolution.md) — MVP import resolution semantics: relative path resolution with extension auto-resolution, importable declaration catalog (`export block`, `export text`, `skill` via whole-module only), no-shadowing collision rules for selective and whole-module imports, no re-export in MVP, cycle rejection, duplicate-import auto-merge, unused-import auto-removal, effect propagation via union across import boundaries, and closure enforcement at export time
- [control-flow.md](control-flow.md) — MVP control-flow body syntax: statement forms inside `flow:` (bindings, bare calls, bare names, inline strings, `return`, `if`/`elif`/`else`), condition expression vocabulary (`not`, `and`, `or`, `==`, `!=`, dot access), return placement rules, no nested `flow:`, indentation interaction, and IR mapping
- [comments.md](comments.md) — MVP comment syntax: `//` line comments, no block comments, Markdown-safe, stripped from compiled output, preserved by repair
- [compiled-output.md](compiled-output.md) — compiled `.md` file shape: YAML frontmatter (`name`, `description`), fixed section order (Effects, Inputs, Instructions, Output, When To Use), H3 sub-sections for Steps/Constraints, formatting rules, import inlining, and authoring-construct erasure
- [generated-definitions.md](generated-definitions.md) — MVP `generated text` declaration: repair-materialized definitions for undefined bare names, `text`-identical grammar with `generated` prefix, placement ordering, idempotence via name resolution, no-shadowing auto-deletion, compile-time inlining, and promotion paths
- [preferences.md](preferences.md) — MVP preferences model: prefs are ordinary `export text`/`export int`/`export float` declarations imported like any other name; no `pref(...)` call, no `reads_prefs` effect, compile-time resolution, standard prefs library shipped with compiler (import scheme TBD)
- [llm-repair-pass.md](llm-repair-pass.md) — source-to-source LLM repair for invalid Glyph files before deterministic IR compilation
- [specialization-and-inheritance.md](specialization-and-inheritance.md) — post-MVP compile-time specialization for expert agents using `agent`, `abstract agent`, slots, overrides, appends, prepends, locked constraints, and optional traits

## How To Read These Docs

- Start with `principles.md` and `boundaries.md`.
- Then read `authoring-surface.md` for the current source-language direction.
- Read `data-flow-and-calls.md` for value passing, function-like calls, and explicit data flow.
- Read `effects.md` for the MVP effect vocabulary, syntax, inference semantics, and extension policy.
- Read `ir-roles.md` for the closed MVP instruction role set and constraint strength/polarity model.
- Read `values-and-literals.md` for primitive values, identifier rules, and name resolution.
- Read `declaration-headers.md` for the exact header-line grammar of each MVP top-level declaration, parameter syntax, return type markers, and import forms.
- Read `block-structure.md` for significant indentation rules, sub-section header syntax, blank line and line continuation semantics, and the no-Markdown-passthrough decision.
- Read `types.md` for the MVP type vocabulary: primitive type names, named domain types as opaque labels, nominal matching, and deferred structural features.
- Read `calls-and-args.md` for call-site argument syntax: positional-then-named rules, default skipping, nested calls, callee resolution, and IR call-node normalization.
- Read `section-vocabulary.md` for the canonical sub-section headers inside declaration bodies, their spelling, mandatory/optional rules, recommended source ordering, body-level constraint normalization, and source-to-compiled-output mapping.
- Read `import-resolution.md` for MVP import resolution semantics: path resolution, importable declarations, collision rules, re-export policy, cycle handling, duplicate and unused import auto-fixes, effect propagation, and closure enforcement timing.
- Read `control-flow.md` for the MVP control-flow body syntax: statement forms inside `flow:`, `if`/`elif`/`else` branching, condition expressions, dot access, `return` rules, and IR mapping.
- Read `compiled-output.md` for the compiled `.md` file shape, section ordering, formatting rules, and how authoring constructs are erased.
- Read `generated-definitions.md` for the `generated text` declaration form: repair-only authorship, placement ordering, idempotence, no-shadowing auto-deletion, promotion paths, and compile-time inlining identical to `text`.
- Read `preferences.md` for how Glyph models user/project preferences as ordinary exported constants with no dedicated call form or effect.
- Read `llm-repair-pass.md` for how invalid but readable source is repaired before compilation.
- Read `specialization-and-inheritance.md` for post-MVP inheritance-like reuse through explicit specialization rather than general class inheritance.
- The rest of the spec will be rebuilt from scratch around these decisions.

## Current Posture

- `principles.md` and `boundaries.md` are the strongest statements in the folder.
- `authoring-surface.md` captures the first concrete language-shape decisions for the rebuilt spec, including `.glyph.md` source modules, one-file-to-one same-basename `.md` output, path-based imports, exported text/blocks, and the MVP base declaration kinds (`import`, value-binding `text`/`int`/`float`, `block`, `skill`) with `export` as a visibility modifier on value-binding and block kinds and `generated` as a repair-authorship modifier on `text`.
- `data-flow-and-calls.md` captures the first concrete contract for parameters, calls, local variables, return values, exported-block closure, and effects.
- `effects.md` captures the MVP effect vocabulary (8 keywords), `effects:` clause syntax, compiler inference and propagation, and additive extension policy.
- `ir-roles.md` captures the closed MVP role set for instruction intent, keeps effects separate from roles, and defines constraint strength/polarity plus conservative repair inference for `always`.
- `values-and-literals.md` captures the MVP primitive value surface: strings, numbers, booleans, `none`, identifier normalization, no-shadowing rule, and name resolution.
- `declaration-headers.md` captures the MVP declaration header syntax: no trailing colon on headers, parentheses always required on callable declarations (`skill`, `block`, `export block`), `-> ReturnType` for returns (mandatory on `export block`), value-binding declarations (`text`/`int`/`float` and their `export` variants) use `= <literal>`, whole-module imports require `as <alias>`, selective imports use `{ name, name as alias }`, and parameter syntax reserves the `name: Type = default` slots.
- `block-structure.md` captures the MVP block structure decisions: Python-style significant indentation with 4-space indent unit, colon-terminated sub-section headers (`flow:`, `effects:`, etc.) with inline and long-form variants, blank lines as non-structural separators, implicit line continuation inside paired delimiters, and no Markdown passthrough in source files.
- `types.md` captures the MVP type vocabulary: five primitive types (`String`, `Int`, `Float`, `Bool`, `None`) in PascalCase convention, named domain types as opaque semantic labels with nominal matching at call boundaries, no structural type definitions or checking, and deferred features including structural contracts, collection types, and enum types.
- `calls-and-args.md` captures the MVP call-site syntax: positional-then-named arguments (no positional after named), comma-separated inside parens, default skipping via named args, trailing commas allowed, nested calls desugared to flat IR nodes, bare and single-level qualified callee resolution, and IR call-node normalization where all args become named.
- `section-vocabulary.md` captures the MVP sub-section header set (`effects:`, `constraints:`, `inputs:`, `outputs:`, `flow:`, `when_to_use:`), spelling conventions, mandatory/optional rules per declaration kind, recommended source ordering (effects-first), body-level constraint normalization into `constraints:` sections, no `context:` section (inline only), and the explicit mapping from source sections to compiled-output sections.
- `import-resolution.md` captures the MVP import resolution semantics: relative path resolution with `.glyph.md` extension auto-resolution, importable declaration catalog (`export block`, `export text`, `skill` via whole-module only), no-shadowing collision rules extending `values-and-literals.md` uniformly to imports, no re-export in MVP, circular import rejection with cycle diagnostics, duplicate-import auto-merge, unused-import auto-removal per name, effect propagation via set union across import boundaries, and closure enforcement at the exporter's compile time with importers trusting the export contract.
- `control-flow.md` captures the MVP control-flow body syntax: six statement forms inside `flow:` (bindings, bare calls, bare names, inline strings, `return`, `if`/`elif`/`else`), Python-style colon-terminated `if`/`elif`/`else` branching, condition expression vocabulary (`not`, `and`, `or`, `==`, `!=`, single-level dot access, boolean calls), `return` placement and early return, no nested `flow:`, indentation interaction with `block-structure.md`, IR mapping for all statement forms, and compiled-output projection rules for conditional logic.
- `compiled-output.md` captures the compiled `.md` file shape: YAML frontmatter (`name`, `description`), fixed section order (Effects → Inputs → Instructions → Output → When To Use), H3 sub-sections (`### Steps`, `### Constraints`), formatting rules, import inlining with source auto-fix for unused imports, and complete authoring-construct erasure.
- `generated-definitions.md` captures the MVP `generated text` declaration form: repair-only authorship (authors use `text`), `generated text <name> = <string-literal>` grammar identical to `text` with a `generated` prefix, ordering rule (all generated declarations after non-generated), idempotence via name resolution, no-shadowing auto-deletion when an author-written `text` exists, not exportable, compile-time inlining identical to `text`, and three promotion paths (edit body, promote to `text`, promote to `export text` in a library).
- `preferences.md` captures the MVP preferences model: preferences are ordinary `export text`/`export int`/`export float` declarations (no dedicated `pref` keyword, no `pref(...)` call form, no `reads_prefs` effect), imported and resolved like any other name, inlined at compile time, and overridden only by editing source in MVP. A standard prefs library shipped with the compiler is planned; import scheme and override mechanism are deferred.
- `llm-repair-pass.md` captures the source-preserving repair contract for compiler-blocking issues.
- `specialization-and-inheritance.md` captures a post-MVP reuse model for expert-agent variants.
- Earlier first-pass drafts (language core, semantics, types & effects, constraints, IR, compiler, output, validation, gap checklist) were removed and will be rebuilt.

## Relationship to Research

Design decisions build on top of the [agent-skill-dsl research](../research/agent-skill-dsl/). Consolidated wiki pages from research are promoted here as individual design files once they stabilise into decisions.
