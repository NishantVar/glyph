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

## Independent Agent Finding

**Verdict:** Reproduced / confirmed.

**Reproduction/Refutation:** The current CI workflow still runs only `cargo run -p glyph-cli -- check --strict .agents/skills` and `cargo run -p glyph-cli -- check --strict .agents/commands/glyph` for dogfood coverage. Both commands exit 0 locally with no output, but `check` does not read, regenerate, or compare sibling `.md` files. In a scratch copy of `.agents/skills/e2e_tests/SKILL.glyph` plus its sibling `SKILL.md`, I appended a stale marker to the copied Markdown. `cargo run -q -p glyph-cli -- check --strict tmp/bug-052-ci-repro.N2zI5w/SKILL.glyph` still exited 0. A subsequent `cargo run -q -p glyph-cli -- compile --output tmp/bug-052-ci-repro.N2zI5w/rederived.md tmp/bug-052-ci-repro.N2zI5w/SKILL.glyph` followed by `diff -u tmp/bug-052-ci-repro.N2zI5w/SKILL.md tmp/bug-052-ci-repro.N2zI5w/rederived.md` exited 1 and exposed drift.

**Evidence:** `.github/workflows/ci.yml` contains no recompile or diff step after the two dogfood `check --strict` commands. `scripts/hooks/pre-commit` formats and checks staged `.glyph` files only; it does not regenerate or diff committed `.md` outputs. `docs/reference/cli.md` states that `glyph check` runs only Parse and Analyze and produces no output files. The scratch diff showed the re-derived output adds `3. Output: the human-readable aggregate report and, when at least one file was tested, the absolute path to the archive directory.` to the copied dogfood `SKILL.md`, and removes the synthetic stale marker. Because the copied `SKILL.md` came from the committed artifact before the marker was added, the missing `Output:` line is real current corpus drift in addition to the synthetic stale-marker proof.

**Resolution Input:** Preserve the existing recommended resolution. A minimal deterministic gate should recompile at least `.agents/skills/e2e_tests/SKILL.glyph` and `.agents/skills/e2e_tests/glyph_review.glyph` to a temp directory and diff the generated Markdown against the committed siblings. Broader coverage can add `.agents/commands/glyph/*.glyph` and `examples/minimal_glyph_tour.glyph`, with any LLM-dependent files explicitly excluded or run with the required filler enabled.
