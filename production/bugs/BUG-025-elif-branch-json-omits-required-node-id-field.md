# BUG-025: elif_branch JSON omits required `node_id` field

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/emit_ir.rs:571-583`
**Found by:** ir-emitir | **Audit date:** unknown-date

## Description

`serialize_elif` comments "ElifBranch doesn't have its own node_id in current IR; generate a synthetic one" but then never inserts any `node_id` key. The IR-JSON contract lists `node_id | string | yes (required)` for ElifBranch (docs/reference/ir-json.md §ElifBranch, with worked example `"node_id": "n9"`), and docs/architecture/ir-schema.md §Allocation states every ElifBranch receives an ID. Root cause: the `IrElifBranch` struct in ir.rs has no `node_id` field, so emit cannot produce one. Any branch with an `elif` arm produces an `elif_branch` JSON object missing a contract-required field, breaking consumers that key/validate elif nodes by id.

## Trigger / Reproduction

Any `.glyph` file containing a branch with an `elif` arm. The emitted `elif_branch` object in the IR JSON will be missing the required `node_id` field. For example, compiling `predicate_const_multi_arm.glyph` (a corpus fixture) and inspecting the `.ir.json` output confirms the object contains `body`, `condition`, `kind`, and `predicate_shape` but no `node_id`.

## Evidence

```rust
// ElifBranch doesn't have its own node_id in current IR; generate a synthetic one.
m.insert("kind".into(), Value::String("elif_branch".into()));
m.insert("condition".into(), Value::String(elif.condition.clone()));
```

## Recommended Resolution

Add a `node_id: NodeId` field to `IrElifBranch`, allocate it during Lower's pre-order traversal, and emit it as `node_id_str(elif.node_id)` in `serialize_elif`. The existing test at `emit_ir.rs:1412–1415` checks `predicate_shape` on the elif object but does not assert presence of `node_id` — a test assertion should be added as well.

## Verification Notes

Running the actual compiler on `predicate_const_multi_arm.glyph` confirms the bug: the emitted `elif_branch` object contains `body`, `condition`, `kind`, and `predicate_shape` fields but no `node_id`. The IR JSON contract in `docs/reference/ir-json.md` §ElifBranch marks `node_id` as required, and `docs/architecture/ir-schema.md` §Allocation explicitly states every `ElifBranch` inside a `Branch` receives an ID. `IrElifBranch` at `ir.rs:469–482` has only three fields (`condition`, `body`, `predicate_shape`/`classification`) — no `node_id` field — confirming the root cause. The proposed fix is correct and complete.

## Independent Agent Finding

**Verdict:** Reproduced. The production bug is valid: emitted IR JSON for an `elif_branch` omits the contract-required `node_id` field.

**Reproduction/Refutation:** Copied `crates/glyph-cli/tests/fixtures/predicate_const_multi_arm.glyph` to `tmp/bug-025/predicate_const_multi_arm.glyph` and ran `cargo run -q -p glyph-cli -- compile tmp/bug-025/predicate_const_multi_arm.glyph --emit-ir`. The compile exited `0` and wrote `tmp/bug-025/predicate_const_multi_arm.ir.json`. Inspecting the emitted `elif_branch` with `jq '.. | objects | select(.kind? == "elif_branch")' tmp/bug-025/predicate_const_multi_arm.ir.json` showed no `node_id`; `jq -r '.. | objects | select(.kind? == "elif_branch") | keys | @json' ...` returned `["body","condition","kind","predicate_shape"]`, and `jq -r '[.. | objects | select(.kind? == "elif_branch") | has("node_id")] | @json' ...` returned `[false]`.

**Evidence:** Graphify located the relevant implementation path at `serialize_elif()` in `crates/glyph-core/src/emit_ir.rs`, `IrElifBranch` in `crates/glyph-core/src/ir.rs`, and the IR schema allocation docs. A bounded ast-grep read of `serialize_elif()` confirmed it inserts `kind`, `condition`, `body`, and `predicate_shape`, but never inserts `node_id`. A bounded ast-grep read of `IrElifBranch` confirmed the struct has `condition`, `body`, `predicate_shape`, and `classification`, but no `node_id`. The contract still requires the field: `docs/reference/ir-json.md` §ElifBranch lists `node_id` as required, and `docs/architecture/ir-schema.md` §Allocation says every `ElifBranch` inside a `Branch` receives an ID.

**Resolution Input:** Preserve the existing suggested resolution. Add `node_id: NodeId` to `IrElifBranch`, allocate it during Lower's pre-order traversal for each `elif` arm, emit it from `serialize_elif` as `node_id_str(elif.node_id)`, and add a regression assertion that every emitted `elif_branch` carries `node_id`.
