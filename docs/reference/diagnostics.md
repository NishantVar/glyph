# Glyph Diagnostics (Reference)

This is the **public contract** for structured diagnostics emitted by the Glyph compiler. Tools, agents, IDE integrations, and downstream skill authors depend on the diagnostic shape, classification tiers, and ID scheme described here. IDs in this catalog are stable: once published, an ID is never renamed or reassigned.

The architectural rationale for this contract (why three tiers, why the ID scheme is shaped this way) lives in [[docs/architecture/diagnostics]].

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

### Span semantics

- **1-indexed** for both line and column. Matches what authors see in editors and CLI output.
- **Inclusive end.** A single-character span has `start == end`. A span over `foo` on line 3 starting at column 5 is `start: {3, 5}, end: {3, 7}`.
- **Multi-line spans are legal.** A span over a multi-line construct (e.g., an unterminated `"""` block string, a malformed multi-line declaration header) sets `end.line > start.line`. The `end.col` value applies to its own line.
- **Columns are unambiguous.** Tabs are a compile error, so column = number of characters from start of line. No tab-width interpretation is needed.
- **Synthetic-diagnostic fallback.** Some diagnostics arise from phases that operate post-Lower or post-Repair, where the precise authored location may not survive. In those cases the span falls back, in order, to: (1) the closest authored construct's span (e.g., the parameter or call that lowered to the offending IR node), (2) the enclosing declaration's header span, (3) `start: {1, 1}, end: {1, 1}` with `file:` set.

## Classification

Diagnostics use a three-tier classification:

| Classification | Meaning | Compilation continues? |
|---|---|---|
| `error` | Hard stop, no repair possible | No |
| `repairable` | Repair pass can likely fix this | Paused until repair attempt |
| `warning` | Non-blocking observation | Yes |

**Classification is deterministic per ID.** Each diagnostic ID maps to a single classification given its triggering context. A small number of IDs document a `repairable / error` split (notably `G::analyze::applies-on-undescribed-block`); in those cases the classification is fully determined by the trigger location (e.g., same-file vs. imported), and the catalog entry below specifies which side fires when. The classification of a given firing is not free-running — it never drifts at runtime for the same trigger.

## ID Scheme

Diagnostic IDs are namespaced by canonical compiler phase, human-readable, and stable:

```
G::<phase>::<name>
```

Phase namespaces:

- `G::parse::*` — Parse phase
- `G::analyze::*` — Analyze phase (name/role/effect analysis)
- `G::imports::*` — Import resolution (logically distinct subset of Analyze)
- `G::repair::*` — Repair notifications and Repair execution failures
- `G::validate::*` — Validate phase (post-Lower IR invariants)
- `G::expand::*` — Expand: Step 2 execution failures (agent-scope) and Phase 6b structural validation (compiler-scope, implemented in `glyph validate-output`)
- `G::build::*` — Project-level build orchestration (multi-file compilation order, partial-failure handling)

**Stability guarantee.** Once an ID exists it is never renamed or reassigned. It can be deprecated and replaced by a new ID, but the old one keeps its meaning.

## Catalog Completeness Rule

The catalog below grows as the compiler is implemented. The completeness meta-rule is:

1. **Every check in any phase that can fail MUST have exactly one diagnostic ID.** No check may emit an unstructured error string, a bare exception, or a generic fallback diagnostic.
2. **The ID follows the `G::<phase>::<descriptive-name>` convention** above.
3. **Every diagnostic has the full shape** defined in §Diagnostic Shape.
4. **Classification is deterministic per ID.**

## Catalog

### Parse phase

| ID | Classification | Trigger |
|---|---|---|
| `G::parse::tab-indent` | repairable | Tabs used instead of 4-space indentation |
| `G::parse::mixed-indent` | repairable | Tabs and spaces on the same line |
| `G::parse::nested-flow` | error | `flow:` inside `flow:` |
| `G::parse::effects-disabled` | error | `effects:` sub-section used without `--enable-effects`. The effects subsystem is gated; remove the `effects:` line or pass `--enable-effects`. |
| `G::parse::none-with-effects` | error | *(Gated — requires `--enable-effects`.)* `effects: none, reads_files` — `none` alongside other keywords. Detectable in Parse because the token sequence is unambiguous, but semantically an error — `none` exclusivity is a hard rule, not repairable. |
| `G::parse::multiple-with` | error | Chained `with ... with ...` on a single call |
| `G::parse::with-on-bare-name` | error | `with` modifier on a non-call statement |
| `G::parse::operator-in-expression` | repairable | Operator token (`+`, `-`, `*`, `/`, etc.) in expression position; MVP has no value-level operators |
| `G::parse::param-slot-in-non-instruction-string` | repairable | A `{name}` slot appears in a string position that is not instruction-bearing (e.g., parameter default value, `description:` field); braces are stripped and the content is treated as literal |
| `G::parse::return-not-terminal` | error | `return` appears before the last statement of `flow:` |
| `G::parse::return-in-branch` | error | `return` appears inside an `if`/`elif`/`else` body |
| `G::parse::multiple-returns` | error | More than one `return` statement in a single `flow:` |
| `G::parse::duplicate-subsection` | repairable | A sub-section header (e.g., `description:`) appears more than once in a single declaration body. Repair deterministically merges the duplicates by splicing each later occurrence's body into the first occurrence; the merge is purely textual concatenation with comment trivia preserved at the boundary. |
| `G::parse::empty-file` | error | The source file is empty or contains only whitespace and comments — no declarations to compile |
| `G::parse::empty-flow` | error | A `flow:` sub-section is present but its body contains zero statements; either remove the `flow:` header (for a constraint-only skill) or add at least one statement |
| `G::parse::multiple-skills` | error | A `.glyph` file contains more than one `skill` declaration; MVP requires exactly one skill per file because compiled output is named after the skill |
| `G::parse::applies-no-parens` | error | `BLOCKNAME.applies` appears without `()`; the trigger predicate form requires explicit parentheses |
| `G::parse::applies-with-args` | error | `BLOCKNAME.applies(...)` is called with arguments; the trigger predicate is zero-arity |
| `G::parse::none-as-return-type` | repairable | A declaration header uses `-> None` as a return-type annotation. Declarations with no meaningful return omit `->` entirely. Repair deterministically strips the trailing ` -> None` from declaration headers. Match is case-insensitive on `none` with identifier-boundary semantics; the value keyword `none` (in `return none`, `effects: none`, value positions) is preserved. |
| `G::parse::malformed-output-target` | error | A `return <...>` output-target candidate matches neither valid form. Identifier-form failures: empty `<>`, whitespace inside brackets, dot access, call syntax, or any non-identifier character. Descriptive-form failures: empty `<"">` (the description string must be non-empty). The same diagnostic ID covers both; the message names the offending shape. |
| `G::parse::output-target-outside-return` | error | An output-target literal — `<name>` (identifier form) or `<"…">` (descriptive form) — appears outside the single MVP-legal position: the terminal top-level `return …` expression. The same diagnostic ID covers both forms. |
| `G::parse::bad-indent` | error | Leading indentation on a non-blank line is not a multiple of 4 spaces. Glyph requires consistent 4-space indents. |
| `G::parse::unterminated-string` | error | A `"…"` string literal is missing its closing `"` (typically because the newline terminates the line before the close quote is seen). |
| `G::parse::unexpected-char` | error | The tokenizer encountered a character that is not part of any Glyph token (e.g., `@`, `!`, `#` in source position). Distinct from `G::parse::operator-in-expression`, which fires only for the repairable arithmetic operators `+`, `-`, `*`, `/`. |
| `G::parse::unexpected` | repairable / error | Catch-all parser failure. **Repairable**: parser bails with an unstructured error and no specific ID is wired. **Error**: parsing produces no AST and no other diagnostic was raised — a hard fallback so `glyph check` cannot silently exit 0 on an unparseable source. |

### Analyze phase

| ID | Classification | Trigger |
|---|---|---|
| `G::analyze::undefined-name` | repairable | Bare name doesn't resolve to any declaration; repair generates a definition |
| `G::analyze::undefined-call` | repairable | Parens-call doesn't resolve; repair generates a `generated block` |
| `G::analyze::name-collision` | error | Two value-namespace names collide after case normalization. Cross-namespace canonical-equal pairs (e.g., `type Mode` + `block mode_name`) do not fire this — they live in disjoint namespaces. |
| `G::analyze::type-case-violation` | error | A type identifier is not strict PascalCase (no underscores, leading uppercase). Fires for `type` decls, `-> Foo` return annotations, `param: Foo` parameter type annotations, and selective type imports. Examples that fail: `type repo_context`, `-> plan`, `param: Repo_Context`. |
| `G::analyze::value-case-violation` | error | A value identifier is not strict snake_case (lowercase letters, digits, underscores; no uppercase). Fires for `const`, `block`, `export block`, parameters, local bindings, and import aliases. Examples that fail: `const RepoContext`, `block MakePlan`, parameter `paramName`. |
| `G::analyze::inconsistent-type-spelling` | warning | Two raw spellings of the same canonical type name appear in a single compilation unit (e.g., `RepoContext` and `Repocontext` after case-insensitive canonicalization). Non-blocking; the author should settle on one spelling. |
| `G::analyze::import-private` | error | Tried to import a non-exported declaration |
| `G::analyze::import-skill` | error | Tried to selectively import a skill |
| `G::analyze::circular-import` | error | Import cycle detected |
| `G::analyze::missing-file` | error | Import path doesn't resolve to a file |
| `G::analyze::duplicate-import` | repairable | Same file imported twice; merged |
| `G::analyze::unused-import` | repairable | Imported name never referenced; removed |
| `G::analyze::ambiguous-role` | repairable | Can't determine instruction role from context |
| `G::analyze::effects-under-declared` | error | *(Gated — requires `--enable-effects`.)* Declared effects are a subset of inferred effects |
| `G::analyze::effects-over-declared` | warning | *(Gated — requires `--enable-effects`.)* Declared effects include keywords not inferred from the body; non-blocking, surfaced for author cleanup |
| `G::analyze::missing-effects` | repairable | *(Gated — requires `--enable-effects`.)* A declaration omits `effects:` entirely and the inferred set is non-empty; Repair auto-adds the inferred effects |
| `G::analyze::nominal-mismatch` | error | Type name mismatch at a call boundary |
| `G::analyze::generic-type-name` | warning | An identifier in return-type position is one of the 13 banned generic type names: `String`, `Int`, `Float`, `Bool`, `None`, `List`, `Set`, `Map`, `Array`, `Dict`, `Tuple`, `Object`, `Any`. Match is case-insensitive (ASCII). Non-blocking; compilation continues. Suggestion: replace the generic name with a domain type that carries semantic meaning (e.g., `BranchName`, `FilePath`, `Summary`). **Precedence:** `-> None` in return-type position is intercepted earlier by `G::parse::none-as-return-type`; this diagnostic surfaces end-to-end for the other 12 banned names. |
| `G::analyze::lossy-coercion` | error | Lossy numeric conversion, e.g. `3.7` where integer expected |
| `G::analyze::missing-return` | repairable | Export block lacks `return` on a code path |
| `G::analyze::export-missing-return-type` | repairable | A `skill`, private `block`, or `export block` body has at least one `return <expr>` with a meaningful return value but the header lacks a `-> DomainType` annotation. Return types must be explicit when the declaration has a meaningful return and omitted otherwise. Bare `return` and `return none` (any case) do not trigger this diagnostic — those are the no-meaningful-return form. The ID is preserved for continuity with the pre-broadened export-block-only rule. |
| `G::analyze::typed-decl-missing-return` | error | A `skill`, private `block`, or `export block` declares `-> SomeType` on its header but has no value-producing `return` in its body. The contract demands a value of `SomeType`; bare `return`, `return none` (any case), and shorthand bodies all fire. Hard error — no repair, because the author's declared contract cannot be honored by synthesizing a value. |
| `G::analyze::return-of-no-value-call` | error | `return <call>` where the callee resolves to a same-file `block` / `export block` or an imported `export block` whose header declares no `-> Type` annotation. The caller's `return` position demands a value but the callee produces none, so the `return` cannot be honored. Symmetric to the assignment-side check (`x = <call>` against a void callee). **Suppression:** when the callee identifier does not resolve to any declared block at all, this diagnostic is suppressed so `G::analyze::undefined-call` (or `G::analyze::stdlib-missing-import` for stdlib-shaped calls) surfaces alone as the single root cause. Hard error — no repair, because the author must either drop the `return` or call a different block. |
| `G::analyze::output-target-shadows-binding` | error | `return <name>` uses an output target name that already resolves to a visible binding (parameter, const, block, export, or import depending on scope). The target must be a fresh output name, not a reference. |
| `G::analyze::placeholder-string-return` | repairable | A domain-typed declaration ends with a string literal whose entire content is an angle-bracketed placeholder such as `"<current_branch>"` or `"<root cause analysis including affected files and severity>"`. Repair rewrites it to the appropriate output-target form, bifurcating on placeholder shape: identifier-shaped contents become identifier form (`return <current_branch>`), non-identifier-shaped contents (whitespace, punctuation, etc.) become descriptive form (`return <"root cause analysis including affected files and severity">`). Ordinary strings without `<…>` framing and untyped declarations are unaffected; empty `"<>"` is not repaired. |
| `G::analyze::closure-violation` | error | Export block depends on hidden caller context |
| `G::analyze::stdlib-missing-import` | repairable | `subagent()` used without importing `@glyph/std` |
| `G::imports::unknown-stdlib-module` | error | An import path under the reserved `@glyph/` virtual namespace does not resolve to a known compiler-embedded stdlib module. The MVP recognises only `@glyph/std`; any other `@glyph/*` path fires this diagnostic. |
| `G::analyze::unknown-param-slot` | error | A `{name}` slot in an instruction-bearing string does not resolve to a parameter or local binding in scope at the slot's source position |
| `G::analyze::nested-branch` | repairable | A `Branch` appears inside another `Branch`'s arm body; Repair will auto-extract it into a `generated block` |
| `G::analyze::empty-skill-body` | error | A `skill` declaration has no `description:`, no `flow:`, no `constraints:`, no `effects:` — there is nothing to project. A skill must have at least one of `flow:` (with statements) or `constraints:` (with markers); a constraint-only skill is legal. |
| `G::analyze::no-exports-in-library` | error | A library file (zero `skill` declarations) has zero `export` declarations — it has no consumer-visible contribution. Add at least one `export block` or `export const`. |
| `G::analyze::missing-required-arg` | error | A `call <name>(...)` flow statement omits a positional argument for a callee parameter that has no default. Applies uniformly to private `block`, same-file `export block`, and imported `export block` callees. The diagnostic span pins the offending callee identifier at the call site (not the enclosing skill header), so an IDE can highlight the broken call. Skill parameters are *not* subject to this rule — they are runtime-required inputs surfaced in `## Parameters`. |
| `G::analyze::missing-description` | repairable | A `skill` declaration omits `description:`; Repair generates one from the skill name and body and adds it as a `description:` sub-section in the source |
| `G::analyze::const-in-flow` | repairable | A bare name (or undefined identifier) appears in `flow:` without a keyword prefix (`require`/`avoid`/`must`/`context`); `const` declarations are passive constants and are not legal as flow instruction steps. Repair adds parentheses and materializes a `generated block`. |
| `G::analyze::applies-on-non-block` | error | `NAME.applies()` was called where `NAME` resolves to something other than a `block` or `export block` declaration (e.g., a `const`, an `import` alias, a parameter). The trigger predicate is defined only on blocks. |
| `G::analyze::applies-on-undescribed-block` | repairable / error | `BLOCKNAME.applies()` is called on a block that lacks a `description:` sub-section. **Repairable** when the block is defined in the same file under compilation; Repair adds a trigger-shaped `description:` to the block. **Error** when the block is imported from another file; the author must edit the source library directly because Repair is single-file. |
| `G::analyze::condition-non-boolean-non-predicate` | error | An `if` / `elif` condition contains a token that is neither boolean-kinded nor a recognised predicate form. The token resolves to an `int`- or `float`-kinded declaration (the compiler does not implicitly truth-test numerics), or to another non-string, non-boolean kind that condition position cannot accept. The author must rewrite the condition explicitly (e.g., compare to a literal). Opaque/domain-kinded bindings fall through to boolean treatment and do not trigger this error. |
| `G::analyze::unmerged-duplicate-subsection` | error | A `Skill`, `Block`, or `ExportBlock` AST node still has a non-empty `extra_subsections` slot when Analyze runs — i.e., the parser recorded duplicate sub-sections (`G::parse::duplicate-subsection`) but the deterministic merge did not run or could not consume them (e.g., `--no-repair`, `glyph fmt --check`, or a merge failure). Analyze emits this hard error so Lower never sees an inconsistent declaration shape. The author must either re-run with Repair enabled or fix the duplicates manually. |

### Validate phase

Validate is the final correctness gate before any LLM touches the IR. It checks both pre-Lower contracts (closure, effect propagation across imports) and post-Lower IR invariants. The IDs below cover the post-Lower invariant checks specific to Validate.

| ID | Classification | Trigger |
|---|---|---|
| `G::validate::duplicate-node-id` | error | Two IR nodes within the same file share the same stable node ID assigned by Lower |
| `G::validate::unresolved-callee` | error | A `Call` node's callee does not resolve post-Lower (after UFCS desugaring and branch extraction); the call graph cannot be closed |
| `G::validate::malformed-branch` | error | A `Branch` IR node lacks the shape Expand expects — missing `if` arm, or an arm body that is not well-formed |
| `G::validate::recursive-call` | error | The local block-to-block call graph within a file contains a cycle; recursion is forbidden in MVP |
| `G::validate::empty-step` | error | A Step-projecting IR node has empty body text; silently-empty Steps are rejected |

### Build phase

Build-phase diagnostics cover project-level orchestration concerns rather than a specific compiler phase. They fire when compiling multiple files together.

| ID | Classification | Trigger |
|---|---|---|
| `G::build::skipped-due-to-failed-import` | warning | A file was skipped (no `.md` written) because a transitive dependency failed earlier in the same build. The diagnostic surfaces the failed dependency's file path so the author knows which upstream failure cascaded. |
| `G::build::import-outside-out-dir` | warning | Under `--out-dir`, a transitive import resolves outside the input root and therefore cannot be mirrored under the output directory. The imported file is compiled in-place next to its source, and the warning identifies the offending path so the author knows the output tree is not self-contained. |

### Validate-output phase (structural validation)

Structural validation is implemented in the `glyph validate-output` subcommand. These are **compiler-scope** diagnostics — deterministic checks that the Markdown output faithfully projects the input IR. All are classification `error`.

| ID | Classification | Trigger |
|---|---|---|
| `G::expand::extra-h2` | error | Step 2 emitted an H2 outside the catalogue (`## Parameters`, `## Context`, `## Steps`, `## Constraints`, or freeform headings if enabled) |
| `G::expand::missing-instructions` | error | RETIRED. Reserved for forward-compat; no longer emitted — its role is now covered by `extra-h2` and the body H2 count checks. |
| `G::expand::extra-h3` | error | RETIRED. Reserved for forward-compat; with body sections at H2, the only legal H3 is `### Procedure: <name>` (which has its own dedicated diagnostics). |
| `G::expand::step-count-mismatch` | error | Number of top-level `## Steps` items does not match expected count |
| `G::expand::substep-count-mismatch` | error | Number of lettered sub-steps in a Branch arm does not match Step-projecting node count |
| `G::expand::constraint-count-mismatch` | error | Number of `## Constraints` items does not match top-level `Constraint` node count |
| `G::expand::context-count-mismatch` | error | Number of `## Context` items does not match top-level `Context` node count |
| `G::expand::step-order-mismatch` | error | Step order diverges from `flow:` order |
| `G::expand::invented-param-ref` | error | `{...}` reference does not match any declared parameter |
| `G::expand::dropped-param-ref` | error | A parameter reference from Step 1 output was silently removed by Step 2 |
| `G::expand::unresolved-local-ref` | error | A `local_ref` slot survived as a literal `{name}` token — Step 2 failed to resolve it into prose |
| `G::expand::output-target-leak` | error | A literal output-target token survived in compiled Markdown instead of being folded into natural prose. Covers both forms: `<name>` (identifier form) and `<"…">` / its bare quoted description text (descriptive form). |
| `G::expand::modifier-leaked` | error | `with` modifier string appears verbatim in output |
| `G::expand::llm-required-for-call` | error | A `Call` site has a `with` modifier or non-empty `local_refs` that requires LLM-grade prose, but the current compiler build is using the deterministic stub filler. Fires per failing `IrCall` at Step 2 fill time (pre-6b). Remediation: wire the LLM expand filler, or remove the `with` modifier / rewrite the local reference. |
| `G::expand::llm-required-for-param-description` | error | A parameter has no effective description: no inline per-param `<"…">` annotation and no matching type-registry entry. Fires per offending parameter at Step 2 fill time (pre-6b) when the current compiler build is using the deterministic stub filler, which cannot synthesize prose. Remediation: add an inline `<"…">` description on the parameter slot, declare a `type Foo = <"…">` with a matching annotation, or wire the LLM expand filler. |
| `G::expand::params-section-mismatch` | error | `## Parameters` item count does not match `InputContract` parameter count |
| `G::expand::params-section-missing` | error | Skill has parameters but `## Parameters` section is absent |
| `G::expand::params-section-spurious` | error | Skill has no parameters but `## Parameters` section is present |
| `G::expand::frontmatter-returned` | error | Step 2 returned YAML frontmatter |
| `G::expand::malformed-markdown` | error | Output does not parse as valid structural Markdown |
| `G::expand::procedure-count-mismatch` | error | Number of `### Procedure:` sections does not match `same_file_procedure` projection count |
| `G::expand::procedure-name-mismatch` | error | Procedure H3 name does not match any `same_file_procedure` callee |
| `G::expand::procedure-step-count-mismatch` | error | Numbered items in a procedure section do not match callee's flow node count |
| `G::expand::procedure-ref-missing` | error | A `same_file_procedure` Call produced no procedure reference in its Step prose |
| `G::expand::procedure-ref-dangling` | error | Step references a procedure name with no matching `### Procedure:` section |
| `G::expand::procedure-duplicate` | error | Same procedure name appears in two or more `### Procedure:` sections |
| `G::expand::procedure-order` | error | `### Procedure:` sections not ordered by first reference from `## Steps` |
| `G::expand::description-shape-missing` | error | A raw `<name>.applies()` condition string survived literally in the output; a description-driven branch must render using the resolved description prose, not the raw trigger expression |
| `G::expand::predicate-prose-missing` | error | A predicate's resolved prose was not found in the output; covers all three forms — `.applies()` (from block `description:`), const (from `const` declaration), and literal (the quoted text itself). |

### Repair notifications

| ID | Classification | Trigger |
|---|---|---|
| `G::repair::generated-const` | warning | A `generated const` was materialized for an undefined bare name |
| `G::repair::generated-block` | warning | A `generated block` was materialized for an undefined parens-call |
| `G::repair::branch-extracted` | warning | A nested `Branch` was auto-extracted into a `generated block` to keep compiled output at one level of sub-steps |
| `G::repair::inferred-effects` | warning | *(Gated — requires `--enable-effects`.)* Repair deterministically inferred and auto-added an `effects:` sub-section for a declaration that omitted it; informational — the author should review the added effects |
| `G::repair::constraint-tension` | warning | Repair's LLM scan identified two constraints in the same declaration that are in friction but both reasonable to hold. Build proceeds; both constraints survive into compiled output. |

### Repair execution failures

| ID | Classification | Trigger |
|---|---|---|
| `G::repair::llm-unavailable` | error | Repair LLM call failed transiently (network or 5xx) after retries with exponential backoff |
| `G::repair::output-invalid` | error | Repair LLM produced output that does not parse as valid Glyph; no retry |
| `G::repair::no-convergence` | error | Repair loop exhausted its iteration budget with `repairable` diagnostics still present |
| `G::repair::constraint-contradiction` | error | Repair's LLM scan identified two constraints in the same declaration that cannot both be satisfied; the author must edit one |
| `G::repair::constraint-scan-malformed` | error | Repair's constraint-scan LLM output did not conform to the expected JSON shape after retries with info-rich feedback |
| `G::repair::predicate-generation-failed` | error | Repair LLM produced an empty or malformed string when generating predicate prose for a condition-position bare name routed to predicate semantics. Non-repairable; the author must add the `const` declaration manually. |

### Expand execution failures (agent-scope)

Expand Step 2 execution-level failures that are the agent's responsibility, independent of structural validation. Structural validation diagnostics are listed under §Validate-output phase above.

| ID | Classification | Trigger |
|---|---|---|
| `G::expand::llm-unavailable` | error | Step 2 LLM call failed transiently (network or 5xx) after retries with exponential backoff |

## Interaction With Repair

The repair pass receives the full `Diagnostic[]` array as a snapshot, not a stream. After repair modifies the source, the compiler re-runs Parse + Analyze from scratch, producing a fresh diagnostic set. Repair is accepted when no `error` or `repairable` diagnostics remain.

## Interaction With Compiled Output

Diagnostics are internal compiler artifacts. They do not appear in compiled `.md` files. Warning-level diagnostics (like `G::repair::generated-const`) are surfaced to the author through compiler CLI output or IDE integration, not through the compiled skill.
