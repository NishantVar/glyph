# BUG-034: Skill body bare-name line with trailing tokens aborts whole-file parse with misleading 'expected start of line'

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/parse.rs:3293-3298`
**Found by:** parse-1 | **Audit date:** unknown-date

## Description

In `parse_skill_body_line`, the catch-all `_` arm's bare-name branch consumes only the single keyword token (`self.pos += 1`) and pushes it to `body_bare_names`, but does NOT skip the rest of the line. Compare `parse_block_decl`'s equivalent fallthrough (lines 2589-2601) which deliberately restores pos and drains every token to the next `LineStart` ("consume the rest of the line as before so legacy block bodies keep parsing").

When a bare-name body line carries extra words (e.g. an indent-1 line `note this is important`), the cursor is left mid-line on a non-`LineStart` token. The enclosing `while let Some(1) = self.current_line_indent()` loop in `parse_skill` (line 989) then sees `None` and breaks, `parse_skill` returns, and `parse_file`'s loop calls `expect_line_start()` on a mid-line token, returning `ParseError::Unexpected { message: "expected start of line" }`. The entire file fails to parse and the rest of the skill — including its `flow:` — is silently dropped.

The same root cause also affects the body-level marker arms `require`/`avoid`/`must` (skill ~2896-2921, block ~2218-2243) and the `context <name|string>` body arm (skill ~3122-3148, block ~2391-2418): all emit the same misleading `G::parse::unexpected` whole-file failure when given a trailing-token operand.

## Trigger / Reproduction

```
skill foo()
    somename extra trailing tokens
    flow:
        "x"
```

Run `glyph check`: emits only `G::parse::unexpected: expected start of line` anchored at `extra`; the `flow:` section is silently dropped. The identical body inside a `block` parses cleanly.

## Evidence

```rust
} else {
    // Bare name at body level — not a recognized keyword.
    self.pos += 1;
    body_bare_names.push(kw.clone());
}  // <-- trailing tokens on the line are never consumed
```

After `self.pos += 1`, the cursor is left mid-line. The outer while-loop in `parse_skill` checks `current_line_indent()`, which returns `None` when not positioned on a `LineStart`, causing it to exit immediately and abandon the skill body.

## Recommended Resolution

After parsing a single-operand body construct in `parse_skill_body_line` (and the marker/context arms in both skill and block parsers), drain any remaining tokens to the next `LineStart`/EOF, mirroring `parse_block_decl`'s fallthrough at lines 2596-2600:

```rust
while !self.at_eof() && !matches!(self.peek().kind, TokenKind::LineStart { .. }) {
    self.pos += 1;
}
```

Alternatively, centralize a `skip_to_line_end()` helper and call it at the end of every body-line arm so the cursor always lands on a `LineStart`, preventing the over-broad `expected start of line` cascade.

## Verification Notes

Reproduced end-to-end: running `cargo run -p glyph-cli -- check` on `skill foo()\n    somename extra trailing tokens\n    flow:\n        "x"\n` emits exactly `G::parse::unexpected: expected start of line` anchored at `extra`, while the identical input in a `block` parses cleanly. A skill with a bare-name body line and NO trailing tokens compiles normally and advances to analysis (showing `G::analyze::missing-description`), confirming parsing succeeds when the cursor lands correctly. With trailing tokens, the flow section is silently dropped — no `G::analyze::empty-skill-body` error is emitted — confirming the parse cursor is left mid-line. The same failure was reproduced with `require someName extra` and `context someName extra`, confirming all three affected arms.
