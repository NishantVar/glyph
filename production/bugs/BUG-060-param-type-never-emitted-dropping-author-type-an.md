# BUG-060: Param `type` never emitted, dropping author type annotations from IR JSON

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/emit_ir.rs:181-195`
**Found by:** ir-emitir | **Audit date:** unknown-date

## Description

`serialize_param` never emits the `type` field; an inline comment states "type: omitted when duck-typed (always omitted in current IR)". But `IrParam` carries `type_annotation: Option<String>` lowered from the source `name: Type` form, and the contract (`docs/reference/ir-json.md §Param`) defines `type | TypeTag | omitted when duck-typed` — i.e. it must be present when the author annotated a type.

Trigger: any typed param such as `ctx: RepoContext`. The annotation is silently dropped from `--emit-ir` output even though it exists in the IR, so downstream tooling reading the IR JSON cannot see param types.

## Trigger / Reproduction

Compile a `.glyph` file containing `skill fix_bug(ctx: RepoContext = <"the repository context">, ...)` with `--emit-ir`. The output param JSON for `ctx` will have no `"type"` field despite the `: RepoContext` annotation in source.

## Evidence

```rust
// type: omitted when duck-typed (always omitted in current IR)
if let Some(ref default) = param.default {
    // ... emit "default" field
}
// param.type_annotation is never read here
```

## Recommended Resolution

When `param.type_annotation` is `Some`, emit the `type` field per §TypeTag Serialization. Because `IrParam.type_annotation` is `Option<String>` (raw annotation text), the fix requires converting it to a `TypeTag` first:

1. Make `name_to_typetag` `pub(crate)` in `lower.rs`.
2. In `serialize_param`, add:
   ```rust
   if let Some(ref ann) = param.type_annotation {
       m.insert("type".into(), typetag_to_json(&crate::lower::name_to_typetag(ann)));
   }
   ```

Alternatively, change `IrParam.type_annotation` from `Option<String>` to `Option<TypeTag>` and update `lower.rs` to store the already-lowered `TypeTag` — this is cleaner since `lower.rs` already calls `name_to_typetag` for skill/block return types.

## Verification Notes

Running `--emit-ir` on a file with `risk: RiskLevel = "medium"` confirms the output param JSON has no `"type"` field, even though `IrParam.type_annotation` is `Some("RiskLevel")`. The `serialize_param` function never reads `param.type_annotation`, confirmed by the comment "always omitted in current IR". The IR JSON contract in `docs/reference/ir-json.md §Param` explicitly specifies `"type" | TypeTag | no | Omitted when duck-typed`, making this a real contract violation. Severity is low because type annotations are currently informational-only and the compiled `.md` output is unaffected.
