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
