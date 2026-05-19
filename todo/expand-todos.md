# Expand — Outstanding Work

Deferred Phase 6b and Expand-related items extracted from [[docs/architecture/expand]]. These are work-tracking notes, not design decisions.

## Phase 6b Validation — Deferred Checks

From [[docs/architecture/expand]] §4.3:

- **Full Markdown well-formedness via a real Markdown parser.** Today Phase 6b uses lightweight structural checks plus `G::expand::malformed-markdown`. A pull-parser-based pass would catch a broader class of malformation but is not required for MVP.
- **No-embedded-HTML scan.** A scan rejecting raw HTML in Step 2 output is deferred. False-positive risk on legitimate constraint prose mentioning HTML tags is real, and consuming LLMs treat the file as text. Worth revisiting if HTML leakage shows up in practice.
- **Predicate-framing verbatim check.** From [[docs/architecture/expand]] §3.3: the pure-predicate Branch framing sentences (`Decide whether <…> applies and, if so:` / `Decide which of the following applies and follow only that path:` / `Otherwise:`) are not checked for verbatim match today. If drift becomes a problem, add a structural check keyed on `resolved_predicates` shape.

## Step 2 — Open Questions

(None currently extracted beyond the deferred 6b checks above. Add new TODOs here as they come up.)

## Relift — Untyped-Return Fold Tests Ignored Under PRD #159

Five tests across two crates are currently `#[ignore]`d (or have had their fixtures removed or typed), all exercising the same legacy surface:

**Unit tests** in `crates/glyph-core/src/lib.rs`:

- `return_call_folds_into_final_step` (~L3475) — asserts the row-3-style fold sentence `"Return the result of summarize_changes()."`.
- `return_bare_name_folds_into_final_step` (~L4256) — asserts `"Return the result of result."`.

**Integration tests** in `crates/glyph-cli/tests/type_decls.rs`:

- `return_row1_descriptive_target_produces_x` — asserted on `"Inspect the scope. Produce: a structured diagnosis."` (§8.4 row 1). Driver fixture `crates/glyph-cli/tests/corpus/valid/return_row1_descriptive.glyph` has been **deleted** because `strict_compile_all_valid_files_exit_zero` (`strict_mode.rs`) iterates top-level `valid/*.glyph` non-recursively and would otherwise bail on it.
- `return_row4_named_no_type_just_produces_name` — asserted on `"Inspect the scope. Produce \`diagnosis\`."` (§8.4 row 4). Driver fixture `crates/glyph-cli/tests/corpus/valid/return_row4_named_no_type.glyph` has been **deleted** for the same reason.

**Integration test** in `crates/glyph-cli/tests/multi_file_acceptance.rs`:

- `fix_bug_return_folded` — asserted on `"Return the result of summarize_changes()."` (§8.4 row 3). Driver fixture `crates/glyph-cli/tests/corpus/multi-file/fix_bug.glyph` is **kept but now typed**: a `type ChangeSummary = <"…">` declaration was added and `skill fix_bug(scope = ".") -> ChangeSummary` was annotated to satisfy #160's broadened rule and keep the multi-file project's other tests (`fix_bug_snapshot`, `fix_bug_constraints`, etc.) compiling. The typed-return path bypasses the legacy fold, so the row-3 assertion can no longer reach end-to-end through compile.

**Why ignored.** All five tests exercise the legacy expand-pass behaviour that folds a meaningful `return <expr>` on an **untyped** skill header into a row-1 / row-4 / row-3 sentence (`expand.rs:339-353`, gated on `!skill_has_return_type && !skill_has_oc`). PRD #159 (issue #160) now makes that surface `Repairable` via `G::analyze::export-missing-return-type`. Either the analyzer fires and `compile_source` short-circuits to `CompileOutcome::Diagnostics` (untyped fixture case — rows 1 / 4 / unit tests), or the fixture has been typed to clear the analyzer and the fold-gate `!skill_has_return_type` then suppresses the fold (row-3 multi-file case). Either way the legacy fold never runs end-to-end and the integration-level assertions can no longer be exercised through the compile pipeline.

The fold behaviour itself is still correct and still wanted as defence-in-depth (rows 1 & 3 & 4 of design §8.4 / `compute_return_sentence`): if a future code path lands an untyped meaningful return into the IR (e.g. via internal IR rewrites or relaxed-mode flags), the expand pass must still produce a sensible final sentence rather than emit a malformed step. The analyzer is the **first** line of defence; the expand-pass fold is the **second**.

**Relift plan.** Re-express all five tests at the expand-pass level so they bypass the analyzer:

1. Construct an `IrArena` directly (no `compile_source` call, no CLI invocation), populating the same skill+block shape the fixtures previously fed in through source.
2. Call `expand_step1_with_imported_descriptions` against that arena.
3. Assert on the markdown fold text exactly as before — for the integration tests, the assertions migrate from CLI-output inspection to in-process markdown inspection.

The two deleted `.glyph` fixtures do not need to be restored; the IR-level reconstruction supersedes them. The kept-but-typed `fix_bug.glyph` stays as the multi-file project's driver fixture for its other assertions; the relifted row-3 IR shape is an in-memory mirror of the pre-edit untyped form. Once relifted, delete the `#[ignore]` attributes (or delete the tests entirely and replace them with the IR-level versions in `glyph-core`), and remove this entry.

**Links.** PRD #159, issue #160, `expand.rs::compute_return_sentence` defence-in-depth note.

---

- **Scoped constraints on `IrCall`.** Lower callee constraints into a new `IrCall.scoped_constraints` field; serialize via `emit_ir.rs` (today hardcoded to `[]`); extend the §3.3 triviality predicate in `crates/glyph-core/src/emit/scaffold.rs::call_needs_llm_fill` with `|| !c.scoped_constraints.is_empty()`; extend `SpanPayload` and `StubFillError` accordingly; reuse the span-emission machinery from the 2026-05-18 CallBodyShape spec.
- **Real source spans on `IrCall`.** Thread a `SourceSpan` (or byte-offset pair) through `IrCall` from parser → lower → IR so `G::expand::llm-required-for-call` can carry a real source span instead of the synthetic zero-width file-level span the CallBodyShape spec ships with.
