# BUG-052: CI never re-derives or diffs the committed compiled .md dogfood corpus, so stale outputs go undetected

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `.github/workflows/ci.yml:30-33`
**Found by:** gap:dogfood-corpus-output-drift | **Audit date:** unknown-date

## Description

The "Glyph dogfood check" CI step only runs `glyph check --strict` on `.agents/skills` and `.agents/commands/glyph`. `check` runs Phases 1-2 (Parse + Analyze) only, writes no output files, and never lowers/emits Markdown — so it cannot detect when a committed compiled `.md` no longer matches what the current compiler emits. The committed `.md` files (e.g. `.agents/skills/e2e_tests/SKILL.md`, `.agents/skills/e2e_tests/glyph_review.md`, `.agents/commands/glyph/*.md`, `examples/minimal_glyph_tour.md`) are a frozen dogfood corpus, but nothing re-derives them. Concrete trigger: an emitter fix lands (e.g. ADR-0026 `Output:` return-step rendering), the committed `.md` was produced by the older buggy emitter, yet `glyph check --strict` still exits 0 because the `.glyph` sources are valid. The pre-commit hook (`scripts/hooks/pre-commit`) has the same gap — it runs `glyph fmt`/`glyph check` on staged `.glyph` files but never recompiles to `.md` or diffs against the committed output. Result: stale compiled output is committed and remains green forever.

## Trigger / Reproduction

1. An emitter fix lands (any change to `emit/scaffold.rs`, `emit/mod.rs`, or template rendering).
2. The committed `.md` files in `.agents/skills/e2e_tests/` and `.agents/commands/glyph/` reflect the pre-fix emitter.
3. CI runs `glyph check --strict` on the `.glyph` sources — exits 0 because sources are syntactically valid.
4. The stale `.md` outputs remain in the repo with no CI failure to flag the drift.

## Evidence

```yaml
      - name: Glyph dogfood check
        run: |
          cargo run -p glyph-cli -- check --strict .agents/skills
          cargo run -p glyph-cli -- check --strict .agents/commands/glyph
```

`docs/reference/cli.md` confirms: "`check` runs Phases 1 (Parse) and 2 (Analyze) only ... No output files are produced."

## Recommended Resolution

Add a CI step that recompiles the representative deterministic corpus to a temp dir (`glyph compile --output <tmp> <src>` per file, or `--out-dir`) and `diff`s the result against the committed `.md`, failing on any divergence. For files that require the LLM expand filler (param descriptions, with-modifier/local-ref calls), gate them out of the deterministic check or run with the LLM filler enabled. At minimum, gate the scaffold-emittable files (the `e2e_tests` `SKILL.md` / `glyph_review.md`) so emitter regressions in the dogfood corpus are caught.

## Verification Notes

The CLI reference explicitly confirms `check` "Writes no output files" and runs only Phases 1-2. The repository commits `.md` files alongside `.glyph` sources. The compiler is documented as fully deterministic for the phases producing `.md` output, making a diff-based CI gate straightforwardly feasible for files that don't require LLM phases. The pre-commit hook similarly only runs `glyph fmt`/`glyph check` without recompiling.
