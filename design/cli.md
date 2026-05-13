# Glyph CLI — v0 Surface

This document defines the command-line interface for the Glyph compiler, version 0. It covers subcommands, flags, output conventions, exit codes, and diagnostic formatting. The CLI is implemented in Rust using `clap` (derive macro flavor) for argument parsing and `codespan-reporting` for pretty diagnostics.

The compiler is **fully deterministic**. LLM-assisted phases — Phase 3 (Repair) and Phase 6 Step 2 (Expand reshaping) — are not part of the compiler binary. They are implemented as an external agent skill that invokes the CLI and handles LLM work between invocations. See `build-foundation.md` §Agent Workflow Summary for the full orchestration model.

## Subcommands

### `glyph compile <path>`

Run the compiler's deterministic phases: Parse (1), Analyze (2), Lower (4), Validate (5), Expand Step 1 (6-Step1), and Emit (7). Produces compiled `.md` files and optionally `.ir.json` sidecar files.

The compiler does **not** run Phase 3 (Repair) or Phase 6 Step 2 (Expand reshaping). Those are the agent's responsibility. If Phase 2 produces `repairable` diagnostics, the compiler stops after Phase 2 and exits with code 2. The agent performs LLM repair on the source and re-invokes. If Phase 2 is clean, the compiler continues through the remaining deterministic phases to produce output.

- `<path>` is a file (`*.glyph`) or directory. Directory mode globs `**/*.glyph` recursively.
- **Directory mode compiles every file in scope, unconditionally.** There is no reachability filter: a library file with no in-scope consumer still compiles (and may produce zero emitted artifacts, exit 0). Reachability-based pruning is post-MVP.
- Transitive dependencies are auto-discovered via DAG closure: if `a.glyph` imports `b.glyph`, the compiler processes `b` even if the user only named `a`. Already-valid cached dependencies may be skipped.
- Library files (zero `skill` declarations) that produce no `.md` output succeed silently (exit 0, info-level log at `-v`).

### `glyph check <path>`

Run Phases 1 (Parse) and 2 (Analyze) only. Reports all diagnostics — errors, repairable, and warnings — without continuing to Lower, Validate, Expand, or Emit. No output files are produced.

This is the fast lint mode: parse and analyze source, report what's wrong, exit. Useful for quick feedback loops and CI pre-checks. If the source passes `check` with exit 0, `compile` will proceed past Phase 2 (though post-Lower Phase 5 validation can still catch rare invariant violations).

Accepts the same `<path>` semantics as `compile` (file or directory, DAG closure).

### `glyph validate-output <ir-json-path> <md-path>`

Run the 24 deterministic Phase 6b structural checks against Step 2 output. Takes the resolved IR JSON (`foo.ir.json` from `--emit-ir`) and the agent-rewritten Markdown (`foo.md`) as positional arguments. Validates section shape, role preservation (step/constraint counts and ordering), parameter reference integrity, procedure section correctness, and content shape constraints.

This is a post-Step-2 validation gate: the agent invokes `glyph compile` (which runs Phases 1–7 and writes mechanical `.md` + `.ir.json`), performs LLM prose reshaping on the `.md`, then runs `validate-output` to confirm the rewritten `.md` still structurally matches the IR. See `agent-skill.md` §`glyph validate-output` for the full diagnostic catalog and workflow integration.

- Exit `0`: validation passed, `.md` is structurally correct.
- Exit `1`: structural violations found, diagnostics emitted.
- Exit `3`: invocation error (missing file, bad path, IO failure).

Accepts `--format` flag (same as `compile`/`check`) for diagnostic output format.

### `glyph fmt <path>`

Run Phase 3a (deterministic source rewrites) only. No LLM, no IR construction, no compiled output. Rewrites the `.glyph` source files in place.

`fmt` runs in **two strata** to maximise the work it can do on imperfect source:

1. **Pre-Parse text-level rewrites.** Operate on raw source text without an AST and run first, so they may turn a previously-rejecting source into one Phase 1 can accept.
   - Tab → 4-space conversion
   - Mixed indentation fix
2. **Post-Parse AST-level rewrites.** Require a successful Phase 1.
   - Duplicate import merging
   - Unused import removal

If Phase 1 succeeds (after the pre-Parse pass), both strata run and the file is rewritten. If Phase 1 fails after the pre-Parse pass, `fmt` emits the parse diagnostic and writes only the pre-Parse text fixes (if any); it does not perform the AST-level rewrites. This partial-success behaviour is intentional — pre-Parse fixes are often the prerequisite for the next `glyph compile` to even produce structured diagnostics.

**`fmt` does not reorder sections.** Sub-section order on disk is the author's source order; the compiler places each section at the canonical body position via the D9 merge algorithm (sections written explicitly in source anchor to their source position; synthetic sections — `Parameters`, hoisted `Constraints` / `Context` — slot into their default canonical position). The recommended source order is `description → effects → goal → constraints → context → flow`; authors who follow it get compiled body order matching the default. The parser itself remains permissive (see `language-surface.md` §2.5): unformatted source with sub-sections in any order still parses.

Analogous to `rustfmt` / `gofmt`. Fast, offline, idempotent.

## Flags

### Global flags (all subcommands)

| Flag | Short | Description |
|------|-------|-------------|
| `--help` | `-h` | Print help and exit |
| `--version` | `-V` | Print `glyph <version>` and exit |
| `-v` | | Set log level to info (phase boundaries, file processing) |
| `-vv` | | Set log level to debug (IR diffs, detailed phase output) |
| `--color <when>` | | Terminal color mode: `always`, `never`, `auto` (default: `auto`). Also respects `NO_COLOR` and `CLICOLOR` environment variables. |
| `--enable-effects` | | Enable the effects subsystem (parsing, inference, validation, output emission). Default: **off**. When off, `effects:` sub-sections are rejected at parse time (`G::parse::gated-section`), all effect-related diagnostics are suppressed, and the `effects` frontmatter field is omitted from compiled output. See `ir-and-semantics.md` §3. |

Logging uses verbosity-gated `eprintln!` to stderr. Default level is warn (errors and warnings only). `-v` adds info (phase start/end, files processed). `-vv` adds debug (IR snapshots, diagnostic details). No `RUST_LOG` or `tracing` dependency in v0; structured logging may be added post-MVP when incremental builds or watch mode warrant it.

### `compile` flags

| Flag | Short | Description |
|------|-------|-------------|
| `--out-dir <path>` | `-o` | Override output directory. Default: compiled `.md` lands next to its `.glyph` source. Procedure subdirectories are created relative to this location. |
| `--emit-ir` | | Emit the post-Step-1 resolved IR as a sidecar JSON file next to the compiled `.md` (e.g., `fix_bug.ir.json`). See §IR JSON Output. |
| `--format <fmt>` | `-f` | Diagnostic output format: `pretty` (default, uses `codespan-reporting`) or `json` (structured, for agent consumption). See §Diagnostic Output. |
| `--strict` | | Treat `repairable` diagnostics as hard errors: exit code 1 instead of 2. No `.md` output is written. Useful for CI gates and lint-clean enforcement. |

### `check` flags

| Flag | Short | Description |
|------|-------|-------------|
| `--format <fmt>` | `-f` | Diagnostic output format: `pretty` or `json`. |

### `fmt` flags

| Flag | Short | Description |
|------|-------|-------------|
| `--check` | | Don't write changes; exit 1 if any file would be reformatted. CI mode for formatting. |

## Output Directory Convention

By default, compiled files are placed next to their source:

```
project/
  skills/
    fix_bug.glyph      → fix_bug.md
    review_tools.glyph → review_tools.md
                             review_tools/          (procedure subdirectory)
                               review-code.md
```

With `--out-dir build/`:

```
project/
  skills/
    fix_bug.glyph
    review_tools.glyph
  build/
    fix_bug.md
    review_tools.md
    review_tools/
      review-code.md
```

Procedure subdirectories (for Tier 3 external-file projections) are always created relative to the output location, preserving the same relative structure.

## Exit Codes

| Code | Meaning | Agent action |
|------|---------|--------------|
| `0` | Success. `.md` (and `.ir.json` if `--emit-ir`) written. | Proceed to Expand Step 2 (LLM reshaping). |
| `1` | Hard errors. Cannot compile. | Surface diagnostics to author. Do not attempt repair. |
| `2` | Repairable diagnostics only. Pipeline stopped after Phase 2. | Agent performs LLM repair on source, re-invokes. |
| `3` | Invocation error. Bad flags, missing path, permission denied, IO failure. | Surface error to user. Stop. |

**`1` wins over `2`.** If both hard errors and repairable diagnostics exist, exit `1`. No point repairing if a hard error blocks compilation anyway.

**`glyph check`:** Same exit code semantics — 0 (clean), 1 (hard errors), 2 (repairable only), 3 (invocation error).

**`glyph fmt --check`:** Exit 0 if no changes needed, exit 1 if any file would be reformatted.

**`glyph fmt` (without `--check`):** Exit 0 on success, exit 3 for invocation/IO errors.

**`glyph validate-output`:** Exit 0 if validation passed, exit 1 if structural violations found, exit 3 for invocation errors.

## IR JSON Output

The `--emit-ir` flag on `compile` outputs the **post-Step-1 resolved IR** as a JSON file (`foo.ir.json`) alongside the compiled `.md`. This is the IR after Expand Step 1 (deterministic resolution) — it includes `resolved_body_text`, `projection_mode`, `site_modifier`, and other resolved fields from `ir-schema.md` §Resolved IR.

The JSON uses a **nested tree** shape (children inlined under parents) rather than a flat arena dump. Each node carries its `node_id` as an attribute. This shape is natural for the agent to read during Expand Step 2 reshaping.

The agent reads the IR JSON, performs LLM reshaping (Step 2) with full structural context — including `with` modifiers, roles, constraint attributes — and writes the final polished `.md`.

**Library files produce no IR JSON.** The IR JSON envelope is rooted in `Skill` per `ir-json-schema.md`, and a library file (zero `skill` declarations, only blocks/text/imports) has no Skill to root the envelope on. `--emit-ir` is therefore a silent no-op for IR on library files: the compiler does not write `foo.ir.json`, and the stdout NDJSON wrapper still emits a `{"file": ..., "diagnostics": [], "emitted": [...]}` line listing any procedure `.md` artifacts produced. Library IR caching is deferred until incremental compilation exists; until then it would be dead bytes with no consumer.

**Not available on `check`.** `check` runs only Phases 1-2 and does not reach Expand Step 1, so it cannot produce the resolved IR shape.

## Diagnostic Output

### Channel discipline

| Channel | Content | When |
|---------|---------|------|
| **stdout** | `error` + `repairable` diagnostics (JSON) | `--format=json` only |
| **stderr** | `warning` diagnostics + fatal compiler errors | Always (pretty-printed via `codespan-reporting`) |
| **stderr** | All diagnostics (pretty-printed) | `--format=pretty` (default) |

In **pretty mode** (default): all diagnostics go to stderr. Standard CLI behavior for humans.

In **JSON mode** (`--format=json`): actionable diagnostics (`error` + `repairable`) go to **stdout** as structured JSON for agent consumption. Warnings and fatal compiler errors (IO failures, internal bugs) go to stderr. This lets the agent pipe stdout cleanly without parsing human-readable noise.

### Pretty format example

```
error[G::analyze::undefined-call]: unresolved call `inspect_failure`
  ┌─ skills/fix_bug.glyph:6:9
  │
6 │         inspect_failure(scope) with "focus on auth boundaries"
  │         ^^^^^^^^^^^^^^^ no declaration found for this name
  │
  = hint: repair will generate a definition for this call
```

### JSON format shape

Output uses **NDJSON** (newline-delimited JSON): one complete JSON object per line, no top-level array wrapper, line-buffered to stdout. Files emit in **topological compile order**, matching the order the compiler processes them in (per `pipeline.md` §Multi-File Compilation Order). The agent reads stdout incrementally without waiting for build completion.

Each line is a complete `{"file": ..., "diagnostics": [...], "emitted": [...]}` object:

```jsonl
{"file":"skills/lib/util.glyph","diagnostics":[],"emitted":["skills/lib/util.md"]}
{"file":"skills/fix_bug.glyph","diagnostics":[{"id":"G::analyze::undefined-call","classification":"repairable","message":"unresolved call `inspect_failure`","span":{"file":"skills/fix_bug.glyph","start":{"line":6,"col":9},"end":{"line":6,"col":23}},"related":[],"hints":["repair will generate a definition for this call"]}],"emitted":[]}
```

The `diagnostics` array matches the `Diagnostic` shape defined in `diagnostics.md`. The `emitted` array lists output paths produced for that file (compiled `.md`, `.ir.json` if `--emit-ir`, procedure `.md` files); it is empty when the file did not progress past Phase 2.

Each file's diagnostics are grouped into a single JSON object on a single line so consuming tools know when a file's set is complete: the line terminator is the boundary marker.

## Multi-File Behavior

When `<path>` is a directory or when named files have imports:

1. The compiler discovers all `.glyph` files in scope (directory glob or DAG closure from named roots).
2. Files are processed in topological order per `pipeline.md` §Multi-File Compilation Order.
3. Partial failure follows `pipeline.md` §Partial Failure Policy: failed files skip their dependents, successful files still emit output.
4. Diagnostics are emitted per-file as each file completes (streaming in pretty mode, one JSON object per file in JSON mode).
5. Exit code: `0` only if every file succeeds. `1` wins over `2` — if any file has hard errors, the entire build exits `1` even if other files only have repairable diagnostics.

## Pipeline Stop Behavior

The compiler stops after Phase 2 (Analyze) if repairable diagnostics exist. It does **not** continue to Lower/Validate/Emit on a dirty AST. Each re-invocation after agent repair runs the full pipeline from scratch. This guarantees that diagnostics are always accurate — later phases never see broken input from earlier phases.

## On-Failure Disk State Guarantee

`compile` uses an **atomic-rename pattern** for both compiled `.md` and emitted `.ir.json`. Phase 7 writes outputs to `foo.md.tmp` and `foo.ir.json.tmp` first, then renames to the final paths only after the entire pipeline succeeds for that file. On hard-fail (exit `1`) or repairable-stop (exit `2`), tmp files are deleted and any prior successful outputs from a previous build are left untouched. The user never observes a half-written `.md` or a stale `.ir.json` paired with a fresh `.md`. See `pipeline.md` §Phase 7 Emit for the full contract.

## Stdin

Not supported in v0. The compiler requires file paths for import resolution, diagnostic spans, and output placement. A `--stdin --filename <virtual>` mode may be added post-MVP if editor integration demands it.

## What Is Not In v0

The following CLI features are explicitly deferred:

- **`glyph init`** — project scaffolding / config file generation.
- **`glyph watch`** — file-watching with incremental recompilation.
- **`glyph lsp`** — Language Server Protocol integration.
- **Incremental compilation** — the pipeline supports it architecturally (`pipeline.md` §Cacheability) but v0 re-runs all phases on every invocation.
- **SARIF output** — standardized static analysis format. `--format=json` covers tooling needs for now.
- **Config file** (`glyph.config.yaml` or similar) — project-level compiler settings. v0 uses flags only.
- **Manpages** — `--help` is the documentation for v0.
- **Stdin support** (`--stdin --filename <virtual>`) — for editor integration; blocked on virtual file path semantics for imports and diagnostics.
- **`tracing` / structured logging** — v0 uses verbosity-gated `eprintln!`. Add `tracing` when incremental builds or watch mode warrant structured log filtering.
- **Embedded LLM mode** — v0 compiler is fully deterministic. LLM phases (Repair, Expand Step 2) live in an external agent skill. An embedded mode (`glyph-llm` crate) may be added post-MVP if single-binary deployment is needed.

## Implementation Notes

- **Crate layout:** `glyph-cli` (binary) + `glyph-core` (library). See `build-foundation.md` §A1.
- **Arg parsing:** `clap` with derive macros.
- **Diagnostics rendering:** `codespan-reporting` for pretty stderr output.
- **Logging:** Verbosity-gated `eprintln!`. `-v` = info, `-vv` = debug. Default = warn.
- **Color detection:** `codespan-reporting` respects `--color` flag and `NO_COLOR` / `CLICOLOR` environment variables.
- **Binary name:** `glyph`. Single binary, subcommand dispatch.
- **Dependencies:** `serde`, `serde_json`, `clap`, `codespan-reporting`. See `build-foundation.md` §Dependencies.

## Cross-References

- **Build foundation:** `build-foundation.md` — crate layout, IR representation, agent workflow, dependency inventory.
- **Pipeline:** `pipeline.md` — phase definitions, multi-file order, partial failure, cacheability.
- **Diagnostics:** `diagnostics.md` — diagnostic shape, classification, ID scheme.
- **Repair:** `repair.md` — repair sub-steps (3a/3b/3c), LLM calls, convergence.
- **Compiled output:** `compiled-output.md` — file naming, procedure subdirectories, output structure.
- **IR schema:** `ir-schema.md` — JSON shape for `--emit-ir` output, resolved IR fields.
- **Agent skill:** `agent-skill.md` — agent workflow, repair guidance, Step 2 rules, `validate-output` integration, Phase 6b diagnostic catalog.
- **Expand:** `expand.md` — Phase 6b validation gate, structural diagnostic IDs used by `validate-output`.
- **Visualization:** `todo.md` §Visualization — `--emit-ir` is the v0 answer to external visualization tooling.
