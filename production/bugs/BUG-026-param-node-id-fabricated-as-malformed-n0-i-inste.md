# BUG-026: Param node_id fabricated as malformed `n0_<i>` instead of a real arena id

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/emit_ir.rs:690-696`
**Found by:** ir-emitir | **Audit date:** unknown-date

## Description

When serializing skill params, the emitter synthesizes the param node_id as `format!("n0_{}", i)` because `IrParam` carries no `node_id`. This violates the documented node-id format `n<u32>` (docs/architecture/ir-schema.md §Format) and the allocation contract "Every `Param` on a declaration" receives a real pre-order ID. The IR-JSON worked example shows params with normal ids (`"node_id": "n1"`). Trigger: any skill with parameters. Consumers that cross-reference node ids (e.g. diagnostics quoting `n<id>`, validate-output) cannot resolve `n0_0`-style ids, and ids are not stable arena references.

## Trigger / Reproduction

Any `.glyph` skill with one or more parameters. The emitted IR JSON will contain param objects with `node_id` values like `"n0_0"`, `"n0_1"` rather than the correct `"n1"`, `"n2"` etc. Since `IrParam` has no `node_id` field, no real arena id is available to emit.

## Evidence

```rust
let param_nid = format!("n0_{}", i);
serialize_param(p, &param_nid)
```

## Recommended Resolution

Add a `node_id: NodeId` field to `IrParam`, allocate it in pre-order during Lower (as is done for every other IR node), and emit it via `node_id_str` so param ids conform to `n<u32>` and are real arena references. Manually-crafted test fixtures in `validate_output.rs` (lines 2488, 2669-2670, 2706) already use the correct `"n1"`, `"n2"` format for params — the emitter should match.

## Verification Notes

`IrParam` at `ir.rs:197-206` has no `node_id` field (only `name`, `default`, `description`, `type_annotation`), unlike every other IR node struct (e.g. `IrInlineInstruction` at L210, `IrConstraint` at L224). The fabricated `n0_X` format diverges from the documented `n<u32>` canonical form and from the manually-crafted test fixtures. Severity is medium rather than high because current runtime consumers (`validate_output.rs`) operate on param `name` fields and do not cross-reference by `node_id` — so the malformed IDs cause a spec violation but do not crash or silently corrupt compiled markdown output today. No existing test asserts on param `node_id` values produced by the real compiler, so the violation is undetected at runtime.

## Independent Agent Finding

**Verdict:** Reproduced. The public `glyph compile --emit-ir` path emits fabricated parameter `node_id` values of the form `n0_<index>`, which violates the documented `n<u32>` node-id shape.

**Reproduction/Refutation:** Created a minimal scratch skill under `tmp/bug-026-independent/` with two parameters and one flow instruction, then compiled it through the CLI with IR emission:

```bash
cargo run -q -p glyph-cli --bin glyph -- compile --emit-ir --out-dir tmp/bug-026-independent/out tmp/bug-026-independent/param_node_id_repro.glyph
jq -r '.skill.params[] | [.name, .node_id, .kind] | @tsv' tmp/bug-026-independent/out/param_node_id_repro.ir.json
```

Observed param rows:

```text
scope	n0_0	param
mode	n0_1	param
```

A focused scan of all emitted object `node_id` fields that do not match `^n[0-9]+$` returned only:

```text
n0_0
n0_1
```

`glyph validate-output` on the generated `.ir.json` and `.md` exited 0, so the current validator accepts this malformed ID shape today.

**Evidence:** Graphify identified `IrParam` and the CLI/IR JSON path as relevant. Bounded exact checks found `IrParam` at `crates/glyph-core/src/ir.rs:197-206` has no `node_id` field, `serialize_param(param: &IrParam, node_id: &str)` writes the passed string into JSON, and `crates/glyph-core/src/emit_ir.rs:694` still contains `format!("n0_{}", i)`. The documentation still requires every IR node, including `Param`, to carry a `node_id` of the form `n<integer>`.

**Resolution Input:** The existing recommended resolution remains appropriate: add a real `node_id: NodeId` to `IrParam`, allocate it during Lower, and serialize it with `node_id_str`. Add a compiler-path regression test that compiles a skill with parameters and asserts the emitted param IDs match `^n[0-9]+$` and are not synthetic `n0_<index>` strings; optionally add an IR JSON validation check so `validate-output` or an adjacent validator rejects malformed `node_id` values instead of silently passing them.
