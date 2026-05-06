# Glyph — Known Bugs (Deferred)

Pre-existing bugs discovered during development that don't block the current
milestone but should be fixed eventually. Each entry should name the file/line,
the symptom, the impact, and the proposed fix.

## Parser

- **Silent parse failure on `name = call(...)` binding inside `flow:`.** The
  parser's `parse_with_diagnostics_opts` returns `None` (parse failure) without
  pushing any diagnostic when it encounters a flow-level binding of the form
  `ctx = inspect_repo(scope)`. The language surface (`language-surface.md`)
  treats this as valid syntax — flow can contain variable bindings whose right-
  hand side is a call expression — but the parser doesn't accept it.

  - **Reproduction:** `glyph check
    crates/glyph-cli/tests/corpus/valid/imports/fix_bug.glyph.md` — exits 0,
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
    `crates/glyph-cli/tests/corpus/multi-file/fix_bug.glyph.md` instead, which
    is structurally identical but doesn't include the binding.

## Formatter (issue #109 follow-ups, codex pass-4 P2)

- **Inline-form `description:` merge separator may not match design intent.**
  `crates/glyph-core/src/fmt.rs` `emit_merged_descriptions` joins the bodies of
  duplicate inline `description: "..."` sub-sections with a single `\n`. The
  multi-line bare form's merge rule in `design/repair.md` §4.11.4 specifies
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

- **`emit/constraint.rs::normalize` lowercases acronyms at the leading
  character.** `crates/glyph-core/src/emit/constraint.rs:25-36` lowercases the
  first uppercase character of every hard constraint and every soft `avoid`
  body before rendering. This corrupts intentionally-capitalized leading
  tokens (acronyms, product names): `avoid: HTTP requests without retries`
  renders as `Avoid hTTP requests without retries.` instead of
  `Avoid HTTP requests without retries.` Same applies to `must` and
  `must avoid`. **Fix:** only lowercase the leading character when the
  token is a single uppercase letter followed by lowercase (i.e. a normal
  capitalized English word), or simpler: strip the leading-character
  transformation entirely and trust author casing. Add a corpus test with
  an acronym-leading constraint body. Pre-existing — separate from the
  scaffold-with-spans branch but worth fixing as part of the locked-template
  hardening pass.
