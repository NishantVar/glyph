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

## Independent Agent Finding

**Verdict:** Reproduced. The report is valid.

**Reproduction/Refutation:** I compiled the existing typed-param fixture `crates/glyph-cli/tests/corpus/valid/type_level_lookup.glyph`, which contains `skill assess(risk: RiskLevel = "medium")`, with `--emit-ir` and wrote outputs only under `tmp/bug-060/`.

Command:

```sh
cargo run -q -p glyph-cli -- compile crates/glyph-cli/tests/corpus/valid/type_level_lookup.glyph --emit-ir --format json --output tmp/bug-060/type_level_lookup.md
```

The command exited 0 and emitted `tmp/bug-060/type_level_lookup.ir.json`. Inspecting the param node with:

```sh
jq '.. | objects | select(.kind? == "param")' tmp/bug-060/type_level_lookup.ir.json
```

produced:

```json
{
  "default": {
    "kind": "string",
    "value": "medium"
  },
  "kind": "param",
  "name": "risk",
  "node_id": "n0_0"
}
```

The source has `risk: RiskLevel`, but the emitted param JSON has no `"type"` field, so this reproduces the reported drop.

**Evidence:** Graphify located `serialize_param()` in `crates/glyph-core/src/emit_ir.rs` and `IrParam` in `crates/glyph-core/src/ir.rs`. Bounded source reads confirmed `IrParam` has `type_annotation: Option<String>`, lower copies `p.type_annotation` into `IrParam`, and `serialize_param` only inserts `node_id`, `kind`, `name`, and optional `default`. The IR JSON contract's `Param` table defines `type | TypeTag | no | Omitted when duck-typed`, and TypeTag serialization represents domain types as `{"domain_type": "repo_context"}` style objects.

**Resolution Input:** Keep the existing recommended resolution. Either emit `param.type_annotation` by converting the raw annotation through the same TypeTag conversion used for returns, or store `Option<TypeTag>` on `IrParam` during lowering and serialize that directly. No source-code changes were made by this reproduction pass.
