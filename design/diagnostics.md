# Glyph Diagnostics

This document defines the shape of structured diagnostics emitted by the Glyph compiler. It is the contract that `repair.md` §3 references as "structured diagnostics from earlier deterministic passes" and that `pipeline.md` Phase 2 (Analyze) produces.

## Diagnostic Shape

Every diagnostic the compiler emits is a structured record:

```text
Diagnostic {
  id:             String          // stable identifier, never changes across compiler versions
  classification: Classification  // see Classification below
  message:        String          // human-readable summary, one sentence
  span:           SourceSpan      // primary location in source
  related:        SourceSpan[]?   // other locations that contribute (e.g. the other side of a name collision)
  hints:          String[]?       // actionable suggestions for the author
}

SourceSpan {
  file:  String          // file path
  start: { line, col }   // 1-based
  end:   { line, col }   // 1-based, inclusive
}
```

**Span semantics.**

- **1-indexed** for both line and column. Matches what authors see in editors and CLI output.
- **Inclusive end.** A single-character span has `start == end`. A span over `foo` on line 3 starting at column 5 is `start: {3, 5}, end: {3, 7}`.
- **Multi-line spans are legal.** A span over a multi-line construct (e.g., an unterminated `"""` block string, a malformed multi-line declaration header) sets `end.line > start.line`. The `end.col` value applies to its own line.
- **Columns are unambiguous.** Tabs are a compile error (`language-surface.md` §2.2), so column = number of characters from start of line. No tab-width interpretation is needed.
- **Synthetic-diagnostic fallback.** Some diagnostics arise from phases that operate post-Lower or post-Repair, where the precise authored location may not survive. In those cases the span falls back, in order, to: (1) the closest authored construct's span (e.g., the parameter or call that lowered to the offending IR node), (2) the enclosing declaration's header span, (3) `start: {1, 1}, end: {1, 1}` with `file:` set. Phase 1–5 invariants guarantee enough provenance to reach option (1) or (2) for every diagnostic emitted in MVP; option (3) is reserved for diagnostics whose provenance is genuinely the file as a whole (e.g., `G::analyze::no-exports-in-library`).

## Classification

Diagnostics use the three-tier classification from `pipeline.md` Phase 2:

| Classification | Meaning | Compilation continues? |
|---|---|---|
| `error` | Hard stop, no repair possible | No |
| `repairable` | Phase 3 (Repair) can likely fix this | Paused until repair attempt |
| `warning` | Non-blocking observation | Yes |

Phase 3 internally distinguishes deterministic auto-fixes (3a) from LLM-assisted fixes (3b). That distinction is an implementation detail of the repair loop, not part of the diagnostic shape. See `todo.md` (Diagnostics section) for a possible future formalization.

## ID Scheme

Diagnostic IDs are namespaced by canonical pipeline phase, human-readable, and stable:

```
G::<phase>::<name>
```

- `G::parse::*` — Phase 1 (Parse)
- `G::analyze::*` — Phase 2 (Analyze)
- `G::imports::*` — Phase 2 import resolution (subset of Analyze with its own namespace because import resolution is logically distinct from name/role/effect analysis)
- `G::repair::*` — Phase 3 notifications (e.g. "generated a definition") and Phase 3 execution failures (e.g. LLM unavailable)
- `G::validate::*` — Phase 5 (Validate)
- `G::expand::*` — Phase 6 (Expand): Step 2 execution failures (agent-scope) and Phase 6b structural validation (compiler-scope, implemented in `glyph validate-output`)
- `G::build::*` — project-level build orchestration (multi-file compilation order, partial-failure handling); not tied to a specific phase

Once an ID exists it is never renamed or reassigned. It can be deprecated and replaced by a new ID, but the old one keeps its meaning.

The `::` separator avoids collision with `.` (module access) and `/` (file paths).

## Catalog Completeness Rule

The catalog below is representative, not exhaustive. It grows as the compiler is implemented. The completeness meta-rule is:

1. **Every check in Phases 1–5 and 6b that can fail MUST have exactly one diagnostic ID.** No check may emit an unstructured error string, a bare exception, or a generic fallback diagnostic.
2. **The ID follows the `G::<phase>::<descriptive-name>` convention** defined above.
3. **Every diagnostic has the full shape** defined in §Diagnostic Shape (id, classification, message, span, optional related spans, optional hints).
4. **Classification is deterministic per ID.** Each diagnostic ID maps to exactly one of `error`/`repairable`/`warning`. The same ID never changes classification based on context.
5. **Implementers add IDs as checks are implemented**, following this convention. The representative catalog below serves as guidance and a naming reference.

## Examples

Representative diagnostics implied by the current design.

### Parse phase

| ID | Classification | Trigger |
|---|---|---|
| `G::parse::tab-indent` | repairable | Tabs used instead of 4-space indentation (`language-surface.md` §2.2) |
| `G::parse::mixed-indent` | repairable | Tabs and spaces on the same line (`language-surface.md` §2.2) |
| `G::parse::nested-flow` | error | `flow:` inside `flow:` (`data-flow.md`) |
| `G::parse::effects-disabled` | error | `effects:` sub-section used without `--enable-effects`. The effects subsystem is gated; remove the `effects:` line or pass `--enable-effects` (`ir-and-semantics.md` §3). |
| `G::parse::none-with-effects` | error | *(Gated — requires `--enable-effects`.)* `effects: none, reads_files` — `none` alongside other keywords (`ir-and-semantics.md` §3). Detectable in Parse because the token sequence is unambiguous, but semantically an error — `none` exclusivity is a hard rule, not repairable. |
| `G::parse::multiple-with` | error | Chained `with ... with ...` on a single call (`data-flow.md`) |
| `G::parse::with-on-bare-name` | error | `with` modifier on a non-call statement (`data-flow.md`) |
| `G::parse::operator-in-expression` | repairable | Operator token (`+`, `-`, `*`, `/`, etc.) in expression position; MVP has no value-level operators (`values-and-names.md`) |
| `G::parse::param-slot-in-non-instruction-string` | repairable | A `{name}` slot appears in a string position that is not instruction-bearing (e.g., parameter default value, `description:` field); braces are stripped and the content is treated as literal (`values-and-names.md`) |
| `G::parse::return-not-terminal` | error | `return` appears before the last statement of `flow:` (`data-flow.md` §Return Semantics) |
| `G::parse::return-in-branch` | error | `return` appears inside an `if`/`elif`/`else` body (`data-flow.md` §Return Semantics) |
| `G::parse::multiple-returns` | error | More than one `return` statement in a single `flow:` (`data-flow.md` §Return Semantics) |
| `G::parse::duplicate-subsection` | repairable | A sub-section header (e.g., `description:`) appears more than once in a single declaration body. Phase 3a deterministically merges the duplicates by splicing each later occurrence's body into the first occurrence (no LLM, no contradiction-check); the merge is purely textual concatenation with comment trivia preserved at the boundary (`repair.md` §4.11, `language-surface.md` §2.5). |
| `G::parse::empty-file` | error | The source file is empty or contains only whitespace and comments — no declarations to compile (`language-surface.md` §File-Level Rules) |
| `G::parse::empty-flow` | error | A `flow:` sub-section is present but its body contains zero statements; either remove the `flow:` header (for a constraint-only skill) or add at least one statement (`data-flow.md`) |
| `G::parse::multiple-skills` | error | A `.glyph` file contains more than one `skill` declaration; MVP requires exactly one skill per file because compiled output is named after the skill (`language-surface.md` §File-Level Rules) |
| `G::parse::applies-no-parens` | error | `BLOCKNAME.applies` appears without `()`; the trigger predicate form requires explicit parentheses (`ir-and-semantics.md` §Block Trigger Predicate) |
| `G::parse::applies-with-args` | error | `BLOCKNAME.applies(...)` is called with arguments; the trigger predicate is zero-arity (`ir-and-semantics.md` §Block Trigger Predicate) |
| `G::parse::none-as-return-type` | repairable | A declaration header uses `-> None` as a return-type annotation (e.g., `block foo() -> None`, `export block foo() -> None`). The `None` type annotation has been removed in MVP; declarations with no meaningful return omit `->` entirely (`types.md` §`none` Value, `language-surface.md` §3.3). Phase 3a (pre-Parse text-level rewrite, `glyph fmt` stratum 1) deterministically strips the trailing ` -> None` from `skill` / `block` / `export block` / `generated block` declaration headers. Match is case-insensitive on `none` with identifier-boundary semantics; the value keyword `none` (in `return none`, `effects: none`, value positions) is preserved. |
| `G::parse::malformed-output-target` | error | A `return <...>` output-target candidate matches neither valid form. Identifier-form failures: empty `<>`, whitespace inside brackets, dot access, call syntax, or any non-identifier character (`values-and-names.md` §No Value-Level Operators). Descriptive-form failures: empty `<"">` (the description string must be non-empty). The same diagnostic ID covers both; the message names the offending shape. **Known gap (MVP):** an unterminated descriptive form such as `<"…<EOF>` currently emits no structured diagnostic — the tokenizer's `UnterminatedString` path falls through silently rather than surfacing as a parse-error. The intended behavior is for the tokenizer to raise a generic `UnterminatedString` parse-error before this rule is reached; promoting that path to emit a structured diagnostic is tracked as a follow-up. The `descriptive_form_unterminated_produces_no_structured_diagnostic` integration test pins the current silent behavior as a regression fence. |
| `G::parse::output-target-outside-return` | error | An output-target literal — `<name>` (identifier form) or `<"…">` (descriptive form) — appears outside the single MVP-legal position: the terminal top-level `return …` expression. The same diagnostic ID covers both forms. |

### Analyze phase

| ID | Classification | Trigger |
|---|---|---|
| `G::analyze::undefined-name` | repairable | Bare name doesn't resolve to any declaration; repair generates a definition (`repair.md` §5) |
| `G::analyze::undefined-call` | repairable | Parens-call doesn't resolve; repair generates a `generated block` (`repair.md` §5) |
| `G::analyze::name-collision` | error | Two names collide after case normalization (`values-and-names.md`) |
| `G::analyze::import-private` | error | Tried to import a non-exported declaration (`imports.md` §2) |
| `G::analyze::import-skill` | error | Tried to selectively import a skill (`imports.md` §2) |
| `G::analyze::circular-import` | error | Import cycle detected (`imports.md` §5) |
| `G::analyze::missing-file` | error | Import path doesn't resolve to a file (`imports.md` §1) |
| `G::analyze::duplicate-import` | repairable | Same file imported twice; merged (`imports.md` §6) |
| `G::analyze::unused-import` | repairable | Imported name never referenced; removed (`imports.md` §7) |
| `G::analyze::ambiguous-role` | repairable | Can't determine instruction role from context (`ir-and-semantics.md` §2) |
| `G::analyze::effects-under-declared` | error | *(Gated — requires `--enable-effects`.)* Declared effects are a subset of inferred effects (`ir-and-semantics.md` §3) |
| `G::analyze::effects-over-declared` | warning | *(Gated — requires `--enable-effects`.)* Declared effects include keywords not inferred from the body; non-blocking, surfaced for author cleanup (`ir-and-semantics.md` §3) |
| `G::analyze::missing-effects` | repairable | *(Gated — requires `--enable-effects`.)* A declaration (skill, block, or export block) omits `effects:` entirely and the inferred set is non-empty; Phase 3a auto-adds the inferred effects (`ir-and-semantics.md` §3) |
| `G::analyze::nominal-mismatch` | error | Type name mismatch at a call boundary (`types.md`) |
| `G::analyze::generic-type-name` | warning | An identifier in return-type position (`-> ReturnType` on `skill`, `export block`, or private `block` declaration headers) is one of the 13 banned generic type names: `String`, `Int`, `Float`, `Bool`, `None`, `List`, `Set`, `Map`, `Array`, `Dict`, `Tuple`, `Object`, `Any`. Match is case-insensitive (ASCII). Non-blocking; compilation continues. Suggestion: replace the generic name with a domain type that carries semantic meaning (e.g., `BranchName`, `FilePath`, `Summary`) (`types.md` §Primitive Kinds (IR-Only)). **Precedence:** `-> None` in return-type position is intercepted earlier by `G::parse::none-as-return-type` (repairable, Phase 3a auto-fix); this diagnostic surfaces end-to-end for the other 12 banned names. The validator retains `None` in its banned list for defense in depth at any future call sites where parse interception does not apply. |
| `G::analyze::lossy-coercion` | error | Lossy numeric conversion, e.g. `3.7` where integer expected (`values-and-names.md`) |
| `G::analyze::missing-return` | repairable | Export block lacks `return` on a code path (`language-surface.md` §3.3) |
| `G::analyze::export-missing-return-type` | repairable | An `export block` body has at least one `return <expr>` with a meaningful return value but the header lacks a `-> DomainType` annotation. Export-block return types must be explicit when the block has a meaningful return and omitted otherwise (`language-surface.md` §3.3, `types.md` §`none` Value). Bare `return` and `return none` do not trigger this diagnostic — those are the no-meaningful-return form, and the header correctly omits `->`. Reparability is via Phase 3b (LLM-assisted inference of the `DomainType` name from the body); Phase 3a's deterministic strata cannot synthesize a domain-type name. |
| `G::analyze::output-target-shadows-binding` | error | `return <name>` uses an output target name that already resolves to a visible binding (parameter, const, block, export, or import depending on scope). The target must be a fresh output name, not a reference. |
| `G::analyze::placeholder-string-return` | repairable | A domain-typed declaration ends with a string literal whose entire content is an angle-bracketed placeholder such as `"<current_branch>"` or `"<root cause analysis including affected files and severity>"`. Phase 3a rewrites it to the appropriate output-target form, bifurcating on placeholder shape: identifier-shaped contents become identifier form (`return <current_branch>`), non-identifier-shaped contents (whitespace, punctuation, etc.) become descriptive form (`return <"root cause analysis including affected files and severity">`). Ordinary strings without `<…>` framing and untyped declarations are unaffected; empty `"<>"` is not repaired. See `repair.md` §7. |
| `G::analyze::closure-violation` | error | Export block depends on hidden caller context (`data-flow.md`) |
| `G::analyze::stdlib-missing-import` | repairable | `subagent()` used without importing `@glyph/std` (`stdlib.md`) |
| `G::imports::unknown-stdlib-module` | error | An import path under the reserved `@glyph/` virtual namespace does not resolve to a known compiler-embedded stdlib module. The MVP recognises only `@glyph/std`; any other `@glyph/*` path fires this diagnostic (`stdlib.md`, `imports.md`). |
| `G::analyze::unknown-param-slot` | error | A `{name}` slot in an instruction-bearing string does not resolve to a parameter or local binding in scope at the slot's source position (`values-and-names.md`) |
| `G::analyze::nested-branch` | repairable | A `Branch` appears inside another `Branch`'s arm body; Repair will auto-extract it into a `generated block` (`repair.md` §4.9) |
| `G::analyze::empty-skill-body` | error | A `skill` declaration has no `description:`, no `flow:`, no `constraints:`, no `effects:` — there is nothing to project. A skill must have at least one of `flow:` (with statements) or `constraints:` (with markers); a constraint-only skill is legal (`compiled-output.md`) |
| `G::analyze::no-exports-in-library` | error | A library file (zero `skill` declarations) has zero `export` declarations — it has no consumer-visible contribution. Add at least one `export block` or `export const` (`language-surface.md` §File-Level Rules) |
| `G::analyze::missing-required-arg` | error | A `call <name>(...)` flow statement omits a positional argument for a callee parameter that has no default. Applies uniformly to private `block`, same-file `export block`, and imported `export block` callees (PRD #103 / Issues #104, #105). The diagnostic span pins the offending callee identifier at the call site (not the enclosing skill header), so an IDE can highlight the broken call. Skill parameters are *not* subject to this rule — they are runtime-required inputs surfaced in `## Parameters`. |
| `G::analyze::missing-description` | repairable | A `skill` declaration omits `description:`; Repair generates one from the skill name and body and adds it as a `description:` sub-section in the source (`ir-and-semantics.md` §4, `compiled-output.md` §Frontmatter) |
| `G::analyze::const-in-flow` | repairable | A bare name (or undefined identifier) appears in `flow:` without a keyword prefix (`require`/`avoid`/`must`/`context`); `const` declarations are passive constants and are not legal as flow instruction steps. Repair adds parentheses and materializes a `generated block` (`language-surface.md` §3.4, `repair.md` §5) |
| `G::analyze::applies-on-non-block` | error | `NAME.applies()` was called where `NAME` resolves to something other than a `block` or `export block` declaration (e.g., a `const`, an `import` alias, a parameter). The trigger predicate is defined only on blocks (`ir-and-semantics.md` §Block Trigger Predicate) |
| `G::analyze::applies-on-undescribed-block` | repairable / error | `BLOCKNAME.applies()` is called on a block that lacks a `description:` sub-section. **Repairable** when the block is defined in the same file under compilation; Repair adds a trigger-shaped `description:` to the block. **Error** when the block is imported from another file; the author must edit the source library directly because Repair is single-file (`ir-and-semantics.md` §Block Trigger Predicate, `repair.md` §9) |
| `G::analyze::unmerged-duplicate-subsection` | error | A `Skill`, `Block`, or `ExportBlock` AST node still has a non-empty `extra_subsections` slot when Analyze runs — i.e., the parser recorded duplicate sub-sections (`G::parse::duplicate-subsection`) but Phase 3a's deterministic merge did not run or could not consume them (e.g., `--no-repair`, `glyph fmt --check`, or a 3a merge failure). Analyze emits this hard error so Lower never sees an inconsistent declaration shape; Lower treats `extra_subsections` non-empty as an unreachable invariant violation. The author must either re-run with Phase 3a enabled or fix the duplicates manually (`language-surface.md` §2.5, `repair.md` §4.11). |

### Validate phase

Phase 5 (Validate) is the final correctness gate before any LLM touches the IR. It checks both pre-Lower contracts (closure, effect propagation across imports) and post-Lower IR invariants. The IDs below cover the post-Lower invariant checks specific to Validate (see `pipeline.md` Phase 5).

| ID | Classification | Trigger |
|---|---|---|
| `G::validate::duplicate-node-id` | error | Two IR nodes within the same file share the same stable node ID assigned by Lower (`ir-schema.md` §Node Identifiers) |
| `G::validate::unresolved-callee` | error | A `Call` node's callee does not resolve post-Lower (after UFCS desugaring and branch extraction); the call graph cannot be closed (`pipeline.md` Phase 5) |
| `G::validate::malformed-branch` | error | A `Branch` IR node lacks the shape Phase 6b expects — missing `if` arm, or an arm body that is not well-formed (`pipeline.md` Phase 5) |
| `G::validate::recursive-call` | error | The local block-to-block call graph within a file contains a cycle; recursion is forbidden in MVP (`pipeline.md` Phase 5) |
| `G::validate::empty-step` | error | A Step-projecting IR node has empty body text; silently-empty Steps are rejected (`pipeline.md` Phase 5) |

### Build phase

Build-phase diagnostics cover project-level orchestration concerns rather than a specific compiler phase. They fire when compiling multiple files together (`pipeline.md` §Multi-File Compilation Order).

| ID | Classification | Trigger |
|---|---|---|
| `G::build::skipped-due-to-failed-import` | warning | A file was skipped (Phases 1–7 not run, no `.md` written) because a transitive dependency failed earlier in the same build. The diagnostic surfaces the failed dependency's file path so the author knows which upstream failure cascaded (`pipeline.md` §Multi-File Compilation Order, Partial Failure Policy). |

### Validate-output phase (Phase 6b)

Phase 6b structural validation, implemented in the `glyph validate-output` subcommand. These are **compiler-scope** diagnostics — deterministic checks that Step 2's Markdown output faithfully projects the input IR. All 27 are classification `error`. The canonical specification lives in `expand.md` §4.2; the workflow integration is in `agent-skill.md` §`glyph validate-output`.

**By-construction satisfaction.** With the scaffold-with-spans architecture (`expand.md` §3.5), the deterministic emitter owns all section structure and list cardinality. The diagnostics in the table below marked **(by construction)** cannot fire for output produced by the scaffold path — the structure that they would catch is emitted by the deterministic emitter, not the LLM. 6b retains them as **defense in depth**: they remain enforced for hand-written or regenerated output (e.g., a Step 2 retry that reads and rewrites `foo.md` directly rather than re-filling spans), and they continue to fire for any future migration where the LLM is given a wider surface than scaffolded spans.

By-construction-satisfied for scaffolded portions:

- `extra-h2`, `missing-instructions`, `extra-h3`
- `step-count-mismatch`, `step-order-mismatch`
- `constraint-count-mismatch`, `context-count-mismatch`
- `params-section-mismatch`, `params-section-missing`, `params-section-spurious`
- `frontmatter-returned`
- `procedure-count-mismatch`, `procedure-name-mismatch`
- `procedure-step-count-mismatch`, `procedure-ref-missing`, `procedure-ref-dangling`
- `procedure-duplicate`, `procedure-order`

| ID | Classification | Trigger |
|---|---|---|
| `G::expand::extra-h2` | error | Step 2 emitted an H2 other than `## Instructions` |
| `G::expand::missing-instructions` | error | Step 2 did not emit `## Instructions` |
| `G::expand::extra-h3` | error | Step 2 emitted an H3 not matching `### Context`, `### Steps`, `### Constraints`, or `### Procedure: <name>` |
| `G::expand::step-count-mismatch` | error | Number of top-level `### Steps` items does not match expected count |
| `G::expand::substep-count-mismatch` | error | Number of lettered sub-steps in a Branch arm does not match Step-projecting node count |
| `G::expand::constraint-count-mismatch` | error | Number of `### Constraints` items does not match top-level `Constraint` node count |
| `G::expand::context-count-mismatch` | error | Number of `### Context` items does not match top-level `Context` node count (the IR's `context` array length on the skill/block) |
| `G::expand::step-order-mismatch` | error | Step order diverges from `flow:` order |
| `G::expand::invented-param-ref` | error | `{...}` reference does not match any declared parameter |
| `G::expand::dropped-param-ref` | error | A parameter reference from Step 1 output was silently removed by Step 2 |
| `G::expand::unresolved-local-ref` | error | A `local_ref` slot survived as a literal `{name}` token — Step 2 failed to resolve it into prose |
| `G::expand::output-target-leak` | error | A literal output-target token survived in compiled Markdown instead of being folded into natural prose. Covers both forms: `<name>` (when `OutputContract.form == Identifier`) and `<"…">` / its bare quoted description text (when `OutputContract.form == Description`). The diagnostic ID is shared across forms; the validator's textual scan checks both leak shapes for every contract. See `expand.md` §4.1 rule 6c. |
| `G::expand::modifier-leaked` | error | `with` modifier string appears verbatim in output |
| `G::expand::params-section-mismatch` | error | `## Parameters` item count does not match `InputContract` parameter count |
| `G::expand::params-section-missing` | error | Skill has parameters but `## Parameters` section is absent |
| `G::expand::params-section-spurious` | error | Skill has no parameters but `## Parameters` section is present |
| `G::expand::step-too-long` | error | A non-conditional step or sub-step exceeds three sentences |
| `G::expand::constraint-multi-sentence` | error | A constraint is more than one sentence |
| `G::expand::frontmatter-returned` | error | Step 2 returned YAML frontmatter |
| `G::expand::malformed-markdown` | error | Output does not parse as valid structural Markdown |
| `G::expand::procedure-count-mismatch` | error | Number of `### Procedure:` sections does not match `same_file_procedure` projection count |
| `G::expand::procedure-name-mismatch` | error | Procedure H3 name does not match any `same_file_procedure` callee |
| `G::expand::procedure-step-count-mismatch` | error | Numbered items in a procedure section do not match callee's flow node count |
| `G::expand::procedure-ref-missing` | error | A `same_file_procedure` Call produced no procedure reference in its Step prose |
| `G::expand::procedure-ref-dangling` | error | Step references a procedure name with no matching `### Procedure:` section |
| `G::expand::procedure-duplicate` | error | Same procedure name appears in two or more `### Procedure:` sections |
| `G::expand::procedure-order` | error | `### Procedure:` sections not ordered by first reference from `### Steps` |
| `G::expand::description-shape-missing` | error | A raw `<name>.applies()` condition string survived literally in the output; a description-driven branch must render using the resolved description prose, not the raw trigger expression |
| `G::expand::predicate-prose-missing` | error | A predicate's resolved prose was not found in the output. Covers all three predicate forms: `.applies()` (prose from block `description:`), const-form (prose from `const` declaration), and literal-form (the quoted text itself). Fired by Phase 6b's positive predicate-prose check |

### Repair notifications

| ID | Classification | Trigger |
|---|---|---|
| `G::repair::generated-const` | warning | A `generated const` was materialized for an undefined bare name (`repair.md` §5) |
| `G::repair::generated-block` | warning | A `generated block` was materialized for an undefined parens-call (`repair.md` §5) |
| `G::repair::branch-extracted` | warning | A nested `Branch` was auto-extracted into a `generated block` to keep compiled output at one level of sub-steps (`repair.md` §4.9) |
| `G::repair::inferred-effects` | warning | *(Gated — requires `--enable-effects`.)* Phase 3a deterministically inferred and auto-added an `effects:` sub-section for a declaration that omitted it; informational — the author should review the added effects (`ir-and-semantics.md` §3, `pipeline.md` Phase 3a) |
| `G::repair::constraint-tension` | warning | Phase 3c LLM scan identified two constraints in the same declaration that are in friction but both reasonable to hold (`repair.md` §4.10). Build proceeds; both constraints survive into compiled output. |

### Repair execution failures

| ID | Classification | Trigger |
|---|---|---|
| `G::repair::llm-unavailable` | error | Repair LLM call failed transiently (network or 5xx) after 3 retries with exponential backoff (`repair.md` §8) |
| `G::repair::output-invalid` | error | Repair LLM produced output that does not parse as valid Glyph; no retry (`repair.md` §8) |
| `G::repair::no-convergence` | error | Repair loop exhausted 3 iterations with `repairable` diagnostics still present (`repair.md` §8, `pipeline.md` Phase 3) |
| `G::repair::constraint-contradiction` | error | Phase 3c LLM scan identified two constraints in the same declaration that cannot both be satisfied; the author must edit one (`repair.md` §4.10) |
| `G::repair::constraint-scan-malformed` | error | Phase 3c LLM output did not conform to the expected JSON shape after 2 retries with info-rich feedback (`repair.md` §4.10) |

### Expand execution failures (agent-scope)

Step 2 execution-level failures that are the agent's responsibility, independent of structural validation. Phase 6b structural diagnostics (27 IDs) are compiler-scope — see the Validate-output section above.

| ID | Classification | Trigger |
|---|---|---|
| `G::expand::llm-unavailable` | error | Step 2 LLM call failed transiently (network or 5xx) after 3 retries with exponential backoff (`expand.md` §5) |

## Interaction With Repair

The repair pass (Phase 3) receives the full `Diagnostic[]` array as a snapshot, not a stream. After repair modifies the source, the compiler re-runs Parse + Analyze from scratch, producing a fresh diagnostic set. Repair is accepted when no `error` or `repairable` diagnostics remain. See `pipeline.md` Phase 3 for the bounded loop.

## Interaction With Compiled Output

Diagnostics are internal compiler artifacts. They do not appear in compiled `.md` files. Warning-level diagnostics (like `G::repair::generated-const`) are surfaced to the author through compiler CLI output or IDE integration, not through the compiled skill.

## Cross-References

- **Pipeline** (`pipeline.md`): Phase 2 emits diagnostics; Phase 3 consumes them.
- **Repair** (`repair.md` §3): Input contract references "structured diagnostics."
- **Values and names** (`values-and-names.md`): Name collision and shadowing rules.
- **Imports** (`imports.md`): Import-specific error conditions.
- **IR and semantics** (`ir-and-semantics.md`): Role inference and effect validation.
- **Standard library** (`stdlib.md`): Stdlib-specific resolution.
