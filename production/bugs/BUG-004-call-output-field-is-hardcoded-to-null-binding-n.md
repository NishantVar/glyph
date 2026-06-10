# BUG-004: Call `output` field is hardcoded to null; binding name only surfaces in undocumented `bound_name`

**Severity:** high | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/emit_ir.rs:342`
**Found by:** ir-emitir | **Audit date:** unknown-date

## Description

The documented IR-JSON contract (`docs/reference/ir-json.md` line 144 and `docs/architecture/ir-schema.md`) defines `Call.output` as the binding name when a flow assignment `x = call(...)` is written, else null. `serialize_call` always inserts `"output": Value::Null` and instead emits the real binding name under an undocumented field `bound_name` (from `IrCall.bound_name`).

Any consumer reading `output` per the published contract receives null for every assigned call, while the actual binding sits in a field that is not part of the documented schema. The corpus file `crates/glyph-cli/tests/corpus/valid/legal_cross_kind.ir.json` provides a live artifact: it contains `"bound_name": "link_mode"` and `"output": null` for a call that is clearly an assignment (`link_mode = ask_skills_link_mode(...)`). The only existing test asserts `call["bound_name"]` but never checks `call["output"]`, so the divergence from the documented schema goes untested.

## Trigger / Reproduction

Any skill flow with an assigned call, e.g.:

```
diagnosis = analyze_error(scope)
```

Running `glyph compile --emit-ir` produces a Call node with `"output": null` and `"bound_name": "diagnosis"`. Per the contract, `"output"` should be `"diagnosis"` and `"bound_name"` is undocumented.

## Evidence

```rust
// serialize_call always hardcodes output to null:
m.insert("output".into(), Value::Null);

// ...later, real binding name emitted under undocumented field:
m.insert(
    "bound_name".into(),
    match &c.bound_name {
        Some(n) => Value::String(n.clone()),
        None    => Value::Null,
    },
);
```

## Recommended Resolution

Populate `output` from `c.bound_name` instead of hardcoding null:

```rust
m.insert(
    "output".into(),
    match &c.bound_name {
        Some(n) => Value::String(n.clone()),
        None    => Value::Null,
    },
);
```

Then either drop the redundant `bound_name` field entirely or document it in `ir-json.md` as a deprecated alias. Update the existing test to assert `call["output"]` rather than `call["bound_name"]`.

## Verification Notes

Code at `emit_ir.rs` line 307 unambiguously hardcodes `"output": Value::Null` in `serialize_call`, while the lines that follow correctly populate `bound_name` from `c.bound_name`. The corpus fixture `legal_cross_kind.ir.json` confirms the behavior in emitted artifacts. The field `bound_name` appears nowhere in `ir-json.md`. The existing test covers `bound_name` but not `output`, so the contract violation is untested. Fix is a one-line change.
