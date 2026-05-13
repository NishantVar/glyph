# Glyph Repair — Author-Facing Contract

This document describes how Repair behaves from a Glyph author's point of view: what it may change in your source, what it must preserve, how generated definitions appear in your file, and where the line between repair and error sits.

For the algorithms, prompts, retry policy, and phase mechanics that implement these guarantees, see [[docs/architecture/repair]].

## 1. What Repair Is

Repair is a source-to-source pass that turns invalid or under-specified Glyph source into valid Glyph source before the deterministic compiler runs. It is not the compiler. It fixes compiler-blocking issues so the normal compiler can continue; it never produces compiled agent instructions directly.

Repair is the primary content-generation mechanism for novice authors. A novice using only the kernel surface (`skill`, `require`/`avoid`, `flow:`, quoted strings, calls with parens, `with` modifier) writes source containing many undefined bare names and parens-calls. Repair materializes these as `generated const` and `generated block` declarations so the source compiles. Those generated definitions become the novice's effective "library" until they promote entries to hand-written `const` or `block`.

## 2. What Repair May Change

The repair pass may add:

- Explicit role or constraint markers when context makes the intended role, strength, and polarity clear (e.g. promoting `unrelated_edits` to `avoid unrelated_edits`).
- `generated const` definitions for undefined bare names used with keyword prefixes (`require`/`avoid`/`must`/`context`) and for unresolved compound names like `avoid_unrelated_edits`.
- `generated block` definitions for undefined parens-calls and for undefined bare names in `flow:` (with parens added).
- Missing type annotations when inference fails.
- Missing `effects:` on any declaration whose inferred set is non-empty.
- A `description:` on a `skill` that lacks one.
- A `generated block` extracted from a nested branch (`if`/`elif`/`else` nested inside another arm), replacing the inner branch with a call.
- Standard-library imports (`@glyph/std`) when an unresolved name matches a stdlib entry. Non-stdlib auto-import is deferred.
- Rewrites of legacy `-> None` return-type annotations and placeholder string returns (`return "<...>"`) on domain-typed declarations into the canonical form.

The repair pass may remove:

- Duplicate import lines, unused imports, and duplicate sub-sections that merge cleanly into one occurrence.
- Syntax that is invalid and has a clear local correction.

The repair pass should not remove meaningful instructions.

## 3. What Repair Must Preserve

### Readability

The repaired file must still look like the author's Glyph file. The pass preserves:

- Names and shorthand identifiers — repair never inlines or expands a name at its use site.
- Comments verbatim — repair may insert new code around comments but must not delete, move, or rewrite comment text. Three sub-rules govern comment placement during duplicate sub-section merges (see [[docs/architecture/repair]] §4.4 Duplicate Sub-Section Merge).
- Source ordering, section structure, indentation style, inline text, and string content.
- The author's import block — repair may add a stdlib import or remove duplicate/unused imports, but never rewrites the author's selective-import set, aliases, or imported-file declarations.

### Intent Potency

Repair may make existing author intent **explicit** but must not make it **stronger** than the source supports.

- Repair may add syntax that clarifies an already-present instruction.
- Repair may add a generated definition whose meaning is implied by the shorthand name and local context.
- Repair must not upgrade a weak instruction into a hard requirement without evidence.
- Repair must not add new obligations, effects, imports, exports, or safety claims merely because they seem useful.

Acceptable: `unrelated_edits` may become `avoid unrelated_edits` because the context already carries avoid-like intent.

Unacceptable: `think_about_tests` must not become `require add_full_test_suite` — that upgrades a weak consideration into a strong behavioral obligation.

When potency is ambiguous, repair chooses the weakest compiling form that preserves the author's wording, or returns a diagnostic asking for author input.

### Names As Anchors For Idempotence

If a bare name already resolves to any declaration — `const`, `generated const`, import, parameter, or local binding — repair skips it. There is no fingerprinting or hashing; the question is simply "does this name resolve to something?" If yes, do not regenerate.

## 4. How Generated Definitions Appear

Repair materializes two kinds of generated declarations. Both follow the same stability, placement, and promotion rules.

**`generated const`** — for undefined bare names used with keyword prefixes (`require`/`avoid`/`must`/`context`):

```glyph
generated const preserve_existing_patterns = "Follow the repository's existing patterns before introducing new abstractions."

generated const root_cause_before_fix = """
    Identify the root cause before proposing or applying a fix.
"""
```

**`generated block`** — for undefined parens-calls and for undefined bare names in `flow:` (where Repair adds parens):

```glyph
generated block inspect_failure(area)
    "Inspect the failure in {area} and identify what is failing."

generated block summarize_changes()
    "Summarize what was changed and why."
```

Rules authors can rely on:

- The `generated` keyword always marks machine-created declarations. Authors do not hand-write `generated`; authors use plain `const`, `block`, or `export block`.
- A generated block always has a **single-string body** — one inline `"..."` or block `"""..."""` string, no `flow:`, no other sub-sections. If the name implies multiple steps, repair emits one summarizing instruction and inserts a `//` comment above the declaration suggesting promotion to a hand-written `block`.
- All generated declarations live at the **end of the file**, after every non-generated top-level declaration.
- Generated declarations are **not exportable**. Neither `export generated const` nor `export generated block` is valid. To share across files, promote first.
- A generated body may reference its parameters by name (`"{area}"`). It cannot contain calls into other declarations — generated bodies introduce no cross-file dependencies.

### Routing Table

| Use site of the undefined name | Repair materializes |
|---|---|
| `flow:` step (no keyword prefix) | `generated block` (parens added first) |
| `if` / `elif` condition | `generated const` |
| Constraint marker (`require X`, `avoid X`, `must X`) | `generated const` |
| Context marker (`context X`) | `generated const` |
| Parens-call (`X(...)`) anywhere | `generated block` |

A bare name with a keyword prefix becomes a `const`; a callable becomes a `block`. Never both for the same name.

### Compile-Time Behavior

Generated declarations compile identically to their hand-written counterparts. At a use site, a `generated const` resolves to its string content; a `generated block` call expands to its single-string body, with `{param}` references preserved as named runtime slots. The `generated` marker is erased — no provenance marker appears in the compiled `.md`.

## 5. Idempotence As A Contract

Running repair twice on the same source, diagnostics, imports, standard library, and compiler schema produces no further source changes after the first accepted repair.

Repair may change the file again only when one of its inputs changes:

- The author edits the source.
- Imports or standard-library definitions change.
- Compiler syntax, typing, or validation rules change.
- Diagnostics change.
- The author explicitly requests regeneration or migration.

Repair itself is LLM-driven and is **not byte-deterministic** across runs of un-repaired source. The recommended workflow:

1. Author writes source using the novice kernel.
2. Author runs the compiler locally. Repair fires, writes back to the `.glyph` file, compilation succeeds.
3. Author **commits the post-repair source**. Subsequent compiles find no repairable diagnostics, skip Repair entirely, and produce identical IR.

Once committed post-repair source is in place, downstream builds (CI, other contributors) are reproducible by construction.

## 6. The Repair / Error Boundary

Repair fixes compiler-blocking issues only when the author's intent is locally recoverable. The following are **never repaired** — they are hard errors that demand author action:

- **Author-written name collisions.** Two hand-written declarations sharing a name. Repair cannot infer which the author intended.
- **Generated-vs-generated collisions.** Two different unresolved use sites would produce the same generated name.
- **Contradictory constraints on the same declaration.** When two constraints cannot both be satisfied, repair surfaces the contradiction; it does not silently drop one. (Soft tensions get a warning and ship; hard contradictions fail the build.)
- **Cross-file changes.** Repair only edits the file currently being compiled. If a diagnostic requires adding `export` to a declaration in another file, or importing from a file the author did not reference, repair surfaces a non-repairable diagnostic. Cross-file repair is deferred.
- **Predicate generation failure.** If the LLM cannot produce a sensible condition string for an undefined name in `if`/`elif` position, repair errors out and the author must write the `const` manually.
- **Repair non-convergence.** If repairable diagnostics remain after the compiler-configured iteration cap, the compiler surfaces the residual diagnostics and fails the build.

The principle: repair generates *new* content from missing references; it does not *delete* or *contradict* authored content. Any judgment call that would override what the author wrote belongs with the author.

## 7. No-Shadowing With Generated Definitions

If a hand-written declaration (`const`, `block`, `export block`) exists with the same name as a generated one in the same file, the compiler emits a warning and deletes the generated declaration, keeping the author-written version.

This is the **only** case where the compiler auto-deletes a declaration. The author's explicit declaration always supersedes the machine-generated version.

## 8. Promoting Generated Definitions

Authors interact with generated declarations in three ways. All work through ordinary name resolution and the idempotence rule above; no special compiler behavior is needed.

- **Edit the body.** The declaration stays `generated const` / `generated block`. Repair sees the name is defined and skips it. For `generated block`, edits are constrained to the single-string body until promoted.
- **Promote to `const` or `block`.** Delete the word `generated`. A promoted `block` may then add `flow:`, `effects:`, `constraints:`, and a multi-step body, and may be moved anywhere in the file.
- **Promote to imported library.** Move the content into another `.glyph` file as `export const` or `export block`, import it back, and delete the local generated declaration.

## 9. Comments

Glyph uses `//` for line comments. `//` may appear at the start of a line or after code on the same line. `//` inside a string literal is not a comment. Comments are stripped during compilation and do not appear in the compiled `.md`. Block comments and doc-comments are deferred beyond the MVP.
