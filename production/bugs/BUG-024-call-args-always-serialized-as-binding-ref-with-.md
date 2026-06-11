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

## Independent Agent Finding

**Verdict:** Reproduced / confirmed.

**Reproduction/Refutation:** Created a temporary repro file at
`tmp/bug024-repro.glyph` with a valid `produce("my_dir")` call and ran:

```bash
cargo run -q -p glyph-cli -- compile --emit-ir --format json tmp/bug024-repro.glyph
```

The command exited `0` and wrote `tmp/bug024-repro.ir.json`. Inspecting the emitted call
node with:

```bash
jq '.skill.flow[] | select(.kind == "call") | {node_id,target,args}' tmp/bug024-repro.ir.json
```

produced:

```json
{
  "node_id": "n2",
  "target": "produce",
  "args": {
    "my_dir": {
      "kind": "binding_ref",
      "name": "my_dir",
      "node_id": "n2_0"
    }
  }
}
```

A targeted node-id scan:

```bash
jq -r '.. | objects | select(has("node_id")) | .node_id' tmp/bug024-repro.ir.json \
  | rg -n -v '^n(0|[1-9][0-9]*)$'
```

reported `3:n2_0`, confirming the fabricated call-arg expression ID does not match the
documented node-id format.

**Evidence:** Graphify located the relevant implementation nodes:
`serialize_call()`, `FlowStmt`, and `IrCall`. Bounded source reads confirmed the pipeline
erases argument kind before IR emission:

- `crates/glyph-core/src/parse.rs:4089-4122` accepts both `TokenKind::Ident` and
  `TokenKind::StringLit` but pushes both into the same `Vec<String>`.
- `crates/glyph-core/src/ast.rs:371-395` stores `FlowStmt::Call.args` as `Vec<String>`.
- `crates/glyph-core/src/ir.rs:349-358` stores `IrCall.args` as `Vec<String>`.
- `crates/glyph-core/src/lower.rs:1210-1268` clones the AST args into the IR call.
- `crates/glyph-core/src/emit_ir.rs:320-340` emits every arg as
  `{"kind":"binding_ref","name":arg}` and fabricates `node_id` with
  `format!("n{}_{}", c.node_id.0, i)`.

The documented contract requires expression node IDs to use `n<integer>` and says every
`Expr` sub-node receives an ID (`docs/reference/ir-json.md:49`,
`docs/architecture/ir-schema.md:321`). It also defines string literals as
`{"kind":"literal","value":{"kind":"string","value":"."}}`
(`docs/reference/ir-json.md:506-525`).

**Resolution Input:** Preserve the existing recommended resolution. The evidence supports
the report's claim that this is a multi-layer fix: parse/AST/lower must preserve call-arg
kind before `serialize_call` can emit correct `binding_ref` vs `literal` expression shapes
and assign conforming real expression node IDs.
