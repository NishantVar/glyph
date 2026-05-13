# Glyph Compiler Pipeline

This document is the durable architecture reference for the Glyph compiler pipeline. It defines the seven phases, their ordering, the Safety Sandwich pattern that bounds LLM-assisted passes with deterministic checks, multi-file compilation order, partial-failure policy, and cacheability.

For author-visible behavior (what each construct means, what repair preserves, what compiled output looks like) see the corresponding `design/` files: [[ir-and-semantics]], [[design/repair]], [[design/compiled-output]]. For the IR node schema see [[ir-schema]]. For implementation mechanics (function names, internal state machines, retry constants, loop structure) see the code and tests.

## Overview

The Glyph compiler has **seven phases** in two LLM-bounded stages:

```
Source (.glyph)
  → 1. Parse           (deterministic)
  → 2. Analyze         (deterministic)
  → 3. Repair          [LLM, bounded loop]
  → 4. Lower           (deterministic)
  → 5. Validate        (deterministic)
  → 6. Expand          [deterministic + LLM]
  → 7. Emit            (deterministic)
Output (.md)
```

All seven phases operate once per source file. Phases 1-5 produce a validated IR; Phases 6-7 take the validated IR and produce the compiled Markdown. Compilation is **parameterless** — parameters appear in the compiled output as named slots that the consuming LLM resolves from context at runtime.

## Safety Sandwich

Every LLM-assisted phase is bounded by deterministic phases that check its work:

```
Deterministic [1. Parse + 2. Analyze]
  → LLM [3. Repair]
  → Deterministic [re-run 1+2, then 4. Lower + 5. Validate]
  → LLM [6. Expand]
  → Deterministic [7. Emit]
```

The invariant: **deterministic compiler passes own correctness; any LLM-assisted step runs inside those boundaries and is checked afterward.** Source is never compiled unless it has passed deterministic Validate; compiled output is never written unless it has passed deterministic Emit.

## Phase Invariants

Each phase owns a contract that later phases (and the LLM passes inside the sandwich) rely on. The contract is what must hold at the phase boundary, regardless of the implementation details inside the phase.

### Phase 1: Parse (deterministic)

**Input:** raw `.glyph` source text. **Output:** loose source AST per file + import dependency DAG across files.

Invariants:

- Parse never sees a partial file. Either it produces a structural AST for the whole file or it emits exactly one parse diagnostic and stops (bail at first parse error within a single file).
- The output AST is purely structural: names are unresolved, types are not checked, roles are not assigned.
- Multi-file: Parse reads every `import` path, builds the file-dependency DAG, and rejects cycles here (before any later phase runs).
- A pre-Parse text-rewrite stratum (tab → spaces, mixed-indentation fix) operates on the source text before any AST exists; those repairs are batched in a single pass and are exempt from the bail-at-first rule.

### Phase 2: Analyze (deterministic)

**Input:** loose source AST. **Output:** annotated AST with inferred metadata + structured diagnostics.

Invariants:

- Analyze is read-only. It does not modify source. It does not build the IR.
- Every name, role, type, and effect that can be determined deterministically is determined here.
- Where determinism fails, Analyze emits a structured diagnostic tagged with one of three classifications:
  - `error` — hard stop, cannot continue.
  - `repairable` — the LLM repair pass can likely fix this.
  - `warning` — non-blocking observation.
- The diagnostic contract is the only thing Phase 3 (Repair) reads. Repair never reads the raw AST directly.

### Phase 3: Repair (LLM + deterministic, bounded loop)

**Input:** original source + annotated AST + structured diagnostics from Phase 2. **Output:** repaired `.glyph` source written back to the file.

Invariants:

- Repair edits source, not IR. It is the **only** phase that writes back to `.glyph` files.
- Repair has two layers: a deterministic source-rewrite layer (always run, no LLM) and an LLM-assisted layer (runs only if repairable diagnostics remain after the deterministic layer).
- A separate constraint-conflict scan runs once per declaration after the main loop converges; it emits diagnostics but does not modify source.
- The loop is bounded — if repairable diagnostics still remain after the bound is exhausted, Repair **hard-fails**. No `.md` is emitted.
- Repair is per-file only. It does not edit dependencies or add new imports.
- Idempotence: running Repair on already-valid source produces zero changes. The mechanism is name resolution — if every name resolves and every role is determined, the diagnostic queue is empty and no layer runs.
- Repair preserves author intent. It may materialize generated definitions when intent is clear, but it does not add behavior the author did not imply.

The exact iteration bound, the prompt shape, the deterministic-rewrite list, and the conflict-scan algorithm live in code and in [[design/repair]].

### Phase 4: Lower (deterministic)

**Input:** valid, repaired source AST (zero errors, zero repairable diagnostics). **Output:** typed IR — the strict internal representation that all later phases operate on.

Invariants:

- Lower is a one-way source-to-IR transformation. It does not touch the source file.
- Every shortcut in the source AST is resolved into its explicit form in IR: UFCS desugared, positional arguments named, nested calls flattened, defaults filled, callees resolved.
- Every IR node receives a **stable file-local identifier**. Same post-repair source produces same IDs. This guarantee is what the cacheability strategy and the Phase 6b structural validation rely on.
- Body-level and flow-top-level constraint and context markers are hoisted into their respective lists; branch-scoped markers stay inline. This IR-level normalization runs regardless of whether the Phase 3a source-rewrite already performed an equivalent source-to-source hoist.
- Lower does not generate prose and does not validate correctness — those belong to Phase 6 and Phase 5 respectively.

### Phase 5: Validate (deterministic)

**Input:** typed IR from Phase 4. **Output:** the same IR (unchanged) if valid, or hard errors if not.

Invariants:

- Validate is the final correctness gate before any LLM touches the IR.
- Validate does not change the IR. It is a pure pass/fail gate.
- Validate enforces both pre-Lower contracts (closure of `export block`, effect-set superset across imports and inlines, completeness of name resolution, type matching at call boundaries) and post-Lower IR invariants (unique stable IDs, every `Call` callee resolves, well-formed branches, no recursion within a file, no silently empty Steps).
- Effect validation: when the author declares `effects:`, the declared set must be a superset of the inferred set. Effect-propagation violations are hard errors, not warnings; Repair (Phase 3) may add missing effect keywords when confidence is high.
- Closure of `export block` is enforced once per file at the export boundary, not transitively across imports. An importer sees only the imported callee's declared contract.

### Phase 6: Expand (deterministic + LLM)

**Input:** validated IR from Phase 5. **Output:** expanded IR — every node carries its final agent-facing prose, with parameter references preserved as `{param}` slots.

Invariants:

- Compilation is parameterless: Expand does not receive concrete argument values. Parameters survive into compiled output as named slots that the consuming LLM resolves at runtime.
- Expand is split into two strict steps: **deterministic resolution first, then LLM reshaping**. The intermediate artifact (the resolved IR) is inspectable and independent of LLM behavior.
- Step 1 (deterministic) preserves `{param}` slots, tags local-binding references, assembles parameter metadata, inlines bare-name `const` references, computes block projection tiers, and populates the predicate side-map on `Branch` nodes. After Step 1, every node has resolved content.
- Step 2 (deterministic emitter + LLM span fill) walks the resolved IR and produces a typed Markdown scaffold with typed `Span` placeholders. The scaffold owns all deterministic structure (section headers, list numbering, constraint template, return-fold suffix, pure-predicate Branch projection, external-file Call Step template). The LLM fills typed spans (parameter descriptions, mixed-condition Branch headers, Description return-folds, Call-body prose); failed spans retry in isolation without re-flowing the deterministic structure.
- Skills with no spans bypass the LLM entirely — the deterministic emitter produces complete, byte-stable Markdown.
- A separate role-preservation gate (Phase 6b) checks that Step 2 has not silently dropped or reshuffled roles. Retry / deterministic-fallback / hard-fail policy lives in [[docs/architecture/expand]].
- Expand does not change the source file. Expand does not alter the IR's structure (roles, types, effects, call graph). It only adds prose content.

### Phase 7: Emit (deterministic)

**Input:** expanded IR from Phase 6. **Output:** compiled `.md` file (and, when `--emit-ir`, `.ir.json`).

Invariants:

- Emit is pure formatting. No LLM involvement. No content generation.
- All authoring constructs are erased: imports, const references, `generated const`/`generated block` markers, comments, module paths, `with` modifiers. Parameter names survive only as `{param}` references in Steps/Constraints and as entries in the `## Parameters` section.
- **Atomic rename on disk.** Phase 7 writes `foo.md.tmp` (and `foo.ir.json.tmp` when `--emit-ir` is set), then renames each to its final path only after the pipeline succeeds for that file. On hard-fail anywhere in Phases 1–7, the `.tmp` files are deleted and any prior `foo.md` / `foo.ir.json` on disk is left untouched.
- **Startup cleanup of stale `.tmp` siblings.** Before writing any new `.tmp`, Phase 7 scans the output paths it is about to write and deletes any pre-existing `.tmp` siblings for those paths. This handles leftovers from a prior run that crashed between writing a `.tmp` and renaming it. No lockfile, no process supervisor; the sweep is idempotent and self-contained per file. `.tmp` files for paths *not* in this build are left untouched.
- A library file (zero `skill` declarations) runs through Emit unchanged: no skill-level `.md` is produced, but each `export block` above the inline threshold emits a standalone procedure `.md` into a per-source subdirectory. A library producing zero `.md` files is a valid, exit-zero outcome.

## Multi-File Compilation Order

When compiling multiple `.glyph` files that import each other:

1. **Phase 1 builds the import DAG across all files and topological-sorts it.** Cycles are rejected as hard errors before any later phase runs.

2. **Leaves compile first.** Files with no imports go through the full pipeline first.

3. **Dependency readiness.** An importing file cannot enter Phase 2 (Analyze) until the imported file has passed Phase 5 (Validate). The importer needs the dependency's validated IR for name resolution, type matching, and effect propagation.

4. **Strictly serial compilation.** Files compile one at a time, in topological order. There is no threadpool, no `rayon`, no async fan-out. Parallelism (including the architecturally-independent overlap of a dependency's Expand/Emit with an importer's Parse/Analyze/Lower/Validate) is a post-MVP optimization.

5. **Repair is per-file only.** Repair only edits the current file. It does not edit dependencies or add new imports. Each file gets its own bounded repair budget; there is no cross-file trigger propagation.

6. **Per-file repair iteration accounting.** The compiler is stateless across `glyph compile` invocations — every invocation re-parses every file. The repair iteration counter is owned by the agent and is per-file; the hard-fail bound is per-file, not per-build.

7. **Consumer-side projection-tier word counts.** During a library's Phase 6 Step 1, the resolved expanded prose for each `export block` is computed once and the word count is recorded as a derived in-memory field on the validated IR's `ExportBlock` node. Consumers depend on this when running their own Phase 6 Step 1 projection-tier decision. Topological order guarantees the value is computed before any consumer needs it. The field is not part of the IR JSON serialization.

8. **Directory-mode scope: every file, no reachability filter.** When the user invokes `glyph compile dir/`, every `.glyph` file in scope compiles unconditionally, regardless of whether any in-scope skill reaches it through imports. A library file with no in-scope consumer still goes through Phases 1–7 and may produce zero emitted artifacts. Reachability filtering is post-MVP.

### Partial Failure Policy

When some files in a multi-file build fail, the compiler uses a **skip-dependents, leave-stale-`.md`, partial-output** policy:

1. **Skip-dependents.** For each file in topological order: if **all** of its (transitive) imports validated successfully **in this build**, run Phases 1–7 normally. Otherwise mark the file as skipped-due-to-dep and do not run any phase on it. The skip emits a warning naming the failed dependency's file path.

2. **Atomic per-file emission.** `.md` files are written atomically per file at the end of Phase 7. A file either fully succeeds (its `.md` is written or replaced) or its `.md` is not touched.

3. **Stale `.md` policy.** If a previous build emitted `b.md` and the current build fails (or skips) `b.glyph`, the existing `b.md` on disk is **left in place**. The compiler emits a stderr note that the on-disk version reflects the previous successful build and may be out of sync. Authors who want stale outputs purged must delete them manually; the compiler never deletes a previously emitted `.md` on a failed re-build.

4. **Exit code.** The build exits `0` only if every file succeeded. If any file failed or was skipped, exit `1`. A partial build produces partial output but signals failure via the exit code.

## Visualization

Visualization is not a pipeline phase. It is a **separate output path** that branches off after Phase 5 (Validate):

```
                              ┌──→ 6. Expand → 7. Emit → compiled .md
5. Validate ──→ validated IR ─┤
                              └──→ Graph renderer → visual output
```

The graph renderer reads the validated IR and projects it as a data-flow graph: parameters are entry nodes, calls are operation nodes, bindings are value edges, returns are exit nodes, effects are annotations on call nodes, branches are decision nodes. The renderer does not need Expand's prose; it works with the structural IR directly. The output format (JSON, DOT, Mermaid, etc.) is a tooling decision, not a pipeline decision. The pipeline's obligation is that the validated IR is a clean, well-structured format that supports graph projection.

## Source-To-Source vs. IR-Only Transforms

The pipeline distinguishes transforms that touch `.glyph` source from those that operate only on IR. This boundary is load-bearing for cacheability and for the per-file repair iteration model.

| Transform | Phase | Touches `.glyph`? |
|---|---|---|
| Unconditional constraint hoisting (body-level + flow-top-level) | 3a | Yes |
| Unconditional context hoisting (body-level + flow-top-level) | 3a | Yes |
| Unused import removal | 3a | Yes |
| Duplicate import merging | 3a | Yes |
| Section reorder to convention | 3a | Yes |
| `generated const` / `generated block` materialization | 3b | Yes |
| Missing `description:` / `effects:` generation | 3b | Yes |
| Role/constraint marker addition | 3b | Yes |
| Positional → named args | 4 | No (IR only) |
| Nested call desugaring | 4 | No (IR only) |
| Default value filling | 4 | No (IR only) |
| Effect propagation (union) | 4 | No (IR only) |
| `with` modifier recording | 4 | No (IR only) |
| Parameter metadata assembly | 6 (Step 1) | No (in-memory) |
| Block projection tier assignment | 6 (Step 1) | No (in-memory) |
| Bare name / inline string passthrough | 6 (Step 1) | No (in-memory) |
| Call-node expansion into prose | 6 (Step 2) | No (in-memory) |
| `with` modifier reshaping | 6 (Step 2) | No (in-memory) |
| Constraint rewording | 6 (Step 2) | No (in-memory) |
| Return folding into final step | 6 (Step 2) | No (in-memory) |
| Conditional projection to sub-steps | 6 (Step 2) | No (in-memory) |

## Cacheability

All seven phases produce output that depends only on source content and imports — there is no argument-dependent variation. If the source file and its imports have not changed, the entire pipeline output can be reused.

| Phases | Cacheable? | Key |
|---|---|---|
| 1-5 (Parse through Validate) | Yes | Post-repair source file content hash + **transitive** import content hashes |
| 6-7 (Expand + Emit) | Yes | Post-repair source hash + **transitive** import content hashes |

**Post-repair hashing.** The cache key is the **post-repair** source hash, not the original author-written source. Repair (Phase 3) writes back to the `.glyph` file, so the source on disk after a successful compile already includes all repairs. Subsequent compilations of the same file find no repairable diagnostics, skip repair, and produce the same validated IR — which matches the cached entry.

**Transitive dependency hashes.** The cache key includes the post-repair source hashes of **all transitive dependencies**, not just direct imports. If a library file changes, its procedure `.md` files may change, which means every consumer whose cache key includes that library's hash is stale and must recompile. This is conservative — a library change triggers consumer recompilation even if the change did not affect the specific export the consumer uses. Fine-grained per-export invalidation is a post-MVP optimization.

**Step 2 non-determinism caveat.** Step 2 (LLM reshaping) is not idempotent across model versions or repeated runs at temperature > 0. Byte-stable caching of compiled output requires including the Step 2 output in the cache entry. If the source has not changed and a cached Step 2 output exists, the pipeline may skip Step 2 entirely and reuse the cached prose.

Incremental compilation and build caching are **deferred** from MVP. The pipeline design supports them since all phases are argument-independent, but the MVP compiler may re-run all phases on every compilation.

## Cross-References

- **Foundations:** [[foundations]] — deterministic passes own correctness; novice learnability.
- **Source syntax and author-visible behavior:** [[language-surface]], [[ir-and-semantics]], [[design/repair]], [[design/compiled-output]], [[data-flow]], [[imports]], [[types]], [[stdlib]].
- **IR node schema and JSON contract:** [[ir-schema]], [[ir-json]].
- **Decision rationale (why this shape):** `docs/adr/` — see in particular the ADRs on the Safety Sandwich, the parameterless compilation model, the seven-phase decomposition, and the strictly-serial multi-file strategy.
