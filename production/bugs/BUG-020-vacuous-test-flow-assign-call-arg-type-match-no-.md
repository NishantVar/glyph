# BUG-020: Vacuous test: flow_assign_call_arg_type_match_no_diag source is indented and never parses, so the analyze type-match path it claims to pin is never exercised

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/analyze.rs:11174-11195`
**Found by:** analyze-4 | **Audit date:** unknown-date

## Description

The test `flow_assign_call_arg_type_match_no_diag` is intended to guard against false positives
in `G::analyze::call-arg-type-mismatch` (the negative counterpart of
`flow_assign_call_arg_type_mismatch_emits_diag` at line 11124). But its `src` string indents
every top-level declaration by 4 spaces.

Glyph requires top-level declarations at indent 0. `Parser::parse_file` returns
`Err(ParseError::Unexpected{..., "top-level declaration must be at indent 0"})` on the first
indented line. `parse_with_diagnostics_opts` catches that error, pushes a single
`G::parse::unexpected` (Repairable) diagnostic, and returns `None`. In
`check_source_with_effects`, the `if let Some(file) = parsed` guard is false, so
`analyze::analyze_with_diagnostics` is NEVER called and no `G::analyze::call-arg-type-mismatch`
can ever be emitted.

Result: `diag_ids(src)` returns `["G::parse::unexpected"]`, and the test's only assertion
`!ids.contains("G::analyze::call-arg-type-mismatch")` is satisfied vacuously. A regression
that makes the type-matching path wrongly fire `call-arg-type-mismatch` on matching
`RepoContext` types would NOT be caught — the test provides false coverage.

## Trigger / Reproduction

Run `cargo test flow_assign_call_arg_type_match_no_diag -- --nocapture`. The test passes
but only `G::parse::unexpected` is in the diagnostic bag — the analyze pass is never reached.
Any change to the call-arg type-matching logic (correct or broken) leaves this test
unaffected.

## Evidence

```rust
// analyze.rs line ~11175 — fixture with 4-space-indented top-level declarations:
let src = "\
    block produce(scope = \".\") -> RepoContext
        \"produce\"

    block consume(input: RepoContext = \"x\") -> Plan
        \"consume\"

    skill demo() -> Plan
        description: \"demo\"
        flow:
            ctx = produce(\".\")\
            plan = consume(ctx)\
            return plan\
";

// parse_file (parse.rs:799-804):
// let indent = self.expect_line_start()?;
// if indent != 0 { return Err(ParseError::Unexpected{ ... "top-level declaration must be at indent 0" }) }

// parse_with_diagnostics_opts: Err(e) => { push G::parse::unexpected; return None }

// check_source_with_effects (lib.rs:252):
// if let Some(file) = parsed { analyze... }  // None => analyze skipped entirely
```

## Recommended Resolution

De-indent the fixture so every top-level declaration starts at column 0, matching its
sibling test at line 11124. Use a raw string to avoid escape confusion:

```rust
let src = r#"block produce(scope = ".") -> RepoContext
    "produce"

block consume(input: RepoContext = "x") -> Plan
    "consume"

skill demo() -> Plan
    description: "demo"
    flow:
        ctx = produce(".")
        plan = consume(ctx)
        return plan
"#;
```

After de-indenting, optionally add an assertion that no `G::parse::*` diagnostic is present
to lock the fixture as parseable so future indentation regressions surface immediately.

## Verification Notes

The full execution path was traced end-to-end and all claims confirmed. The tokenizer
converts 4 leading spaces to `LineStart { indent: 1 }` (no `BadIndent` because 4 is a
multiple of 4), but `parse_file` immediately returns `Err(ParseError::Unexpected)` on
seeing `indent != 0`. The test was run live and confirmed to pass while producing only
`G::parse::unexpected`. Every other test in the surrounding range uses column-0 declaration
headers; this is the only one with indented top-level declarations.

## Independent Agent Finding

**Verdict:** Reproduced. The current test is vacuous: it passes because the fixture fails
in Phase 1 parsing before `analyze_with_diagnostics` can evaluate call argument types.

**Reproduction/Refutation:** I inspected the current test fixture and confirmed
`flow_assign_call_arg_type_match_no_diag` still starts each top-level declaration with
four spaces, unlike `flow_assign_call_arg_type_mismatch_emits_diag`, whose declarations
start at column 0. A targeted test run passes, but a matching temporary fixture reports
only `G::parse::unexpected` for nonzero top-level indentation. A de-indented control
fixture reaches Phase 2 analysis, proving the indentation is what prevents the original
negative test from exercising the analyzer path.

**Evidence:**

- Graphify query for `flow_assign_call_arg_type_match_no_diag`,
  `parse_with_diagnostics_opts`, and `check_source_with_effects` identified the relevant
  nodes in `analyze.rs`, `parse.rs`, and `lib.rs`.
- `rg -n -C 18 "flow_assign_call_arg_type_(mismatch_emits_diag|match_no_diag)|fn diag_ids" crates/glyph-core/src/analyze.rs`
  showed the positive sibling fixture starts at column 0, while the negative fixture at
  `analyze.rs:11175-11188` has four leading spaces before `block`/`skill`.
- `sed -n '786,812p' crates/glyph-core/src/parse.rs` confirmed `parse_file` rejects any
  top-level `LineStart` where `indent != 0` with the message
  `top-level declaration must be at indent 0, got {indent}`.
- `sed -n '232,270p' crates/glyph-core/src/lib.rs` confirmed analysis runs only under
  `if let Some(file) = parsed`, so a parse failure skips `analyze_with_diagnostics`.
- `cargo test -p glyph-core flow_assign_call_arg_type_match_no_diag -- --nocapture`
  exited 0 and reported
  `test analyze::flow_assign_tests::flow_assign_call_arg_type_match_no_diag ... ok`
  with `1 passed; 0 failed; 910 filtered out`.
- `cargo run -p glyph-cli -- check tmp/bug020-indented.glyph` exited 2 and emitted a
  single diagnostic:
  `warning[G::parse::unexpected]: top-level declaration must be at indent 0, got 1`
  at `tmp/bug020-indented.glyph:1:1`.
- `cargo run -p glyph-cli -- check tmp/bug020-deindented.glyph` reached analysis and
  emitted `G::analyze::typed-decl-missing-return` diagnostics for `produce` and `consume`,
  with no `G::parse::unexpected` and no `G::analyze::call-arg-type-mismatch`.

**Resolution Input:** Preserve the suggested resolution: de-indent the fixture so the
test reaches analysis, and add an assertion that no `G::parse::*` diagnostic is present.
Under the current analyzer, the de-indented fixture may still produce
`G::analyze::typed-decl-missing-return` because the blocks declare return types without
explicit value-producing `return` statements. If the intent is a fully clean negative
fixture, add explicit returns or otherwise align the fixture with current typed-block
rules; at minimum, the parse-diagnostic assertion prevents this exact vacuous coverage
from recurring.
