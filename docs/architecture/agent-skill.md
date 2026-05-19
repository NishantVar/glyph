# Agent Skill Design

This document specifies the **agent skill** — a Markdown skill file that any coding agent (Claude Code, Copilot, Cursor, etc.) loads to orchestrate the Glyph compiler's LLM-dependent phases. The skill is LLM-agnostic: it contains workflow instructions and domain knowledge; the agent's own LLM handles all generation.

The compiler is a deterministic CLI (`glyph`). The agent skill tells the coding agent *when* to invoke the compiler, *how* to interpret its output, and *what* to do in the LLM phases (Phase 3b/3c repair, Phase 6 Step 2 expansion).

## Architectural Boundary

| Responsibility | Owner |
|---|---|
| Phases 1, 2, 4, 5, 6-Step1, 7 | Compiler (deterministic) |
| Phase 3a (`glyph fmt`) | Compiler (deterministic) |
| Phase 3b (repair generation) | Agent (LLM, guided by skill) |
| Phase 3c (constraint conflict scan) | Agent (LLM, guided by skill) |
| Phase 6 Step 2 (prose reshaping) | Agent (LLM, guided by skill) |
| Phase 6b (structural validation) | Compiler (`glyph validate-output`, deterministic) |

The compiler never calls an LLM. The agent never runs deterministic compilation logic. All deterministic validation lives in the compiler.

## Workflow State Machine

The skill encodes this state machine. The agent follows it top-to-bottom.

**`glyph fmt` runs as the first step of the Phase 3 repair loop when the compiler exits with code 2.** It handles all deterministic auto-fixes (§Phase 3a). If `glyph fmt` changes the file, the agent re-invokes `glyph compile` before deciding whether to proceed to Phase 3b (LLM repair). This ensures that no LLM call is wasted on a diagnostic that a deterministic rewrite can solve.

## Phase 3a — Deterministic auto-fixes (`glyph fmt`)

When `glyph compile` exits 2 (repairable), the agent first runs `glyph fmt <path>`. It performs these deterministic source rewrites without any LLM call:

- Tab → 4-space, mixed-indentation fix (`G::parse::tab-indent`, `G::parse::mixed-indent`)
- Legacy `-> None` strip (`G::parse::none-as-return-type`)
- Constraint hoisting, context hoisting, canonical sub-section reorder
- Duplicate sub-section merge (#109, `G::parse::duplicate-subsection`)
- Duplicate import collapse (#107, `G::analyze::duplicate-import`)
- Unused import removal (#108, `G::analyze::unused-import`)
- Stdlib auto-import (#110, `G::analyze::stdlib-missing-import`)
- Const-in-flow parens-add (#111, `G::analyze::const-in-flow`)
- Effects auto-insert (#112, `G::analyze::missing-effects`, gated on `--enable-effects`)
- Placeholder return rewrite (#113, `G::analyze::placeholder-string-return`)

All fixes are idempotent and comment-preserving. After fmt, if the file was changed, the agent re-runs `glyph compile`. If the exit code is still 2, the agent enters Phase 3b (LLM repair pass) — which now handles only the remaining semantic repairs that require generation (undefined names, undefined calls, ambiguous roles, missing-return, missing-description, applies-on-undescribed-block, nested-branch).

```
                    ┌──────────────────────────────┐
                    │  glyph compile <path>         │
             ┌──────│  --format json --emit-ir      │
             │      └──────────────┬───────────────┘
             │                     │
        exit 1                exit 2                    exit 0
        (hard errors)         (repairable)              (success)
             │                     │                        │
             ▼                     ▼                        ▼
        Surface errors       ┌──────────┐           Phase 3c: Constraint
        to author. STOP.     │ iter < 3? │           conflict scan (agent
                             └────┬─────┘           LLM, per declaration
                              yes │  no             with ≥2 constraints)
                                  │   │                     │
                                  │   ▼              contradiction? ──► STOP
                                  │  STOP (hard             │ no
                                  │  fail)                  ▼
                                  │                  Read foo.ir.json
                                  ▼                  Read foo.md
                         ┌──────────────────┐               │
                         │  glyph fmt <path> │               ▼
                         └────────┬─────────┘        Step 2: Reshape
                                  │                  foo.md using IR
                         ┌────────▼─────────┐        (agent LLM work)
                         │ changed? ────────┼──yes──┐       │
                         └────────┬─────────┘       │       ▼
                                  no                │  glyph validate-output
                                  │                 │   foo.ir.json foo.md
                                  ▼                 │       │          │
                           Agent fixes source       │  exit 0      exit 1
                           using diagnostics        │  (pass)    (fail, retry < 2)
                           (Phase 3b: LLM)          │     │          │
                                  │                 │     ▼          ▼
                                  │                 │    DONE    Revise foo.md
                                  │                 │            using diagnostics,
                                  │                 │            retry Step 2
                                  └──► loop back ◄──┘
                                       to compile
```

### Exit Code Contract

| Exit code | Meaning | Agent action |
|---|---|---|
| `0` | Success. `foo.md` and `foo.ir.json` written. | Proceed to Step 2. |
| `1` | Hard errors. Cannot compile. | Surface diagnostics to user. Stop. |
| `2` | Repairable diagnostics only. Pipeline stopped after Phase 2. | Read JSON diagnostics from stdout, fix source, re-invoke. |
| `3` | Invocation error (bad flags, missing path, IO failure). | Surface error to user. Stop. |

### Iteration Budgets

| Phase | Max iterations | On exhaustion |
|---|---|---|
| Repair loop (3b) | 3 (per file) | Hard fail. Surface residual diagnostics to user. |
| Phase 3c retry (malformed LLM output) | 2 | Emit `G::repair::constraint-scan-malformed`. Hard fail. |
| Step 2 retry (6b validation failure) | 2 | Hard fail. Surface 6b diagnostics to user. |
| Transient failure retry (network/5xx) | 3 | Emit `llm-unavailable` diagnostic. Hard fail. |

**Repair iteration accounting is per-file and owned by the agent.** The compiler is stateless across invocations: each `glyph compile` invocation re-parses every file and emits per-file diagnostics, but the agent maintains the iteration counter for each file. The counter only increments for files that emit `repairable` diagnostics in that invocation; a file that emits zero diagnostics is "done" and is skipped on subsequent LLM repair passes even though the compiler still re-processes it. The 3-iteration hard-fail limit is therefore per-file, not per-build: if file A converges on iteration 1 and file B needs iteration 3, the build still succeeds; only file B's hypothetical iteration 4 would hard-fail.

## Phase 3b: Repair Guidance

When `glyph compile` exits with code 2, stdout contains NDJSON — one JSON object per file per line, each with a `diagnostics` array. The agent reads these and edits the source `.glyph` file to fix them, then re-invokes the compiler.

The agent receives diagnostics in this shape (via `--format json`):

```json
{
  "file": "path/to/foo.glyph",
  "diagnostics": [
    {
      "id": "G::analyze::undefined-name",
      "classification": "repairable",
      "message": "bare name 'preserve_existing_patterns' does not resolve",
      "span": { "file": "foo.glyph", "start": {"line": 3, "col": 5}, "end": {"line": 3, "col": 33} },
      "hints": ["Add a 'const' or 'generated const' declaration for this name."]
    }
  ]
}
```

In multi-file builds, `--format json` emits **NDJSON**: one complete `{"file": ..., "diagnostics": [...], "emitted": [...]}` JSON object per line, no top-level array wrapper, line-buffered. Files appear in topological compile order — the order the compiler processes them. The agent reads stdout line by line and dispatches each file's diagnostics as soon as that line arrives, without waiting for the whole build to finish.

### Repair Patterns by Diagnostic ID

Each repairable diagnostic has a specific fix pattern. The agent applies these to the source file.

**Naming and placement rule:** All `generated const` and `generated block` declarations go **after** all non-generated top-level declarations in the file. If an author later writes a same-named declaration, the generated one is superseded and should be deleted.

**No-overwrite rule.** Repair never silently overwrites, deletes, or renames an existing declaration — generated or otherwise — to make room for a generated one. If the LLM proposes a `generated const` or `generated block` whose name collides with any existing top-level declaration in the file, the compile hard-fails with `G::analyze::name-collision`. The author resolves the collision manually: rename one of the conflicting declarations, or explicitly delete the stale `generated` declaration themselves. Repair is also forbidden from mutating any existing declaration with the conflicting name.

**Formatting hygiene for repair output.** The agent's repair output should aim for clean form: 4-space indentation only (no tabs), `generated const` and `generated block` declarations appended after all non-generated top-level declarations, no double blank lines. `glyph fmt` is re-invoked at the start of every repair iteration (see §Workflow State Machine), so it will normalize whitespace, deduplicate imports, insert missing stdlib imports, and apply the other deterministic auto-fixes on the LLM's output before the next compile. The LLM does not need to be perfect, but should not rely on fmt to fix semantic mistakes.

#### Parse-phase repairables

| Diagnostic ID | Fix |
|---|---|
| `G::parse::operator-in-expression` | Glyph has no value-level operators in MVP. Rewrite the expression as a plain call or inline string. E.g., `x + y` → `combine(x, y)` or an inline instruction string. |
| `G::parse::param-slot-in-non-instruction-string` | `{name}` slots are only valid in instruction-bearing positions (flow statements, constraint text). Move the slot to an instruction string or remove it. |
| `G::parse::duplicate-subsection` | Phase 3a handles this deterministically — no LLM action needed. The compiler's deterministic merge ([[docs/architecture/repair]] §4.4) splices the duplicate body and its comment trivia into the first occurrence and removes the duplicate header. If this diagnostic appears in the 3b residual set, it is a compiler bug; Analyze should have surfaced `G::analyze::unmerged-duplicate-subsection` (error) instead. |

#### Analyze-phase repairables

| Diagnostic ID | Fix |
|---|---|
| `G::analyze::undefined-name` | Add a `generated const <name> = "<single-string content>"` declaration at the bottom of the file (after all non-generated declarations). Infer the content from the name and its usage context in the flow. |
| `G::analyze::undefined-call` | Add a `generated block <name>(<inferred-params>)` with a single-string body (the `flow:`-omitted shorthand per [[language-surface]] §3.2). Infer parameter names from the call arguments. The body should be a single instruction string describing what the block does. Place after all non-generated declarations. |
| `G::analyze::ambiguous-role` | Add an explicit role marker. If the statement is meant as a constraint, prefix with `require` or `avoid`. If it's meant as a step, ensure it's an instruction string or call. |
| `G::analyze::missing-return` | Add a `return` statement as the last line of the `flow:` body. Infer the return expression from the block's purpose. |
| `G::analyze::nested-branch` | Extract the inner branch into a `generated block` declaration. Replace the inner branch with a call to the new block. The generated block's body should be a single instruction string summarizing the extracted branch logic. |
| `G::analyze::missing-description` | Add a `description:` sub-section to the `skill` declaration with a single-string summary of when and why to use this skill. Infer the description from the skill name, parameters, effects, constraints, and flow body. The description should focus on the skill's trigger condition (when an agent should select it), not its implementation steps. |
| `G::analyze::applies-on-undescribed-block` | Add a `description:` sub-section to the **block** named in the diagnostic, with a single-string summary of **when this block applies** — i.e. the user-intent or runtime condition under which the calling `if`/`elif` arm should fire. Infer the trigger from the block name, the body of the arm that uses `BLOCKNAME.applies()`, and any sibling arms. Phrase as a condition (e.g. "When the user asks to fork a terminal pre-loaded with a plan."), not as an implementation summary. Repairable only when the block is defined in the same file under compilation; if the block is imported, this diagnostic is an error and the author must edit the source library directly. |

### Repair Principles

- **Fix all diagnostics in one pass.** Apply all fixes to the source file before re-invoking the compiler. Don't fix one at a time.
- **Preserve author intent.** Don't rename things, reorder unrelated code, or add features. Fix only what the diagnostics flag.
- **Generated content is minimal.** `generated const` bodies are one sentence. `generated block` bodies are one instruction string. Don't over-elaborate.
- **Infer from context.** When generating content for `undefined-name` or `undefined-call`, read the name itself and its usage in the surrounding flow to write a reasonable single-string body. E.g., `preserve_existing_patterns` → `"Follow the repository's existing patterns before introducing new abstractions."`
- **Don't export generated declarations.** `export generated const` and `export generated block` are invalid syntax.

## Phase 3c: Constraint Conflict Scan

After the repair loop converges (exit 0) but **before** Step 2, the agent scans for constraint conflicts within each declaration that has 2 or more constraints.

For each such declaration, the agent analyzes the constraint set and produces a structured assessment:

**Input:** The constraints from the declaration, as text with their strength and polarity.

**Output:** For each pair of constraints, classify as:
- `contradiction` — the two constraints are mutually exclusive (e.g., "always use verbose logging" + "minimize all output"). This is a hard error: compilation fails, the author must edit.
- `tension` — the constraints pull in different directions but can coexist with judgment (e.g., "be thorough" + "be concise"). This is a warning: build proceeds, both constraints survive.
- `none` — no conflict.

**Scope rule:** Only scan constraints within the same declaration. Cross-scope constraints (a skill's constraints vs. a called block's constraints) are intentional composition and not scanned.

If the agent's conflict assessment is malformed (can't parse its own output as the expected structure), retry up to 2 times. After 2 failures, hard fail with `G::repair::constraint-scan-malformed`.

## Phase 6 Step 2: Prose Reshaping

After `glyph compile` exits 0, the compiler has written:
- `foo.md` — Markdown produced by the deterministic emitter (Phase 7 output). Frontmatter is final. Section structure, list numbering, constraint rendering (the locked four-form template), pure-`applies()` Branch projection, the external-file Call Step template, and the `Identifier`-form return-fold suffix are final. `## Parameters` contains the parameter list with name, optional type, default/required marker, and an effective description (inline `<"…">` wins over a type-registry entry); a `ParamDescription` span is emitted only for parameters with no effective description, and the LLM expand pass is required to fill it (the stub filler hard-fails otherwise). Where the agent is responsible for prose, the deterministic emitter has marked typed spans with `SpanKind` ∈ `{ParamDescription, DescriptionReturnFold, BranchCondition, CallBodyShape}` (see [[docs/architecture/expand]] §3.5).
- `foo.ir.json` — the full resolved IR (post-Step-1) as JSON.

The agent's job is **scoped to filling spans** — not regenerating Markdown. The agent rewrites span content **in place** on `foo.md`, producing human-quality prose for the LLM-owned slots while preserving every literal chunk emitted by the deterministic emitter. The full per-span contract is enumerated in [[llm_expand_pass]]. The frontmatter, section headers, list numbering, and the locked-template wording (constraints, return-fold suffixes, external-file Step, pure-`applies()` Branch headers) are **not touched** — they are deterministic. For `## Parameters`, author-supplied and type-registry descriptions are deterministic literals; the agent fills a `ParamDescription` span only when a parameter has no effective description, deriving prose from the parameter's name, type, usage context, and default value (if any). It must not add, remove, or rename parameters (the parameter list skeleton is compiler-owned).

### What the agent rewrites

The agent reads `foo.ir.json` and rewrites the body sections (`## Context`, `## Steps`, `## Constraints`, and any `### Procedure: <name>` sub-sections) to:

1. **Expand Call nodes into natural prose.** A Call like `inspect_failure(scope)` with resolved body "Inspect the failure in {area}" becomes a Step like "Inspect the failure in {scope}, focusing on auth boundaries and permission checks."

2. **Apply `with` modifiers.** The `site_modifier` field on Call nodes contains emphasis text. Weave it into the Step prose naturally. The modifier string must **not** appear verbatim in the output — it shapes the wording, it doesn't get quoted.

3. **Constraints are deterministic — not the agent's job.** The locked four-form template ([[docs/reference/compiled-output]] §Constraint Rendering, mirrored in [[GLYPH_LANGUAGE_GUIDE]] §7.2 canonical form) is rendered by the compiler. The agent does **not** reword constraints, regenerate the `## Constraints` section, or paraphrase strength/polarity wording.

4. **Project mixed-condition Branch arm headers (`BranchCondition` span only).** Pure-`applies()` Branches and the `Otherwise:` arm header are emitted deterministically per [[docs/architecture/expand]] §3.3. The agent fills only the headers for arms whose condition is a code-shaped expression that mixes `applies()` calls with other operators — e.g., `block_x.applies() and not is_dry_run` → `If the user wants a structured plan and this is not a dry run:`. Letters reset per arm (`a.`, `b.`, `c.`) and are emitted by the deterministic emitter.

5. **Fold `Description`-form returns (`DescriptionReturnFold` span).** When the `OutputContract.form` is `Description("…")`, the agent paraphrases the description into a Step-shaped sentence inside the locked Description-suffix wrapper. The `Identifier` form (`return <name>`) is folded deterministically — the agent does not touch it.

6. **Render procedure references.** For `same_file_procedure` projection Call nodes, the Step prose says "(follow the <name> procedure below)" and the `### Procedure: <name>` section contains the callee's expanded flow. For `external_file` projection, the Step prose is the locked template `Load and follow the procedure in \`{procedure_path}\`.` — emitted deterministically; the agent does not touch it.

7. **Preserve `{param}` references exactly.** Parameter slots like `{scope}` pass through unchanged. Don't invent new ones. Don't drop existing ones.

8. **Resolve `local_ref` slots into prose.** Local binding references like `{diagnosis}` (where `diagnosis` is from an assignment, not a declared parameter) must be resolved into natural-language cross-references — e.g., "the diagnosis from your earlier analysis." They must **not** survive as literal `{name}` tokens in the output.

### What the agent does NOT do

- Don't touch the frontmatter (name, description, effects).
- Don't add, remove, or rename parameters in `## Parameters` — only generate their descriptions.
- Don't add sections beyond `## Context`, `## Steps`, `## Constraints`, and any `### Procedure: <name>` nested under the last body H2.
- Don't add code blocks, tables, or HTML to the instructions.
- Don't exceed 3 sentences per Step (non-conditional) or per sub-step.
- Don't exceed 1 sentence per Constraint.
- Don't return YAML frontmatter as part of Step 2 output.

### Retry semantics on `validate-output` failure

When `glyph validate-output` exits 1, the agent retries Step 2 (budget = 2 per §Iteration Budgets). Retries use **revise-with-feedback**: each retry reads the previous attempt's `foo.md` together with the structural diagnostics from `validate-output`, and the agent's prompt asks the LLM to fix the specific violations rather than regenerating from scratch off the mechanical compiler output.

After exhaustion (2 failed retries), the **last failed `foo.md` is left on disk** and the `validate-output` diagnostics are surfaced to the user. The agent does not silently revert to the mechanical compiler output — the user needs to see the failed prose to diagnose the persistent structural mismatch.

### Nodes that skip Step 2

If a flow node is already complete prose — an `InlineInstruction` (literal string from source) or a resolved `InstructionRef` (text reference) — it passes through as-is. Only `Call` nodes with resolved bodies, `Branch` containers, `Return` nodes, and `Constraint` nodes need LLM reshaping.

## `glyph validate-output` — Phase 6b

A compiler subcommand that deterministically validates Step 2 output against the IR.

### Invocation

```
glyph validate-output <ir-json-path> <md-path> [--format pretty|json]
```

- **Inputs:** `foo.ir.json` (resolved IR from `--emit-ir`) + `foo.md` (agent-rewritten Markdown).
- **Exit 0:** Validation passed. `foo.md` is structurally correct.
- **Exit 1:** Structural violations found. Diagnostics on stderr (pretty) or stdout (JSON).
- **Exit 3:** Invocation error (missing file, bad path, IO failure).

### Validation Check Categories

All checks are deterministic. The validator parses the Markdown structurally (heading extraction, list-item counting) and cross-references against the IR JSON. The authoritative catalog of `G::expand::*` IDs, classifications, and exact check semantics lives in [[docs/reference/diagnostics]]. From the agent's perspective the checks fall into four buckets:

- **Section shape** — only canonical peer-level H2s (`## Parameters`, `## Context`, `## Steps`, `## Constraints`) and the nested `### Procedure: <name>` H3s may appear.
- **Role preservation (1-to-1 count matching against IR)** — Step / sub-step / Constraint counts and order match the IR; positional ordering is checked.
- **Procedure sections** — procedure count, name, step count, reference linkage, ordering, and uniqueness match the IR's `same_file_procedure` Call set.
- **Parameter references** — every `{name}` slot in the output resolves to an IR parameter; no IR parameter is silently dropped; local-binding refs are resolved into prose and do not survive as `{name}` tokens.
- **Content shape** — body sections do not begin with YAML, `with` modifier strings never leak verbatim, and the output parses as valid structural Markdown.

### Counting Rules

**Step counting under nested branches.** A top-level `Branch` contributes exactly **1** to the top-level Step count. Inside an arm, the sub-step count equals the number of direct Step-projecting children of that arm; a `Branch` nested inside another `Branch`'s arm counts as **1 sub-step** and does **not** expand into n sub-steps per its own arms — recursion stops at the first nesting level. In practice, [[docs/architecture/repair]] §4.1 auto-extracts nested branches into `generated block` declarations before Phase 6b runs, so the validator typically sees a `Call` to the extracted block rather than a literal nested `Branch`; this counting rule is the defensive fallback for cases where extraction did not run. The step-count formula in [[docs/architecture/expand]] (`(Step nodes) + (Branch nodes × 1) − (Return folds)`) is consistent with this rule.

### Implementation Notes

The Markdown parser for `validate-output` is minimal: line-by-line heading extraction (`##` / `###`), numbered-list-item counting (`1.`, `2.`, ...), bulleted-list-item counting (`- `), and `{name}` token scanning via regex. No full CommonMark parser required. Estimated ~300 LOC in `glyph-core`.

## IR JSON Schema

The `--emit-ir` flag causes `glyph compile` to write `foo.ir.json` alongside `foo.md`. This is the **post-Step-1 resolved IR** — bare names inlined, projection tiers assigned, parameter slots preserved, `with` modifiers attached to Call nodes.

**The canonical IR JSON schema lives in [`../reference/ir-json.md`](../reference/ir-json.md).** That document specifies the top-level envelope, per-node-kind JSON shapes, enum serialization (all snake_case), Expression/Value unions, versioning policy (`ir_version` as monotonic integer, independent of compiler version), and a complete worked example. Both `--emit-ir` and `validate-output` use this schema as their contract.

Key points for the agent:

- **Envelope:** `{"ir_version": 2, "compiler": "glyph 0.1.0", "source_file": "...", "skill": {...}}`.
- **All enums are lowercase snake_case.** Role values are `"input_contract"`, `"step"`, `"constraint"`, `"context"`, `"output_contract"`. TypeTag built-ins are `"string"`, `"int"`, etc. Domain types are `{"domain_type": "<name>"}`.
- **Every node carries `node_id`** (string, e.g. `"n0"`), including Param and Expr sub-nodes.
- **Expression and Value unions use a `kind` discriminator.** See `../reference/ir-json.md` §Expression Union and §Value Union.
- **Version check:** If `ir_version > KNOWN_MAX`, warn and attempt to proceed. Ignore unknown fields.

## What the Skill File Looks Like

The agent skill is a plain Markdown file (e.g., `glyph-compile.skill.md`) that a coding agent loads. It encodes the workflow state machine, repair patterns, and Step 2 rules from this document as agent instructions.

The skill is **not** a `.glyph` file. It does not compile itself.

The skill references the `glyph` CLI binary and expects it to be on `PATH`. It does not import any libraries, call any APIs, or depend on any specific LLM provider.

### Shipping Location

The agent skill ships inside the `glyph` repo at a known path (e.g., `glyph-cli/agent/glyph.skill.md`). Users copy the file into their coding agent's skill directory.

Deferred work (installer subcommand, dogfooding the skill as `.glyph`) is tracked in [[agent-skill-todos]].

## Interactions With Other Docs

- **[[docs/reference/cli]]** — exit code 3 is reserved for invocation errors (previously overloaded on exit 2). `validate-output` subcommand defined there.
- **[[docs/reference/diagnostics]]** — `G::expand::*` diagnostic IDs are compiler-scope (implemented in `validate-output`), not agent-scope.
- **[[ir-schema]]** — JSON serialization shapes defined here are the `serde_json` projection of the Rust IR types from [[ir-schema]].
- **[[ir-json]]** — canonical IR JSON serialization contract; produced by `--emit-ir` and consumed by `validate-output`.
- **[[docs/architecture/repair]]** — deterministic auto-fix mechanics (`glyph fmt`) for Phase 3a; LLM repair patterns extend that contract here.
- **[[docs/architecture/expand]]** — span model and Phase 6b validator implementation; this doc defines what the agent fills.
- **[[design/compiled-output]]** — constraint wording exemplars in §Step 2 are the authoritative patterns matching the locked four-form template.
