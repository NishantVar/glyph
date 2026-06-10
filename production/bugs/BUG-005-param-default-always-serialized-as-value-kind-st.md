# BUG-005: Param default always serialized as Value kind "string", mislabeling int/float/bool/none defaults

**Severity:** high | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/emit_ir.rs:181-195`
**Found by:** ir-emitir | **Audit date:** unknown-date

## Description

`serialize_param` strips surrounding quotes from the pre-rendered `IrParam.default` string and unconditionally emits `{"kind": "string", "value": raw}` for every parameter default, regardless of the actual type.

The language guide (GLYPH_LANGUAGE_GUIDE.md) states that defaults can be a literal (string, int, float, bool, `none`). The IR-JSON Value union (`docs/reference/ir-json.md` §Value Union) requires:
- int → `{"kind": "int", "value": 42}` (JSON number)
- float → `{"kind": "float", "value": 3.14}` (JSON number)
- bool → `{"kind": "bool", "value": true}` (JSON bool)
- none → `{"kind": "none"}`
- string → `{"kind": "string", "value": "text"}`

For non-string defaults (e.g. `attempts: Attempts = 3`), `strip_prefix('"')` fails and `unwrap_or(default)` keeps the raw string `"3"`, but the kind is still forced to `"string"` — producing `{"kind": "string", "value": "3"}` instead of `{"kind": "int", "value": 3}`. This is a silent correctness violation for every non-string default.

## Trigger / Reproduction

Any skill or block with a non-string parameter default:

```
skill retry(attempts: Count = 3, verbose: Bool = true, mode: Mode = none)
    flow:
        // ...
```

Running `glyph compile --emit-ir` produces param nodes with `"kind": "string"` for all three defaults instead of `"int"`, `"bool"`, and `"none"` respectively.

## Evidence

```rust
// serialize_param — unconditional "string" kind regardless of actual type:
let raw = default
    .strip_prefix('"')
    .and_then(|s| s.strip_suffix('"'))
    .unwrap_or(default);
m.insert("default".into(), json!({ "kind": "string", "value": raw }));
// For default "3": raw = "3", emitted as {"kind":"string","value":"3"} — wrong kind, wrong JSON type
// For default "true": raw = "true", emitted as {"kind":"string","value":"true"} — wrong kind
// For default "none": raw = "none", emitted as {"kind":"string","value":"none"} — wrong kind
```

## Recommended Resolution

Carry the value kind through from lowering rather than re-parsing the pre-rendered string at emit time. Options:

1. Change `IrParam.default` from `Option<String>` to `Option<IrValue>` (an enum with String/Int/Float/Bool/None variants) and emit the correct Value-union shape in `serialize_param`.
2. Alternatively, inspect the raw string at emit time: if it parses as an integer emit `{"kind":"int","value":<number>}`, if it parses as a float emit `{"kind":"float","value":<number>}`, if it is `"true"`/`"false"` emit `{"kind":"bool","value":<bool>}`, if it is `"none"` emit `{"kind":"none"}`, else treat as a string literal.

Option 1 is preferred as it avoids ambiguity between the string literal `"true"` and the bool `true`. The existing test for param defaults only covers a string default (`= "."`); add cases for int, float, bool, and none.

## Verification Notes

Code at `emit_ir.rs` lines 186–193 confirmed exactly as described. The lower.rs tests confirm the storage shape: `params[0].default.as_deref() == Some("true")` for bool, `Some("42")` for int, `Some("none")` for none. The IR-JSON spec explicitly requires typed Value-union shapes for non-string literals. The only existing emit-IR test for param defaults covers a string default, leaving all other types untested. Bug is silent — produces wrong-kind JSON without any error or warning.
