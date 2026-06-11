# BUG-035: `type` decl omitted from section-reset list, so its description string is mis-highlighted as a flow/context string

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/semantic_tokens.rs:288-293`
**Found by:** semtok-typepos | **Audit date:** unknown-date

## Description

The lex pass tracks the active `Section` (None/Description/Context/Constraints/Flow/Effects) and only resets it to `None` when an indent-0 identifier is one of the top-level decl keywords in `is_top_level_decl_keyword`. That list (`skill | block | export | generated | const | import`) is missing `type`, which is a real top-level declaration keyword (`parse.rs` `RESERVED_KEYWORDS = ["type"]`, `ast.rs` `TypeDecl`).

Glyph type decls carry a descriptive string: `type RepoContext = <"...">`. The tokenizer lexes `<"...">` as separate `LAngle` + `StringLit` + `RAngle` tokens, so the inner `"..."` is a plain `StringLit`. When a bare `type` decl appears at indent 0 after a skill's `flow:` (or `context:`) section, the lex pass leaves `section == Flow`, and the StringLit is classified `GlyphFlowString` (or `GlyphContextString`) instead of plain `String`. The AST pass does nothing for type decls (`Decl::TypeDecl(_) => {}`, line 510), so the wrong classification survives `sort_and_dedup`.

`export type ...` is unaffected because `export` is in the reset list; only the bare `type` form leaks the prior section.

## Trigger / Reproduction

```
skill main()
    description: "d"
    flow:
        "hi"

type RepoContext = <"the inspected repo state">
```

The `"the inspected repo state"` string is emitted as `GlyphFlowString` (flow-string highlight color) instead of `String`.

## Evidence

```rust
fn is_top_level_decl_keyword(s: &str) -> bool {
    matches!(
        s,
        "skill" | "block" | "export" | "generated" | "const" | "import"
    )
}
```

`"type"` is absent from the match arm. When a bare `type` declaration appears at indent 0, the lex pass does not reset `section = Section::None`, leaving the prior section active for any tokens inside the type declaration.

## Recommended Resolution

Add `"type"` to the match arm in `is_top_level_decl_keyword`:

```rust
fn is_top_level_decl_keyword(s: &str) -> bool {
    matches!(
        s,
        "skill" | "block" | "export" | "generated" | "const" | "import" | "type"
    )
}
```

This ensures an indent-0 `type` declaration resets the section state to `None`, matching the other top-level decl keywords.

## Verification Notes

`is_top_level_decl_keyword` lists `"skill" | "block" | "export" | "generated" | "const" | "import"` with `"type"` absent. The lex pass at lines 248-252 resets `section = Section::None` only when this function returns true. In `parse.rs`, `RESERVED_KEYWORDS = &["type"]` confirms `type` is a real top-level decl keyword. The `StringLit` branch classifies any string as `GlyphFlowString` when `section == Section::Flow`. The AST pass has `Decl::TypeDecl(_) => {}` (no-op), so the wrong lex-pass classification survives. The fix of adding `"type"` to `is_top_level_decl_keyword` is correct and complete.

## Independent Agent Finding

**Verdict:** Reproduced / confirmed.

**Reproduction/Refutation:** I created a disposable scratch Cargo harness under `tmp/bug035-repro` that depends on the local `glyph-core` crate and calls `collect_semantic_tokens` on the report's trigger. The bare `type` declaration after a `flow:` section reproduced the incorrect classification; the string inside `<"...">` was emitted as `glyphFlowString` instead of plain `string`. A control case with `export type` emitted the type description string as plain `string`, matching the report's claim that `export` resets section state. A bare `type` after `context:` similarly emitted `glyphContextString`.

**Evidence:**

- Source inspection, bounded to `crates/glyph-core/src/semantic_tokens.rs`, found `is_top_level_decl_keyword` matches only `"skill" | "block" | "export" | "generated" | "const" | "import"`; `"type"` is absent.
- Targeted reproduction command:

```sh
CARGO_TARGET_DIR=tmp/bug035-repro/target cargo run --quiet --manifest-path tmp/bug035-repro/Cargo.toml
```

Output summary:

```text
case=bare_type_after_flow
line=5 start=20 length=26 type=11(glyphFlowString) modifiers=0
case=export_type_after_flow
line=5 start=0 length=6 type=0(keyword) modifiers=0
line=5 start=27 length=26 type=7(string) modifiers=0
case=bare_type_after_context
line=5 start=20 length=26 type=12(glyphContextString) modifiers=0
```

This confirms the report's root cause: an indent-0 bare `type` declaration does not reset the lex pass `section`, so the type description string inherits the preceding `flow:` or `context:` section classification.

**Resolution Input:** Preserve the existing recommended resolution: add `"type"` to `is_top_level_decl_keyword`. Add a focused semantic-token regression test for bare `type` after `flow:` and/or `context:`; keeping the `export type` control would guard the intended reset behavior.
