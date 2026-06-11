# BUG-057: Sentence-terminal punctuation check is ASCII-only; non-ASCII terminators get a spurious trailing period

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/emit/constraint.rs:25-27`
**Found by:** emit-core | **Audit date:** unknown-date

## Description

`ends_with_sentence_punctuation` matches only ASCII `.`, `!`, `?`. A constraint whose body ends in non-ASCII sentence-final punctuation (e.g. CJK full stop `。` U+3002, full-width `！`/`？`, or an ellipsis `…`) is treated as unterminated, so `render` appends an ASCII `.`, producing output like `**Must:** 保持专注。.` with a redundant period.

The same ASCII-only test recurs in `emit/mod.rs:179` (procedure context preamble) and `scaffold.rs:933`, so the defect is consistent across constraint and context rendering.

Trigger: any constraint or context body written in a language that uses non-ASCII terminal punctuation.

## Trigger / Reproduction

Compile a `.glyph` file containing a `require` or `must` constraint whose string value ends in a CJK full stop (`。`), full-width exclamation mark (`！`), full-width question mark (`？`), or horizontal ellipsis (`…`). The emitter appends an ASCII period to the already-terminated sentence.

## Evidence

```rust
fn ends_with_sentence_punctuation(text: &str) -> bool {
    matches!(text.chars().last(), Some('.') | Some('!') | Some('?'))
}
```

## Recommended Resolution

Extend the `matches!` arm in `constraint.rs` to include `Some('。') | Some('！') | Some('？') | Some('…')` so the common non-ASCII sentence terminators are recognized:

```rust
fn ends_with_sentence_punctuation(text: &str) -> bool {
    matches!(
        text.chars().last(),
        Some('.') | Some('!') | Some('?') | Some('。') | Some('！') | Some('？') | Some('…')
    )
}
```

Note: the fix description originally claimed identical duplicates exist in `emit/mod.rs:179` and `scaffold.rs:933`, but the audit grep found `ends_with_sentence_punctuation` is only defined and called in `constraint.rs`. No changes are needed in `mod.rs` or `scaffold.rs`.

## Verification Notes

End-to-end compilation of a `.glyph` file with `const focus_cjk = "保持専注。"` followed by `require focus_cjk` produced `**Require:** 保持専注。.` — the CJK full stop (U+3002) is not recognized, so an ASCII period is appended. The tokenizer stores raw UTF-8 bytes unchanged through to the emitter, so non-ASCII const values pass through. The fix is straightforward: add the four common non-ASCII terminators to the single `matches!` call in `constraint.rs`.

## Independent Agent Finding

**Verdict:** Reproduced.

**Reproduction/Refutation:** I created a temporary `tmp/bug057/repro.glyph` skill with four `require` constraints whose const values ended in `。`, `！`, `？`, and `…`, then compiled it with:

```sh
cargo run -q -p glyph-cli -- compile tmp/bug057/repro.glyph --output tmp/bug057/repro.md
```

The command exited 0 and produced redundant ASCII periods after all four already-terminal non-ASCII punctuation marks.

**Evidence:** Graphify located `ends_with_sentence_punctuation()` at `crates/glyph-core/src/emit/constraint.rs:25`. An ast-grep check found the implementation still matches only `Some('.') | Some('!') | Some('?')`. The generated Markdown contained:

```md
- **Require:** 保持专注。.
- **Require:** 保持专注！.
- **Require:** 保持专注？.
- **Require:** 保持专注….
```

This confirms the reported emitter behavior through the real CLI path.

**Resolution Input:** Preserve the existing recommended resolution: extend the single `matches!` arm in `constraint.rs` to include the common non-ASCII terminators `。`, `！`, `？`, and `…`. I did not edit source code.
