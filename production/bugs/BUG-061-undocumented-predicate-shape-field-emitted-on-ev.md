# BUG-061: Undocumented `predicate_shape` field emitted on every branch/elif

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/emit_ir.rs:557-564`
**Found by:** ir-emitir | **Audit date:** unknown-date

## Description

`serialize_branch` and `serialize_elif` emit a `predicate_shape` object (`has_boolean_token`, `has_predicate_token`, `has_compositional_operator`). This field is not part of the documented `Branch`/`ElifBranch` JSON shape in `docs/reference/ir-json.md` (Branch fields: `node_id`, `kind`, `condition`, `then_body`, `elif_branches`, `else_body`, `resolved_predicates`) nor `docs/architecture/ir-schema.md`.

Notably, the `IrBranch.predicate_shape` struct field itself is annotated `#[serde(skip)]` in `ir.rs`, signaling it was meant to stay out of JSON — but the manual emitter overrides this intent by explicitly inserting the key.

Per the versioning policy, additive fields are permitted without an `ir_version` bump and consumers must ignore unknown fields, so this is non-breaking. However, it is undocumented schema drift between the emitter and the published contract.

## Trigger / Reproduction

Run `--emit-ir` on any `.glyph` file containing a branch (`if`/`elif`). The resulting JSON will contain a `predicate_shape` object on every `Branch` and `ElifBranch` node that does not appear in the documented schema.

## Evidence

```rust
m.insert("predicate_shape".into(), serde_json::json!({
    "has_boolean_token": br.predicate_shape.has_boolean_token,
    "has_predicate_token": br.predicate_shape.has_predicate_token,
    "has_compositional_operator": br.predicate_shape.has_compositional_operator
}));
```

## Recommended Resolution

Either:

- **Document it:** Add `predicate_shape` to `docs/reference/ir-json.md §Branch` and `§ElifBranch` as an additive optional field, and update `docs/architecture/ir-schema.md` to match; or
- **Remove it:** Stop emitting `predicate_shape` in `serialize_branch` and `serialize_elif` to match the `#[serde(skip)]` annotation on the IR struct field.

## Verification Notes

`IrBranch.predicate_shape` and `IrElifBranch.predicate_shape` are both `#[serde(skip)]` in `ir.rs`, indicating developer intent to exclude the field from JSON. Yet `serialize_branch` and `serialize_elif` in `emit_ir.rs` explicitly insert it. The published contract in `ir-json.md §Branch` documents seven fields and `§ElifBranch` four fields — `predicate_shape` appears in neither. This is confirmed schema drift: every `--emit-ir` Branch and ElifBranch node carries an undocumented field. Non-breaking under the versioning policy (consumers must ignore unknown fields), hence low severity.
