---
name: roundtrip
description: Round-trip test of the Glyph compile and decompile pipelines. Walks `source` for .glyph files, pairs each with its sibling .md, dispatches one sub-agent per (file, direction) pair in parallel, semantically compares each pipeline's output against the original artifact, and prints an aggregate report with all drift surfaced verbatim.
---

## Parameters

- **source**:
  path to a .glyph file, a directory to walk, or `.` for the current working directory. The orchestrator expands this into a concrete list of .glyph files at runtime.
  Default: ".".
- **direction**:
  one of `both`, `compile`, or `decompile`. Selects which pipeline directions are exercised — `both` runs every surviving file through both directions in parallel.
  Default: "both".

## Instructions

### Context

- **purpose-is-smoke-detection-not-byte-equality**

  The round-trip test is a smoke detector for the Glyph compile and decompile pipelines, not a regression baseline. Both pipelines are LLM-driven (repair, prose expansion, semantic validation, reverse-mapping), so byte-equality is not the right oracle — semantic equivalence is. A clean run signals the pipelines preserved meaning; drift signals a regression to investigate. Decompile is non-deterministic, so running the same file twice may surface different drift reports.

- **library-files-without-sibling-md-are-indirectly-tested**

  Library .glyph files (those with `export …` declarations but no `skill`) often have no sibling .md of their own — their exports compile away into consumers. The round-trip test does not dispatch such libraries as their own entries. Coverage of their Tier-3 procedure files comes through the closure-wide procedure comparison run by each consumer's compile sub-agent; coverage of their decompile path is indirect through consumers.

- **compile-subagent-contract**

  The compile sub-agent's contract:

  Inputs (passed by the orchestrator):
  - The absolute path of the external original .glyph (source of truth, never written to).
  - The absolute path of the external original .md (sibling of the original, source of truth, never written to).
  - The absolute path of the original procedure directory `<external D>/<entry-relpath-stem>/` (may be absent).
  - The absolute path of the workspace root `<workspace>/` (a recursive copy of the closure root, under the archive).
  - The entry's relative path inside the workspace (e.g. `commands/glyph/decompile.glyph`).

  Task:
  1. Scrub the generated-artifact slots for every .glyph file in the import closure inside the workspace. For each closure-relpath `<closure-relpath>.glyph`, delete `<workspace>/<closure-relpath>.md`, `<workspace>/<closure-relpath>.ir.json`, and `<workspace>/<closure-relpath-stem>/` if they exist. The scrub covers procedure directories owned by imported library files, not just the entry's own directory. Missing slots are no-ops. The source-of-truth `.glyph` files themselves stay in place; those are the inputs to the recompile.
  2. Run `/glyph:compile <workspace>/<entry-relpath>.glyph`. Answer `inline` if the compile flow prompts for compile mode.
  3. Verify `<workspace>/<entry-relpath>.md` exists after the pipeline. Because step 1 deleted the copied original, presence here proves the pipeline wrote new output. If missing, set `STATUS: failed` with reason `/glyph:compile produced no top-level .md`, capture the pipeline output for diagnostics, and skip the remaining steps.
  4. Invoke `decompile_review(original_md=<external original>.md, recompiled_md=<workspace>/<entry-relpath>.md)` to compare the top-level skill output.
  5. Build the closure-wide union of procedure filenames. For every `<closure-relpath>.glyph` in the import closure, take the set of `<kebab>.md` names present under either `<external D>/<closure-relpath-stem>/` (read from the external original) or `<workspace>/<closure-relpath-stem>/` (workspace, freshly regenerated). Key each procedure by the compound `(<closure-relpath-stem>, <kebab>)` to avoid cross-library collisions. For each key: if present on both sides, invoke `decompile_review` on the pair and capture the report; if present on only one side, record a flagged difference (`procedure <closure-stem>/<kebab> present in original, missing from recompile` or the converse).
  6. Concatenate every captured review report into `<archive>/<file-slug>/compile/reports.txt`.
  7. Return a single trailing message in the contract shape: `STATUS: <pass|differences|failed>` / `FILE: <relpath of the original .glyph>` / `DIRECTION: compile` / verbatim review body or diagnostic.

  Failure modes:
  - Recompiled top-level .md not produced at the expected path → `STATUS: failed`, reason `/glyph:compile produced no top-level .md`. The closure-wide scrub is what makes this signal trustworthy — without it, the recursive copy of the original would falsely satisfy the existence check.
  - One-sided procedure file → folded into the review report as a flagged difference; status stays `differences` rather than `failed`.
  - Partial-pipeline success (LLM phases failed but deterministic .md was emitted) is not a separate failure mode — the resulting drift surfaces through `decompile_review` and is reported as `differences`.

  The compile sub-agent never restores anything: the workspace is isolated and originals are untouched regardless of how the flow terminates.

- **decompile-subagent-contract**

  The decompile sub-agent's contract:

  Inputs (passed by the orchestrator):
  - The absolute path of the external original .glyph (source of truth, never written to).
  - The absolute path of the external original .md (source of truth, never written to).
  - The absolute path of the workspace root `<workspace>/` (a recursive copy of the closure root, under the archive).
  - The entry's relative path inside the workspace.

  Task:
  1. Scrub the generated-artifact slots in the workspace. Entry-level (the primary signal): delete `<workspace>/<entry-relpath>.glyph`, `<workspace>/<entry-relpath>.ir.json`, and `<workspace>/<entry-relpath-stem>/` if they exist — so a failed decompile cannot satisfy the recovered-file existence check against a stale copy of the original. Closure-wide for archive hygiene: for every non-entry `<closure-relpath>.glyph` in the import closure, also delete `<workspace>/<closure-relpath>.md`, `<workspace>/<closure-relpath>.ir.json`, and `<workspace>/<closure-relpath-stem>/`. Keep `<workspace>/<entry-relpath>.md` — that is the input the decompile flow renames to `old_<entry-basename>.md` and reverse-maps from.
  2. Run `/glyph:decompile source_md=<workspace>/<entry-relpath>.md target=<workspace>/<entry-relpath>.glyph`.
  3. Answer the decompile flow's non-interactive prompts: `inline` when its internal `compile()` call asks "sub-agent or inline?", and `no` when its final `finalize_filenames_and_skill_name` step asks whether to revert filenames on failure or non-equivalence. Never escalate these prompts to the orchestrator or the human user. If a future version of the decompile flow surfaces an unknown prompt, default to the least-destructive choice and note the prompt verbatim in the report.
  4. Verify `<workspace>/<entry-relpath>.glyph` exists after the pipeline. Because step 1 deleted the copied original, presence here proves the decompile wrote new output. If missing, set `STATUS: failed` with reason `decompile did not produce a recovered .glyph`, capture the pipeline output, and skip the remaining steps.
  5. Run the independent validity check: `glyph check --strict <workspace>/<entry-relpath>.glyph`, capturing exit code and stderr. `glyph check --strict` is the deterministic write-free CLI command — it does not overwrite the decompile flow's internal recompile artifacts, which stay in the archive for inspection. Write the captured stderr to `<archive>/<file-slug>/decompile/validity-check.stderr`. If the exit code is non-zero, set `STATUS: failed` with reason `recovered .glyph fails deterministic check: <verbatim stderr>` and skip `glyph_review`.
  6. On zero exit, invoke `glyph_review(original_glyph=<external original>.glyph, roundtrip_glyph=<workspace>/<entry-relpath>.glyph)` and write the report to `<archive>/<file-slug>/decompile/report.txt`.
  7. Return a single trailing message in the contract shape: `STATUS: <pass|differences|failed>` / `FILE: <relpath of the original .glyph>` / `DIRECTION: decompile` / verbatim review body or diagnostic.

  Failure modes (all detected via exit code or filesystem state, not prose parsing):
  - Recovered .glyph not produced at the expected path → `STATUS: failed`, reason `decompile did not produce a recovered .glyph`. The scrub step is what makes this signal trustworthy — without it, the recursive copy of the original would falsely satisfy the existence check.
  - `glyph check --strict <recovered>` exits non-zero → `STATUS: failed`, reason `recovered .glyph fails deterministic check` plus verbatim stderr. `glyph_review` is not invoked because the recovered file is malformed.

  The decompile sub-agent never restores anything: the workspace is isolated and originals are untouched regardless of how the flow terminates.

- **archive-layout-reference**

  Archive directory layout (every run, rooted at `/tmp/glyph-roundtrip-<YYYYMMDD-HHMMSS>/`):

  REPORT.md                                       # same aggregate text printed to the user
  <file-slug>/                                    # one per .glyph entry (collision-proof: relpath-encoded + short hash)
    compile/
      reports.txt                                 # concatenated decompile_review outputs (top-level + every closure-owned procedure pair)
      workspace/                                  # recursive copy of import-closure root
        <entry-relpath>.glyph                     # original (copied; source of truth)
        <entry-relpath>.md                        # recompiled (written fresh — scrub deleted the copy of the original first)
        <entry-relpath>.ir.json                   # IR sidecar (informational only)
        <entry-relpath-stem>/<kebab>.md           # entry-owned procedure files, if any (regenerated)
        <library-relpath>.glyph                   # imported library (copied; source of truth)
        <library-relpath-stem>/<kebab>.md         # library-owned procedure files (regenerated; closure-wide scrub deleted copies first)
        <rest-of-closure-tree>/
    decompile/
      report.txt                                  # glyph_review output (absent if skipped)
      validity-check.stderr                       # stderr from `glyph check --strict <recovered>`
      workspace/
        <entry-relpath>.glyph                     # recovered (written by /glyph:decompile)
        <entry-relpath-dir>/old_<entry-basename>.md  # original .md, renamed in place by the decompile flow
        <entry-relpath>.md                        # decompile's internal recompile output (informational)
        <entry-relpath>.ir.json                   # informational only; presence does not classify success
        <rest-of-closure-tree>/
  _orphans/                                       # only populated if a sub-agent crashed mid-write

- **aggregate-report-shape**

  Aggregate report shape (printed to the user and written to `<archive>/REPORT.md`):

  Glyph round-trip test — N files tested, K skipped
  Archive: /tmp/glyph-roundtrip-<YYYYMMDD-HHMMSS>/

  PASS  fix_bug.glyph      compile: equivalent     decompile: equivalent
  DIFF  install.glyph      compile: 2 differences  decompile: equivalent
  FAIL  release.glyph      compile: equivalent     decompile: FAILED — <reason>
  SKIP  orphan.glyph       no sibling .md

  --- Differences ---

  install.glyph (compile):
    <verbatim decompile_review report body>

  release.glyph (decompile): FAILED
    <verbatim error captured from /glyph:decompile or `glyph check --strict`>

  Status keywords:
  - PASS: every direction returned `equivalent`.
  - DIFF: at least one direction returned differences; no failures.
  - FAIL: at least one direction returned `failed`.
  - SKIP: preflight skipped the file before any sub-agent dispatched.

  The orchestrator does not interpret differences — it relays each sub-agent's underlying review report verbatim so the user can judge whether each drift is acceptable.

### Steps

1. Follow the gather-glyph-files-and-preflight procedure below.
2. Decide which of the following applies and follow only that path:
   If every .glyph file was skipped during preflight:
   a. Print a short report to the user stating that no .glyph files survived preflight and listing every skipped entry under a `Skipped` heading with its skip reason. Do not create an archive directory — there is nothing to archive. The skill returns this short report as its output.
   Otherwise:
   a. Create the archive directory at `/tmp/glyph-roundtrip-<YYYYMMDD-HHMMSS>/` using the current local timestamp. Every per-file workspace and per-direction artifact for this run lives directly under this root, with no move-on-completion step. Record the absolute path of this directory — it appears in the aggregate report header and is returned as part of the skill's output.
   b. Follow the spawn-one-subagent-per-file-and-direction procedure.
   c. Scan {source} for any leftover `*.roundtrip.*` files or directories. Under normal operation this sweep finds nothing — every transient lives inside a workspace under the archive root. Strays appear only when a sub-agent crashed before its workspace setup completed, leaving copies behind in {source}. Move every such stray into `<archive>/_orphans/` rather than deleting it; preserving them lets the user investigate post-run.
   d. Print the aggregate report to the user as plain text in the shape defined by aggregate_report_shape, and write the same text to `<archive>/REPORT.md` for post-run inspection. Lead with the header line naming the count of files tested and skipped plus the archive directory's absolute path. List one row per .glyph entry showing per-direction status (`PASS` / `DIFF` / `FAIL` / `SKIP`). Under a `Differences` section, relay every sub-agent's verbatim review-report body — do not interpret, summarise, or normalise the contents. Under a `Failures` section, relay every sub-agent's verbatim error diagnostic. Under a `Skipped` section, list every preflight-skipped entry with its skip reason. The user judges whether each drift or failure is acceptable; the orchestrator is a relay, not an editor.

### Constraints

- Create a separate workspace under the archive root for each (file, direction) pair. Each workspace is a recursive copy of that entry's import-closure root and is the only filesystem location its pipeline writes to. Compile and decompile on the same file run in distinct workspaces, so they never race.
- Dispatch every (file, direction) sub-agent in a single parallel batch. Sequential dispatch defeats the purpose of the orchestrator and inflates wall-clock time. Wait for every sub-agent to finish before aggregating results.
- Relay every sub-agent's review report and error diagnostic verbatim into the aggregate report. The orchestrator is a relay, not an editor — the user judges whether each drift or failure is acceptable, and surface phrasing of the underlying reports must be preserved.
- You must treat every .glyph and .md file under {source} as read-only for the entire run. Every pipeline write — repair, recompile, decompile, internal renames — targets a workspace under the archive root. After any run, no original file has been modified relative to its pre-run content.
- You must never emit the aggregate report as JSON, code-fenced data, or any other structured payload. The aggregate report is plain prose for a human reader, and the underlying sub-agent reports it relays are themselves plain prose.

### Procedure: gather-glyph-files-and-preflight

1. Walk {source} recursively and collect every .glyph file. {source} may point at a single .glyph file (in which case the collection is that single file) or at a directory.
2. Determine the effective source boundary: when {source} is a directory, the boundary is {source} itself; when {source} is a single .glyph file, the boundary is its parent directory. This boundary is what `inside source` means for the rest of preflight, and a single-file `source` does not cause its own enclosing-directory closure to be classified as escaping.
3. For each collected .glyph, check that a sibling .md exists in the same directory with the same basename (e.g. `foo.glyph` pairs with `foo.md`). If the sibling is absent, skip the entry and record the reason `no sibling .md` — the round-trip test needs both anchors, and a library .glyph without a top-level .md is only exercised indirectly through its consumers.
4. For each surviving .glyph, compute its transitive import closure by walking every `import "..."` declaration. Resolve each relative import (`./...` or `../...`) against the importing file's directory and add the resolved absolute path to the closure; recurse into each imported file's own imports. Recognise and exclude virtual `@glyph/*` imports — those are compiler-shipped, not filesystem paths. If any relative import does not resolve to a file on disk, skip the entry and record the reason `unresolved import: <path>`.
5. For each entry whose closure resolved, determine the workspace root `D`: the smallest directory that contains every path in the closure plus the entry itself. If `D` is not inside the effective source boundary — i.e. the closure escapes upward past the boundary — skip the entry and record the reason `import closure escapes source root`.
6. For each surviving entry, compute a collision-proof file slug for the archive layout: take the entry's path relative to the effective source boundary, replace every `/` and `.` with `_`, and append a short hash (first 8 hex chars of the SHA-256 of the entry's absolute path). Two `fix_bug.glyph` files under different sub-directories of {source} produce distinct slugs.
7. Carry forward the per-entry tuple (entry path, sibling .md path, workspace root `D`, entry relpath inside `D`, file slug) for every surviving entry, plus the (entry path, skip reason) pair for every skipped entry. Both lists feed the dispatch and the aggregate report.

### Procedure: spawn-one-subagent-per-file-and-direction

1. Build the set of (file, direction) pairs to dispatch. For each surviving .glyph entry, include a `compile` pair when {direction} is `both` or `compile`, and include a `decompile` pair when {direction} is `both` or `decompile`. For N surviving entries in `both` mode that produces 2N pairs.
2. Before dispatching each pair, create its per-direction workspace by recursively copying the entry's workspace root `D` to `<archive>/<file-slug>/compile/workspace/` or `<archive>/<file-slug>/decompile/workspace/` as appropriate. Relative imports inside the workspace resolve only to other files in the workspace, never back to originals.
3. Dispatch every pair as a sub-agent in a single parallel batch — never sequentially. Pass each compile sub-agent the inputs and task described in compile_subagent_contract; pass each decompile sub-agent the inputs and task described in decompile_subagent_contract. Sub-agents share no context; each is self-contained.
4. Wait for every sub-agent to finish, then collect the single trailing message each returned in the contract shape (`STATUS:` / `FILE:` / `DIRECTION:` header plus a verbatim review or diagnostic body). A sub-agent that returns nothing is treated as `STATUS: failed` with reason `sub-agent terminated without report`. Carry the full set of sub-agent reports forward to the aggregate-report phase.

