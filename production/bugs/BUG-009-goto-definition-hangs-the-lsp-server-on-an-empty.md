# BUG-009: goto-definition hangs the LSP server on an empty inline flow string

**Severity:** high | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-lsp/src/lib.rs:565-592`
**Found by:** x-panics | **Audit date:** unknown-date

## Description

`resolve_param_slot` iterates over every flow inline string collected from the enclosing decl and locates it in the decl's source slice with `body_text[search_from..].find(s.as_str())`, advancing `search_from += rel + s.len()`. Rust's `str::find` returns `Some(0)` for an empty needle, and when `s` is the empty string the advance is `0 + 0 = 0`, so the `while let Some(rel) = ...` loop spins forever — `rel` is always 0, `search_from` never moves, and the cursor-in-string test `cursor < abs_end` is always false because `abs_end == abs_start`. An empty inline flow string is valid, reachable user input: the tokenizer cooks `""` into `StringLit("")`, and `parse.rs:2605-2609` pushes a bare flow string as `FlowStmt::InlineString("")`; `collect_flow_inline_strings`/`gather_strings` then place that empty string into `flow_strings`. Trigger: author a skill/block whose flow contains a bare `""` step, then issue `textDocument/definition` with the cursor anywhere inside that decl that misses all resolutions (the common case for a plain string step). Because `run_stdio` builds a `current_thread` tokio runtime (`lib.rs:655`), this single request wedges the entire LSP server thread, not just one request — a hard hang/DoS.

## Trigger / Reproduction

Author a skill or block whose `flow:` contains a bare `""` step. Open the file in an editor backed by the Glyph LSP. Move the cursor to any position inside that decl and trigger go-to-definition. The LSP server hangs indefinitely.

## Evidence

```rust
let mut search_from = 0usize;
while let Some(rel) = body_text[search_from..].find(s.as_str()) {
    let abs_start = body_start + search_from + rel;
    let abs_end = abs_start + s.len();
    if cursor as usize >= abs_start && (cursor as usize) < abs_end { ... }
    search_from += rel + s.len();   // == 0 when s is empty -> infinite loop
}
```

## Recommended Resolution

Skip empty strings before the search loop. Add `if s.is_empty() { continue; }` at the top of the `for s in &flow_strings` body. Empty flow strings carry no `{name}` slots, so skipping them is correct and complete. Alternatively, guard the loop so a zero-length match breaks: after a match that does not advance, `search_from += (rel + s.len()).max(1)` with a bounds check — but the `continue` approach is simpler.

## Verification Notes

The infinite loop is confirmed end-to-end: (1) the tokenizer emits `StringLit("")` for `""` with no guard, (2) the parser pushes it as `FlowStmt::InlineString("")` with no guard, (3) the LSP's goto_definition calls `check_source_with_resolutions_at_path` which does parse+analyze only — not the IR lowering pipeline that contains the empty-step validator — so empty strings reach `resolve_param_slot` in the AST, (4) inside the while loop, `body_text[search_from..].find("")` always returns `Some(0)`, making `search_from += 0+0 = 0` forever. The cursor check `abs_start..abs_end` is degenerate (zero-width range) so it is never satisfied. Because the runtime is `current_thread` tokio, the spinning loop blocks the entire server. The `if s.is_empty() { continue; }` fix is correct.
