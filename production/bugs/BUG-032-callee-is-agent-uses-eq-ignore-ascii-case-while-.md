# BUG-032: callee_is_agent uses eq_ignore_ascii_case while name_to_typetag canonicalizes underscores — Agent shape diverges from return_type

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/lower.rs:319-337`
**Found by:** lower-1 | **Audit date:** unknown-date

## Description

`callee_is_agent` decides the `IrCall.is_agent` flag with `rt.node.eq_ignore_ascii_case("Agent")`, but the sibling lowering path `name_to_typetag` (lines 55-77) classifies the very same `return_type` text via `canonicalize_identifier` (`domain_registry.rs` strips underscores AND lowercases). The comment in `name_to_typetag` explicitly records that the old `eq_ignore_ascii_case` approach was a bug (Issue #84 codex pass 3 F1) because it missed the underscore axis — but that fix was never applied to `callee_is_agent`.

Concrete trigger: a same-file block declared `block foo() -> A_gent { ... }` and called with a binding `x = foo()`. `name_to_typetag` yields `TypeTag::Agent` (so `IrCall.return_type == Some(Agent)`), yet `callee_is_agent` returns false because `"A_gent".eq_ignore_ascii_case("Agent")` is false (underscore present, lengths differ). Result: `is_agent == false` despite an Agent return type. `emit/scaffold.rs::naming_sentence_for_call` then renders the non-agent prose "Refer to this result as x." instead of "Refer to this agent as 'x.'", and `emit_ir.rs` L407 serializes `is_agent:false`. This is silently wrong compiled output and an internal inconsistency between two fields derived from the same source token.

## Trigger / Reproduction

Declare a block with a return type using underscore canonicalization: `block foo() -> A_gent { ... }` (valid per D6 canonicalization rules; `is_builtin_type_name("A_gent")` returns true via canonicalization, so no diagnostic fires). Then call it with a binding: `x = foo()`. Compiled output will render "Refer to this result as x." instead of "Refer to this agent as 'x.'".

## Evidence

```rust
if let Some(b) = blocks.get(target) {
    if let Some(rt) = b.return_type.as_ref() {
        return rt.node.eq_ignore_ascii_case("Agent");
    }
}
// vs name_to_typetag: let canonical = canonicalize_identifier(name); match canonical.as_str() { ... "agent" => TypeTag::Agent, ... }
```

## Recommended Resolution

Classify the return type with the same canonical form used by `name_to_typetag`: `return canonicalize_identifier(&rt.node) == "agent";` for both the block and export-block arms in `callee_is_agent`, so `is_agent` and `return_type` are always consistent. The same fix should be applied wherever `eq_ignore_ascii_case("Agent")` appears in the lower pass.

## Verification Notes

The grammar accepts identifiers with underscores in type positions without restriction, so `A_gent` is a valid parse. The analyzer does not reject it: `is_builtin_type_name("A_gent")` returns true (because `canonicalize_identifier` strips underscores, yielding `"agent"` which is in `CANONICAL_BUILTINS`). In `callee_is_agent`, `rt.node.eq_ignore_ascii_case("Agent")` returns false for `"A_gent"` (length 7 != 5), while `name_to_typetag("A_gent")` correctly yields `TypeTag::Agent` via canonicalization. Running the compiler on a file with `block foo() -> A_gent` / `x = foo()` produces "Refer to this result as x." instead of "Refer to this agent as 'x.'" — exactly the silent wrong output described. Lower.rs even has a comment at line 88 documenting that the `eq_ignore_ascii_case` approach was a bug for `name_to_typetag` (Issue #84), confirming the fix was known but never applied to `callee_is_agent`.
