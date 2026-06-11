# BUG-003: collect_refs_* drops call-argument names, causing `glyph fmt` to delete imports referenced only as call arguments in skills/blocks

**Severity:** high | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/analyze.rs:7409-7504`
**Found by:** analyze-3 | **Audit date:** unknown-date

## Description

`collect_refs_from_flow_stmt` (FlowStmt::Call arm, line 7411–7413) and `collect_refs_from_return_expr` (ReturnExpr::Call arm, line 7496–7498) insert only `target.node` and never inspect the call's `args: Vec<String>`. Call arguments in Glyph may be bare name references (GLYPH_LANGUAGE_GUIDE §8.1: `plan = make_plan(ctx, risk)`), including references to imported consts.

These ref-collectors feed `FmtSignals.referenced_names` via `collect_refs_from_decl`. `glyph fmt`'s `remove_unused_imports` (fmt.rs:282–315, run unconditionally inside `ast_rewrite`) drops any selective import whose local name is absent from `referenced_names`. When an imported name appears only as a call argument, `referenced_names` never contains it, so the import is silently deleted — producing a file that no longer compiles due to `undefined-name` on the previously-imported identifier. This breaks formatter idempotence and behavior-preservation, and loses a live import.

Note: `ExportBlockDecl` escapes this because `parse_export_block`'s generic body walker harvests every ident on a line into `body_refs`. Skills and Blocks have structured flow with no such fallback.

## Trigger / Reproduction

A file with:
```
import "@acme/consts" { RISK_THRESHOLD }

skill assess(scope: Scope)
    flow:
        plan = make_plan(RISK_THRESHOLD)
```

Running `glyph fmt` silently deletes the `import "@acme/consts" { RISK_THRESHOLD }` line, producing a file that fails `glyph check` with `G::analyze::undefined-name` on `RISK_THRESHOLD`.

## Evidence

```rust
// collect_refs_from_flow_stmt — args ignored:
FlowStmt::Call { target, .. } => {
    out.insert(target.node.clone());
    // args: Vec<String> not visited; bare-name arg references lost
}

// collect_refs_from_return_expr — args ignored:
ReturnExpr::Call { target, .. } => {
    out.insert(target.node.clone());
    // args: Vec<String> not visited; bare-name arg references lost
}
```

## Recommended Resolution

The fix requires two coordinated changes:

1. **At parse time:** Add a parallel `Vec<bool>` (e.g. `args_is_name_ref`) or change `args` to `Vec<CallArg>` with an `Ident`/`Str` variant for `FlowStmt::Call` and `ReturnExpr::Call`, mirroring the `Param.default_is_name_ref` pattern. This is necessary because `args: Vec<String>` currently stores both identifier arguments and string literal arguments as undifferentiated raw strings — simply inserting all args would falsely treat string literal values as name references.

2. **In `collect_refs_from_flow_stmt` and `collect_refs_from_return_expr`:** Iterate only the args flagged as ident (name-ref) and insert them into `out`.

Add a regression test: a skill importing a const used only as a call argument should retain the import after `glyph fmt`.

## Verification Notes

Confirmed by code trace and live reproduction. `collect_refs_from_flow_stmt` at line 7409 and `collect_refs_from_return_expr` at line 7494 both use `..` to ignore `args` entirely. The AST definitions confirm `FlowStmt::Call` and `ReturnExpr::Call` carry `args: Vec<String>` that can contain bare identifier names parsed from `TokenKind::Ident` tokens. Running `glyph fmt` on a file with an import used only as a call argument silently deletes the import. The fix direction is correct but must include a structural discriminator at parse time to avoid treating string literal args as name references.

## Independent Agent Finding

### Verdict

Partially confirmed. Current `glyph fmt` does drop a selective import whose local name is used only as a call argument. However, I could not reproduce the report's downstream claim that the formatted file then fails `glyph check` or `glyph compile` with `G::analyze::undefined-name`; with the current `target/debug/glyph`, the post-format file checks and compiles successfully because call argument identifiers are not generally name-resolved.

### Reproduction/Refutation

Commands run from `/Users/nishantvarshney/genesis/glyph`:

- `mcp__graphify.query_graph("collect_refs call argument names causing -g BUG-003 references compilation pipeline arguments names")`: only shallow graph hits, so I used bounded source reads for exact implementation details.
- `target/debug/glyph check --format json tmp/bug003-repro/main.glyph` before formatting: exit 2 with `G::analyze::unused-import` for `risk_threshold`, even though the file contained `plan = make_plan(risk_threshold)`.
- `target/debug/glyph fmt tmp/bug003-repro/main.glyph`: exit 0.
- `sed -n '1,80p' tmp/bug003-repro/main.glyph` after formatting: the `import "./prefs.glyph" { risk_threshold }` line was removed while `make_plan(risk_threshold)` remained.
- `target/debug/glyph check --format json tmp/bug003-repro/main.glyph` after formatting: exit 0, no diagnostics.
- `target/debug/glyph compile --format json tmp/bug003-repro/main.glyph` after formatting: exit 0, no diagnostics.

### Evidence

Static code trace still supports the formatter-pruning root cause. Bounded reads of `crates/glyph-core/src/analyze.rs` showed `collect_refs_from_flow_stmt` inserts only `target.node` for `FlowStmt::Call { target, .. }`, and `collect_refs_from_return_expr` inserts only `target.node` for `ReturnExpr::Call { target, .. }`. Bounded reads of `crates/glyph-core/src/fmt.rs` showed `remove_unused_imports` keeps selective import names only when the local name is present in `signals.referenced_names`.

The AST/parser shape also supports the recommended fix direction: `FlowStmt::Call` and `ReturnExpr::Call` store `args: Vec<String>`, while parser slices showed both `TokenKind::Ident(a)` and `TokenKind::StringLit(a)` are pushed into the same vector. So collecting all args would preserve imports but would also falsely treat string literal arguments as references.

The undefined-name subclaim appears stale or unsupported by current behavior. `validate_call_args` only reports missing required positional args, and the call-argument analysis path only type-checks names already present in `scope.flow_local_types`; I did not find a general undefined-name check for call argument identifiers. That matches the live post-format `check` and `compile` exit 0 results.

### Resolution Input

Keep the existing suggested resolution for the formatter bug: preserve call argument kind at parse time, then have `collect_refs_from_flow_stmt` and `collect_refs_from_return_expr` collect only identifier arguments into `FmtSignals.referenced_names`. Add a regression test that `glyph fmt` preserves an import used only as a skill/block call argument. If the intended language contract is that bare call arguments must resolve to consts, params, or locals, track that as a separate analyzer bug or expand this fix to add explicit call-argument name resolution; the current formatter bug does not by itself make `glyph check` fail after pruning.
