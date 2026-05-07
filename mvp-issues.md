# Glyph MVP — Proposed Issue Breakdown

Vertical slices for the Glyph compiler MVP. Each slice cuts end-to-end through every relevant layer (tokenizer → parser → AST → analyze → lower → validate → expand-step1 → emit, plus CLI/diagnostics where applicable) and is demoable on its own. AFK = implementable without human sync; HITL = needs human review/decision.

Bars referenced below come from `design/mvp-acceptance.md` §5 (exit criteria).

---

## Dependency overview

```
1 ── 2 ── 3
│    │    
│    ├──── 18  (--strict)
│    └──── 19  (fmt)
│
├── 4 (params)
├── 5 (constraints + text)
├── 16 (atomic emission)
├── 17 (--emit-ir)
│
└── 7 (block calls / Tier 1)
     ├── 6 (effects, full)
     ├── 8 (return)
     ├── 9 (branching + .applies() — needs #7's description field)
     ├── 10 (with)
     ├── 11 (imports, single-file)
     │    ├── 12 (multi-file build)
     │    ├── 13 (library files + closure)
     │    │    └── 15 (Tier 3 procedure files)
     │    └── 21 (stdlib)
     └── 14 (Tier 2 procedure)

20 (validate-output) ← 14, 15, 17
22 (acceptance) ← all
23 (diag coverage) ← all relevant feature slices
```

---

## Slice 1 — Workspace bootstrap & walking skeleton

- **Type:** AFK
- **Blocked by:** None — can start immediately
- **Bars covered:** Bar 1 (initial), Bar 2 (initial)

### What to build

Two-crate Cargo workspace (`glyph-cli` + `glyph-core`) per `build-foundation.md` §A1. Implement the minimum tokenizer, hand-rolled recursive-descent parser, loose AST, Phase 2 Analyze (trivial pass-through), Phase 4 Lower (assigns node IDs, builds arena), Phase 5 Validate (trivial), Phase 6 Step 1 (passthrough — bare-name/inline-string handling for the kernel), and Phase 7 Emit. Wire `glyph compile <file>` to run all phases and write `<name>.md` next to the source. End-to-end success path only — exit 0, zero diagnostics emitted. `update_docs.glyph` from `mvp-acceptance.md` §1 compiles to byte-identical golden snapshot. Includes `Span`, `Spanned<T>`, `LineIndex`, `IrArena`, `NodeId` per `build-foundation.md` §A3/A4. `insta` snapshot test framework wired in `glyph-cli/tests/`. Emit knows the `### Context` H3 exists in the compiled-output spec (`compiled-output.md`) but the walking skeleton's `update_docs.glyph` declares no `context:`, so the section is omitted from the golden snapshot.

### Acceptance criteria

- [ ] `cargo build` produces `glyph` binary
- [ ] `glyph compile tests/corpus/valid/update_docs.glyph` exits 0
- [ ] Emitted `update_docs.md` matches golden snapshot from `mvp-acceptance.md` §1 byte-for-byte
- [ ] Re-running the compile produces byte-identical output
- [ ] `insta` snapshot harness present and used by the walking-skeleton test

---

## Slice 2 — Diagnostic infrastructure

- **Type:** AFK
- **Blocked by:** #1
- **Bars covered:** Bar 1, Bar 2 (JSON determinism)

### What to build

Full `Diagnostic` shape per `diagnostics.md` (id, classification, message, span, related, hints). `DiagBag` accumulator. Classification → exit code mapping (0 / 1 / 2 / 3) per `build-foundation.md` §A6, including the 1-wins-over-2 rule. `--format pretty` (codespan-reporting on stderr) and `--format json` (NDJSON to stdout). JSON determinism per `build-foundation.md` §JSON Determinism: `BTreeMap` for any map, diagnostics sorted by `(file, span.start.byte, id)`. Exercise the diagnostic path end-to-end with two trivial diagnostics from `tests/corpus/invalid/`: `G::parse::empty-file` and `G::parse::empty-flow`.

### Acceptance criteria

- [ ] `glyph compile invalid/empty.glyph` exits 1 with `G::parse::empty-file`
- [ ] `glyph compile invalid/empty_flow.glyph` exits 1 with `G::parse::empty-flow`
- [ ] `--format json` produces a JSON array of diagnostics on stdout
- [ ] Pretty output renders span, message, and source caret to stderr
- [ ] Re-running over identical input produces byte-identical JSON (sorted, BTreeMap)
- [ ] Exit-code rules hold: `1` wins over `2` when both present

---

## Slice 3 — `glyph check` subcommand

- **Type:** AFK
- **Blocked by:** #2

### What to build

`glyph check <path>` runs Phases 1 + 2 only, reports all diagnostics (errors / repairable / warnings), writes no output files, never enters Lower/Validate/Expand/Emit. Same `--format` flag as `compile`. Same exit-code semantics.

### Acceptance criteria

- [ ] `glyph check valid/update_docs.glyph` exits 0 with no files written
- [ ] `glyph check repairable/<file>` exits 2 with diagnostics on stdout (JSON) or stderr (pretty)
- [ ] `glyph check invalid/<file>` exits 1
- [ ] Subcommand parsing accepts file or directory paths

---

## Slice 4 — Parameters and `## Parameters` section

- **Type:** AFK
- **Blocked by:** #1

### What to build

Skill parameters with defaults per `language-surface.md`. `{param}` slot recognition inside instruction-bearing strings (tokenizer emits `ParamSlot` token per `build-foundation.md` §A2). Parameter metadata assembly in Phase 6 Step 1. `## Parameters` emission in Phase 7. Step 1 preserves `{param}` references — no substitution. Wires diagnostics: `G::analyze::missing-required-arg` (PRD #103 / Issues #104, #105 — fires at the call site when a `call <name>(...)` omits a positional argument for any callee parameter without a default; applies uniformly to private `block`, same-file `export block`, and imported `export block`. Skill parameters without defaults are runtime-required inputs and surface in `## Parameters` without firing any diagnostic — see `language-surface.md` §3.10 and `diagnostics.md`), `G::analyze::unknown-param-slot`, `G::parse::param-slot-in-non-instruction-string`.

### Acceptance criteria

- [ ] A skill `foo(scope = ".")` emits a `## Parameters` section with `scope` and its default
- [ ] A skill `foo(scope)` (no default) compiles and surfaces `scope` as runtime-required in `## Parameters` — no diagnostic fires
- [ ] An `export block bar(x)` (no default) compiles cleanly when no caller invokes it; a caller `call bar()` (omitting `x`) fires `G::analyze::missing-required-arg` at the call site (PRD #103 / Issues #104, #105)
- [ ] `{scope}` inside Step text passes through verbatim (not substituted)
- [ ] Parameterless skill omits the `## Parameters` section
- [ ] All three parameter-related diagnostics have triggering corpus files and fire correctly

---

## Slice 5 — Constraints, Context, `text` declarations, and `### Constraints` + `### Context`

- **Type:** AFK
- **Blocked by:** #1

### What to build

`text`/`int`/`float` declarations (private and `export` variants reserved for #13). Constraint markers (`require`, `avoid`, `must`, `must avoid`) at body level and inside `constraints:` sections. Body-level marker hoisting in Phase 4 Lower (per `pipeline.md` §Phase 4 — IR-only normalization, distinct from `glyph fmt` which does it source-side). Role inference for `Constraint`. Strength/polarity assignment per `ir-and-semantics.md` §2. `### Constraints` rendering in Phase 7 with mechanical (non-LLM) phrasing — Step 1 hands resolved text directly to Emit. `valid/constraint_only.glyph` test (skill with constraints but no flow → omit `### Steps`).

**Context (parallel pipeline).** Implement the `Context` IR role and `ContextNode` (`ir-schema.md`, `ir-and-semantics.md`). The `context:` sub-section parses on `skill`/`block`/`export block` bodies; the `context` marker is legal at body level and as a flow statement (per `data-flow.md` §Statement Forms). Lower hoists body-level and flow-top-level `context` markers into the declaration's `context: [ContextNode]` list (deduped by canonical text); branch-scoped `context` markers stay inline (parallel handling to constraints). `text` declarations referenced inside `context:` resolve the same way as inside `constraints:` — placement, not declaration kind, decides the role (`primitives.md` §`text` duality). Phase 7 Emit renders the declaration's `context` list as `### Context` (bulleted, before `### Steps`); the section is conditional and omitted when no context is declared. `### Context` alone is not sufficient — at least one of `### Steps` or `### Constraints` must still be present. `{param}` slots inside `context:` body content emit `G::parse::param-slot-in-non-instruction-string` (same restriction as `description:`).

**`text`-in-flow tightening.** A bare `text` name (or any undefined bare name) appearing in `flow:` without a keyword prefix (`require`/`avoid`/`must`/`context`) is now a compile error: new diagnostic `G::analyze::text-in-flow` (repairable — Repair adds parens and materializes a `generated block`, per `repair.md` §5). Bare names with a keyword prefix continue to materialize as `generated const`.

Diagnostics: `G::analyze::undefined-name`, `G::analyze::ambiguous-role`, `G::analyze::missing-description`, `G::analyze::text-in-flow`, `G::parse::param-slot-in-non-instruction-string` (extends to `context:` bodies).

### Acceptance criteria

- [ ] `valid/constraint_only.glyph` compiles, emits `### Constraints` only (no `### Steps`)
- [ ] `require accuracy` + same-file `text accuracy = "..."` resolves and renders the text content
- [ ] Body-level `avoid X` is hoisted into the IR's `constraints` list during Lower
- [ ] A skill with a `context:` sub-section emits `### Context` before `### Steps` in compiled output
- [ ] Body-level and flow-top-level `context X` markers hoist into the IR's `context` list during Lower
- [ ] Branch-scoped `context` marker stays inline (does not surface in `### Context`)
- [ ] A `text` name referenced from `context:` resolves and renders the underlying string in `### Context`
- [ ] `{param}` inside `context:` body fires `G::parse::param-slot-in-non-instruction-string`
- [ ] Bare `foo` in `flow:` (no parens, no keyword prefix) fires `G::analyze::text-in-flow` (repairable)
- [ ] All listed diagnostics fire on their corpus triggers

---

## Slice 6 — Effects system (full)

- **Type:** AFK
- **Blocked by:** #7

### What to build

Effect inference walking the call graph per `ir-and-semantics.md` §3. Author-declared `effects:` super-set check vs. inferred. Frontmatter emission as YAML flow-sequence list. Effect propagation across imports + private inlines (depends on #11 for the import case but base implementation can land first and #11 extends it). `effects: none` exclusivity check. Diagnostics: `G::analyze::effects-under-declared` (error), `G::analyze::effects-over-declared` (warning, stderr), `G::analyze::missing-effects` (repairable — Phase 3a auto-adds inferred effects for any declaration that omits `effects:` entirely), `G::repair::inferred-effects` (warning, informational), `G::parse::none-with-effects` (error). The walking skeleton's basic frontmatter list emission is upgraded here from "echo declared keywords" to full inference + validation.

### Acceptance criteria

- [ ] Inferred effects union from call graph matches declared `effects:`
- [ ] Over-declared effects produce a warning on stderr but exit 0
- [ ] Under-declared effects produce error → exit 1
- [ ] `effects: none, reads_files` rejected with `G::parse::none-with-effects`
- [ ] Frontmatter shows `effects: [reads_files, writes_files]` in canonical order
- [ ] Frontmatter omits the `effects:` field entirely when set is empty

---

## Slice 7 — Block calls (Tier 1 inline projection)

- **Type:** AFK
- **Blocked by:** #1

### What to build

`block` declarations (private; `export block` deferred to #13). Optional `description:` sub-section on `block` (and `export block` once #13 lands) per `ir-and-semantics.md` §Block Trigger Predicate — a single-string body, stored on the `Block` IR node as `description: Option<String>`. The description has no effect on Tier 1 projection here; it's surfaced for the `.applies()` consumer in #16 and for compiled output in #18. **Single-string shorthand:** when a `block` body contains only a single instruction string and no other sub-sections (no `effects:`, `constraints:`, `context:`, `flow:`), the `flow:` header may be omitted (`language-surface.md` §3.2). The bare string is always a `Step` — never context. Generated blocks (#5/#23 surface) use the same shorthand. Calls with positional/named args, UFCS recognition (deferred desugaring to Phase 4 per `pipeline.md` §Phase 4). Phase 4 desugaring: positional → named, UFCS → `foo(receiver, args)`, defaults filled, callee resolution, flat calls only. Phase 6 Step 1 projection-tier heuristic (Tier 1 inline if resolved word count < 150 — see `compiled-output.md`). Tier 1 expansion: callee body text inlined into the caller's `### Steps`. Diagnostics: `G::analyze::undefined-call`, `G::validate::unresolved-callee`, `G::validate::recursive-call`, `G::validate::empty-step`, `G::validate::duplicate-node-id`. Last four are unit-tested with hand-crafted IR per `mvp-acceptance.md` §4.4.

### Acceptance criteria

- [ ] Call to a same-file private block expands inline in `### Steps`
- [ ] Block declaration with `description: "..."` parses and the description is reachable on the `Block` IR node
- [ ] Block declaration without `description:` parses; the field is `None` on the IR node
- [ ] A block with a single instruction string and no other sub-sections parses without `flow:` (single-string shorthand); the string lowers to a `Step`
- [ ] Resolved word count is computed once per block
- [ ] All Validate-phase diagnostics in scope have unit tests with hand-crafted IR
- [ ] `undefined-call` fires on a parens-call to an unknown name (repairable)

---

## Slice 8 — Return folding

- **Type:** AFK
- **Blocked by:** #7

### What to build

`return` parsing (terminal-only at flow root). Mechanical return-folding in Phase 6 Step 1: the resolved return text is appended to the final Step verbatim (Step 2's "summarize and return" reshaping is the agent's job). Diagnostics: `G::parse::return-not-terminal`, `G::parse::return-in-branch`, `G::parse::multiple-returns`, `G::analyze::missing-return`.

### Acceptance criteria

- [ ] `return summarize_changes()` becomes the last sentence of the final numbered step
- [ ] Private blocks may omit `return`; export blocks (#13) require it
- [ ] All four return-related diagnostics fire on their corpus triggers

---

## Slice 9 — Branching (if / elif / else)

- **Type:** AFK
- **Blocked by:** #1, #7 (block `description:` field, consumed by `.applies()` side-map)

### What to build

`if`/`elif`/`else` parsing inside `flow:`. Branch-condition `==` is **not** a value-level operator — it's branch-syntax-only and does **not** trigger `G::parse::operator-in-expression` (see `mvp-acceptance.md` §2.1 note on `branching.glyph`). Phase 4 builds `Branch { condition, then_body, elif_branches, else_body }`. Phase 6 Step 1 emits one numbered Step for the branch chain, with lettered sub-steps per arm (`a.`, `b.`, `c.`, reset per arm). Constraint markers inside branch bodies stay inline (per `pipeline.md` §Phase 4); `context` markers inside branch bodies stay inline by the same rule (parallel to constraints — they render as part of the conditional Step prose, never surface in `### Context`). Diagnostics: `G::parse::nested-flow`, `G::analyze::nested-branch`, `G::validate::malformed-branch`, `G::parse::operator-in-expression`.

**Block trigger predicate** per `ir-and-semantics.md` §Block Trigger Predicate. Parse `BLOCKNAME.applies()` (and `module_alias.block_name.applies()`) as a zero-arity special form recognised only in `if`/`elif` condition position — NOT general UFCS. Receiver must resolve to a same-file `block`/`export block` or an imported block. Phase 4 records the call shape; Phase 6 Step 1 populates a `applies_descriptions: { block_name → resolved_description }` side-map on the `Branch` IR node by looking up each `.applies()` receiver's `description:` (depends on #15 description parsing). The `condition` field stays a `String`; no new `Expression` variant. Compiled output projects pure-applies arms via the §Description-Driven Branch Projection rules in `compiled-output.md` (Step 1 emits the description-keyed shape; full prose reshape is Step 2's job, validated by #30). New diagnostics: `G::parse::applies-no-parens` (error — receiver written without `()`), `G::parse::applies-with-args` (error — `.applies(...)` called with arguments), `G::analyze::applies-on-non-block` (error — receiver is `text`/import-alias/parameter), `G::analyze::applies-on-undescribed-block` (repairable for same-file blocks per `repair.md` §9; error for imported blocks).

### Acceptance criteria

- [ ] `valid/branching.glyph` compiles; output uses lettered sub-steps per arm
- [ ] `==` in `if` condition does NOT trigger `operator-in-expression`
- [ ] `nested-branch` fires when a branch is nested inside a branch
- [ ] `malformed-branch` Validate diag has unit test
- [ ] `BLOCKNAME.applies()` parses inside `if`/`elif` and is rejected outside branch-condition position
- [ ] `applies_descriptions` side-map is populated post-Step-1 keyed by block name; missing description on imported block hard-errors with `applies-on-undescribed-block`
- [ ] All four `applies-*` diagnostics have triggering corpus files (or unit tests for the imported-block error variant)
- [ ] Pure-applies branch arms render via the description-keyed projection in Step 1's mechanical output (full prose reshape verified externally)
- [ ] A `context` marker inside a branch body stays inline (renders in the conditional Step prose, does not surface in `### Context`)

---

## Slice 10 — `with` modifier (IR-only recording)

- **Type:** AFK
- **Blocked by:** #7

### What to build

`with "<modifier text>"` parsing, attached only to call expressions. Phase 4 Lower stores it as `site_modifier` on the `Call` IR node. Mechanical `.md` (Step 1 only) uses the resolved body text **without** applying the modifier — modifier application is the agent's Step 2 responsibility. The IR JSON (#17) preserves `site_modifier` so the agent can reshape post-compile. Diagnostics: `G::parse::multiple-with`, `G::parse::with-on-bare-name`, `G::parse::none-with-effects` (already covered in #6 but related).

### Acceptance criteria

- [ ] `inspect_repo(scope) with "..."` parses and stores the modifier on the Call node
- [ ] Compiled `.md` from Step 1 does NOT apply the modifier (mechanical text only)
- [ ] `multiple-with` fires on chained `with` clauses
- [ ] `with-on-bare-name` fires when `with` follows a bare name (no parens)

---

## Slice 11 — Imports (single-file resolution)

- **Type:** AFK
- **Blocked by:** #7

### What to build

`import "./path.glyph" { name1, name2 }` parsing and whole-module imports. Path resolution. Cross-file name resolution in Phase 2 (importer reads dependency's validated IR). Cycle rejection in Phase 1. Effects propagation across imports per `data-flow.md` §Effect Propagation. Diagnostics: `G::analyze::missing-file`, `G::analyze::circular-import`, `G::analyze::import-private`, `G::analyze::import-skill`, `G::analyze::duplicate-import` (repairable), `G::analyze::unused-import` (repairable).

### Acceptance criteria

- [ ] `fix_bug.glyph` resolves names imported from `prefs.glyph` and `repo_tools.glyph`
- [ ] Circular-import path is included in the diagnostic message
- [ ] Importing a private (non-exported) name fails with `import-private`
- [ ] Importing a skill (not a block/text) fails with `import-skill`
- [ ] Duplicate / unused imports are repairable diagnostics → exit 2

---

## Slice 12 — Multi-file build orchestration

- **Type:** AFK
- **Blocked by:** #11

### What to build

DAG construction in Phase 1 across all in-scope `.glyph` files. Topological sort. Strictly serial compilation (no `rayon`, no async — per `build-foundation.md` §A5). Dependency-readiness gate (importer's Phase 2 only after dependency's Phase 5). Directory-mode (`glyph compile dir/`) compiles every file in scope unconditionally — no reachability filter. Partial failure policy per `pipeline.md` §Partial Failure Policy: skip-dependents, leave-stale-`.md`, partial-output, exit 1 if any file fails. Diagnostic: `G::build::skipped-due-to-failed-import` (warning).

### Acceptance criteria

- [ ] `glyph compile dir/` processes every `.glyph` even if not transitively reached
- [ ] Files compile in topological order (libraries before consumers)
- [ ] Failure in `b.glyph` skips `c.glyph` (which imports it) with the build warning
- [ ] Stale `c.md` left untouched on disk after `c.glyph` skip; stderr note emitted
- [ ] Build exits 1 if any file failed; partial output present for successful files

---

## Slice 13 — Library files (export blocks/text + closure check)

- **Type:** AFK
- **Blocked by:** #11, #7, #5

### What to build

Library detection: file with zero `skill` declarations. `export block`, `export text`, `export int`, `export float` declarations. Library Emit: zero `.md` for the library's skill section (none exists); export blocks with resolved word count ≥ 150 emit standalone procedure files (foundation for #15). Closure check on `export block` per `data-flow.md` §Closure: parameters, return type, effects, constraints all explicit; private definitions invisible to importers. Diagnostics: `G::analyze::no-exports-in-library`, `G::analyze::name-collision`, `G::analyze::closure-violation`, `G::analyze::missing-return` (export blocks must have explicit return on every code path).

### Acceptance criteria

- [ ] `prefs.glyph` (export-text-only) compiles to zero `.md`, exit 0
- [ ] `repo_tools.glyph` compiles; large export blocks queued for procedure-file emission (Slice 15 lands the actual file write)
- [ ] Closure-violation fires when export block references private free variables
- [ ] Library with zero exports → `no-exports-in-library` (error)
- [ ] Sibling exports visited in source order (deterministic on-disk output)

---

## Slice 14 — Tier 2 same-file procedure projection

- **Type:** AFK
- **Blocked by:** #7

### What to build

Phase 6 Step 1 promotes a Call to Tier 2 when callee meets the same-file procedure heuristic per `compiled-output.md` §Three-Tier Block Projection (block ≥ 4 statements or reused, but private to the file). Phase 7 emits `### Procedure: <name>` section under `## Instructions` containing the callee's expanded flow. Caller `### Steps` references the procedure by name. Procedure-related Validate-output diagnostics (full coverage in #20): `procedure-count-mismatch`, `procedure-name-mismatch`, etc. — covered structurally here, validated externally in #20.

### Acceptance criteria

- [ ] `valid/explicit_blocks.glyph` (4+ statement private block) emits a `### Procedure: <name>` section
- [ ] Caller's `### Steps` cites the procedure by name
- [ ] Procedure section ordering is deterministic
- [ ] Tier 1 callee (small block) still inlines (regression check)

---

## Slice 15 — Tier 3 external-file procedure projection

- **Type:** AFK
- **Blocked by:** #13, #14

### What to build

Standalone procedure `.md` files for export blocks above the Tier 1 threshold per `pipeline.md` §Phase 7. Subdirectory naming: `repo_tools.glyph` with `export block inspect_repo` → `repo_tools/inspect-repo.md`. `kind: procedure` frontmatter to distinguish from skills. Caller (in another file) references the procedure file path at runtime — compiled `.md` is no longer fully self-contained for Tier 3.

### Acceptance criteria

- [ ] `repo_tools.glyph` emits `repo_tools/inspect-repo.md` and `repo_tools/run-tests.md`
- [ ] Procedure files carry `kind: procedure` in frontmatter
- [ ] Consumer's compiled `.md` references the procedure files at the conventional path
- [ ] Re-running produces byte-identical procedure files

---

## Slice 16 — Atomic emission and stale `.tmp` cleanup

- **Type:** AFK
- **Blocked by:** #1

### What to build

Phase 7 writes through `foo.md.tmp` and (if `--emit-ir`) `foo.ir.json.tmp`, then renames to final paths only after the entire pipeline succeeds for that file. On hard-fail anywhere in 1–7, `.tmp` files are deleted and any prior `foo.md` / `foo.ir.json` is left untouched. At Phase 7 startup, sweep stale `.tmp` siblings of paths this build is about to write (idempotent, no-lockfile). `.tmp` files for paths *not* in this build are left alone. Per `pipeline.md` §Phase 7 last two bullets.

### Acceptance criteria

- [ ] Mid-pipeline crash leaves no `.tmp` files and no half-written `.md`
- [ ] Prior successful `.md` survives a failed re-build
- [ ] Stale `.tmp` from a SIGINT'd previous run is cleaned at startup
- [ ] Same rules apply uniformly to `.md`, `.ir.json`, and procedure files

---

## Slice 17 — `--emit-ir` flag and resolved IR JSON

- **Type:** AFK
- **Blocked by:** #1 (extended after each feature lands; final shape ready before #20)

### What to build

`--emit-ir` flag on `glyph compile`. Custom serializer that walks the post-Step-1 IR arena and produces a nested-tree JSON (children inlined under parents, each node carries `node_id`) per `build-foundation.md` §IR JSON Serialization and `ir-json-schema.md`. Includes `resolved_body_text`, `projection_mode`, `site_modifier`, parameter slots — every field the agent needs for Step 2 reshaping. Role enum serializes the full five-role set: `"input_contract"`, `"step"`, `"constraint"`, `"context"`, `"output_contract"` (`ir-json-schema.md` §Enum Casing). Also serializes:
- Optional `description` on `Block` / `ExportBlock` (from #15) — emitted as a string when present, omitted when `None`.
- `applies_descriptions: { block_name → resolved_description }` on `Branch` (from #16), populated post-Step-1 — emitted as an empty object when no `.applies()` call is present in the branch condition.
- `local_refs: [LocalRef]` parallel array on every `ResolvedCall` per `ir-schema.md` and `ir-json-schema.md` worked example — encodes argument-position bindings the agent's Step 2 must resolve into prose. Phase 6b's `unresolved-local-ref` (#30) fires when Step 2 leaves these as literal `{name}` tokens.
- New JSON node kind `"context"` (`ContextNode`, fields: `node_id`, `kind: "context"`, `text`) — `ir-json-schema.md` §ContextNode.
- New `context: [ContextNode]` array on Skill/Block/ExportBlock JSON (always present, may be empty) — top-level declared context entries.
- New `callee_context: [ContextNode] | null` on every resolved Call JSON (parallel to `callee_constraints`) — present only when `projection_mode != "inline"`, null otherwise.

JSON written through `.tmp` (uses #16 infra). BTreeMap discipline throughout.

### Acceptance criteria

- [ ] `glyph compile foo.glyph --emit-ir` writes `foo.ir.json` next to `foo.md`
- [ ] JSON byte-identical across runs
- [ ] Includes `site_modifier` for `with`-modified calls
- [ ] Includes `projection_mode` for every Call node (inline / same-file / external)
- [ ] Includes `description` on Block/ExportBlock when set in source; omitted when absent
- [ ] Includes `applies_descriptions` on every Branch node (empty object permitted)
- [ ] Includes `local_refs` parallel array on every ResolvedCall
- [ ] Role enum includes `"context"` value
- [ ] Skill/Block/ExportBlock JSON carries a `context: []` array (empty when no context declared)
- [ ] Resolved Call JSON carries a `callee_context` field (null when inline; array when same-file or external projection)
- [ ] `ContextNode` serializes as `{ node_id, kind: "context", text }`
- [ ] Conforms to `ir-json-schema.md`

---

## Slice 18 — `--strict` mode

- **Type:** AFK
- **Blocked by:** #2

### What to build

`--strict` flag on `glyph compile` (and `glyph check`). Promotes `repairable` diagnostics to errors → exit 1 instead of exit 2. Bar 4 of the MVP exit criteria.

### Acceptance criteria

- [ ] `--strict` passes on every file in `tests/corpus/valid/` and `multi-file/`
- [ ] `--strict` fails (exit 1) on every file in `tests/corpus/repairable/`
- [ ] Without `--strict`, the same repairable files exit 2

---

## Slice 19 — `glyph fmt` subcommand (Phase 3a only)

- **Type:** AFK
- **Blocked by:** #1, #2

### What to build

Phase 3a deterministic source rewrites per `cli.md` §`glyph fmt` and `pipeline.md` §Phase 3a. **Two strata:**
1. Pre-Parse text-level: tab → 4-space conversion, mixed-indentation fix.
2. Post-Parse AST-level: unconditional constraint hoisting, **unconditional context hoisting** (body-level + flow-top-level `context` markers move into a `context:` sub-section, parallel to the constraint hoisting rule per `pipeline.md` §Phase 3a), duplicate-import merge, unused-import removal, canonical sub-section reorder.

**Canonical sub-section order** (`ir-and-semantics.md` §Source Order Convention): `description:` → `effects:` → `context:` → `constraints:` → `flow:`. Any non-canonical order in source is rewritten to this layout. Branch-scoped context/constraint markers are not rewritten — only unconditional ones at body level or flow top-level.

`--check` mode: don't write, exit 1 if anything would change. Idempotent. If Phase 1 fails after the pre-Parse pass, write only the pre-Parse fixes and surface the parse diagnostic.

### Acceptance criteria

- [ ] `glyph fmt foo.glyph` rewrites tabs to 4 spaces in place
- [ ] Body-level constraint markers move into a `constraints:` section
- [ ] Body-level and flow-top-level `context` markers move into a `context:` section (creating it if absent)
- [ ] Branch-scoped `context` / constraint markers are NOT rewritten (stay inline in the branch)
- [ ] Sub-sections reordered to canonical layout: `description:` → `effects:` → `context:` → `constraints:` → `flow:`
- [ ] `glyph fmt --check` exits 1 when changes would be made, 0 when already formatted
- [ ] `glyph fmt` is idempotent: running twice produces identical output

---

## Slice 20 — `glyph validate-output` subcommand (Phase 6b)

- **Type:** AFK
- **Blocked by:** #14, #15, #17

### What to build

`glyph validate-output <ir-json> <md>` per `cli.md` §`glyph validate-output`. All 26 `G::expand::*` structural checks per `mvp-acceptance.md` §4.1 Validate-output table: section shape (`extra-h2`, `missing-instructions`, `extra-h3` — allowed H3s under `## Instructions` are `### Context`, `### Steps`, `### Constraints`, `### Procedure: <name>`), step/constraint/**context** count + ordering (new `G::expand::context-count-mismatch` fires when the number of `### Context` bullets does not match the IR's top-level `context` array length), parameter-slot integrity (`invented-param-ref`, `dropped-param-ref`, `unresolved-local-ref` — the last fires when a `local_refs` entry survived Step 2 as a literal `{name}` token), modifier leakage, params-section presence, length limits (`step-too-long`, `constraint-multi-sentence`), procedure-section integrity (count/name/order/duplicates/dangling refs), `frontmatter-returned`, `malformed-markdown`. Description-driven branch arms are validated structurally against the `applies_descriptions` side-map — pure-applies arms must project the description-keyed shape per `compiled-output.md` §Description-Driven Branch Projection. Exit 0 on clean, 1 on violations, 3 on invocation error.

### Acceptance criteria

- [ ] All 26 `G::expand::*` diagnostic IDs have unit tests with hand-crafted `.ir.json` + `.md` pairs
- [ ] `extra-h3` accepts `### Context`, `### Steps`, `### Constraints`, and `### Procedure: <name>` (and only those)
- [ ] `context-count-mismatch` fires when the number of `### Context` bullets diverges from the IR `context` array length
- [ ] `unresolved-local-ref` fires when a `local_refs` entry appears as a literal `{name}` in the `.md`
- [ ] Description-driven branch validation rejects a pure-applies arm rendered without the description-keyed shape
- [ ] `validate-output` accepts both `--format pretty` and `--format json`
- [ ] Compiler's own emitted `.md` + `.ir.json` always passes `validate-output`

---

## Slice 21 — Stdlib MVP entries

- **Type:** AFK
- **Blocked by:** #11

### What to build

Stdlib module registry (built into `glyph-core`) per `stdlib.md`. MVP stdlib has three entries:
- `subagent` (author-facing) — invoke a sub-agent
- `send` (author-facing) — message-passing primitive
- `load` (compiler-internal, not author-importable)

Stdlib name resolution as the last fallback in Phase 2 (after same-file → imported → qualified). Effect signatures per stdlib entry. Diagnostics: `G::analyze::stdlib-missing-import` (repairable — author used a stdlib name without importing it), `G::imports::unknown-stdlib-module`.

### Acceptance criteria

- [ ] `subagent` and `send` resolvable when imported from the stdlib module path
- [ ] `load` is NOT resolvable from author source (compiler-internal only)
- [ ] `stdlib-missing-import` repairable fires when `subagent`/`send` used without import
- [ ] `unknown-stdlib-module` error fires on import of a stdlib path that doesn't exist
- [ ] Each stdlib entry's effect signature propagates correctly through the call graph

---

## Slice 22 — Multi-file acceptance project

- **Type:** AFK
- **Blocked by:** #4, #5, #6, #7, #8, #9, #10, #11, #12, #13, #14, #15, #17

### What to build

The 5-skill project under `tests/corpus/multi-file/` per `mvp-acceptance.md` §3: `prefs.glyph`, `repo_tools.glyph`, `fix_bug.glyph`, `review_pr.glyph`, `update_docs.glyph`. End-to-end test asserts:
- DAG order respected: leaves before consumers
- Cross-file name resolution works
- All five files exit 0
- Byte-identical re-run (Bar 2 verification)

This slice is purely integration: it adds no new compiler features; it confirms Bar 3.

### Acceptance criteria

- [ ] `glyph compile tests/corpus/multi-file/` exits 0
- [ ] Each of the 5 files produces its expected `.md` (or library-zero-output for `prefs`)
- [ ] Procedure files for `repo_tools` exports land at expected paths
- [ ] Re-running produces byte-identical `.md`, `.ir.json`, and procedure files
- [ ] At least one skill exercises `BLOCKNAME.applies()` against a same-file block (with `description:`) and an imported block (description in the library file)
- [ ] At least one skill exercises a top-level `context:` sub-section (renders as `### Context` in compiled output) and at least one location exercises a branch-scoped `context` marker (stays inline in conditional Step prose)
- [ ] Snapshot tests cover all emitted artifacts via `insta`

---

## Slice 23 — Diagnostic coverage backfill (Bar 5)

- **Type:** AFK
- **Blocked by:** all relevant feature slices (#2, #4–#15, #19, #20, #21)

### What to build

Backfill any diagnostic IDs not already triggered by an earlier slice. Walk the `mvp-acceptance.md` §4.1 catalog (77 IDs total: 17 Parse + 27 Analyze + 1 Imports + 5 Validate + 1 Build + 26 Validate-output) and confirm each has a triggering test. The Analyze count includes `G::analyze::text-in-flow` (added in #14, repairable — bare name in `flow:` without keyword prefix). The Validate-output count includes `G::expand::context-count-mismatch` (added in #30, error — `### Context` bullet count disagrees with IR `context` array length). Add missing corpus files in `tests/corpus/repairable/` and `tests/corpus/invalid/`. Add unit tests with hand-crafted IR for any Validate diagnostics not exercised by corpus runs (per `mvp-acceptance.md` §4.4). Add the warning triggers (`effects-over-declared`, `repair::inferred-effects`) via `valid/` corpus files that compile successfully with stderr warnings.

### Acceptance criteria

- [ ] Every one of the 77 compiler-scope diagnostic IDs has at least one triggering test
- [ ] Test inventory is enumerable (a single source of truth maps ID → test name)
- [ ] Bar 5 of `mvp-acceptance.md` §5 is met

---

## Per-issue context budget

Every implementer agent should load these **universal** files (small, high signal):

- `mvp-issues.md` — its own slice spec (this file, the relevant slice + dependency overview)
- `design/pipeline.md` — phase boundaries it must respect
- `design/build-foundation.md` — crate layout, exit codes, JSON determinism, error model
- `design/AGENTS.md` (or `CLAUDE.md` symlink) — project-level conventions

Beyond that, per slice:

### Slice 1 — Walking skeleton
- `design/mvp-acceptance.md` §1 (walking skeleton spec, golden snapshot)
- `design/language-surface.md` (kernel grammar — drives tokenizer/parser surface)
- `design/ir-schema.md` (node types, ID scheme)
- `design/ir-and-semantics.md` §1–§2 (the 4 MVP roles)
- `design/compiled-output.md` (frontmatter + Steps/Constraints rendering)
- `design/diagnostics.md` (Span semantics, Diagnostic shape stub)

### Slice 2 — Diagnostic infrastructure
- `design/diagnostics.md` (full)
- `design/mvp-acceptance.md` §4 (catalog being plumbed; structure only)

### Slice 3 — `glyph check`
- `design/cli.md` §`glyph check`

### Slice 4 — Parameters
- `design/language-surface.md` (param syntax, `{slot}` rules)
- `design/data-flow.md` (parameter semantics)
- `design/compiled-output.md` §`## Parameters`
- `design/ir-schema.md` (parameter / InputContract nodes)

### Slice 5 — Constraints + text
- `design/ir-and-semantics.md` §2, §Body-Level Constraint Normalization, §Flow-Level Constraint Markers
- `design/language-surface.md` (`text`/`int`/`float`, marker syntax)
- `design/compiled-output.md` §Constraint Rendering
- `design/ir-schema.md` (Constraint, InstructionRef)

### Slice 6 — Effects (full)
- `design/ir-and-semantics.md` §3
- `design/data-flow.md` §Effect Propagation
- `design/compiled-output.md` (frontmatter `effects:` rules)

### Slice 7 — Block calls + Tier 1
- `design/compiled-output.md` §Three-Tier Block Projection (central)
- `design/data-flow.md` (named args, UFCS, nested-call desugaring, flat-calls invariant)
- `design/ir-schema.md` (Call, ResolvedCall)
- `design/language-surface.md` (block decls, call syntax, `description:` sub-section)
- `design/ir-and-semantics.md` §`description:` Section (block description field shape)
- `design/expand.md` §Step 1 (projection assignment)

### Slice 8 — Return folding
- `design/data-flow.md` §Return
- `design/language-surface.md` (`return` syntax)
- `design/compiled-output.md` (return-folding rules)
- `design/ir-schema.md` (Return node)
- `design/expand.md` §Step 1 (return folding)

### Slice 9 — Branching
- `design/language-surface.md` (`if`/`elif`/`else` grammar)
- `design/compiled-output.md` §Conditional Projection (lettered sub-steps), §Description-Driven Branch Projection (pure-applies arms)
- `design/ir-schema.md` (Branch node, `applies_descriptions` side-map)
- `design/ir-and-semantics.md` §Block Trigger Predicate (`.applies()` semantics + 4 diagnostics)
- `design/expand.md` §Step 1 (conditional projection)
- `design/mvp-acceptance.md` §2.1 (the `==` clarification)

### Slice 10 — `with` modifier
- `design/data-flow.md` §`with`
- `design/expand.md` §Step 2 (only to know what NOT to apply in Step 1)
- `design/ir-schema.md` (`site_modifier` on Call)
- `design/language-surface.md` (`with` syntax)

### Slice 11 — Imports (single-file)
- `design/imports.md` (full)
- `design/language-surface.md` (import syntax)
- `design/data-flow.md` §Closure Across Imports

### Slice 12 — Multi-file orchestration
- `design/imports.md` (full, esp. §multi-file order)
- `design/pipeline.md` §Multi-File Compilation Order, §Partial Failure Policy (already in universal but re-read these sections)
- `design/cli.md` §`glyph compile` (directory mode)

### Slice 13 — Library files + closure
- `design/data-flow.md` §Closure (full)
- `design/language-surface.md` §File-Level Rules, export syntax
- `design/imports.md` (export visibility)
- `design/compiled-output.md` (library emission rules)
- `design/ir-schema.md` (ExportBlock, etc.)

### Slice 14 — Tier 2 same-file procedure
- `design/compiled-output.md` §Three-Tier Block Projection (central)
- `design/expand.md` §Step 1 (tier assignment)
- `design/ir-schema.md` (ResolvedCall, projection_mode)

### Slice 15 — Tier 3 external-file procedure
- `design/compiled-output.md` §Three-Tier Block Projection (central)
- `design/pipeline.md` §Phase 7 (procedure file emission, atomic rename) — already universal
- `design/expand.md` §Step 1
- `design/ir-schema.md`

### Slice 16 — Atomic emission + `.tmp` cleanup
- `design/pipeline.md` §Phase 7 (atomic rename + startup `.tmp` sweep — already universal; focus there)

### Slice 17 — `--emit-ir` and IR JSON
- `design/ir-json-schema.md` (the JSON shape — central; includes `applies_descriptions`, `local_refs`, optional `description`)
- `design/ir-schema.md` (resolved IR structure, LocalRef, Block/ExportBlock description field)
- `design/expand.md` §Step 1 (which fields populate the resolved IR)
- `design/cli.md` §`compile` flags

### Slice 18 — `--strict` mode
- `design/cli.md` (`--strict` flag)
- `design/mvp-acceptance.md` §5 Bar 4

### Slice 19 — `glyph fmt`
- `design/cli.md` §`glyph fmt` (full)
- `design/pipeline.md` §Phase 3a (already universal; focus there)
- `design/ir-and-semantics.md` §Body-Level Constraint Normalization
- `design/imports.md` §6 (dup merge), §7 (unused removal)

### Slice 20 — `glyph validate-output`
- `design/expand.md` §4 (25 structural diagnostics — central, includes `unresolved-local-ref`)
- `design/agent-skill.md` §`glyph validate-output`
- `design/cli.md` §`glyph validate-output`
- `design/ir-json-schema.md` (input format, including `local_refs` + `applies_descriptions`)
- `design/compiled-output.md` (what's being validated against, including §Description-Driven Branch Projection)

### Slice 21 — Stdlib
- `design/stdlib.md` (full)
- `design/imports.md` (stdlib resolution path)
- `design/ir-and-semantics.md` §3 (effect signatures)

### Slice 22 — Multi-file acceptance project
- `design/mvp-acceptance.md` §3 (the 5-skill project — central)
- This is integration-only; rely on previously-implemented slices

### Slice 23 — Diagnostic coverage backfill
- `design/mvp-acceptance.md` §4 (full catalog — central)
- `design/diagnostics.md` (Diagnostic shape)

### Files no slice needs to read directly (handle with care)

These are reference/design-rationale docs. They inform the spec but implementers shouldn't need them:

- `design/foundations.md` — design principles (already baked into other docs)
- `design/repair.md` — Phase 3a (deterministic rewrites) is compiler-side but its rules are already in `pipeline.md` §Phase 3a + `cli.md` §`glyph fmt` + `imports.md` §6–§7; Phase 3b (LLM repair) and §3c (constraint conflict scan) are agent-side. No slice needs `repair.md` directly.
- `design/agent-skill.md` — agent-side workflow (except slice 20 which references its `validate-output` section)
- `design/types.md` — MVP type-checking is minimal; only nominal matching at boundaries (already in `pipeline.md` §Phase 5)
- `design/preferences.md` — global preferences are ordinary export decls
- `design/values-and-names.md` — name-resolution rules (covered in `pipeline.md` §Phase 2 + `imports.md`)
- `design/todo.md` — explicitly post-MVP

---

## Open questions for review

1. **Granularity of effects (Slice 6) vs. walking skeleton (Slice 1).** The skeleton has `effects: reads_files, writes_files` in frontmatter. Slice 1 stubs this as "echo declared keywords." Slice 6 adds inference + propagation + the over/under-declared diagnostics. Acceptable, or fold the basic emission into #6 and drop it from #1?
2. **Closure check (Slice 13).** Bundled with library files since closure is for `export block`. Could be its own slice if it grows. Keep bundled?
3. **`--emit-ir` (Slice 17) sequencing.** Final shape needs every IR feature, but a v0 (just skeleton's IR) could land right after #1 and grow with each feature. Options: (a) one slice that lands fully right before #20; (b) thin slice early plus extension as features ship. Preference?
4. **HITL slices.** Everything is marked AFK because the spec is highly prescriptive. Any of these you want to flag for explicit checkpoint review (e.g., #17 IR JSON shape, #14/#15 projection-tier behaviour)?
5. **Should multi-file orchestration (#12) split into "DAG + serial compile" and "partial-failure policy + skip-dependents"?** They're independently demoable.
