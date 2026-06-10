# BUG-059: Parameter-description multi-line threshold uses byte length, not character count (doc says 120 chars)

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/emit/templates.rs:60`
**Found by:** emit-scaffold | **Audit date:** unknown-date

## Description

`render_param_bullet` selects the multi-line bullet form when `desc_text.len() > 120`. `str::len()` returns the byte length in Rust, but the docstring (line 38) and intent are "exceeds 120 chars." For descriptions containing multi-byte UTF-8 (accented Latin, CJK, emoji), the byte count exceeds 120 well before 120 characters, causing such a description to be rendered in the multi-line block form earlier than intended.

The skill emitter uses the identical byte check (`scaffold.rs:1135` `desc_text.len() > 120`), so the two paths agree with each other but both diverge from the documented char-based rule. Impact is formatting-only (single-line vs multi-line bullet); no content is lost.

## Trigger / Reproduction

Author a param with a description containing multi-byte UTF-8 characters (e.g. CJK or emoji) whose byte length exceeds 120 but whose character count does not. The param bullet will be rendered in multi-line form when single-line form was intended.

## Evidence

```rust
Some(desc_text) if desc_text.contains('\n') || desc_text.len() > 120 => {
    // multi-line form
}
```

## Recommended Resolution

Replace `desc_text.len() > 120` with `desc_text.chars().count() > 120` in both locations so both paths match the documented char-based rule:

- `crates/glyph-core/src/emit/templates.rs:60`
- `crates/glyph-core/src/emit/scaffold.rs:1135`

## Verification Notes

Both `templates.rs` line 60 and `scaffold.rs` line 1135 use `desc_text.len() > 120` (byte count). The docstring at `templates.rs` line 38 explicitly states "exceeds 120 chars", making the intent clear. For ASCII-only descriptions the two are equivalent, but for multi-byte UTF-8 descriptions the byte count can exceed 120 well before 120 logical characters are reached. The proposed fix — `desc_text.chars().count() > 120` at both sites — is correct and complete.
