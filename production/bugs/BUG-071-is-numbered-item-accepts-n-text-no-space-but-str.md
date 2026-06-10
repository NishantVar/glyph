# BUG-071: is_numbered_item accepts `N.text` (no space) but strip_number_prefix only strips `N. ` (with space), leaving the marker in the item text

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/validate_output.rs:268-292`
**Found by:** validate-1 | **Audit date:** unknown-date

## Description

`is_numbered_item` returns true as soon as it sees a digit run followed by `.` — it does not require a following space, so `"1.text"` is accepted as a numbered item. `strip_number_prefix` (lines 286-292) strips using `s.find(". ")` which requires a dot followed by a space; for `"1.text"` there is no `". "`, so it returns the string unstripped, leaving `"1.text"` as the step text. The two helpers disagree on what constitutes a valid numbered marker. This produces a `ListItem` whose `text` still contains the `1.` prefix, which can then break downstream prose/identifier matching in `check_step_order` (the leading `1.` becomes part of the matched text). Only reachable on agent-reshaped Markdown that omits the space after the dot (the emitter always emits `N. `), so practical impact is limited, but it is a genuine internal inconsistency.

## Trigger / Reproduction

An LLM-reshaped compiled `.md` file where a numbered step is written without a space after the dot:

```markdown
1.First step text
2.Second step text
```

`is_numbered_item("1.First step text")` returns `true`; `strip_number_prefix("1.First step text")` returns `"1.First step text"` unchanged. The resulting `ListItem.text` contains the marker, breaking downstream step-order matching.

## Evidence

```rust
// is_numbered_item: returns true at first '.' regardless of following space
for c in chars {
    if c == '.' { return true; }
    if !c.is_ascii_digit() { return false; }
}

// strip_number_prefix: requires ". "
if let Some(pos) = s.find(". ") {
    s[pos + 2..].to_string()
} else {
    s.to_string()  // marker left in text for "1.text" inputs
}
```

## Recommended Resolution

Make both helpers agree on the marker boundary. Either:

1. Update `is_numbered_item` to require a space (or end of string) after the `.`:
   ```rust
   if c == '.' { return chars.next().map_or(true, |next| next == ' '); }
   ```
2. Or update `strip_number_prefix` to strip up to and past the `.` even without a following space:
   ```rust
   if let Some(pos) = s.find('.') { s[pos + 1..].trim_start_matches(' ').to_string() }
   ```

Option 1 is preferable as it keeps the stricter/correct definition consistent with what the emitter always produces.

## Verification Notes

Code inconsistency confirmed at lines 268-292. `is_numbered_item` accepts `"1.text"` (no guard on trailing space); `strip_number_prefix` uses `find(". ")` and returns input unchanged for `"1.text"`. Both `parse_md_structure` and `body_h2_items` use this pair. Impact is limited: `check_step_order` uses substring `contains()` matching so most cases still match; the only realistic hard breakage is the `starts_with("Output:")` check at line 1721 for a `return` node. The emitter always produces `N. ` with space, so this is only reachable on non-standard agent-reshaped Markdown.
