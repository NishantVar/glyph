# BUG-074: JSON-mode channel discipline violated: warnings go to stdout, not stderr

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `docs/reference/cli.md:170-178`
**Found by:** x-contract | **Audit date:** unknown-date

## Description

`cli.md` §Diagnostic Output specifies that in JSON mode (`--format=json`) only `error` + `repairable` diagnostics go to stdout as JSON, while `warning` diagnostics go to stderr (pretty-printed). The CLI does not partition by classification: `emit_diagnostics` passes the entire sorted `DiagBag` — including `warning`-class diagnostics — to `render_ndjson`, which writes every diagnostic as JSON to stdout (`main.rs:701-733`). A successfully-compiling file that emits a warning (e.g. `G::analyze::generic-type-name`, `G::analyze::inconsistent-type-spelling`) lands in `FileOutcome::Compiled{diagnostics}` and its warning is serialized to stdout in JSON mode. An agent piping stdout expecting only actionable error/repairable JSON gets warnings mixed in, and never sees the documented stderr warning stream.

The existing integration test suite (`check_subcommand.rs` lines 79-95 and `output_flag.rs` `out_dir_outside_root_warning_in_json`) explicitly asserts that warnings appear on stdout in JSON mode. The behavior is intentional by the tests; the divergence is a documentation bug in `cli.md`, not a code bug.

## Trigger / Reproduction

Run `glyph compile --format=json` on a skill that produces a `warning`-class diagnostic (e.g. `G::analyze::generic-type-name`). Observe that the warning NDJSON line appears on stdout, contradicting `cli.md`'s specification that warnings go to stderr in JSON mode.

## Evidence

```markdown
<!-- cli.md lines 170-178 (documented contract) -->
| **stdout** | `error` + `repairable` diagnostics (JSON) | `--format=json` only |
| **stderr** | `warning` diagnostics + fatal compiler errors | Always (pretty-printed) |

In **JSON mode** (`--format=json`): actionable diagnostics (`error` + `repairable`) go to
**stdout** as structured JSON for agent consumption. Warnings and fatal compiler errors
(IO failures, internal bugs) go to stderr.
```

```rust
// main.rs emit_diagnostics (lines 701-708) — no classification filter applied
fn emit_diagnostics(bag: DiagBag, format: OutputFormat, ...) {
    let sorted = bag.sorted(); // includes Classification::Warning entries
    match format {
        OutputFormat::Json => render_ndjson(&sorted), // ALL diags to stdout
        OutputFormat::Pretty => render_pretty(&sorted, &mut stderr),
    }
}
```

## Recommended Resolution

Amend `cli.md` §Diagnostic Output to state that ALL diagnostic classifications (error, repairable, and warning) are serialized to stdout as NDJSON in JSON mode. Adding a filter in code to suppress warnings from stdout would break the existing integration tests in `check_subcommand.rs` and `output_flag.rs` that explicitly expect warning-class diagnostics on stdout in JSON mode.

## Verification Notes

The code trace is unambiguous: `emit_diagnostics` calls `bag.sorted()` with no classification filter and passes the full result to `render_ndjson` in JSON mode. Warning-tier diagnostics like `G::analyze::generic-type-name` and `G::analyze::unused-import` are confirmed reachable. The test suite actively asserts warnings on stdout in JSON mode, confirming this is intentional behavior — the documentation is the sole outlier. The fix is a doc update only.
