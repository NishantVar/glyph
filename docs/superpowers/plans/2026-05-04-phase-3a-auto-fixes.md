# Phase 3a Auto-Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land five new deterministic auto-fixes in `glyph fmt` (#107 duplicate-import collapse, #108 unused-import removal, #110 stdlib auto-import, #111 const-in-flow parens-add, #112 effects auto-insert), close #113 (placeholder return rewrite, already implemented) with a doc note, and update agent-skill docs (#114) to reflect the new Phase 3a/3b split. Single PR.

**Architecture:** All auto-fixes live in `crates/glyph-core/src/fmt.rs` as additions to the post-Parse AST stratum. A new "Analyze stratum" between parse and AST-rewrite calls into `analyze.rs` to obtain structured signals (referenced names, unresolved names, inferred effects). Each fix consumes those signals plus the AST and rewrites source text. Public `fmt_source(source, enable_effects) -> FmtResult` signature is unchanged.

**Tech Stack:** Rust, `cargo`, `glyph-core` workspace crate, existing `parse` / `analyze` modules.

**Spec:** `docs/superpowers/specs/2026-05-04-phase-3a-auto-fixes-design.md`
**Parent issue:** #106

---

## File Structure

| File | Status | Responsibility |
|---|---|---|
| `crates/glyph-core/src/fmt.rs` | modify | Add 5 auto-fix functions + Analyze-stratum call. Existing `ast_rewrite` keeps shape; new helpers added before/after. |
| `crates/glyph-core/src/analyze.rs` | modify | Expose `pub fn fmt_signals(file: &SourceFile) -> FmtSignals` returning structured signals (referenced names, unresolved names, inferred effects per decl). New `pub struct FmtSignals`. |
| `crates/glyph-core/src/lib.rs` | optional modify | Re-export `fmt_signals` if needed by tests. |
| `crates/glyph-cli/tests/fmt.rs` | modify | Add CLI corpus fixture for multi-fix integration. |
| `crates/glyph-cli/tests/corpus/fmt/<new>.glyph.md` | create | Corpus input + expected output pair for the integration test. |
| `design/agent-skill.md` | modify | Update Phase 3a list (now 7 deterministic fixes) and Phase 3b table (remove the 7 now-deterministic items). |
| `REPAIR_PASS_SPEC.md` | modify | Drop the 7 now-deterministic items per PRD §Further Notes. |
| `docs/superpowers/plans/2026-05-04-phase-3a-auto-fixes.md` | this file | The plan itself. |

**Decomposition note.** Each auto-fix is a self-contained function in `fmt.rs` (file-level fixes vs per-decl fixes). The fmt.rs file already exceeds 990 lines, but per project convention (existing `strip_legacy_none_return_types`, `placeholder_string_return_target`, etc. all coexist there) we keep them together. If post-PR review wants a split, it's a follow-up — not in scope.

---

## Task 0: Add Analyze stratum + `FmtSignals` helper

**Files:**
- Modify: `crates/glyph-core/src/analyze.rs` (add new public struct + function)
- Modify: `crates/glyph-core/src/fmt.rs` (call the new helper from `fmt_source`)

**Goal:** Expose a single entry point that fmt calls to get all structured signals for the auto-fixes. No auto-fix logic yet — just the plumbing.

- [ ] **Step 1: Write the failing test**

Add at the bottom of `crates/glyph-core/src/analyze.rs` (in the existing `#[cfg(test)] mod tests` block, or create a new test):

```rust
#[test]
fn fmt_signals_extracts_referenced_unresolved_and_effects() {
    use crate::parse;
    use crate::span::LineIndex;
    use crate::diagnostic::DiagBag;

    let src = r#"import "@glyph/std" { send }

skill main()
    description: "Main."
    flow:
        send("hi")
        subagent("nested")
"#;
    let line_index = LineIndex::new(src);
    let mut bag = DiagBag::new();
    let file = parse::parse_with_diagnostics_opts(src, 0, "<t>", &line_index, &mut bag, true).unwrap();

    let signals = crate::analyze::fmt_signals(&file);

    assert!(signals.referenced_names.contains("send"));
    assert!(signals.referenced_names.contains("subagent"));
    assert!(signals.unresolved_names.contains("subagent"),
        "subagent is not imported and not local — should be unresolved");
    assert!(!signals.unresolved_names.contains("send"),
        "send is imported, should not be unresolved");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p glyph-core fmt_signals_extracts_referenced_unresolved_and_effects -- --nocapture`
Expected: compile error — `fmt_signals` and `FmtSignals` don't exist yet.

- [ ] **Step 3: Implement `FmtSignals` and `fmt_signals` in `analyze.rs`**

Append near the bottom of `analyze.rs` (before `#[cfg(test)]`):

```rust
/// Structured signals extracted from a parsed `SourceFile` for `glyph fmt`'s
/// auto-fix pass to consume. Single-file scope — no cross-file resolution.
#[derive(Debug, Default)]
pub struct FmtSignals {
    /// All names referenced anywhere in the file (in calls, returns, contexts,
    /// constraints, etc.).
    pub referenced_names: std::collections::HashSet<String>,
    /// Names referenced that don't resolve to a local declaration or any
    /// import in this file. Used by stdlib auto-import (#110) and const-in-flow
    /// parens-add (#111).
    pub unresolved_names: std::collections::HashSet<String>,
    /// Inferred effect set per top-level declaration name. Empty for decls
    /// where inference produced nothing.
    pub inferred_effects: std::collections::HashMap<String, Vec<String>>,
}

/// Extract `FmtSignals` from a parsed file. Single-file analysis only —
/// imports are recognized by name (selective) or by alias (whole-module),
/// but the imported module's contents are not loaded.
pub fn fmt_signals(file: &SourceFile) -> FmtSignals {
    let mut signals = FmtSignals::default();

    // Collect locally-bound names (decls, imports).
    let mut bound_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for decl in &file.decls {
        match decl {
            Decl::Const(c) => { bound_names.insert(c.node.name.clone()); }
            Decl::Block(b) => { bound_names.insert(b.node.name.clone()); }
            Decl::ExportBlock(b) => { bound_names.insert(b.node.name.clone()); }
            Decl::Skill(s) => { bound_names.insert(s.node.name.clone()); }
            Decl::Import(imp) => match &imp.node.kind {
                crate::ast::ImportKind::Selective(names) => {
                    for n in names {
                        let local = n.alias.clone().unwrap_or_else(|| n.name.node.clone());
                        bound_names.insert(local);
                    }
                }
                crate::ast::ImportKind::WholeModule { alias } => {
                    bound_names.insert(alias.clone());
                }
            },
        }
    }

    // Walk all decls and collect referenced names from flow bodies, returns,
    // contexts, constraints, and bare-names tracked by the parser.
    for decl in &file.decls {
        collect_refs_from_decl(decl, &mut signals.referenced_names);
        if let Some((name, effects)) = infer_decl_effects(decl) {
            if !effects.is_empty() {
                signals.inferred_effects.insert(name, effects);
            }
        }
    }

    // Compute unresolved = referenced - bound.
    for name in &signals.referenced_names {
        if !bound_names.contains(name) {
            signals.unresolved_names.insert(name.clone());
        }
    }

    signals
}

/// Collect all referenced names (call targets, return-call targets, context
/// references, constraint references, body bare names) from a single decl.
fn collect_refs_from_decl(decl: &Decl, out: &mut std::collections::HashSet<String>) {
    match decl {
        Decl::Skill(s) => {
            for stmt in &s.node.flow { collect_refs_from_flow_stmt(stmt, out); }
            for n in &s.node.body_bare_names { out.insert(n.clone()); }
        }
        Decl::Block(b) => {
            for stmt in &b.node.flow { collect_refs_from_flow_stmt(stmt, out); }
        }
        Decl::ExportBlock(b) => {
            // ExportBlock has a terminal_return only.
            if let Some(expr) = &b.node.terminal_return {
                collect_refs_from_return_expr(expr, out);
            }
        }
        Decl::Const(_) | Decl::Import(_) => {}
    }
}

fn collect_refs_from_flow_stmt(stmt: &crate::ast::FlowStmt, out: &mut std::collections::HashSet<String>) {
    use crate::ast::FlowStmt;
    match stmt {
        FlowStmt::Call { name, .. } => { out.insert(name.clone()); }
        FlowStmt::Return(expr) => collect_refs_from_return_expr(expr, out),
        FlowStmt::If { branches, else_branch, .. } => {
            for b in branches { for s in &b.body { collect_refs_from_flow_stmt(s, out); } }
            if let Some(else_body) = else_branch {
                for s in else_body { collect_refs_from_flow_stmt(s, out); }
            }
        }
        FlowStmt::BareName(n) => { out.insert(n.clone()); }
        _ => {}
    }
}

fn collect_refs_from_return_expr(expr: &crate::ast::ReturnExpr, out: &mut std::collections::HashSet<String>) {
    use crate::ast::ReturnExpr;
    match expr {
        ReturnExpr::Call { name, .. } => { out.insert(name.clone()); }
        ReturnExpr::Bare(n) => { out.insert(n.clone()); }
        _ => {}
    }
}

/// Inferred effect set for a single decl. Reuses existing analyze internals;
/// returns `(decl_name, effects)` for skill/block decls, `None` otherwise.
fn infer_decl_effects(decl: &Decl) -> Option<(String, Vec<String>)> {
    match decl {
        Decl::Skill(s) => {
            if !s.node.effects.is_empty() {
                // User declared effects — leave alone, return empty so auto-fix doesn't fire.
                return Some((s.node.name.clone(), Vec::new()));
            }
            let effects = infer_effects_for_flow(&s.node.flow);
            Some((s.node.name.clone(), effects))
        }
        Decl::Block(b) => {
            let effects = infer_effects_for_flow(&b.node.flow);
            Some((b.node.name.clone(), effects))
        }
        _ => None,
    }
}

/// Walk a flow body and accumulate effects for stdlib calls. Mirrors
/// `analyze::stdlib_block_effects`.
fn infer_effects_for_flow(flow: &[crate::ast::FlowStmt]) -> Vec<String> {
    let mut effects = std::collections::BTreeSet::new();
    fn walk(stmt: &crate::ast::FlowStmt, effects: &mut std::collections::BTreeSet<String>) {
        use crate::ast::{FlowStmt, ReturnExpr};
        match stmt {
            FlowStmt::Call { name, .. } => {
                if let Some(eff) = stdlib_block_effects(name) {
                    for e in eff { effects.insert((*e).to_string()); }
                }
            }
            FlowStmt::Return(ReturnExpr::Call { name, .. }) => {
                if let Some(eff) = stdlib_block_effects(name) {
                    for e in eff { effects.insert((*e).to_string()); }
                }
            }
            FlowStmt::If { branches, else_branch, .. } => {
                for b in branches { for s in &b.body { walk(s, effects); } }
                if let Some(e) = else_branch { for s in e { walk(s, effects); } }
            }
            _ => {}
        }
    }
    for stmt in flow { walk(stmt, &mut effects); }
    effects.into_iter().collect()
}
```

Note: exact AST variant names (`FlowStmt::Call`, `ReturnExpr::Bare`, etc.) must match what's in `ast.rs`. If a variant is named differently in the actual code, adjust accordingly — match the existing variant pattern from `analyze.rs`'s flow walks.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p glyph-core fmt_signals_extracts_referenced_unresolved_and_effects -- --nocapture`
Expected: PASS.

If AST variant names don't match, fix them per existing `analyze.rs` flow-walking patterns (search for `match stmt {` in `analyze.rs`).

- [ ] **Step 5: Wire `fmt_signals` into `fmt_source`**

In `crates/glyph-core/src/fmt.rs`, modify `fmt_source` to call `analyze::fmt_signals` after parse and pass the signals to `ast_rewrite`. Update the `ast_rewrite` signature.

```rust
pub fn fmt_source(source: &str, enable_effects: bool) -> FmtResult {
    let mut bag = DiagBag::new();
    let after_preparse = preparse_rewrite(source);
    let after_preparse = strip_legacy_none_return_types(&after_preparse);
    let line_index = LineIndex::new(&after_preparse);
    let parsed = parse::parse_with_diagnostics_opts(&after_preparse, 0, "<fmt>", &line_index, &mut bag, enable_effects);

    match parsed {
        Some(file) => {
            let signals = crate::analyze::fmt_signals(&file);
            let after_ast = ast_rewrite(&after_preparse, &file, &signals, enable_effects);
            let changed = after_ast != source;
            FmtResult { output: after_ast, changed, diagnostics: bag }
        }
        None => {
            let changed = after_preparse != source;
            FmtResult { output: after_preparse, changed, diagnostics: bag }
        }
    }
}
```

Update `fn ast_rewrite(source: &str, file: &crate::ast::SourceFile)` signature to:
`fn ast_rewrite(source: &str, file: &crate::ast::SourceFile, signals: &crate::analyze::FmtSignals, enable_effects: bool) -> String`

For now, `signals` and `enable_effects` are unused in `ast_rewrite` — they will be consumed by Tasks 1-5.

- [ ] **Step 6: Run full test suite**

Run: `cargo test -p glyph-core`
Expected: all existing tests still pass (the new function is unused inside ast_rewrite so far).

- [ ] **Step 7: Commit**

```bash
git add crates/glyph-core/src/analyze.rs crates/glyph-core/src/fmt.rs
git commit -m "fmt: add FmtSignals helper to expose analyze info to auto-fixes (#106)"
```

---

## Task 1: #107 Duplicate import collapse

**Files:**
- Modify: `crates/glyph-core/src/fmt.rs` (add `collapse_duplicate_imports` function + tests)

**Goal:** When a source file has two imports with the same path, collapse them. Whole-module wins over selective; two selectives merge selector lists.

- [ ] **Step 1: Write the failing tests**

Append to the `#[cfg(test)] mod tests` block at the bottom of `fmt.rs`:

```rust
#[test]
fn fmt_collapse_two_whole_module_imports_same_path() {
    let src = r#"import "@glyph/std" { send }
import "@glyph/std" { send }

skill main()
    description: "Main."
    flow:
        send("hi")
"#;
    let result = fmt_source(src, true);
    let expected = r#"import "@glyph/std" { send }

skill main()
    description: "Main."
    flow:
        send("hi")
"#;
    assert_eq!(result.output, expected);
    assert!(result.changed);
}

#[test]
fn fmt_collapse_two_selective_imports_unions_selectors() {
    let src = r#"import "@glyph/std" { send }
import "@glyph/std" { subagent }

skill main()
    description: "Main."
    flow:
        send("hi")
        subagent("x")
"#;
    let result = fmt_source(src, true);
    // First-occurrence-wins ordering: send first, subagent appended.
    assert!(result.output.contains(r#"import "@glyph/std" { send, subagent }"#));
    // Only one import line.
    assert_eq!(result.output.matches(r#"import "@glyph/std""#).count(), 1);
    assert!(result.changed);
}

#[test]
fn fmt_collapse_imports_no_op_when_paths_differ() {
    let src = r#"import "./a.glyph.md" { foo }
import "./b.glyph.md" { bar }

skill main()
    description: "Main."
    flow:
        foo()
"#;
    let result = fmt_source(src, true);
    assert_eq!(result.output, src);
    assert!(!result.changed);
}

#[test]
fn fmt_collapse_imports_idempotent() {
    let src = r#"import "@glyph/std" { send }
import "@glyph/std" { subagent }

skill main()
    description: "Main."
    flow:
        send("x")
        subagent("y")
"#;
    let once = fmt_source(src, true).output;
    let twice = fmt_source(&once, true).output;
    assert_eq!(once, twice, "fmt should be idempotent");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glyph-core fmt_collapse_ -- --nocapture`
Expected: all 4 tests FAIL — fmt currently leaves duplicates as-is.

- [ ] **Step 3: Implement the fix**

Add a new function `collapse_duplicate_imports(source: &str, file: &SourceFile) -> String` in `fmt.rs`. Call it from `ast_rewrite` *before* the existing per-decl rewrite loop, by pre-rewriting the source text. The simplest implementation:

```rust
fn collapse_duplicate_imports(source: &str, file: &crate::ast::SourceFile) -> String {
    use std::collections::HashMap;
    use crate::ast::{Decl, ImportKind};

    // Group import decls by path, in source order.
    #[derive(Default)]
    struct Group {
        first_line_idx: usize,
        is_whole_module: bool,
        whole_module_alias: Option<String>,
        // For selective imports: ordered, deduped names with their alias.
        selective_names: Vec<(String, Option<String>)>,
        line_indices: Vec<usize>, // all source-line indices for this path
    }

    let lines: Vec<&str> = source.lines().collect();

    // Build a map from import-decl-position to source-line-index.
    // Imports at file top — their line index in source equals the AST decl's
    // header line. Use the existing decl_starts logic.
    let mut import_line_idx: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !line.starts_with(' ') && !line.starts_with('\t') && line.trim().starts_with("import ") {
            import_line_idx.push(i);
        }
    }

    let mut groups: HashMap<String, Group> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    let mut import_seq = 0usize;
    for decl in &file.decls {
        if let Decl::Import(imp) = decl {
            if import_seq >= import_line_idx.len() { break; }
            let line_idx = import_line_idx[import_seq];
            import_seq += 1;

            let entry = groups.entry(imp.node.path.clone()).or_insert_with(|| {
                order.push(imp.node.path.clone());
                Group { first_line_idx: line_idx, ..Group::default() }
            });
            entry.line_indices.push(line_idx);

            match &imp.node.kind {
                ImportKind::Selective(names) => {
                    for n in names {
                        let key = (n.name.node.clone(), n.alias.clone());
                        if !entry.selective_names.iter().any(|e| e == &key) {
                            entry.selective_names.push(key);
                        }
                    }
                }
                ImportKind::WholeModule { alias } => {
                    entry.is_whole_module = true;
                    entry.whole_module_alias = Some(alias.clone());
                }
            }
        }
    }

    // No duplicates? Return early.
    if !groups.values().any(|g| g.line_indices.len() > 1) {
        return source.to_string();
    }

    // Build output: for each line, decide whether to keep, replace, or drop.
    let mut to_drop: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut replacements: HashMap<usize, String> = HashMap::new();

    for path in &order {
        let g = &groups[path];
        if g.line_indices.len() <= 1 { continue; }
        // Keep first; drop rest.
        for &idx in g.line_indices.iter().skip(1) {
            to_drop.insert(idx);
        }
        // Rewrite first line to the merged form.
        let merged = if g.is_whole_module {
            format!(
                r#"import "{}" as {}"#,
                path,
                g.whole_module_alias.as_deref().unwrap_or("")
            )
        } else {
            let names = g.selective_names.iter()
                .map(|(n, a)| match a {
                    Some(alias) => format!("{} as {}", n, alias),
                    None => n.clone(),
                })
                .collect::<Vec<_>>().join(", ");
            format!(r#"import "{}" {{ {} }}"#, path, names)
        };
        replacements.insert(g.first_line_idx, merged);
    }

    let mut out = String::with_capacity(source.len());
    for (i, line) in lines.iter().enumerate() {
        if to_drop.contains(&i) { continue; }
        if let Some(repl) = replacements.get(&i) {
            out.push_str(repl);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    if !source.ends_with('\n') && out.ends_with('\n') { out.pop(); }
    out
}
```

In `ast_rewrite`, at the very top (before the existing decl-walking loop), pre-rewrite:

```rust
fn ast_rewrite(source: &str, file: &crate::ast::SourceFile, signals: &crate::analyze::FmtSignals, enable_effects: bool) -> String {
    // Phase 3a fixes that operate at file-level on the raw source text.
    let source = collapse_duplicate_imports(source, file);
    // ... existing logic continues, but operating on `&source` ...
```

**Important:** because `collapse_duplicate_imports` rewrites the source text, `file` (the parsed AST) is now stale relative to the rewritten source. The existing per-decl walking logic in `ast_rewrite` relies on `decl_idx` matching `file.decls[decl_idx]`. If we drop import lines, the `decl_starts` recomputation handles new line indices, but `file.decls.get(decl_idx)` returns AST decls in their original order, which still includes the dropped imports. Solution: re-parse after the import collapse.

```rust
fn ast_rewrite(source: &str, file: &crate::ast::SourceFile, signals: &crate::analyze::FmtSignals, enable_effects: bool) -> String {
    let collapsed = collapse_duplicate_imports(source, file);
    if collapsed != source {
        // Re-parse so AST aligns with the new source.
        let line_index = crate::span::LineIndex::new(&collapsed);
        let mut bag = crate::diagnostic::DiagBag::new();
        if let Some(reparsed) = crate::parse::parse_with_diagnostics_opts(&collapsed, 0, "<fmt>", &line_index, &mut bag, enable_effects) {
            let new_signals = crate::analyze::fmt_signals(&reparsed);
            return ast_rewrite_inner(&collapsed, &reparsed, &new_signals, enable_effects);
        }
        // Re-parse failed? Return collapsed source without further rewrites.
        return collapsed;
    }
    ast_rewrite_inner(source, file, signals, enable_effects)
}
```

Rename the existing `ast_rewrite` body to `ast_rewrite_inner` (same signature minus the `source` ownership: it stays `&str`).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p glyph-core fmt_collapse_ -- --nocapture`
Expected: all 4 tests PASS.

Then run: `cargo test -p glyph-core`
Expected: full suite still passes.

- [ ] **Step 5: Commit**

```bash
git add crates/glyph-core/src/fmt.rs
git commit -m "fmt: collapse duplicate imports (#107)"
```

---

## Task 2: #108 Unused import removal

**Files:**
- Modify: `crates/glyph-core/src/fmt.rs` (add `remove_unused_imports` function + tests)

**Goal:** Drop selective import names that are never referenced. If all names in a selective list are unused, drop the line. Whole-module imports whose alias is never referenced are dropped entirely.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn fmt_remove_unused_selective_name_keeps_used_one() {
    let src = r#"import "@glyph/std" { send, subagent }

skill main()
    description: "Main."
    flow:
        send("hi")
"#;
    let result = fmt_source(src, true);
    assert!(result.output.contains(r#"import "@glyph/std" { send }"#),
        "expected only `send` to remain, got: {}", result.output);
    assert!(!result.output.contains("subagent"));
    assert!(result.changed);
}

#[test]
fn fmt_remove_unused_drops_entire_line_when_all_unused() {
    let src = r#"import "@glyph/std" { send, subagent }

skill main()
    description: "Main."
    flow:
        return "<done>"
"#;
    let result = fmt_source(src, true);
    assert!(!result.output.contains("import"),
        "expected import line dropped, got: {}", result.output);
    assert!(result.changed);
}

#[test]
fn fmt_remove_unused_no_op_when_all_used() {
    let src = r#"import "@glyph/std" { send, subagent }

skill main()
    description: "Main."
    flow:
        send("x")
        subagent("y")
"#;
    let result = fmt_source(src, true);
    assert_eq!(result.output, src);
    assert!(!result.changed);
}

#[test]
fn fmt_remove_unused_idempotent() {
    let src = r#"import "@glyph/std" { send, subagent }

skill main()
    description: "Main."
    flow:
        send("x")
"#;
    let once = fmt_source(src, true).output;
    let twice = fmt_source(&once, true).output;
    assert_eq!(once, twice);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glyph-core fmt_remove_unused_ -- --nocapture`
Expected: all 4 tests FAIL.

- [ ] **Step 3: Implement the fix**

Add `remove_unused_imports(source: &str, file: &SourceFile, signals: &FmtSignals) -> String` in `fmt.rs`. Call it after `collapse_duplicate_imports` in the wrapper:

```rust
fn remove_unused_imports(source: &str, file: &crate::ast::SourceFile, signals: &crate::analyze::FmtSignals) -> String {
    use crate::ast::{Decl, ImportKind};

    let lines: Vec<&str> = source.lines().collect();
    let mut import_line_idx: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !line.starts_with(' ') && !line.starts_with('\t') && line.trim().starts_with("import ") {
            import_line_idx.push(i);
        }
    }

    let mut to_drop: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut replacements: std::collections::HashMap<usize, String> = std::collections::HashMap::new();

    let mut import_seq = 0usize;
    for decl in &file.decls {
        let Decl::Import(imp) = decl else { continue };
        if import_seq >= import_line_idx.len() { break; }
        let line_idx = import_line_idx[import_seq];
        import_seq += 1;

        match &imp.node.kind {
            ImportKind::Selective(names) => {
                let kept: Vec<_> = names.iter()
                    .filter(|n| {
                        let local = n.alias.as_deref().unwrap_or(&n.name.node);
                        signals.referenced_names.contains(local)
                    })
                    .collect();
                if kept.is_empty() {
                    to_drop.insert(line_idx);
                } else if kept.len() < names.len() {
                    let names_str = kept.iter()
                        .map(|n| match &n.alias {
                            Some(a) => format!("{} as {}", n.name.node, a),
                            None => n.name.node.clone(),
                        })
                        .collect::<Vec<_>>().join(", ");
                    replacements.insert(
                        line_idx,
                        format!(r#"import "{}" {{ {} }}"#, imp.node.path, names_str),
                    );
                }
            }
            ImportKind::WholeModule { alias } => {
                if !signals.referenced_names.contains(alias) {
                    to_drop.insert(line_idx);
                }
            }
        }
    }

    if to_drop.is_empty() && replacements.is_empty() {
        return source.to_string();
    }

    let mut out = String::with_capacity(source.len());
    for (i, line) in lines.iter().enumerate() {
        if to_drop.contains(&i) { continue; }
        if let Some(repl) = replacements.get(&i) {
            out.push_str(repl);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    if !source.ends_with('\n') && out.ends_with('\n') { out.pop(); }
    out
}
```

In `ast_rewrite`, chain this after `collapse_duplicate_imports` (re-parse if either rewrites the source — keep one re-parse path that runs only if `collapsed_then_unused != source`).

```rust
fn ast_rewrite(source: &str, file: &crate::ast::SourceFile, signals: &crate::analyze::FmtSignals, enable_effects: bool) -> String {
    let after_collapse = collapse_duplicate_imports(source, file);
    let after_unused = if after_collapse != source {
        // Re-parse + re-signals before the next file-level pass.
        let line_index = crate::span::LineIndex::new(&after_collapse);
        let mut bag = crate::diagnostic::DiagBag::new();
        match crate::parse::parse_with_diagnostics_opts(&after_collapse, 0, "<fmt>", &line_index, &mut bag, enable_effects) {
            Some(re) => {
                let new_signals = crate::analyze::fmt_signals(&re);
                remove_unused_imports(&after_collapse, &re, &new_signals)
            }
            None => after_collapse,
        }
    } else {
        remove_unused_imports(source, file, signals)
    };

    if after_unused != source {
        let line_index = crate::span::LineIndex::new(&after_unused);
        let mut bag = crate::diagnostic::DiagBag::new();
        if let Some(re) = crate::parse::parse_with_diagnostics_opts(&after_unused, 0, "<fmt>", &line_index, &mut bag, enable_effects) {
            let new_signals = crate::analyze::fmt_signals(&re);
            return ast_rewrite_inner(&after_unused, &re, &new_signals, enable_effects);
        }
        return after_unused;
    }

    ast_rewrite_inner(source, file, signals, enable_effects)
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p glyph-core fmt_remove_unused_ -- --nocapture`
Expected: all 4 tests PASS.

Then: `cargo test -p glyph-core`
Expected: full suite passes.

- [ ] **Step 5: Commit**

```bash
git add crates/glyph-core/src/fmt.rs
git commit -m "fmt: remove unused imports (#108)"
```

---

## Task 3: #110 Stdlib auto-import

**Files:**
- Modify: `crates/glyph-core/src/fmt.rs` (add `auto_import_stdlib` function + tests)

**Goal:** When a name like `subagent`, `send`, or `load` is referenced and unresolved, append it to the `@glyph/std` selective import (or insert one if absent).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn fmt_auto_import_stdlib_inserts_new_import_when_absent() {
    let src = r#"skill main()
    description: "Main."
    flow:
        send("hi")
"#;
    let result = fmt_source(src, true);
    assert!(result.output.starts_with(r#"import "@glyph/std" { send }"#),
        "expected stdlib import inserted at top, got: {}", result.output);
    assert!(result.changed);
}

#[test]
fn fmt_auto_import_stdlib_appends_to_existing() {
    let src = r#"import "@glyph/std" { send }

skill main()
    description: "Main."
    flow:
        send("x")
        subagent("y")
"#;
    let result = fmt_source(src, true);
    assert!(result.output.contains(r#"import "@glyph/std" { send, subagent }"#),
        "expected subagent appended, got: {}", result.output);
    assert!(result.changed);
}

#[test]
fn fmt_auto_import_no_op_when_user_shadowed() {
    let src = r#"const subagent = "user-defined"

skill main()
    description: "Main."
    flow:
        send_value(subagent)
"#;
    let result = fmt_source(src, true);
    // `subagent` resolves to the local const — no auto-import.
    assert!(!result.output.contains("@glyph/std"),
        "should not auto-import when name is locally bound");
}

#[test]
fn fmt_auto_import_no_op_when_name_not_in_registry() {
    let src = r#"skill main()
    description: "Main."
    flow:
        zorp("bogus")
"#;
    let result = fmt_source(src, true);
    assert!(!result.output.contains("@glyph/std"));
    assert!(!result.changed);
}

#[test]
fn fmt_auto_import_idempotent() {
    let src = r#"skill main()
    description: "Main."
    flow:
        send("x")
"#;
    let once = fmt_source(src, true).output;
    let twice = fmt_source(&once, true).output;
    assert_eq!(once, twice);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glyph-core fmt_auto_import_ -- --nocapture`
Expected: all 5 tests FAIL.

- [ ] **Step 3: Implement the fix**

```rust
fn is_stdlib_name(name: &str) -> bool {
    matches!(name, "subagent" | "send" | "load")
}

fn auto_import_stdlib(source: &str, file: &crate::ast::SourceFile, signals: &crate::analyze::FmtSignals) -> String {
    use crate::ast::{Decl, ImportKind};

    // Which stdlib names are referenced AND unresolved in this file?
    let mut to_import: Vec<String> = signals.unresolved_names.iter()
        .filter(|n| is_stdlib_name(n))
        .cloned().collect();
    to_import.sort(); // deterministic order
    if to_import.is_empty() {
        return source.to_string();
    }

    let lines: Vec<&str> = source.lines().collect();

    // Find an existing `@glyph/std` selective import line to extend.
    let mut existing_idx: Option<usize> = None;
    let mut existing_names: Vec<String> = Vec::new();
    let mut import_seq = 0usize;
    let mut import_line_indices: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !line.starts_with(' ') && !line.starts_with('\t') && line.trim().starts_with("import ") {
            import_line_indices.push(i);
        }
    }

    for decl in &file.decls {
        if let Decl::Import(imp) = decl {
            if import_seq >= import_line_indices.len() { break; }
            let line_idx = import_line_indices[import_seq];
            import_seq += 1;
            if imp.node.path == "@glyph/std" {
                if let ImportKind::Selective(names) = &imp.node.kind {
                    existing_idx = Some(line_idx);
                    for n in names {
                        existing_names.push(
                            match &n.alias {
                                Some(a) => format!("{} as {}", n.name.node, a),
                                None => n.name.node.clone(),
                            }
                        );
                    }
                    break;
                }
            }
        }
    }

    let mut out = String::with_capacity(source.len() + 64);

    if let Some(idx) = existing_idx {
        // Append missing names (deduped) to the existing line.
        let mut all = existing_names.clone();
        for n in &to_import {
            if !all.contains(n) { all.push(n.clone()); }
        }
        let new_line = format!(r#"import "@glyph/std" {{ {} }}"#, all.join(", "));
        for (i, line) in lines.iter().enumerate() {
            if i == idx {
                out.push_str(&new_line);
            } else {
                out.push_str(line);
            }
            out.push('\n');
        }
    } else {
        // Insert a new line. Place it before the first non-import top-level decl.
        // Simplest: prepend to file with one trailing blank line.
        let new_line = format!(r#"import "@glyph/std" {{ {} }}"#, to_import.join(", "));
        // If there are existing imports, insert AFTER the last import.
        if let Some(&last_import) = import_line_indices.last() {
            for (i, line) in lines.iter().enumerate() {
                out.push_str(line);
                out.push('\n');
                if i == last_import {
                    out.push_str(&new_line);
                    out.push('\n');
                }
            }
        } else {
            out.push_str(&new_line);
            out.push('\n');
            out.push('\n');
            for line in &lines {
                out.push_str(line);
                out.push('\n');
            }
        }
    }

    if !source.ends_with('\n') && out.ends_with('\n') { out.pop(); }
    out
}
```

Chain this after `remove_unused_imports` in `ast_rewrite`'s pre-walk pipeline (with another re-parse if changed). The pattern is:

```rust
let after_stdlib = if changed_so_far {
    reparse_and_apply(after_unused, |s, f, sig| auto_import_stdlib(s, f, sig))
} else {
    auto_import_stdlib(source, file, signals)
};
```

Factor a small helper to avoid copy-paste:

```rust
fn reparse_and_run<F>(source: &str, enable_effects: bool, f: F) -> String
where F: FnOnce(&str, &crate::ast::SourceFile, &crate::analyze::FmtSignals) -> String {
    let line_index = crate::span::LineIndex::new(source);
    let mut bag = crate::diagnostic::DiagBag::new();
    match crate::parse::parse_with_diagnostics_opts(source, 0, "<fmt>", &line_index, &mut bag, enable_effects) {
        Some(file) => {
            let signals = crate::analyze::fmt_signals(&file);
            f(source, &file, &signals)
        }
        None => source.to_string(),
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p glyph-core fmt_auto_import_ -- --nocapture`
Expected: all 5 tests PASS.

Then: `cargo test -p glyph-core`
Expected: full suite passes.

- [ ] **Step 5: Commit**

```bash
git add crates/glyph-core/src/fmt.rs
git commit -m "fmt: auto-import @glyph/std for stdlib references (#110)"
```

---

## Task 4: #111 Const-in-flow parens-add

**Files:**
- Modify: `crates/glyph-core/src/fmt.rs` (per-decl rewrite — add to `rewrite_decl_body` flow handling)

**Goal:** When a bare name appears in a `flow:` body and doesn't resolve as a local binding or import, rewrite it as `name()`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn fmt_const_in_flow_adds_parens_to_unresolved_bare_name() {
    let src = r#"skill main()
    description: "Main."
    flow:
        helper
"#;
    let result = fmt_source(src, true);
    assert!(result.output.contains("helper()"),
        "expected `helper` rewritten to `helper()`, got: {}", result.output);
    assert!(result.changed);
}

#[test]
fn fmt_const_in_flow_no_op_when_resolves_to_local_const() {
    let src = r#"const helper = "x"

skill main()
    description: "Main."
    flow:
        helper
"#;
    let result = fmt_source(src, true);
    // bare name resolves to `const helper` → leave alone (LSP/analyze handles ambiguous-role)
    assert!(!result.output.contains("helper()"));
}

#[test]
fn fmt_const_in_flow_no_op_when_resolves_to_local_block() {
    let src = r#"block helper() -> Report
    description: "Helper."
    flow:
        return "<x>"

skill main()
    description: "Main."
    flow:
        helper
"#;
    let result = fmt_source(src, true);
    // bare `helper` already resolves to a local block; analyze handles via ambiguous-role.
    // fmt's parens-add only fires when truly unresolved.
    assert!(!result.output.contains("helper()\n"),
        "should not auto-paren when name resolves locally");
}

#[test]
fn fmt_const_in_flow_idempotent() {
    let src = r#"skill main()
    description: "Main."
    flow:
        helper
"#;
    let once = fmt_source(src, true).output;
    let twice = fmt_source(&once, true).output;
    assert_eq!(once, twice);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glyph-core fmt_const_in_flow_ -- --nocapture`
Expected: all 4 tests FAIL.

- [ ] **Step 3: Implement the fix**

The cleanest place is inside `rewrite_decl_body`'s flow-section handling. Look up where the existing function builds the flow body output and modify lines that match a bare-name pattern. We need access to `signals.unresolved_names`.

Add a param to `rewrite_decl_body`:

```rust
fn rewrite_decl_body(
    body_lines: &[&str],
    ast_decl: Option<&crate::ast::Decl>,
    signals: &crate::analyze::FmtSignals,
) -> String { ... }
```

Then within the flow-section serialization, walk each line; for indent-1 lines whose trimmed content is a bare identifier (matches `[A-Za-z_][A-Za-z0-9_]*`), check `signals.unresolved_names`. If unresolved, append `()`:

```rust
fn rewrite_bare_name_in_flow_line(line: &str, signals: &crate::analyze::FmtSignals) -> Option<String> {
    let indent_len = line.len() - line.trim_start().len();
    let indent = &line[..indent_len];
    let trimmed = line.trim();
    if trimmed.is_empty() { return None; }
    // Bare identifier (no parens, no `=`, no other punctuation).
    if !trimmed.chars().all(|c| c == '_' || c.is_ascii_alphanumeric()) { return None; }
    let first = trimmed.chars().next()?;
    if !(first == '_' || first.is_ascii_alphabetic()) { return None; }
    if signals.unresolved_names.contains(trimmed) {
        return Some(format!("{}{}()", indent, trimmed));
    }
    None
}
```

In the flow-section loop in `rewrite_decl_body` (find where it iterates over `Section { kind: SectionKind::Flow, lines }` or similar):

```rust
for line in &flow_section.lines {
    if let Some(repl) = rewrite_bare_name_in_flow_line(line, signals) {
        out.push_str(&repl);
    } else {
        out.push_str(line);
    }
    out.push('\n');
}
```

Update all `rewrite_decl_body` call sites to pass `signals`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p glyph-core fmt_const_in_flow_ -- --nocapture`
Expected: all 4 tests PASS.

Then: `cargo test -p glyph-core`
Expected: full suite passes (including the existing `body_bare_names` analyze tests — they may need updating if any post-fmt source they check changes).

- [ ] **Step 5: Commit**

```bash
git add crates/glyph-core/src/fmt.rs
git commit -m "fmt: parens-add for unresolved bare names in flow (#111)"
```

---

## Task 5: #112 Effects auto-insert

**Files:**
- Modify: `crates/glyph-core/src/fmt.rs` (per-decl rewrite — insert effects sub-section)

**Goal:** When a declaration has no `effects:` sub-section and analyze inferred a non-empty effect set, insert the sub-section. Gated on `enable_effects = true`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn fmt_effects_auto_insert_adds_inferred_effects() {
    let src = r#"import "@glyph/std" { send }

skill main()
    description: "Main."
    flow:
        send("hi")
"#;
    let result = fmt_source(src, true);
    assert!(result.output.contains("effects: spawns_agent"),
        "expected inferred effects inserted, got: {}", result.output);
    assert!(result.changed);
}

#[test]
fn fmt_effects_auto_insert_no_op_when_user_declared() {
    let src = r#"import "@glyph/std" { send }

skill main()
    description: "Main."
    effects: none
    flow:
        send("hi")
"#;
    let result = fmt_source(src, true);
    // User declared `effects: none` — leave it alone even if inferred disagrees.
    assert!(result.output.contains("effects: none"));
    assert!(!result.output.contains("effects: spawns_agent"));
}

#[test]
fn fmt_effects_auto_insert_no_op_when_inferred_empty() {
    let src = r#"skill main()
    description: "Main."
    flow:
        return "<done>"
"#;
    let result = fmt_source(src, true);
    assert!(!result.output.contains("effects:"));
}

#[test]
fn fmt_effects_auto_insert_no_op_when_enable_effects_false() {
    let src = r#"import "@glyph/std" { send }

skill main()
    description: "Main."
    flow:
        send("hi")
"#;
    let result = fmt_source(src, false);
    assert!(!result.output.contains("effects:"));
}

#[test]
fn fmt_effects_auto_insert_idempotent() {
    let src = r#"import "@glyph/std" { send }

skill main()
    description: "Main."
    flow:
        send("hi")
"#;
    let once = fmt_source(src, true).output;
    let twice = fmt_source(&once, true).output;
    assert_eq!(once, twice);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glyph-core fmt_effects_auto_insert_ -- --nocapture`
Expected: all 5 tests FAIL.

- [ ] **Step 3: Implement the fix**

Inside `rewrite_decl_body`, when assembling the canonical sub-section order, check whether an `Effects` section exists in `body_lines`. If absent AND `signals.inferred_effects.get(decl_name)` is non-empty AND `enable_effects = true`, synthesize:

```rust
fn synthesize_effects_section(effects: &[String], indent: &str) -> String {
    let mut s = String::new();
    s.push_str(indent);
    s.push_str("effects: ");
    s.push_str(&effects.join(", "));
    s.push('\n');
    s
}
```

Pass `enable_effects` and the inferred effect set into `rewrite_decl_body`. The existing canonical-order pass already places an `Effects` section between `description:` and `context:`; extend it to insert a synthesized one when missing.

Concretely (in the section-emission loop):

```rust
// After emitting description, before emitting other sections:
let has_effects_section = sections.iter().any(|s| s.kind == SectionKind::Effects);
if !has_effects_section && enable_effects {
    let decl_name = ast_decl.and_then(|d| match d {
        crate::ast::Decl::Skill(s) => Some(s.node.name.as_str()),
        crate::ast::Decl::Block(b) => Some(b.node.name.as_str()),
        _ => None,
    });
    if let Some(name) = decl_name {
        if let Some(effs) = signals.inferred_effects.get(name) {
            if !effs.is_empty() {
                out.push_str(&synthesize_effects_section(effs, "    "));
            }
        }
    }
}
```

Update `rewrite_decl_body`'s signature and call sites to pass `enable_effects: bool`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p glyph-core fmt_effects_auto_insert_ -- --nocapture`
Expected: all 5 tests PASS.

Then: `cargo test -p glyph-core`
Expected: full suite passes.

- [ ] **Step 5: Commit**

```bash
git add crates/glyph-core/src/fmt.rs
git commit -m "fmt: auto-insert inferred effects when missing (#112)"
```

---

## Task 6: #113 Placeholder return rewrite — close issue, no code change

**Goal:** Document that #113 is already implemented. Add one regression test covering the conservative no-rewrite case for special-character placeholders, to lock the current behavior into the test suite.

**Files:**
- Modify: `crates/glyph-core/src/fmt.rs` (one new regression test)

- [ ] **Step 1: Add the regression test**

```rust
#[test]
fn fmt_placeholder_return_no_rewrite_when_inner_contains_quote() {
    // Conservative behavior per design spec: descriptive form refuses
    // to rewrite when inner contains `"`, `\`, `\n`, `\t`, `\r`.
    let src = r#"export block report() -> Report
    description: "Report."
    return "<has \"quote\" inside>"
"#;
    let result = fmt_source(src, true);
    // Should leave the line alone — the diagnostic remains, no malformed output.
    assert!(result.output.contains(r#"return "<has \"quote\" inside>""#));
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p glyph-core fmt_placeholder_return_no_rewrite_when_inner_contains_quote -- --nocapture`
Expected: PASS (existing implementation already handles this).

- [ ] **Step 3: Commit**

```bash
git add crates/glyph-core/src/fmt.rs
git commit -m "fmt: regression test for placeholder return special-char no-op (#113)"
```

---

## Task 7: Integration test — multi-fix source converges to compile exit 0

**Files:**
- Create: `crates/glyph-cli/tests/corpus/fmt/multi_autofix_input.glyph.md`
- Create: `crates/glyph-cli/tests/corpus/fmt/multi_autofix_expected.glyph.md`
- Modify: `crates/glyph-cli/tests/fmt.rs` (add a test that runs fmt on the input and asserts equality with expected, then runs `glyph compile` on the expected and asserts exit 0)

**Goal:** Prove the agent's first fmt-compile iteration converges on a multi-fix file.

- [ ] **Step 1: Create the input fixture**

`crates/glyph-cli/tests/corpus/fmt/multi_autofix_input.glyph.md`:

```glyph
import "@glyph/std" { send }
import "@glyph/std" { send }

skill main()
    description: "Main."
    flow:
        send("hello")
        subagent("nested")
```

This has: duplicate import, missing stdlib auto-import for `subagent`, missing inferred effects.

- [ ] **Step 2: Create the expected output fixture**

`crates/glyph-cli/tests/corpus/fmt/multi_autofix_expected.glyph.md`:

```glyph
import "@glyph/std" { send, subagent }

skill main()
    description: "Main."
    effects: spawns_agent
    flow:
        send("hello")
        subagent("nested")
```

- [ ] **Step 3: Add the integration test to `crates/glyph-cli/tests/fmt.rs`**

Follow the existing test pattern in that file (look for any existing `#[test]` that reads from the corpus). Add:

```rust
#[test]
fn multi_autofix_converges() {
    let input = include_str!("corpus/fmt/multi_autofix_input.glyph.md");
    let expected = include_str!("corpus/fmt/multi_autofix_expected.glyph.md");

    let result = glyph_core::fmt::fmt_source(input, true);
    assert_eq!(result.output, expected, "fmt output mismatch");

    // Now compile the expected output and assert exit 0.
    // Use whatever compile entrypoint exists in glyph-core (look for `compile_source` or similar).
    // If only the CLI surface is available, shell out via assert_cmd.
    // Sketch:
    let line_index = glyph_core::span::LineIndex::new(expected);
    let mut bag = glyph_core::diagnostic::DiagBag::new();
    let parsed = glyph_core::parse::parse_with_diagnostics_opts(expected, 0, "<t>", &line_index, &mut bag, true);
    assert!(parsed.is_some(), "expected output should parse cleanly");

    // Run analyze; assert no `Repairable`-classification diagnostics.
    if let Some(file) = parsed {
        let mut bag2 = glyph_core::diagnostic::DiagBag::new();
        let mut registry = glyph_core::domain_registry::Registry::default();
        let _ = glyph_core::analyze::analyze_with_diagnostics(file, 0, "<t>", &line_index, &mut bag2, &mut registry);
        let repairable: Vec<_> = bag2.iter()
            .filter(|d| d.classification == glyph_core::diagnostic::Classification::Repairable)
            .collect();
        assert!(repairable.is_empty(),
            "expected no repairable diagnostics on post-fmt source, got: {:?}",
            repairable);
    }
}
```

If `Registry::default()` doesn't exist, adapt to the constructor used elsewhere in `analyze.rs` tests (search for `Registry::` in analyze.rs).

- [ ] **Step 4: Run the test**

Run: `cargo test -p glyph-cli multi_autofix_converges -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/glyph-cli/tests/corpus/fmt/multi_autofix_input.glyph.md \
        crates/glyph-cli/tests/corpus/fmt/multi_autofix_expected.glyph.md \
        crates/glyph-cli/tests/fmt.rs
git commit -m "fmt: integration test — multi-fix input converges to clean source (#106)"
```

---

## Task 8: Update `design/agent-skill.md` and `REPAIR_PASS_SPEC.md` (#114)

**Files:**
- Modify: `design/agent-skill.md`
- Modify: `REPAIR_PASS_SPEC.md` (if it exists; if not, skip with a note)

- [ ] **Step 1: Verify `REPAIR_PASS_SPEC.md` location**

Run: `find . -maxdepth 3 -name "REPAIR_PASS_SPEC.md" -not -path "*/target/*"`
If found, note the path. If not, skip Step 3 below and just edit `agent-skill.md`.

- [ ] **Step 2: Update `design/agent-skill.md`**

Locate the Phase 3a block (line ~12-29). Add a note listing the seven now-deterministic auto-fixes:

```markdown
## Phase 3a — Deterministic auto-fixes (`glyph fmt`)

`glyph fmt` runs exactly once at the top of the workflow, before the first `glyph compile`. It performs the following deterministic source rewrites without any LLM call:

- Tab → space, mixed-indentation fix
- Legacy `-> None` strip
- Constraint hoisting, context hoisting
- Canonical sub-section reorder
- Duplicate sub-section merge (#109)
- Duplicate import collapse (#107)
- Unused import removal (#108)
- Stdlib auto-import (#110)
- Const-in-flow parens-add (#111)
- Effects auto-insert (#112, gated on `--enable-effects`)
- Placeholder return rewrite (#113)

All fixes are idempotent and comment-preserving. After fmt, the agent runs `glyph compile`. If exit 2 still, the agent enters Phase 3b (LLM repair pass).
```

Locate the Phase 3b "Repair Guidance" table (around line 91 onward). Remove rows for the seven now-deterministic items. The remaining rows are: anything semantic (undefined-name, undefined-call, missing-description, applies-on-undescribed-block same-file, nested-branch, ambiguous-role, missing-return, export-missing-return-type, param-slot-in-non-instruction-string, operator-in-expression).

- [ ] **Step 3: Update `REPAIR_PASS_SPEC.md` if present**

Drop the seven now-deterministic items per PRD §Further Notes. Keep only LLM-pass repairs.

- [ ] **Step 4: Verify nothing broken**

Run: `cargo test`
Expected: full workspace passes (these are doc-only changes, but check anyway since the corpus may reference design wording).

- [ ] **Step 5: Commit**

```bash
git add design/agent-skill.md
# Add REPAIR_PASS_SPEC.md if it exists.
git commit -m "docs: update agent-skill + repair-pass spec for Phase 3a auto-fixes (#114)"
```

---

## Task 9: PR

- [ ] **Step 1: Push branch**

```bash
git push -u origin repair_deterministic
```

- [ ] **Step 2: Open PR**

```bash
gh pr create --title "Phase 3a: deterministic auto-fixes (#106)" --body "$(cat <<'EOF'
## Summary
- Adds 5 new deterministic auto-fixes to `glyph fmt`: duplicate-import collapse (#107), unused-import removal (#108), stdlib auto-import (#110), const-in-flow parens-add (#111), effects auto-insert (#112).
- Closes #113 (placeholder-return rewrite was already implemented in `fmt.rs`).
- Updates `design/agent-skill.md` and `REPAIR_PASS_SPEC.md` to reflect the new Phase 3a / 3b split (#114).

Implements parent PRD #106. Follows design spec at `docs/superpowers/specs/2026-05-04-phase-3a-auto-fixes-design.md`.

## Test plan
- [ ] `cargo test -p glyph-core` — all per-fix unit tests pass.
- [ ] `cargo test -p glyph-cli multi_autofix_converges` — integration test passes.
- [ ] Smoke: run `glyph fmt` on a real Glyph source containing duplicate imports + missing stdlib reference and confirm output is clean.
- [ ] Verify `glyph compile` exits 0 on post-fmt source from the integration fixture.
EOF
)"
```

- [ ] **Step 3: Confirm PR URL is returned**

Capture the URL output and report it.

---

## Self-review (run after writing the plan)

1. **Spec coverage:**
   - #107 → Task 1 ✓
   - #108 → Task 2 ✓
   - #110 → Task 3 ✓
   - #111 → Task 4 ✓
   - #112 → Task 5 ✓
   - #113 → Task 6 (regression test only; closure noted) ✓
   - #114 → Task 8 ✓
   - Analyze stratum / `FmtSignals` → Task 0 ✓
   - Integration test → Task 7 ✓

2. **Open items the executing agent must verify against the actual codebase:**
   - `ast.rs` variant names (`FlowStmt::Call`, `FlowStmt::BareName`, `ReturnExpr::Call`, `ReturnExpr::Bare`) — confirm by `grep -n "pub enum FlowStmt\|pub enum ReturnExpr" crates/glyph-core/src/ast.rs` before implementing Task 0.
   - `Diagnostic::classification` field name — confirm via `grep "Classification" crates/glyph-core/src/diagnostic.rs` (Task 7 references it).
   - Existing `rewrite_decl_body` section-emission loop structure — read it once before Task 4 / Task 5 to know exactly where to insert hooks.
   - `domain_registry::Registry` constructor — confirm via existing tests in `analyze.rs`.

3. **Type consistency:**
   - `FmtSignals` field names (`referenced_names`, `unresolved_names`, `inferred_effects`) used identically across all tasks ✓
   - `is_stdlib_name` / stdlib name set (`subagent`, `send`, `load`) consistent with `analyze::is_stdlib_block_name` (which today only has `subagent`, `send` — `load` is in `design/stdlib.md` but may need a separate addition; flag for executor)

4. **Known nuance:**
   - `analyze::is_stdlib_block_name` (line 2649) is `subagent` + `send` only. The PRD includes `load`. Executor should reconcile: either add `load` to `is_stdlib_block_name` and `stdlib_block_effects` in this PR, or scope down to `subagent`/`send` and file a follow-up. Recommend: add `load` (with empty effect set) so the auto-import fix covers all three.
