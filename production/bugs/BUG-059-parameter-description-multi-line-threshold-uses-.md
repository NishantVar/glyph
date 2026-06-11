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

## Independent Agent Finding

**Verdict:** Reproduced. The current compiler renders a multi-line parameter bullet when the description is below the documented 120-character threshold but above 120 UTF-8 bytes.

**Reproduction/Refutation:** I created a scratch file at `tmp/bug059-param-threshold.glyph` with one required parameter whose description is 61 copies of U+754C (`界`). That description has 61 characters and 183 UTF-8 bytes. Compiling it with the current CLI rendered the parameter in multi-line form:

```markdown
- **topic**:
  <61 x U+754C>
  Required.
```

Under the documented "exceeds 120 chars" rule, this should have remained the single-line form because 61 characters is not greater than 120.

**Evidence:** Graphify located the relevant implementation nodes at `crates/glyph-core/src/emit/templates.rs` and `crates/glyph-core/src/emit/scaffold.rs`. `ast-grep` found two current threshold checks:

```text
crates/glyph-core/src/emit/templates.rs:60
desc_text.len() > 120

crates/glyph-core/src/emit/scaffold.rs:1135
desc_text.len() > 120
```

Targeted commands run:

```text
python3 -c 's="界"*61; print("chars=%d bytes=%d" % (len(s), len(s.encode("utf-8"))))'
=> chars=61 bytes=183

cargo run -q -p glyph-cli -- compile tmp/bug059-param-threshold.glyph --format json
=> exit 0

sed -n '1,80p' tmp/bug059-param-threshold.md
=> emitted `## Parameters` with `- **topic**:` followed by an indented description line and `Required.`
```

**Resolution Input:** Keep the existing suggested resolution: replace both `desc_text.len() > 120` checks with a character-count check such as `desc_text.chars().count() > 120`, in both `templates.rs` and `scaffold.rs`, so both emitter paths continue to agree while matching the documented character threshold.
