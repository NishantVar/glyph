# Glyph — General Implementation TODOs

Work-tracking items extracted from [[todo]] during the 2026-05-13 design-folder pruning. These are implementation TODOs, internal-detail follow-ups, and migration chores that are not author/product-facing language design questions.

## Effects (Gated)

- **Effect inference for call-graph-free skills.** The effects subsystem is gated behind `--enable-effects` (default: off) because effect inference only walks `FlowStmt::Call` targets. Skills that perform effectful actions directly via inline instructions (no block calls) get an empty inferred set, causing all declared effects to be spuriously flagged as over-declared (`G::analyze::effects-over-declared`). To re-enable effects: (1) fix inference to handle skills with no call graph (e.g., trust author declarations when there are zero calls, or infer from instruction text), (2) flip the `--enable-effects` default to on. The full effects design in [[ir-and-semantics]] §3 remains the target — only the implementation gate needs removal.

## Expand — LLM Span Fill (Possible Improvements)

When Nishant's separate LLM Expand pass lands, it consumes the scaffold-with-spans IR ([[docs/architecture/expand]] §3.5, [[llm_expand_pass]]) and replaces — or per-`SpanKind` overrides — `glyph-core::emit::stub_fill::fill()`. The scaffold and merger are stable across that swap.

1. **LLM paraphrase of `OutputContract.Description`** — replace stub pass-through with LLM-shaped paraphrase in the `DescriptionReturnFold` span.
2. **LLM `with`-modifier weaving** — fill `CallBodyShape` span with modifier-aware prose. Today's stub ignores `site_modifier`.
3. **LLM branch mixed-condition prose conversion** — fill `BranchCondition` span with natural-language prose. Today's stub uses the literal expression.
4. **LLM `{local_ref}` resolution** — needs `IrCall.local_refs` populated by Lower (or Step 1).
5. **LLM scoped-constraint inlining** — needs `IrCall.scoped_constraints` populated by Lower.
6. **LLM parameter descriptions** — fill `ParamDescription` span. Today's stub is empty.
7. **Repair-side canonical-form rewriter** for `avoid:`/`require:`/`must:`/`must avoid:` text not conforming to the canonical contract ([[docs/reference/compiled-output]] §Constraint Rendering, [[GLYPH_LANGUAGE_GUIDE]] §7.2). See [[docs/architecture/repair]] §2.
8. **IR JSON `IrInstructionRef` distinction** — if/when an IR JSON consumer needs to tell anonymous prose (`InlineInstruction`) from resolved const refs (`InstructionRef`).
9. **Cache invalidation on emitter version bumps** — the scaffold-with-spans IR is internal and not part of the IR JSON contract, but compiled-output cache keys ([[compiler-pipeline]] §Cacheability) need to invalidate when emitter version bumps cause byte-level scaffold drift.
10. **Cross-file `IrCall.return_type` resolution (D17)** — already tracked under types. The deterministic emitter currently relies on Step 1 carrying the resolved return type; cross-file resolution gaps surface as missing return-fold suffixes.

## Compiler & Runtime

- **AST-symmetrize the export-block flow.** Skill blocks flow through the IR (`IrSkill`), but `export block` declarations do not — there is no `IrExportBlock` node, and `lib.rs` walks the AST directly to emit export-block procedures. The type-system slate (#85) added `<IDENT>` return support to export blocks via `ExportBlockDecl.terminal_return` (a parser-level field on the AST) plus a small lowering helper, rather than introducing an `IrExportBlock` and migrating the procedure-emission path — full IR symmetry was descoped to keep the slate moving. Follow-up: introduce `IrExportBlock` mirroring `IrSkill`, migrate `lib.rs` procedure-emission to walk IR, retire the AST-direct path. While there, audit `G::analyze::export-missing-return-type` (#82) — flagged as a paper-over that may become redundant once the IR is symmetric. Cites: `parse.rs` (export-block parsing), `lib.rs` (procedure-emission walk), `ir.rs` (missing `IrExportBlock`).

- **Delete `had_preamble` dead-state variable in the Tier 2 / Tier 3 preamble renderers.** `crates/glyph-core/src/emit/scaffold.rs` and `crates/glyph-core/src/emit/mod.rs` carry a `let mut had_preamble = false;` + `had_preamble = true;` + `let _ = had_preamble;` shape that's now load-bearing-free: the blank-line separation between preamble paragraphs and the step list is achieved by each preamble line ending in `\n\n`. The variable was load-bearing before the blank-line fix (it gated a trailing newline guard) and was kept as a no-op to minimize the round-2 diff. Mechanical cleanup: delete the variable declaration, every assignment to it, and the trailing `let _ = had_preamble;` discard. Cosmetic only — no behavior change. Cites: `emit/scaffold.rs` Tier 2 preamble rendering block, `emit/mod.rs` Tier 3 preamble rendering block.

## Tooling

- **Remove `_effects_stub` from the tree-sitter grammar.** `tree-sitter-glyph/grammar.js` carries a hidden `_effects_stub` rule (introduced in M2 commit `b9e0761`) that structurally consumes `effects: a, b, c\n` lines without producing an AST node or any highlight captures. It exists solely to prevent the parse-recovery cascade on three corpus files that still contain `effects:` lines (`crates/glyph-cli/tests/corpus/valid/update_docs.glyph`, `imports/repo_tools.glyph`, `imports/fix_bug.glyph`) — without it, `tree-sitter highlight` breaks on `repo_tools.glyph` because `effects:` is the first body line. **`effects:` is permanently out of MVP** (per the M2 brief and [[language-surface]]), so once those three corpus files are cleaned of `effects:` lines, the stub becomes dead. To remove: delete the `_effects_stub` rule and its two references inside `declaration_body` and `export_block_body` in `grammar.js`, regenerate (`tree-sitter generate`), and confirm `tree-sitter test` plus highlight invocations still pass. Mechanical, no design implications.
