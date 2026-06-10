# BUG-024: Call args always serialized as binding_ref with malformed node_id; literal args mis-typed

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/emit_ir.rs:329-340`
**Found by:** ir-emitir | **Audit date:** unknown-date

## Description

In `serialize_call`, every positional arg in `c.args` is emitted as:

```json
{"node_id": "n<call>_<i>", "kind": "binding_ref", "name": arg}
```

Two defects:

1. **Malformed node_id:** The arg `node_id` is fabricated as
   `format!("n{}_{}", c.node_id.0, i)` (e.g. `n3_0`), which violates the documented
   node-id format `n<u32>` (docs/architecture/ir-schema.md §Format: "lowercase n followed
   by a non-negative decimal integer with no leading zeros"). Every arg expression sub-node
   should have a real pre-order arena ID.

2. **String literal args mis-typed:** String/literal arguments (e.g. `helper("foo")`) are
   mislabeled as `binding_ref` instead of the documented literal Expression shape
   `{"kind":"literal","value":{"kind":"string","value":"foo"}}` (docs/reference/ir-json.md
   §Expression Union). A downstream consumer treats `"foo"` as a reference to a binding
   named `foo`.

Trigger: any call passing a literal argument, e.g. `ctx = produce(".")`.

The root cause is deeper than `serialize_call`: the parse → AST → IR pipeline erases type
information by storing both identifier and string literal args as `Vec<String>` (`ast.rs`
`FlowStmt::Call.args`, `lower.rs` `IrCall.args`). By the time `serialize_call` runs it
cannot distinguish the two kinds.

## Trigger / Reproduction

Create a skill with a call passing a string literal argument (e.g.
`ctx = produce("my_dir")`). Run `glyph compile --emit-ir`. Inspect the IR JSON. The arg
will appear as `{"kind": "binding_ref", "name": "my_dir", "node_id": "n2_0"}` — both the
`kind` and the `node_id` format are wrong.

## Evidence

```rust
// emit_ir.rs lines 329-340:
let arg_node_id = format!("n{}_{}", c.node_id.0, i);
args_map.insert(
    arg.clone(),
    json!({
        "node_id": arg_node_id,   // e.g. "n2_0" — violates n<u32> format
        "kind": "binding_ref",    // wrong for string literals
        "name": arg
    })
);

// Documented correct shape for a string literal arg (ir-json.md §Expression Union):
// {"kind": "literal", "value": {"kind": "string", "value": "foo"}}
```

## Recommended Resolution

The fix requires changes at multiple layers:

1. **Preserve arg kinds through the pipeline:** Change `FlowStmt::Call.args` and
   `IrCall.args` from `Vec<String>` to `Vec<CallArg>` (an enum `Ident(String)` |
   `StringLit(String)`) so the parse-time type is preserved through AST lowering to emit.

2. **Fix `serialize_call`:** Allocate real pre-order arena `NodeId`s for arg expression
   sub-nodes (or store them as `IrNode` entries in the arena) so node_ids conform to
   `n<u32>`. Discriminate on `CallArg` to emit `binding_ref` vs `literal` expression shapes
   per the IR JSON contract.

## Verification Notes

Both defects were confirmed by running the compiler with `--emit-ir` on a file with a string
literal call arg. For `helper("my_dir")`, the IR JSON shows
`{"kind": "binding_ref", "name": "my_dir", "node_id": "n2_0"}` instead of the documented
literal shape. The fabricated `n2_0` format violates the ir-schema.md §Format spec. The
breakage is confined to `--emit-ir` JSON output consumed by the expand agent — wrong `kind`
discrimination causes the agent to misinterpret a string literal as a binding reference.
The fix is a multi-layer change: the arg type erasure at `Vec<String>` must be addressed
first; patching only `serialize_call` is insufficient.
