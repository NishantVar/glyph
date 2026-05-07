# Multi-line Selective Imports Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow selective-import brace lists `{ … }` in `.glyph` files to span multiple lines, so long import lists are readable and the cascading false-error class (`operator-in-expression` on later `->`, `output-target-outside-return` on later `<…>`) cannot be triggered by a wrapped import.

**Architecture:** Add `Parser::skip_line_starts`, a private helper that advances `self.pos` past consecutive `LineStart` tokens. Call it at three positions inside the `TokenKind::Lbrace` arm of `Parser::parse_import` (after `{`, after each `,`, before the closing-brace check). Items themselves remain atomic — `name`, `as`, `alias` must stay on a single line. Replace the final `expect(&Rbrace)` with a peek-and-match producing a clearer diagnostic on missing separators. No tokenizer, AST, IR, lowering, or LSP changes.

**Tech Stack:** Rust 2021. Hand-rolled recursive-descent parser at `crates/glyph-core/src/parse.rs`. Tests live inline in `parse.rs` under a `#[cfg(test)] mod <topic>_tests` block, mirroring `mod const_decl_tests` (line 3344). Test runner: `cargo test -p glyph-core`. Workspace gate: `cargo test --workspace`.

**Reference docs:**
- Design corrigendum: `docs/superpowers/specs/2026-05-06-multi-line-selective-imports-design.md`
- PRD: GitHub issue #116
- Implementation slice: GitHub issue #117

**Worktree:** `.claude/worktrees/multi-line-imports` on branch `worktree-multi-line-imports`. Baseline `cargo test --workspace` passing (643 tests, 0 failures) at commit `6d9ebf4`.

---

## File map

| File | Action | What it owns after this plan |
| --- | --- | --- |
| `crates/glyph-core/src/parse.rs` | Modify | New private method `Parser::skip_line_starts`; rewritten `TokenKind::Lbrace` arm of `parse_import` (lines 917-951); new `mod import_decl_tests` at the bottom (after the existing final `}` at line 5291). |
| `design/language-surface.md` | Modify | One additional rule bullet in §3.5 `import` (around lines 311-319) stating whitespace inside `{ … }` is non-significant and items must stay on a single line. |
| `GLYPH_LANGUAGE_GUIDE.md` | Modify | Multi-line example block added to §5.5 `import` Selective subsection (around line 211) alongside the existing single-line examples. |

No new files. No file moves. Tests live inline in `parse.rs`.

---

## Task 1: Scaffold `mod import_decl_tests` and drive the helper with the trailing-comma equivalence test

**Goal:** Stand up the test module, write the canonical "multi-line ≡ single-line" test as the failing test, then add `Parser::skip_line_starts` and the three call sites to make it pass.

**Files:**
- Modify: `crates/glyph-core/src/parse.rs` (new test module appended at end; new method on `Parser`; new code inside the `TokenKind::Lbrace` arm of `parse_import` at lines 917-951)

- [ ] **Step 1: Append the new test module to `parse.rs`**

The existing file ends at the closing `}` of `mod duplicate_subsection_recovery_tests` on line 5291. Append the following block immediately after that line (no blank-line trim needed; one blank line between modules is fine):

```rust
#[cfg(test)]
mod import_decl_tests {
    //! Issue #116 / #117 — selective-import brace list may span multiple lines.
    //!
    //! Verifies: the helper `Parser::skip_line_starts` is called at three
    //! positions inside the `TokenKind::Lbrace` arm of `parse_import`
    //! (after `{`, after each `,`, before the closing `}` check), and that
    //! items (`name`, optional `as <alias>`) remain atomic. Tests drive
    //! external parser behavior via `parse(...)` — they do not assert on
    //! token positions, byte ranges, or helper call counts.

    use super::*;
    use crate::ast::{Decl, ImportDecl, ImportKind};
    use crate::span::LineIndex;

    /// Parse `src` and return the first decl as an `ImportDecl`. Panics if
    /// the source fails to parse or the first decl isn't an import.
    fn parse_first_import(src: &str) -> ImportDecl {
        let (file, _) = parse(src, 0).expect("source should parse");
        match file.decls.into_iter().next().expect("expected one decl") {
            Decl::Import(spanned) => spanned.node,
            other => panic!("expected Decl::Import, got {:?}", other),
        }
    }

    /// Project a selective `ImportDecl` to `(path, [(name, alias), …])` so
    /// equivalence between single-line and multi-line forms can be asserted
    /// without coupling to outer-span byte ranges or future fields on
    /// `ImportName`.
    fn extract(d: ImportDecl) -> (String, Vec<(String, Option<String>)>) {
        let names = match d.kind {
            ImportKind::Selective(ns) => ns
                .into_iter()
                .map(|n| (n.name.node, n.alias))
                .collect(),
            other => panic!("expected ImportKind::Selective, got {:?}", other),
        };
        (d.path, names)
    }
}
```

- [ ] **Step 2: Add the trailing-comma-equivalence test inside the module**

Append this `#[test]` inside `mod import_decl_tests` (immediately above the closing `}`):

```rust
#[test]
fn multi_line_with_trailing_comma_equals_single_line() {
    let multi = "import \"./x.glyph\" {\n    a,\n    b,\n    c,\n}\n";
    let single = "import \"./x.glyph\" { a, b, c }\n";
    assert_eq!(
        extract(parse_first_import(multi)),
        extract(parse_first_import(single)),
    );
}
```

- [ ] **Step 3: Run the test and confirm it FAILS today**

Run:
```bash
cargo test -p glyph-core import_decl_tests::multi_line_with_trailing_comma_equals_single_line
```

Expected: FAIL. The first `parse(multi, 0).expect(...)` panics because `parse_import` aborts on the `LineStart` after `{`. Error string includes either `expected identifier` or `Unexpected` — exact wording isn't asserted.

- [ ] **Step 4: Add the `skip_line_starts` helper to `impl<'a> Parser<'a>`**

In `crates/glyph-core/src/parse.rs`, find the existing `impl<'a> Parser<'a>` block around line 474 (it currently starts with `fn peek`, `fn bump`, `fn at_eof`, `fn current_line_indent`, `fn expect_line_start`, …). Add `skip_line_starts` immediately after `expect_line_start` (around line 509):

```rust
/// Advance `self.pos` past consecutive `LineStart` tokens.
///
/// Used by callers that delimit a construct with a brace pair and treat
/// inner whitespace as non-significant. Today the only caller is the
/// selective-import branch of `parse_import` (issue #117). Items inside
/// such a construct remain atomic; the helper is called only between
/// items, never inside one. Safe at EOF: `peek()` returns the EOF
/// sentinel (not `LineStart`), so the loop terminates.
fn skip_line_starts(&mut self) {
    while matches!(self.peek().kind, TokenKind::LineStart { .. }) {
        self.pos += 1;
    }
}
```

- [ ] **Step 5: Rewrite the `TokenKind::Lbrace` arm of `parse_import`**

Replace the existing block at `crates/glyph-core/src/parse.rs:918-951` (the entire `TokenKind::Lbrace =>` arm of the `match &self.peek().kind` inside `parse_import`) with:

```rust
TokenKind::Lbrace => {
    // Selective import: `{ name1, name2 as alias2 }`.
    //
    // Whitespace inside `{ … }` is non-significant: line breaks and
    // indentation between import items are allowed; the brace pair is
    // the sole delimiter (`design/language-surface.md` §3.5). Items
    // (`name`, optional `as <alias>`) must stay on a single line —
    // `skip_line_starts` is intentionally NOT called inside an item.
    self.pos += 1; // consume `{`
    self.skip_line_starts();
    let mut names = Vec::new();
    if !matches!(self.peek().kind, TokenKind::Rbrace) {
        loop {
            let (name, name_span) = self.expect_ident(None)?;
            let alias = if let TokenKind::Ident(kw) = &self.peek().kind {
                if kw == "as" {
                    self.pos += 1;
                    let (alias_name, _) = self.expect_ident(None)?;
                    Some(alias_name)
                } else {
                    None
                }
            } else {
                None
            };
            names.push(ImportName { name: Spanned::new(name, name_span), alias });
            match &self.peek().kind {
                TokenKind::Comma => {
                    self.pos += 1;
                    self.skip_line_starts();
                    // Trailing comma before `}` (same- or different-line).
                    if matches!(self.peek().kind, TokenKind::Rbrace) {
                        break;
                    }
                }
                _ => break,
            }
        }
    }
    self.skip_line_starts();
    // Replaces the prior `self.expect(&TokenKind::Rbrace)?` with a
    // peek-and-match that emits a clearer diagnostic when the user
    // forgets a separator (e.g. two names on adjacent lines, no comma).
    if matches!(self.peek().kind, TokenKind::Rbrace) {
        self.pos += 1;
    } else {
        return Err(ParseError::Unexpected {
            span: self.peek().span,
            message: "expected ',' or '}' after import name".into(),
        });
    }
    ImportKind::Selective(names)
}
```

Note: the original code had `self.expect(&TokenKind::Rbrace)?;` *after* the arm's match block (it was the line outside the loop but inside the `Lbrace` arm). The replacement folds that check into the arm itself via the peek-and-match. The arm now ends with the `ImportKind::Selective(names)` value as the match expression result.

- [ ] **Step 6: Run the test again and confirm it PASSES**

Run:
```bash
cargo test -p glyph-core import_decl_tests::multi_line_with_trailing_comma_equals_single_line
```

Expected: PASS.

Then run the full crate test suite to confirm nothing else regressed:
```bash
cargo test -p glyph-core 2>&1 | tail -20
```

Expected: all tests pass (the test count rises by 1 vs. baseline).

- [ ] **Step 7: Commit**

```bash
git add crates/glyph-core/src/parse.rs
git commit -m "parse: allow multi-line selective imports via skip_line_starts (#117)

Add Parser::skip_line_starts helper called at three positions inside
the selective-import brace list: after {, after each ,, and before
the closing } check. Items remain atomic. Replace the final
expect(Rbrace) with a peek-and-match that emits 'expected ',' or '}'
after import name' on missing separators.

First test (multi-line with trailing comma equals single-line) now
passes. AST/IR/LSP/lowering unchanged."
```

---

## Task 2: Layout-coverage tests — no trailing comma, mixed layout, aliases across lines

**Goal:** Lock in three more shape variations that all pass with the helper from Task 1, so future regressions surface as red tests rather than silent shape changes.

**Files:**
- Modify: `crates/glyph-core/src/parse.rs` (new tests inside `mod import_decl_tests`)

- [ ] **Step 1: Add the no-trailing-comma test**

Inside `mod import_decl_tests`, append:

```rust
#[test]
fn multi_line_without_trailing_comma_parses() {
    let src = "import \"./x.glyph\" {\n    a,\n    b,\n    c\n}\n";
    let (path, names) = extract(parse_first_import(src));
    assert_eq!(path, "./x.glyph");
    let bare: Vec<&str> = names.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(bare, vec!["a", "b", "c"]);
    assert!(names.iter().all(|(_, alias)| alias.is_none()));
}
```

- [ ] **Step 2: Add the mixed-layout test**

```rust
#[test]
fn multi_line_mixed_layout_parses() {
    // Some names on the header line, more on subsequent lines, `}` on
    // its own line. Asserts the parser does not require a uniform layout.
    let src = "import \"./x.glyph\" { a, b,\n    c,\n    d,\n}\n";
    let (_, names) = extract(parse_first_import(src));
    let bare: Vec<&str> = names.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(bare, vec!["a", "b", "c", "d"]);
}
```

- [ ] **Step 3: Add the aliases-across-lines test**

```rust
#[test]
fn multi_line_aliases_across_lines_parse() {
    // Items themselves stay on a single line; line breaks between items
    // are exercised. Both aliases survive.
    let src = "import \"./x.glyph\" {\n    foo as f,\n    bar as b,\n}\n";
    let (_, names) = extract(parse_first_import(src));
    assert_eq!(names.len(), 2);
    assert_eq!(names[0].0, "foo");
    assert_eq!(names[0].1.as_deref(), Some("f"));
    assert_eq!(names[1].0, "bar");
    assert_eq!(names[1].1.as_deref(), Some("b"));
}
```

- [ ] **Step 4: Run all three new tests and confirm they PASS**

```bash
cargo test -p glyph-core import_decl_tests::multi_line_without_trailing_comma_parses import_decl_tests::multi_line_mixed_layout_parses import_decl_tests::multi_line_aliases_across_lines_parse
```

Expected: all three PASS. (No new production code is required — the Task 1 fix already covers them. If any fails, the helper or call sites are mis-wired; revisit Task 1 Step 5.)

- [ ] **Step 5: Commit**

```bash
git add crates/glyph-core/src/parse.rs
git commit -m "parse: lock in layout coverage for multi-line imports (#117)

Add three tests covering the no-trailing-comma form, the mixed
header-line + subsequent-line layout, and aliases across lines. All
pass under the Task 1 helper without further parser changes."
```

---

## Task 3: Diagnostic improvement — missing-comma between names on different lines

**Goal:** Drive the `expect(Rbrace) → peek-and-match` change with a negative test that pins both the message text and the diagnostic span.

**Files:**
- Modify: `crates/glyph-core/src/parse.rs` (new test inside `mod import_decl_tests`)

- [ ] **Step 1: Add the missing-comma negative test**

Inside `mod import_decl_tests`, append:

```rust
#[test]
fn multi_line_missing_comma_between_names_diagnostic() {
    // `b` on a new line without a comma after `a`. The diagnostic must
    // mention both `,` and `}` and pin the span to the `b` token, not
    // to a `LineStart`.
    let src = "import \"./x.glyph\" { a\n b\n }\n";
    let err = parse(src, 0).err().expect("expected ParseError");
    match err {
        ParseError::Unexpected { ref message, span } => {
            assert!(
                message.contains(',') && message.contains('}'),
                "message should mention both `,` and `}}`, got: {:?}",
                message
            );
            // Span must sit on the `b` token. Extract it from the source.
            let snippet = &src[span.start as usize..span.end as usize];
            assert_eq!(snippet, "b", "span should cover `b`, got {:?}", snippet);
        }
        other => panic!("expected ParseError::Unexpected, got {:?}", other),
    }
}
```

- [ ] **Step 2: Run the test and confirm it PASSES**

If Task 1 Step 5 was implemented correctly, the peek-and-match produces the message `"expected ',' or '}' after import name"` with span on the offending non-LineStart token (here, `b`).

```bash
cargo test -p glyph-core import_decl_tests::multi_line_missing_comma_between_names_diagnostic
```

Expected: PASS.

If FAIL: most likely the message lacks `,` and `}` characters or the span is on a `LineStart`. Revisit Task 1 Step 5 — check that `self.skip_line_starts()` runs before the peek-and-match (it must, otherwise the span lands on the LineStart) and that the message string matches.

- [ ] **Step 3: Verify no existing tests pinned the prior `expect`-style message**

Grep for any pre-existing test that may have asserted on the old `expect(Rbrace)` error wording:

```bash
grep -rn "expected.*Rbrace\|expected `}`" crates/glyph-core/src/ tests/ 2>&1 | grep -v "import name"
```

Expected: no matches outside the new test. If any pre-existing test pins the old message string for the selective-import case, update it in this commit (the PRD's "Further Notes" anticipated this and accepts the change).

- [ ] **Step 4: Commit**

```bash
git add crates/glyph-core/src/parse.rs
git commit -m "parse: clearer diagnostic on missing comma in selective import (#117)

The peek-and-match introduced in Task 1 emits 'expected ',' or '}'
after import name' with span on the offending token rather than
the previous 'expected }' wording. Negative test pins both the
message contents and the span location."
```

---

## Task 4: Comment composition test

**Goal:** Lock in that the tokenizer's pre-strip of comments (`tokenize.rs:122-126`) composes correctly with the new line-skip behavior. Comment-only lines emit zero tokens; trailing `// …` is stripped pre-tokenization.

**Files:**
- Modify: `crates/glyph-core/src/parse.rs` (new test inside `mod import_decl_tests`)

- [ ] **Step 1: Add the comment-composition test**

Inside `mod import_decl_tests`, append:

```rust
#[test]
fn multi_line_with_comments_parses() {
    // A comment-only line between names + a trailing `// …` after a name.
    // Both should be invisible to the parser by the time it sees the
    // brace list, so the import parses cleanly.
    let src = "\
import \"./x.glyph\" {
    // explanatory note
    a, // why we need a
    b,
}
";
    let (_, names) = extract(parse_first_import(src));
    let bare: Vec<&str> = names.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(bare, vec!["a", "b"]);
}
```

- [ ] **Step 2: Run the test and confirm it PASSES**

```bash
cargo test -p glyph-core import_decl_tests::multi_line_with_comments_parses
```

Expected: PASS without further parser changes (the tokenizer handles comments before the parser ever sees them).

If FAIL: investigate whether the tokenizer is actually stripping the trailing comment in this position, then decide whether to widen the helper or document a limitation. Do not weaken the test silently.

- [ ] **Step 3: Commit**

```bash
git add crates/glyph-core/src/parse.rs
git commit -m "parse: comment lines and trailing comments inside multi-line imports (#117)

Lock in that comment-only lines and trailing // ... composes with the
new LineStart-skipping behavior — the tokenizer pre-strips comments
so the parser sees a clean LineStart-or-ident sequence."
```

---

## Task 5: Cascading-error regression test

**Goal:** Pin the actual user-facing failure mode that motivated this work: a multi-line selective import in a file that *also* contains a legitimate `-> Type` return-type annotation and a legitimate `<output_target>` site. With the fix, none of `G::parse::operator-in-expression` or `G::parse::output-target-outside-return` may fire on the legitimate later constructs.

**Files:**
- Modify: `crates/glyph-core/src/parse.rs` (new test inside `mod import_decl_tests`)

- [ ] **Step 1: Add the regression test**

Inside `mod import_decl_tests`, append:

```rust
#[test]
fn multi_line_import_does_not_cascade_to_arrow_or_output_target() {
    // Reduced inline fixture (do NOT reference any authoring file):
    //   * multi-line selective import (the previously breaking shape)
    //   * later `-> Path` return-type annotation (legit; would mis-fire
    //     `G::parse::operator-in-expression` if parse_import bails)
    //   * later `<output_target>` site (legit; would mis-fire
    //     `G::parse::output-target-outside-return` pre-PR-#140)
    //
    // After the fix, parse_import succeeds, both Arrow and `<` tokens
    // are consumed legitimately, and neither cascade triggers.
    let src = "\
import \"./other.glyph\" {
    foo,
    bar,
}

skill main() -> Path
    flow:
        return <output_target>
";
    let line_index = LineIndex::new(src);
    let mut bag = DiagBag::new();
    let _ = parse_with_diagnostics(src, 0, "t.glyph", &line_index, &mut bag);
    let ids: Vec<String> = bag.iter().map(|d| d.id.clone()).collect();
    assert!(
        !ids.iter().any(|s| s == "G::parse::operator-in-expression"),
        "must not fire operator-in-expression after multi-line-import fix; got: {:?}",
        ids
    );
    assert!(
        !ids.iter().any(|s| s == "G::parse::output-target-outside-return"),
        "must not fire output-target-outside-return after multi-line-import fix; got: {:?}",
        ids
    );
}
```

- [ ] **Step 2: Run the test and confirm it PASSES**

```bash
cargo test -p glyph-core import_decl_tests::multi_line_import_does_not_cascade_to_arrow_or_output_target
```

Expected: PASS.

If FAIL with the wrong fixture (e.g. some unrelated parser-level diagnostic fires that has nothing to do with the cascade), simplify the fixture body — remove or replace `flow:` content, etc. — but DO NOT remove either of the two `assert!(!ids.iter().any(…))` checks. Those are the regression target.

If FAIL because either of the two cascade IDs *does* fire, the parser fix is incomplete or one of the call sites is mis-placed; revisit Task 1 Step 5.

- [ ] **Step 3: Commit**

```bash
git add crates/glyph-core/src/parse.rs
git commit -m "parse: regression test for multi-line-import cascade class (#116, #117)

Inline reduced fixture combining a multi-line selective import with
a later -> Path annotation and a later <output_target> site. Asserts
that neither operator-in-expression nor output-target-outside-return
fires. Locks in the actual user-facing failure mode that motivated
this work."
```

---

## Task 6: Update `design/language-surface.md` §3.5

**Goal:** Anchor the multi-line-allowed rule in the design spec. The PRD's original target was `design/expand.md` §5.5 — that's the wrong file (its §5.5 is "Retry Budget Constants"). The correct home is `design/language-surface.md` §3.5 `import`.

**Files:**
- Modify: `design/language-surface.md` (insert one bullet in the Rules list at §3.5, around lines 311-319)

- [ ] **Step 1: Open `design/language-surface.md` and locate §3.5 Rules block**

The Rules list begins at line 311 (`**Rules:**`). The bullets currently end at line 319 (`MVP imports are path-based.`). Add a new bullet immediately after the existing "Selective import uses `{ name, name as alias, ... }`. Trailing comma allowed." bullet (line 316).

- [ ] **Step 2: Insert the new normative bullet**

Edit the file so the existing bullet:

```markdown
- Selective import uses `{ name, name as alias, ... }`. Trailing comma allowed. Only explicitly exported declarations may be named.
```

is followed immediately by:

```markdown
- Whitespace inside `{ … }` is non-significant: line breaks and indentation between import items are allowed; the brace pair is the sole delimiter. Items (`name`, optional `as <alias>`) must stay on a single line.
```

The other bullets ("A single import statement is either…", "Circular imports are rejected…", "MVP imports are path-based…") stay in their current order below the new bullet.

- [ ] **Step 3: Verify rendering by re-reading the section**

```bash
sed -n '279,325p' design/language-surface.md
```

Confirm the new bullet appears after the trailing-comma bullet and before the "single import statement" bullet, with consistent formatting (leading `- `, sentence case, terminal period).

- [ ] **Step 4: Commit**

```bash
git add design/language-surface.md
git commit -m "design: normative bullet for multi-line selective imports (#116)

§3.5 Rules now states: whitespace inside the selective-import brace
list is non-significant; line breaks and indentation between items
are allowed; the brace pair is the sole delimiter; items themselves
(name, optional 'as alias') must stay on a single line.

This anchors the rule in the spec rather than leaving it implicit
in the parser."
```

---

## Task 7: Update `GLYPH_LANGUAGE_GUIDE.md` §5.5

**Goal:** Show authors the multi-line form with a trailing comma so they don't have to experiment.

**Files:**
- Modify: `GLYPH_LANGUAGE_GUIDE.md` (add a multi-line example block in §5.5 Selective subsection, around lines 207-212)

- [ ] **Step 1: Open `GLYPH_LANGUAGE_GUIDE.md` and locate §5.5 Selective**

Selective examples currently sit at lines 207-212:

```markdown
**Selective:**

\`\`\`glyph
import "./prefs.glyph" { preserve_existing_patterns, validation_strictness }
import "./repo_tools.glyph" { inspect_repo as inspect, has_test_suite }
\`\`\`
```

- [ ] **Step 2: Append a multi-line example block immediately after the existing fenced block**

Insert this right after the closing ` ``` ` of the existing block (still inside the Selective subsection, before the `Rules:` line at 214):

````markdown
For long lists, the brace body may span multiple lines. A trailing comma is allowed:

```glyph
import "./glyph_authoring_passes.glyph" {
    factor_long_instructions_and_texts,
    sort_declarations,
    compile_and_iterate,
}
```

Items themselves stay on a single line — `name as alias` does not split across lines. Indentation inside the braces is for readability only; the parser does not validate it.
````

- [ ] **Step 3: Verify the rendered section**

```bash
sed -n '195,225p' GLYPH_LANGUAGE_GUIDE.md
```

Expected: single-line examples, then the new "For long lists…" prose + multi-line code block, then the Rules list. No duplication of existing content.

- [ ] **Step 4: Commit**

```bash
git add GLYPH_LANGUAGE_GUIDE.md
git commit -m "docs: multi-line selective-import example in language guide (#116)

§5.5 now shows the wrapped form with a trailing comma alongside the
existing single-line examples, with a one-line note that items stay
atomic and inner indentation is unvalidated."
```

---

## Task 8: Workspace verification + format pass

**Goal:** Confirm nothing else regressed across the workspace, and that the new code passes `cargo fmt`.

**Files:** none modified by hand; `cargo fmt` may rewrite `parse.rs` cosmetically.

- [ ] **Step 1: Run the full workspace test suite**

```bash
cargo test --workspace 2>&1 | tail -25
```

Expected: every crate's test count is at least baseline + new tests; zero failures. Specifically, `glyph-core` lib tests should rise by **7** vs. baseline (one test per `#[test]` added in Tasks 1-5).

If anything outside `mod import_decl_tests` regresses, stop and investigate — the change should be entirely additive at the parser level.

- [ ] **Step 2: Run `cargo fmt --all` and inspect the diff**

```bash
cargo fmt --all
git diff --stat
```

Expected: at most cosmetic changes within `parse.rs` (and possibly nowhere if hand-formatting matched rustfmt). Other files should be unchanged.

- [ ] **Step 3: Re-run the workspace tests after fmt**

```bash
cargo test --workspace 2>&1 | tail -10
```

Expected: still all green.

- [ ] **Step 4: Commit (only if `cargo fmt` produced changes)**

```bash
git add -u
git commit -m "parse: cargo fmt over multi-line-imports work (#117)"
```

If `git diff --stat` from Step 2 was empty, skip this step.

- [ ] **Step 5: Final sanity grep — no debug prints, no stray TODOs, no `unwrap()` slipped in**

```bash
git diff main..HEAD -- crates/glyph-core/src/parse.rs | grep -E "^\+" | grep -E "println!|dbg!|TODO|FIXME|\.unwrap\(\)" || echo "OK"
```

Expected output: `OK` (the grep finds nothing). The new `parse_first_import` uses `.expect(...)` deliberately for test-time panic messages — that's allowed; this grep filters for `unwrap()` specifically.

---

## Self-review (run by plan author after writing, before handoff)

- **Spec coverage:** Each AC bullet from issue #117 maps to a task: helper exists (Task 1), three call sites (Task 1), peek-and-match diagnostic (Task 1 + 3), single-line preserved (Task 1 trailing-comma test asserts equivalence), 7 tests in `mod import_decl_tests` (Tasks 1-5: trailing-comma + no-trailing-comma + mixed + aliases + missing-comma + comments + cascade = 7), `design/language-surface.md` bullet (Task 6, retargeted from `expand.md` per design corrigendum), `GLYPH_LANGUAGE_GUIDE.md` example (Task 7), no tokenizer/AST/IR/lowering/LSP changes (none of the tasks touch those files), no existing test regresses (Task 8 Step 1 verifies). ✓
- **Placeholder scan:** No "TBD", "implement later", or vague directives. Every code step shows code; every command is exact. ✓
- **Type consistency:** `parse_first_import → ImportDecl`, `extract: ImportDecl → (String, Vec<(String, Option<String>)>)`, used identically across Tasks 1-5. The peek-and-match diagnostic message string `"expected ',' or '}' after import name"` is identical in Task 1 Step 5 and Task 3 Step 2 (the `,` and `}` characters are what the test 5 assertion checks for). ✓
- **TDD ordering:** Task 1 writes test first → red → implements → green. Tasks 2-5 add tests against the existing fix and assert green directly (the helper from Task 1 covers them); each step explicitly tells the implementer what to do if a test unexpectedly fails. ✓
