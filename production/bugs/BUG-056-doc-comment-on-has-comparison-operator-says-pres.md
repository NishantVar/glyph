# BUG-056: Doc comment on has_comparison_operator says '== presence' but code also sets it for '!='

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/condition.rs:42-44`
**Found by:** expand-cond | **Audit date:** unknown-date

## Description

The field documentation states `has_comparison_operator` reflects '`==` presence', but the classifier sets the flag (and `has_boolean_token`) for both `==` and `!=` (condition.rs lines 444 and 476-478), and `classify_token` returns `Operator` for `!=` (line 348). The runtime behavior (treating `!=` as a comparison operator that disqualifies pure-predicate) is the correct one; the doc is stale/incomplete and will mislead maintainers reasoning about `!=` conditions. Pure documentation-vs-code divergence, no wrong output.

## Trigger / Reproduction

A maintainer reads the field doc `/// \`==\` presence; also sets \`has_boolean_token\` so pure-predicate fails`, then writes a test or analysis assuming that a condition containing only `!=` would NOT set `has_comparison_operator`. In fact it does — the code at lines 476-478 handles both `==` and `!=` identically. The stale doc can cause incorrect assumptions during code review or when reasoning about the pure-predicate classification path.

## Evidence

```rust
// condition.rs lines 42-44: field doc (stale)
/// `==` presence; also sets `has_boolean_token` so pure-predicate fails
/// closed.
pub has_comparison_operator: bool,

// condition.rs lines 444 and 476-478: actual classifier (correct)
if tok == "==" || tok == "!=" {
    // ...
} else if text == "==" || text == "!=" {
    summary.has_boolean_token = true;
    summary.has_comparison_operator = true;
}
```

## Recommended Resolution

Update the field doc to say '`==`/`!=` presence' so it matches the classifier, which handles both equality and inequality comparison operators:

```rust
/// `==`/`!=` presence; also sets `has_boolean_token` so pure-predicate fails
/// closed.
pub has_comparison_operator: bool,
```

## Verification Notes

Code at condition.rs lines 42-44 has doc comment mentioning only `==`, while lines 476-478 show the classifier sets both `has_boolean_token = true` and `has_comparison_operator = true` for the branch `text == "==" || text == "!="`. The runtime behavior is correct — both equality and inequality operators should disqualify pure-predicate conditions — but the doc comment only mentions `==`, making it stale and misleading for maintainers. No wrong output or crash; purely documentation-vs-code divergence.
