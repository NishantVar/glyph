# Glyph — Known Bugs (Deferred)

Pre-existing bugs discovered during development that don't block the current
milestone but should be fixed eventually. Each entry should name the file/line,
the symptom, the impact, and the proposed fix.

## Parser

- **Silent parse failure on `name = call(...)` binding inside `flow:`.** The
  parser's `parse_with_diagnostics_opts` returns `None` (parse failure) without
  pushing any diagnostic when it encounters a flow-level binding of the form
  `ctx = inspect_repo(scope)`. The language surface ([[language-surface]])
  treats this as valid syntax — flow can contain variable bindings whose right-
  hand side is a call expression — but the parser doesn't accept it.

  - **Reproduction:** `glyph check
    crates/glyph-cli/tests/corpus/valid/imports/fix_bug.glyph` — exits 0,
    emits no diagnostics, but no AST is produced internally.
  - **Symptom for downstream tools:** the LSP returns `null` for every cursor
    position on this file (no AST → no resolution table → no go-to-def). Any
    AST-walking pass on this file is a no-op.
  - **Why the corpus AC test still passes:** the test asserts the *absence* of
    `undefined-name` / `undefined-call` diagnostics, which trivially holds when
    no AST is produced. The test is technically green but isn't actually
    exercising what it claims to exercise.
  - **Fix:** extend the parser to accept the `Spanned<Identifier> "="
    CallExpr` production at the start of a flow statement, with a corresponding
    `FlowStmt::Binding` AST node (or whatever shape the existing AST uses for
    same-shape declaration-site bindings). Update Analyze to register the
    binding's name in the local scope. Update Lower to lower the binding into
    the IR shape the rest of the pipeline expects. Add a positive corpus test
    that exercises the binding form, plus a negative test that asserts an
    `undefined-name` diagnostic when the binding's RHS references an unknown
    callee.
  - **Workaround until fixed:** verification of cross-file LSP behavior uses
    `crates/glyph-cli/tests/corpus/multi-file/fix_bug.glyph` instead, which
    is structurally identical but doesn't include the binding.

- **`parse_export_block` flat-token-walk may overwrite earlier sections from
  body-level marker string operands.** `crates/glyph-core/src/parse.rs` uses a
  flat token-walk in `parse_export_block` where `current_section` persists
  across tokens. The body-level `context <name|string>` branch was previously
  re-assigning `description` when the inline `StringLit` token was scanned
  while `current_section == Some("description")` (fixed in #168 round-3 by
  setting `current_section = Some("other")` after the capture). The same
  shape may apply to body-level `must "..."` / `must avoid "..."` /
  `require "..."` / `avoid "..."` markers with string-literal operands when
  they follow a `description: "..."` section header — there is no fixture in
  the corpus that exercises this, and the existing constraint-rendering tests
  use const refs rather than inline strings, so the latent bug (if it exists)
  is unobserved. Fix: audit each body-level constraint-marker branch in
  `parse_export_block` and apply the same `current_section` transition as
  the `context` branch. Add a corpus fixture that combines `description: "..."`
  with body-level `must "literal text"` (and the other three forms) on an
  export block, asserting both the preamble line and the authored description
  survive in the Tier 3 standalone output.

- **`context <name>` marker as a flow statement inside a `block` body drops its
  operand and degrades to a bare `context` step.** A `context`/constraint marker
  used as a flow statement is valid per the language surface (sub-sections:
  "Constraint and context markers may appear … as a flow statement inside
  `flow:`"), and top-level markers should be hoisted into the block's
  `context:` sub-section. Instead, when a `context <const-or-name>` marker
  appears inside a private `block`'s `flow:`, the operand is silently dropped:
  the marker is lowered to an `inline_instruction` node `{role: "step", text:
  "context"}` and the block's `callee_context` is left empty. The same shape is
  likely in `parse_block` as the export-block bug above — the body-level
  `context <name>` branch captures the `context` keyword as a step but never
  attaches the following name/string operand.

  - **Reproduction:** compile `.agents/commands/glyph/decompile.glyph`. Its
    procedure blocks (`classify_and_map_unplaced_content`,
    `recover_conditionals_as_branches`, `append_unmapped_section`,
    `retry_unmapped_with_fresh_eyes`) each carry
    `context decompile_by_semantic_content_not_shape` (and one
    `context classification_table`) as the first flow statement. In
    `decompile.ir.json` every one becomes `{"kind":"inline_instruction",
    "role":"step","text":"context"}` with `callee_context: []`.
  - **Symptom for downstream tools:** the compiled `decompile.md` renders these
    as meaningless numbered steps reading just `1. context` / `3. context`. The
    referenced const prose (here the entire `classification_table` reference,
    ~12 lines) never reaches the procedure in the compiled output, silently
    degrading the consuming agent's guidance. `glyph validate-output` does not
    catch it — structure (step count) is preserved, only the operand content is
    lost.
  - **Fix:** audit the body-level `context`/constraint-marker branch in the
    private-`block` parse path (`crates/glyph-core/src/parse.rs`, mirroring the
    `parse_export_block` fix from #168 round-3). Capture the marker's name/string
    operand and route it to the block's context/constraints collection rather
    than emitting a bare `context` step, then hoist it into `callee_context` in
    Lower. Add a corpus fixture: a private `block` whose `flow:` begins with
    `context <const>`, asserting the const prose surfaces as a `### Context`
    bullet (not a `context` step) in the procedure projection.
  - **Workaround applied:** `decompile.md` was hand-patched (2026-05-30) to
    inline the dropped const prose into the affected steps so the output is
    usable; the patch passes `validate-output` but keeps the content as steps
    rather than `### Context` markers. **Do not recompile `decompile.glyph`
    until this is fixed** — a recompile overwrites the patch and reintroduces
    the bare `context` steps.

## Formatter (issue #109 follow-ups, codex pass-4 P2)

- **Inline-form `description:` merge separator may not match design intent.**
  `crates/glyph-core/src/fmt.rs` `emit_merged_descriptions` joins the bodies of
  duplicate inline `description: "..."` sub-sections with a single `\n`. The
  multi-line bare form's merge rule in [[design/repair]] §4.11.4 specifies
  "concatenate body text with a single blank line between bodies" (i.e. `\n\n`).
  The design is silent on the inline-string form, so #109 chose `\n` as a
  default. If §4.11.4's blank-line rule is meant to apply uniformly to all
  description merges (including inline string-form), change `bodies.join("\n")`
  to `bodies.join("\n\n")`. Needs a designer call to confirm the intent.

- **Trailing comment scanner mishandles strings ending in even backslashes.**
  `crates/glyph-core/src/fmt.rs` `strip_trailing_comment` /
  `trailing_comment_after_keyword` (~lines 741–748) treat any quote preceded by
  `\` as escaped, which is wrong when the string ends with an even number of
  backslashes (e.g. `description: "C:\\" // note`). The closing quote is real,
  but the scanner stays `in_string`, so `glyph fmt` can fail to strip the
  trailing comment, drop the duplicate body from the merge, or lose the moved
  comment entirely. Narrow edge case (rare in agent-skill descriptions) but a
  real correctness bug. Fix: track backslash run length and treat the quote as
  closing when the run length is even.

## Emitter (issue #118 follow-ups, codex pass-N P2/P3)

- **`emit_ir.rs` loses `callee_output_contract` for imported Tier-1 callees.**
  `crates/glyph-core/src/emit_ir.rs:269-271` derives the call's
  `callee_output_contract` field by looking up a same-file `IrBlock` in the
  arena. After fix(#85) the resolved `IrCall.callee_output_contract` is
  populated for imported Tier-1 callees during the cross-file fix-up step, but
  `emit_ir` doesn't read it — so `--emit-ir` JSON emits `null` for the field
  even when the call actually has a resolved output contract. Compiled `.md`
  is unaffected (the emit-time gate reads the field directly off `IrCall`),
  but downstream IR-JSON consumers (LSP, `validate-output`) lose the
  return-contract metadata and can't reliably run imported output-target
  leak checks or return-fold logic. **Fix:** in `emit_ir.rs:269-271`, prefer
  `c.callee_output_contract.clone()` when it's `Some`, and only fall back to
  the arena lookup when it's `None` (same-file callees that haven't been
  hoisted yet — though after lower they always are). Add a regression test
  asserting the field is non-null in the IR JSON for an imported Tier-1
  callee with `-> <identifier>`.

- **Constraint emitter — verb grafting and acronym corruption (resolved).**
  Earlier `crates/glyph-core/src/emit/constraint.rs::render` used a locked
  four-form template that grafted a polarity verb onto each const body
  (`You must …`, `You must never …`, `Avoid …`, capitalised soft-require).
  That coupling produced two distinct defects:
  (a) The `normalize()` helper lowercased the first character of every hard
      body and every soft-avoid body, corrupting intentional acronyms or
      product names (`HTTP requests without retries` → `hTTP requests…`).
  (b) Const bodies authored as declaratives or already-prohibitive
      sentences (`Routing is by …`, `Avoid leaving references …`,
      `Do not make changes …`) collided with the verb prefix and produced
      ungrammatical or doubled bullets (`Avoid routing is by …`,
      `Avoid avoid leaving …`).
  Both defects are gone with the bold colon-marker emitter (see
  [[design/compiled-output]] §Constraint Rendering). The label sits in a
  bold span and is separated from the body by a colon, so the body is its
  own clause: capitalization, phrasing, and punctuation are now the
  author's choice and the emitter never rewrites them. The
  `is_already_prohibition` / `normalize` / `capitalize_first` helpers and
  their unit tests have been removed; issue #141's Phase 5 lint is no
  longer needed for this purpose. One residual author-side concern: a soft
  or hard `avoid` const body that starts with a negation word (`do not`,
  `never`, `no`) still produces a double-negative bullet
  (`**Avoid:** do not touch …`) and is now caught by the
  `negation_in_avoid_const_text` constraint in the teach/decompile skills.

- **Nested Tier 2 procedure call renders as a literal `call <name>` step.**
  When a Tier 2 procedure's flow calls another procedure (`call child_step`),
  the emitted Tier 2 output renders the call as a literal step like
  `4. call child_step` rather than inlining the callee's steps or rendering a
  cross-reference to the child procedure's section. Reviewer of issue #168
  treated this as pre-existing broader Tier 2 procedure-call rendering
  behavior and not a #168 blocker. Surfaced by the fixture
  `crates/glyph-cli/tests/corpus/valid/tier2_nested_block_promotion.glyph`
  which was added as the Finding 1 regression-lock for #168. The desired
  rendering is unspecified — needs a design call between (a) inlining the
  callee's steps into the parent's step list, (b) emitting a cross-reference
  to the child procedure's section, or (c) keeping the literal `call <name>`
  but with a section anchor link. Cites: `crates/glyph-core/src/emit/scaffold.rs`
  step-rendering path.

- **Inlined block's `return <"description">` output target folds onto the
  preceding instruction as a lowercase sentence fragment.** When a `block`
  whose flow ends with `return <"...">` is projected `inline` into a parent
  step (e.g. the callee of a branch-arm call), the scaffold emitter
  concatenates the block's last instruction string and the raw output-target
  description into a single `resolved_body_text`, producing a run-on whose
  second sentence starts lowercase after a period:
  `…catches expand/validate/review failures. the sub-agent's compilation
  outcome — either a success report …`. ADR 0026's `Output:`-step rendering
  only covers a *top-level* `flow:` return; a block's return folded into an
  inline projection has no equivalent deterministic handling, so the lowercase
  output description leaks verbatim into the step body.
  - **Reproduction:** `glyph compile .agents/commands/glyph/teach.glyph
    --emit-ir`; inspect `teach.ir.json` flow node `n18` (the `if
    ask_user(...)` branch). Both arm calls — `compile_via_subagent` (n19) and
    `compile_inline` (n20) — have a `resolved_body_text` ending with the
    lowercase output-target text from their source `return <"...">`
    (`.agents/skills/glyph/passes.glyph:27,32`). Compiled `teach.md` Step 11
    sub-steps show the defect verbatim.
  - **Impact:** the `.md` is grammatically wrong (lowercase sentence-start,
    orphaned noun phrase) and the **LLM expand/review pass is forced to repair
    it** — capitalize and re-join — on every compile. Deterministic structure
    that the emitter owns is being offloaded onto the bounded LLM, which is
    exactly what the pipeline is supposed to avoid. The fix happens to be
    classified `auto_fix`, but it should never reach the LLM at all.
  - **Fix:** in the scaffold emitter's inline-projection path
    (`crates/glyph-core/src/emit/scaffold.rs` step-rendering), decide a
    deterministic rendering for an inlined block's terminal output target —
    either (a) drop the output-target description for inline projections (the
    parent step already states the outcome), or (b) render it as a properly
    capitalized trailing sentence / clause rather than concatenating the raw
    lowercase `<"...">` body. Needs a design call on which; ADR 0026 should be
    extended to state how an inlined (non-top-level) return projects. Add a
    corpus fixture: a block ending in `return <"lowercase description">` called
    from a branch arm, asserting the compiled step body has no lowercase
    sentence-start and `validate-output` exits 0.

## Validator (validate-output) + IR emission

- **Nested same-file procedures (reachable only via a call inside another
  procedure's flow) fail `validate-output` with spurious
  `procedure-count/name/order` mismatches.** When a `block` is called *inside
  another block's flow* — rather than from the skill's top-level `flow:` — the
  scaffold emitter correctly renders it as its own `### Procedure:` section, but
  `validate-output` never counts it as an expected callee, so a clean compile
  (`glyph compile … --emit-ir` exits 0) fails the post-expand safety gate.

  - **Reproduction:** `$OBSIDIAN/goals/.agents/commands/notion-sync.glyph`
    defines `block branch_over_drafter_reply()` (line 80) and calls it at line
    71 from inside `retrospective_triage_yesterday`'s flow (within the `else`
    arm). `glyph compile notion-sync.glyph --format json --emit-ir` → exit 0;
    `glyph validate-output notion-sync.ir.json notion-sync.md` → exit 1 with:
    ```
    error[G::expand::procedure-count-mismatch]: expected 11 procedure sections but found 12
    error[G::expand::procedure-name-mismatch]: procedure section `### Procedure: branch-over-drafter-reply` does not match any callee
    error[G::expand::procedure-order]: procedure sections are not ordered by first reference from `## Steps`
    ```
  - **Verified independently:** a full source↔md semantic-equivalence audit
    confirms the compiled `notion-sync.md` faithfully represents the source
    (all 13 blocks, 10 constraints, 5 context consts present and
    polarity-correct; `branch_over_drafter_reply` rendered both as the
    in-`retrospective` reference and as a standalone section). The output is
    correct — only the verification layer is wrong.
  - **Root cause (two co-conspirators):**
    1. `crates/glyph-core/src/validate_output.rs:1396`
       `collect_procedure_calls` walks the top-level `flow` and recurses into
       `branch` then/elif/else bodies, but **never recurses into a
       `same_file_procedure` call's own `callee_flow`**. So a procedure
       reachable only through a nested call is absent from
       `unique_procedures`. The emitter renders 12 sections; the validator
       expects 11 → `procedure-count-mismatch`; the orphan section matches no
       collected callee → `procedure-name-mismatch`; and because it is never
       referenced from the top-level `## Steps`, the `first_ref_order` walk
       (validate_output.rs:1366-1393) drops it → `procedure-order`.
    2. `crates/glyph-core/src/emit_ir.rs` under-serializes the nested call's
       parent `callee_flow` in the `--emit-ir` JSON: `n36`
       (`retrospective_triage_yesterday`) emits a `callee_flow` of only 3
       `inline_instruction` nodes, dropping the `if/elif/else` branch nodes and
       the nested `branch_over_drafter_reply()` call entirely. Even a fixed
       `collect_procedure_calls` couldn't discover the nested callee from this
       JSON — so both sides need fixing.
  - **Fix:**
    (a) In `emit_ir.rs`, serialize a `same_file_procedure` call's `callee_flow`
        with full fidelity — preserve nested `branch` nodes and nested `call`
        nodes (matching what the in-memory lowered IR and the scaffold emitter
        already walk).
    (b) In `validate_output.rs::collect_procedure_calls`, after pushing a
        `same_file_procedure` target, recurse into that node's `callee_flow`
        (and its branch arms) so transitively-reachable procedure callees join
        the expected set. Likewise extend the `procedure-order` first-reference
        scan (validate_output.rs:1366) to credit references that occur inside a
        parent procedure section, not only inside top-level `## Steps`, so a
        nested procedure orders relative to its referencing parent.
  - **Add a corpus fixture:** a minimal `.glyph` with a top-level flow calling
    one block, that block's flow calling a second block, asserting
    `validate-output` exits 0 and both procedure sections are recognized.
  - **Workaround until fixed:** the compiled `.md` is sound and safe to use;
    the exit-1 from `validate-output` is a false negative for skills with
    nested procedures.

## Testing

- **`multi_file_acceptance.rs::setup_tempdir` cannot copy subdirectories from
  corpus fixtures.** `crates/glyph-cli/tests/multi_file_acceptance.rs`'s
  `setup_tempdir` walks corpus fixture directories with `read_dir` and
  `std::fs::copy()`, which fails when the source directory contains
  subdirectories. The test silently breaks when a fixture happens to contain
  a nested directory — e.g. a Tier 3 standalone-procedure subdir created by
  an earlier `glyph compile` run if the run was invoked without `--out-dir`
  and pointed at the corpus root. Symptom: `cargo nextest run --workspace`
  drops from a healthy count to N-24 with a `setup_tempdir` panic.
  Workaround: keep the multi-file corpus directories free of subdirectories
  (e.g. always pass `--out-dir <tempdir>` when running `glyph compile`
  against any multi-file fixture). Fix: replace the flat `read_dir + copy()`
  loop with a recursive `copy_dir_all` helper, or have it skip non-file
  entries explicitly.
