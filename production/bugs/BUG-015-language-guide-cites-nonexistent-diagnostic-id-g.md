# BUG-015: Language guide cites nonexistent diagnostic ID G::analyze::const-in-flow with wrong classification

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `GLYPH_LANGUAGE_GUIDE.md:196`
**Found by:** x-contract | **Audit date:** unknown-date

## Description

`GLYPH_LANGUAGE_GUIDE.md` §5.4 states that a bare string-valued `const` used in `flow:` without a marker is "an error (`G::analyze::const-in-flow`)". No such ID exists in the compiler or in `diagnostics.md`. The diagnostic that actually fires for a bare name in `flow:` is `G::analyze::text-in-flow` (`analyze.rs:5402-5420`), classified Repairable (not error). So both the ID and the classification in the guide are wrong: an author told to expect a hard `const-in-flow` error instead gets a repairable `text-in-flow` diagnostic, and repair may add parens or a generated block rather than rejecting. The pitfalls table at line 1316 also lists `const-in-flow` without the `G::analyze::` prefix, compounding the inconsistency. The name `const-in-flow` was an earlier planned name that the implementation never adopted.

## Trigger / Reproduction

Write a `.glyph` file with a bare `const`-valued name in `flow:` and run `glyph check`. The compiler emits `G::analyze::text-in-flow` (Repairable), not `G::analyze::const-in-flow` (Error) as the guide claims. An author who stops at the guide description and does not attempt compilation/repair will be misled about the recovery path.

## Evidence

```rust
// GLYPH_LANGUAGE_GUIDE.md line 196:
// "is an error (`G::analyze::const-in-flow`)"

// analyze.rs:5404-5409 (actual compiler):
// id: "G::analyze::text-in-flow".into(),
// classification: Repairable

// docs/reference/diagnostics.md line 156 (stable contract):
// G::analyze::text-in-flow — repairable
// (no entry for G::analyze::const-in-flow exists anywhere in the codebase)
```

## Recommended Resolution

Replace `G::analyze::const-in-flow` with the real `G::analyze::text-in-flow` at line 196, and correct the classification from "error" to "repairable". Apply the same correction to the pitfalls table at line 1316. If a distinct const-specific error ID is later desired, add it to both the compiler and the diagnostics catalog before documenting it.

## Verification Notes

A grep across all Rust source under `crates/` returns zero hits for `const-in-flow` — the ID was never implemented. The actual compiler (`analyze.rs:5409`) emits `G::analyze::text-in-flow` with classification `Repairable`, which is also what `docs/reference/diagnostics.md` (line 156) catalogs as the stable contract. The integration test in `crates/glyph-cli/tests/constraints_context.rs:357` explicitly asserts `G::analyze::text-in-flow` fires. The language guide's pitfalls table (line 1316) also uses the bare name `const-in-flow` without the `G::analyze::` prefix. The name `const-in-flow` appears only in architecture/design docs and a worktree — it was an earlier planned name the implementation never adopted.
