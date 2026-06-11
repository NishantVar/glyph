# BUG-062: Undocumented `callee_description` field emitted on non-inline calls

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/emit_ir.rs:467-469`
**Found by:** ir-emitir | **Audit date:** unknown-date

## Description

For non-inline (Tier 2/3) projections, `serialize_call` inserts a `callee_description` key when the callee block has a description. This field is absent from the documented Call schema in `docs/reference/ir-json.md §Call` (which lists `callee_flow`/`callee_context`/`callee_constraints`/`callee_output_contract`/`procedure_path` but not `callee_description`). It is similarly absent from `docs/architecture/ir-schema.md §ResolvedCall`.

Like `predicate_shape` (BUG-061) this is additive and non-breaking under the versioning policy, but it is emitter/contract drift. The field is intentional and tested, which makes documenting it the preferred resolution over removal.

Trigger: any same-file block call promoted to Tier 2/3 whose block has a `description:`.

## Trigger / Reproduction

Compile with `--emit-ir` a `.glyph` file containing a call to a same-file block that has a `description:` section and is resolved to Tier 2 or Tier 3. The resulting Call node in the IR JSON will contain a `callee_description` key not present in the documented schema.

## Evidence

```rust
if let Some(ref desc) = block.description {
    m.insert("callee_description".into(), Value::String(desc.clone()));
}
```

## Recommended Resolution

Document `callee_description` in `docs/reference/ir-json.md §Call` as an optional additive field (present when the callee block has a `description:`, omitted otherwise), and add it to `docs/architecture/ir-schema.md §ResolvedCall`. Removal is not recommended because the field is already exercised by existing tests.

## Verification Notes

The code at `emit_ir.rs` lines 437-439 conditionally inserts `callee_description` for non-inline calls. The test file `crates/glyph-cli/tests/emit_ir.rs` explicitly tests that this field is present when a block has a description and absent otherwise — confirming the behavior is intentional. A grep across all `docs/` and `design/` directories returns zero matches for `callee_description`, confirming the field is entirely undocumented. Non-breaking under the versioning policy (additive, consumers must ignore unknown fields), hence low severity.

## Independent Agent Finding

**Verdict:** Reproduced.

**Reproduction/Refutation:** I compiled a minimal scratch fixture with `glyph compile --emit-ir` containing a same-file `block review_code()` with a `description:` and four flow statements, called once from `skill fix()`. The emitted `tmp/bug062_repro.ir.json` first flow node was a `call` with `projection_mode: "same_file_procedure"` and `callee_description: "Review code thoroughly."`. This confirms the trigger for a non-inline same-file call.

**Evidence:**

- `cargo run -q -p glyph-cli --bin glyph -- compile tmp/bug062_repro.glyph --emit-ir` exited 0.
- `jq '{source_file, first_call: (.skill.flow[0] | {kind, target, projection_mode, callee_description, callee_flow_type: (.callee_flow | type), has_callee_description: has("callee_description")})}' tmp/bug062_repro.ir.json` reported `kind: "call"`, `target: "review_code"`, `projection_mode: "same_file_procedure"`, `callee_description: "Review code thoroughly."`, `callee_flow_type: "array"`, and `has_callee_description: true`.
- `rg -n "callee_description" docs design` exited 1 with no matches, while targeted source inspection found the only emitter insertion at `crates/glyph-core/src/emit_ir.rs:439` and existing CLI tests assert both present and absent behavior in `crates/glyph-cli/tests/emit_ir.rs`.

**Resolution Input:** Preserve the existing suggested resolution: document `callee_description` as an optional additive Call field in `docs/reference/ir-json.md` and add it to `docs/architecture/ir-schema.md` `ResolvedCall`; do not remove the emitter behavior because it is intentional and already tested.
