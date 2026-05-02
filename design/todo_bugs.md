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
