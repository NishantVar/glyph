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
