# BUG-088: Committed compiled output is stale: trailing return rendered as inline `Produce:` instead of a separate `Output:` step

**Severity:** low | **Confidence:** medium | **Status:** confirmed
**Location:** `.agents/skills/e2e_tests/glyph_review.md:66`
**Found by:** gap:dogfood-corpus-output-drift | **Audit date:** unknown-date

## Description

`glyph_review.glyph`'s flow ends with `return <"the human-readable equivalence report shown to the user">`. The committed `glyph_review.md` renders this as `... Never emit JSON ... — the report is for a human reader. Produce: the human-readable equivalence report shown to the user.` — i.e. a `Produce:` sentence appended inline to the final action step 3 (the older expand-template return style).

Recompiling with the current compiler deterministically emits the return as its own numbered step: `4. Output: the human-readable equivalence report shown to the user.` The committed file is otherwise pure scaffold output, so the current scaffold emitter (ADR-0026 `Output:` style) is the correct oracle and the committed `.md` is stale.

The source passes `glyph check --strict` (exit 0), so CI never flags the drift. The working tree already carries the corrected `.md` as an unstaged change, independently confirming the compiler's current behavior.

## Trigger / Reproduction

Run `cargo run -p glyph-cli -- compile .agents/skills/e2e_tests/glyph_review.glyph` and diff the output against the committed `.agents/skills/e2e_tests/glyph_review.md`. The committed file ends step 3 with the old inline `Produce:` fold; the recompiled output ends step 3 cleanly and adds a separate `4. Output: the human-readable equivalence report shown to the user.`

## Evidence

```markdown
<!-- committed HEAD: step 3 ends with inline Produce: fold -->
3. Print a human-readable report directly to the user as plain text. ... Never emit JSON,
   code-fenced data, or any other structured payload — the report is for a human reader.
   Produce: the human-readable equivalence report shown to the user.

<!-- current compiler output (ADR-0026 Output: style): -->
3. Print a human-readable report directly to the user as plain text. ... Never emit JSON,
   code-fenced data, or any other structured payload — the report is for a human reader.
4. Output: the human-readable equivalence report shown to the user.
```

## Recommended Resolution

Recompile `.agents/skills/e2e_tests/glyph_review.glyph` with the current compiler and commit the regenerated `glyph_review.md` (return as a separate `4. Output: ...` step). Additionally, add a CI gate that recompiles dogfood corpus sources and diffs against committed `.md` outputs to prevent future drift.

## Verification Notes

Reproduced directly: the committed HEAD blob of `.agents/skills/e2e_tests/glyph_review.md` ends step 3 with the old inline fold "...the report is for a human reader. Produce: the human-readable equivalence report shown to the user." with no step 4. Compiling the committed `.glyph` source with the current compiler deterministically emits step 3 ending at "...for a human reader." plus a separate "4. Output: the human-readable equivalence report shown to the user." — exactly the ADR-0026 Output-flow-node style. The working tree already carries this corrected `.md` as an unstaged change. `glyph check` on the source exits 0, so no check-based CI gate flags the drift. Severity is low because this is a dogfood agent-skill corpus file, not compiler source or a crash path — the only impact is an agent reading slightly stale self-instructions.
