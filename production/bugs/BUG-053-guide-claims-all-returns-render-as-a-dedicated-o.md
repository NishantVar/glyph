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
