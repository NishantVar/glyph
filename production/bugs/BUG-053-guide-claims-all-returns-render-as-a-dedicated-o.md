# BUG-053: Guide claims all returns render as a dedicated Output: step, contradicting compiled-output.md and the implementation

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `GLYPH_LANGUAGE_GUIDE.md:1214`
**Found by:** x-contract | **Audit date:** unknown-date

## Description

`GLYPH_LANGUAGE_GUIDE.md` §8.8 and §14 (line 1214) state generically that "the return is rendered as a dedicated `Output:` step at the end of the flow (e.g., `5. Output: …`) rather than folded into the closing sentence of an earlier Step." This contradicts (a) the reference contract `compiled-output.md` §Return Folding (lines 239-248), which says `return <expr>` folds into the final numbered Step with no separate Output section, and (b) the implementation, which only emits a separate `Output: <x>.` step for output-contract returns (skill/block `return <name>`/`return <"...">` per ADR 0026) while ordinary `return <call>`/`return <bare-name>` fold into the final step (`Return the result of <call>.`, expand.rs:600-639). The user-facing guide overstates the Output-step behavior and disagrees with the authoritative reference doc on observable compiled output. ADR 0026 is marked "Proposed" — the guide documents proposed behavior as if it is current.

## Trigger / Reproduction

A skill author reads §14 of the guide, writes `return summarize_changes()`, and expects to see a numbered `Output:` step in the compiled `.md`. The compiler instead folds it into the final step as `Return the result of summarize_changes().` The guide's prediction does not match the compiled output.

## Evidence

```markdown
<!-- GLYPH_LANGUAGE_GUIDE.md line 1214 -->
- The `return` expression is rendered as a dedicated `Output:` step at the end of the flow (e.g., `5. Output: …`); there is no `## Returns` section. See §8.8 and ADR 0026.
```

```
// expand.rs test: return_call_folds_into_final_step
// -> md.contains("Return the result of summarize_changes().")
//
// compiled-output.md §Return Folding (lines 239-248):
// return <expr> folds into the final numbered Step,
// suffix templates: ", and return that as your result."
```

## Recommended Resolution

Reconcile the guide with `compiled-output.md` and the implementation: describe the two real behaviors (output-contract returns → separate `Output:` step; value/call returns → folded into the final step) and ensure the reference doc and guide agree. Note that ADR 0026 is "Proposed" — the guide should not present it as the current behavior until it is accepted and implemented.

## Verification Notes

The contradiction is between documentation files; the compiler's behavior is internally consistent. `compiled-output.md` §Return Folding (lines 243-246) incorrectly attributes fold templates to `return <name>` / `return <"…">` syntax that the implementation actually renders as dedicated `Output:` steps. The guide overstates in the opposite direction by saying ALL returns produce `Output:` steps. Both documents incompletely describe the two-path behavior, but no compiled output is silently wrong and no crash occurs — purely a documentation accuracy issue.

## Independent Agent Finding

### Verdict

Reproduced the documentation-contract mismatch, with one trigger caveat. The guide does overstate current behavior by saying the `return` expression renders as a dedicated numbered `Output:` step, while the reference contract and implementation still have a separate fold path for ordinary meaningful returns. However, the exact public-CLI trigger in this report (`return summarize_changes()` where `summarize_changes` has no return type) does not currently compile to folded Markdown; it fails analysis before emission.

### Reproduction/Refutation

- Reproduced the guide/reference contradiction by reading the cited bounded windows:
  - `GLYPH_LANGUAGE_GUIDE.md:1214` says "`return` expression is rendered as a dedicated `Output:` step".
  - `docs/reference/compiled-output.md:241` says "`return <expr>` in `flow:` folds into the final numbered Step".
- Reproduced the implementation split with focused tests:
  - `cargo test -p glyph-core --lib return_call_folds_into_final_step` passed: `1 passed; 0 failed; 910 filtered out`.
  - `cargo test -p glyph-core --lib skill_output_contract_folds_to_natural_prose` passed: `1 passed; 0 failed; 910 filtered out`.
- Refuted the exact public-CLI reproduction as written:
  - Scratch fixture matching the report's trigger was compiled with `cargo run -q -p glyph-cli -- compile --format json tmp/bug053-return-call.glyph --output tmp/bug053-return-call.md`.
  - The command exited `1` with `G::analyze::export-missing-return-type` and `G::analyze::return-of-no-value-call`, because `summarize_changes` declares no return type. No Markdown was emitted for that exact fixture.

### Evidence

- Graphify first located `crates/glyph-core/src/expand.rs` and the relevant `return_call_folds_into_final_step()` test for the return-rendering path.
- A corrected `rg -n` search for `Output:`, `Return the result`, return-folding tests, and dedicated-output guide text found:
  - Guide claims at `GLYPH_LANGUAGE_GUIDE.md:123`, `:865`, and `:1214`.
  - Reference fold contract at `docs/reference/compiled-output.md:241`.
  - Legacy fold implementation at `crates/glyph-core/src/expand.rs:315`, `:353`, and `:367`.
  - Fold tests at `crates/glyph-core/src/expand.rs:601` and `:626`.
  - Output-contract tests at `crates/glyph-core/src/expand.rs:696`, `:746`, `:840`, and `:857`.
- `docs/adr/0026-return-as-flow-node.md` is still `Status: Proposed`, while the guide cites ADR 0026 as if the behavior is already the general current contract.

### Resolution Input

Keep the existing recommended resolution. It correctly says to document the two current behaviors: output-contract or typed returns can render through the `Output:` path, while ordinary value/call returns can still fold into the final step. Also consider tightening the report's Trigger / Reproduction text so it does not imply the exact untyped `return summarize_changes()` fixture currently reaches Markdown through the public CLI.
