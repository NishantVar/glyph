---
name: compile
description: Use when the user invokes /glyph:compile on a Glyph source file. Runs the full Glyph pipeline — compile, deterministic fmt, LLM repair loop, constraint conflict scan, prose reshape, validate-output — and surfaces the final compiled `.md` to the user.
---

## Parameters

- **source_path** (required)

## Instructions

### Steps

1. Run `glyph compile {source_path} --format json --emit-ir`. The NDJSON diagnostics on stdout, the exit code, and any IR sidecar `.ir.json` next to the source are the inputs to the next steps.
2. Follow the run-repair-loop procedure below.
3. Follow the scan-constraint-conflicts procedure below.
4. Follow the reshape-prose procedure below.
5. Run `glyph validate-output {source_path}.ir.json {source_path}.md`. On exit 0, the build is done. On exit 1, retry the prose reshape pass with the structural diagnostics as revise-with-feedback input. Budget at most 2 retries before surfacing the failure verbatim, and return the absolute path to the final compiled .md file as your result.

### Procedure: run-repair-loop

1. If the most recent `glyph compile` exited 0, this block is a no-op — the mechanical `.md` and `.ir.json` are already written.
2. If it exited 1 (hard errors) or 3 (invocation error), surface the diagnostics verbatim and stop the pipeline.
3. If it exited 2 (repairable diagnostics), run `glyph fmt {source_path}` to apply the deterministic Phase 3a auto-fixes.
4. Re-invoke `glyph compile {source_path} --format json --emit-ir`. If the diagnostics now resolve to exit 0, return.
5. If repairable diagnostics persist, apply the Phase 3b LLM repair pass to {source_path} using the NDJSON diagnostics on stdout, then re-invoke `glyph compile`.
6. Iterate at most 3 times per file. On the 4th attempt, hard-fail and surface the residual diagnostics verbatim.

### Procedure: scan-constraint-conflicts

1. Read the IR sidecar `.ir.json` written by the successful compile.
2. For each declaration with two or more constraints, classify each pair as `contradiction`, `tension`, or `none`.
3. On any contradiction, hard-fail and surface the conflict to the author.
4. Tensions surface as warnings; the build proceeds.

### Procedure: reshape-prose

1. Read the mechanical `.md` written by the compiler and the resolved IR sidecar `.ir.json`.
2. Rewrite the `## Parameters` descriptions using each parameter's name, type, default, and usage context. Do not add, remove, or rename parameters.
3. Rewrite the `## Instructions` section into human-quality prose, following the role-preservation, constraint-wording, parameter-reference, and procedure-reference rules from `design/agent-skill.md`.
4. Leave the YAML frontmatter exactly as the compiler emitted it.

