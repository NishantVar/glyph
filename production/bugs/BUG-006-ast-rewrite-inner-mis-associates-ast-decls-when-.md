# BUG-006: ast_rewrite_inner mis-associates AST decls when a `type` declaration precedes a skill/block (positional index vs text scan)

**Severity:** high | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/fmt.rs:538-607`
**Found by:** fmt-1 | **Audit date:** unknown-date

## Description

`ast_rewrite_inner` builds `decl_ranges` by text-scanning indent-0 lines whose trimmed prefix is one of: `skill `, `block `, `export block `, `export const `, `const `, `generated `, `import ` (lines 544–553). It then looks up the matching AST declaration positionally: `let ast_decl = file.decls.get(decl_idx)` (line 598), where `decl_idx` is the enumeration index into `decl_ranges`.

The AST `Decl` enum also has a `TypeDecl` variant (`type Foo = <"...">` and `export type ...`), which the parser emits into `file.decls` in source order — but the text scanner has no `type `/`export type ` prefix, so type decls are excluded from `decl_starts`. Every declaration appearing after a `type` decl therefore gets a `decl_idx` one less than its true position in `file.decls`, so `file.decls.get(decl_idx)` returns the wrong decl.

`type` declarations are a documented, common feature typically placed at the top of files. Concrete consequences:

1. **Under-application:** For a `type` decl followed by a single skill, decl_idx 0 resolves to `file.decls[0] = TypeDecl` instead of the skill. `placeholder_string_return_target` returns `None` (the `_ => None` arm handles TypeDecl) so `return "<...>"` placeholder rewrites are silently skipped.

2. **Active wrong output:** With a `type` decl followed by two skills alpha and beta, beta's text range (decl_idx 1) resolves to `file.decls[1] = Skill(alpha)`. When `--enable-effects` is set, alpha's inferred effects (`signals.inferred_effects["alpha"]`) are synthesized into beta's body.

## Trigger / Reproduction

Any file placing a `type` or `export type` declaration before a skill or block:

```
type Severity = <"low | medium | high">

skill assess() -> Severity
    flow:
        return "<the severity>"
```

Running `glyph fmt` leaves the `return "<the severity>"` placeholder unrewritten, whereas the same skill without the preceding type decl correctly rewrites it to `return <"the severity">`.

## Evidence

```rust
// Text scanner recognizes these prefixes only (no "type " or "export type "):
// "skill ", "block ", "export block ", "export const ", "const ", "generated ", "import "

// Positional lookup — decl_idx counts only text-scanned keywords:
let ast_decl = file.decls.get(decl_idx);
// When a TypeDecl precedes the skill in file.decls, decl_idx is off-by-one:
// decl_idx 0 → file.decls[0] = TypeDecl  (wrong; expected Skill)
// decl_idx 1 → file.decls[1] = Skill(alpha) when beta was intended
```

## Recommended Resolution

Do not rely on positional alignment between the text scanner and `file.decls`. Use one of:

- **(a)** Add `"type "` and `"export type "` to the text scanner's recognized prefixes so `decl_ranges` matches `file.decls` one-to-one. TypeDecl text ranges then get a `decl_idx` that correctly points to the TypeDecl AST node, and subsequent skill/block indices are correct.
- **(b)** Match each text decl to its AST decl by source span/line number (as the import passes do via `line_col(imp.span.start)`) rather than by enumeration index.

Option (b) is more robust against future new `Decl` variants being added without updating the scanner.

## Verification Notes

Code at fmt.rs lines 544–552 confirmed to list seven prefixes with no `type` or `export type`. Parser at parse.rs lines 856–858 and 926–929 pushes `Decl::TypeDecl` into `file.decls` in source order. Live reproduction: `glyph fmt` on a file with `type T = <"...">` then a skill with `return "<x>"` leaves the placeholder unrewritten. Corpus fixtures `return_row2_named_with_type_decl.glyph` and `return_row5_expr_with_type_decl.glyph` use the triggering pattern but no fmt test exercises them, leaving the misalignment untested.
