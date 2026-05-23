---
name: compile
description: Runs the full Glyph pipeline — compile, fmt, LLM repair loop, prose reshape, validate-output — and surfaces every emitted compiled `.md` (top-level skills and procedure files) to the user.
---

## Parameters

- **source_path**. Required.

## Instructions

### Steps

1. Follow the compile-with-repair procedure below.
2. Follow the expand-and-validate procedure below.
3. Follow the final-review procedure below.
4. Follow the show-pipeline-summary procedure below.

### Procedure: compile-with-repair

1. Run `glyph compile {source_path} --format json --emit-ir`. {source_path} may be a single `.glyph` file or a directory; the compiler walks directories itself per `design/pipeline.md` §Multi-File Compilation Order. Read the exit code and NDJSON diagnostics from stdout.
2. If exit 1 (hard errors) or exit 3 (invocation error): surface the diagnostics verbatim to the user and stop the pipeline.
3. If exit 0: the compiler has written `<dir>/<stem>.md` and `<dir>/<stem>.ir.json` for each compiled source — `<stem>` is the source basename with `.glyph` stripped (extension replacement, never append; `foo.glyph` becomes `foo.md`, never `foo.glyph.md`) — plus any standalone procedure files at `<dir>/<stem>/<kebab-name>.md` for Tier-3 projections and for library export blocks whose expanded prose is at least 150 words. Proceed to the next phase.
4. If exit 2 (repairable diagnostics): enter the repair loop — at most 3 iterations per source file.
5. Each iteration: run `glyph fmt {source_path}` to apply deterministic Phase 3a auto-fixes (tab normalisation, constraint/context hoisting, section reorder, import deduplication/removal). If the file changed, re-run `glyph compile {source_path} --format json --emit-ir`; if exit 0, exit the loop.
6. If repairable diagnostics persist after fmt, load `.agents/skills/glyph/repair.md` and follow its procedure for each offending source file, passing that source path and its NDJSON diagnostics as inputs. The skill writes the rewritten source back to disk. Then re-run `glyph compile {source_path} --format json --emit-ir`; if exit 0, exit the loop.
7. After 3 iterations, if repairable diagnostics remain, hard-fail: surface the residual diagnostics verbatim to the user and stop.

### Procedure: expand-and-validate

1. Enumerate every compiled `.glyph` source under {source_path}: a single file when {source_path} is a `.glyph` file, or every `*.glyph` recursively under {source_path} when it is a directory. For each source, derive `<stem>` by stripping `.glyph` from the basename; the IR sidecar lives at `<dir>/<stem>.ir.json`.
2. For each source, collect the set of emitted `.md` artifacts: the top-level scaffold at `<dir>/<stem>.md` if it exists on disk (present for sources containing at least one `skill` declaration; absent for pure library files, which is normal — not an error), plus every `*.md` inside the sibling subdirectory `<dir>/<stem>/` if that directory exists (these are Tier-3 / library procedure files).
3. For each collected `.md` path, run the expand-then-validate cycle: load `.agents/skills/glyph/expand.md` and follow its procedure, passing that `.md` path as `scaffold_path` and the source's `<dir>/<stem>.ir.json` as `resolved_ir`. The skill writes the expanded Markdown back to the same `.md` path.
4. After expansion, run `glyph validate-output <dir>/<stem>.ir.json <md_path>` for that pair. On exit 0, the file is done. On exit 1, retry the expand pass for that one file using the structural diagnostics as revise-with-feedback input. Budget at most 2 retries per file before surfacing the failure verbatim.
5. A source file that emits zero `.md` files (e.g., a library whose export blocks are all below the 150-word threshold) is a no-op for this phase — that is normal, not an error.

### Procedure: final-review

1. Enumerate every compiled `.glyph` source under {source_path} the same way as `expand_and_validate`: a single file when {source_path} is a `.glyph` file, or every `*.glyph` recursively under {source_path} when it is a directory. For each source, derive `<stem>` by stripping `.glyph` from the basename; the IR sidecar lives at `<dir>/<stem>.ir.json`.
2. For each source, collect the set of emitted `.md` artifacts the same way as `expand_and_validate`: the top-level scaffold at `<dir>/<stem>.md` if it exists on disk, plus every `*.md` inside the sibling subdirectory `<dir>/<stem>/` if that directory exists.
3. For each collected `.md` path, run the review cycle: load `.agents/skills/glyph/review.md` and follow its procedure, passing that `.md` path as `md_path`, the originating `.glyph` source as `source_path`, and the source's `<dir>/<stem>.ir.json` as `resolved_ir`. The skill prints a human-readable review report; when it auto-fixed any findings, it has already written the updated Markdown back to the same `.md` path.
4. Treat the printed report as the source of truth for what to do next. If it lists any items under `Needs your attention`, hard-fail after surfacing the report verbatim — those findings include every contradiction and every ambiguous fix, and the contract is that the author edits the source and recompiles.
5. If the report lists items only under `Auto-fixed`, re-run `glyph validate-output <dir>/<stem>.ir.json <md_path>` to keep the safety-sandwich invariant. On exit 1, hard-fail with the structural diagnostics. On exit 0, the review report is already shown to the user and the file is done.
6. Budget at most 2 review passes per file. After the second pass, surface the final report verbatim — including any remaining `Needs your attention` items as warnings — and proceed without further rewriting.
7. A source file that emitted zero `.md` files in `expand_and_validate` is a no-op for this phase — that is normal, not an error.

### Procedure: show-pipeline-summary

1. Print a single `Issues:` line summarising the main failures encountered across every prior phase — repair iterations consumed in `compile_with_repair`, `validate-output` retries in `expand_and_validate`, and any residual `Needs your attention` items from `final_review`. If no phase encountered any failure, print `Issues: none` instead.
2. Then print an `Output:` list of every emitted `.md` path grouped by source file. Do not print phase tables, exit codes, per-fix snippets, or any other pipeline internals beyond these two sections.

