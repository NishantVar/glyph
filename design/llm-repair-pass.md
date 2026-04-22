# Glyph LLM Repair Pass

This document defines the MVP LLM repair pass for Glyph. The repair pass exists to turn invalid or under-specified Glyph source into valid, still-readable Glyph source before deterministic IR compilation.

## Position In The Pipeline

The repair pass is a source-to-source pass:

```text
loose or invalid Glyph source
    -> parse / resolve / infer diagnostics
    -> LLM repair pass, when needed
valid Glyph source
    -> deterministic source-to-IR compiler
typed IR
    -> compiled agent instructions
```

The repair pass is not the compiler. It fixes compiler-blocking issues so the normal compiler can continue.

LLM repair is part of the MVP. In particular, undefined bare instruction names may be expanded by the LLM during repair by materializing stable `generated text` declarations in source. The use sites should remain readable bare names.

## Purpose

The repair pass should make the smallest source edits needed for the file to compile exactly.

It may fix issues such as:

- missing explicit role or constraint markers when inference cannot resolve an instruction role, strength, or polarity;
- missing or ambiguous type annotations when type inference fails;
- invalid primitive usage;
- malformed block structure or indentation;
- unresolved local declarations for shorthand instructions that are intentionally author-defined;
- undefined bare or shorthand instructions that need stable `generated text` declarations;
- import or declaration shape errors that prevent name resolution.

The pass should preserve the author's intended style and readable shorthand wherever possible.

## Non-Goals

The repair pass must not:

- inline shorthand instruction names as full prose at use sites;
- replace readable aliases with long generated text;
- reinterpret the skill's purpose;
- reorder workflow steps unless the source is structurally invalid and no smaller repair exists;
- silently invent behavior that was not implied by the source;
- make a private `block` importable unless diagnostics and surrounding source clearly establish that the author intended an `export block`;
- produce compiled agent instructions directly.

Deterministic semantic expansion is a later pass. Repair makes source valid; it does not flatten source into instructions.

## Input Contract

The pass receives:

- the original Glyph source;
- structured diagnostics from earlier deterministic passes;
- known local declarations, imports, and standard-library entries;
- the partial source AST when parsing succeeded far enough to produce one;
- compiler rules for valid syntax, role and constraint markers, type annotations, and declaration forms.

The LLM should repair against diagnostics, not free-form guess from scratch.

## Output Contract

The pass returns:

- repaired Glyph source;
- a concise list of changes made;
- any unresolved questions or diagnostics that still need author input;
- a confidence level or equivalent repair status.

The compiler must re-run deterministic parsing, resolution, role inference, typing, and validation on the repaired source. The repair is accepted only if the deterministic compiler accepts it.

## Repair Rules

### 1. Preserve Readability

The repaired file should still look like the author's Glyph file. The pass should preserve:

- names and shorthand identifiers;
- comments;
- ordering;
- section structure;
- indentation style where possible;
- inline text;
- imports and local text blocks unless a diagnostic requires changing them.

### 2. Prefer Minimal Syntax

When a missing annotation blocks compilation, add the smallest disambiguating syntax. For instruction roles and constraints, this means adding only the marker needed to make role, strength, or polarity deterministic.

Example:

```glyph
skill fix_bug(scope)
    unrelated_edits
    preserve_existing_patterns
```

If the compiler cannot infer polarity for the first line but can for the second, repair may produce:

```glyph
skill fix_bug(scope)
    avoid unrelated_edits
    preserve_existing_patterns
```

It should not expand the shorthand:

```glyph
skill fix_bug(scope)
    "Do not make unrelated edits outside the requested scope."
    preserve_existing_patterns
```

When the source uses a compact compound name whose marker is clear, repair should
normalize it to marker-plus-concept form and report the normalization:

```glyph
skill fix_bug(scope)
    avoid_unrelated_edits
```

may become:

```glyph
skill fix_bug(scope)
    avoid unrelated_edits
```

The repair report should mention that `avoid_unrelated_edits` was normalized to
`avoid unrelated_edits`.

### 3. Expand Undefined Bare Names Into Stable Definitions

Undefined bare names or shorthand that are intentionally author-facing should be expanded during MVP repair by generating stable local definitions. They should not be expanded away where they are used.

Example:

```glyph
skill debug_failure(scope)
    root_cause_before_fix
```

If the compiler requires declaration before use, repair may produce a stable `generated text` declaration while preserving the usage:

```glyph
skill debug_failure(scope)
    root_cause_before_fix

generated text root_cause_before_fix = """
    Identify the root cause before proposing or applying a fix.
"""
```

`generated text` declarations should be appended automatically to the end of the current `.glyph.md` file. The repair pass should not inline the generated text at the use site. The semantic commitment is that repair keeps the shorthand name readable while making future compilation deterministic. The LLM expansion happens once, during repair, by creating the `generated text` declaration; later compiler passes resolve from that stable definition.

The declaration syntax is `generated text <name> = <string-literal>`, identical to `text` with a `generated` prefix. The `generated` marker makes it visibly machine-created so authors can review, edit, or promote it into a shared library. See `generated-definitions.md` for the full specification.

### 4. Keep Generated Definitions Stable

When repair generates a definition for shorthand, that definition becomes the deterministic local meaning of the shorthand. Future compiles should reuse the same definition and should not ask an LLM to regenerate it unless:

- the shorthand name changes;
- the `generated text` declaration is deleted;
- the author explicitly asks to regenerate it;
- the compiler schema requires a migration;
- the `generated text` declaration no longer validates against the current language rules.

This turns LLM expansion of undefined bare names into a one-time source repair rather than repeated semantic guessing.

### 5. Follow Intent Potency

Repair may make existing author intent explicit, but it must not make the intent stronger than the source supports.

This is the intent potency rule:

- repair may add syntax that clarifies an already-present instruction;
- repair may add a `generated text` declaration whose meaning is implied by the shorthand name and local context;
- repair may choose an explicit role or constraint marker when diagnostics and wording make the role, strength, or polarity clear;
- repair must not upgrade a weak instruction into a hard requirement without evidence;
- repair must not add new obligations, effects, imports, exports, or safety claims merely because they seem useful.

Examples:

```glyph
skill fix_bug(scope)
    unrelated_edits
```

may become:

```glyph
skill fix_bug(scope)
    avoid unrelated_edits
```

because the shorthand, resolved metadata, or local context already carries avoid-like intent.

But:

```glyph
skill fix_bug(scope)
    think_about_tests
```

should not become:

```glyph
skill fix_bug(scope)
    require add_full_test_suite
```

because that changes a weak consideration into a strong behavioral obligation.

When potency is ambiguous, repair should either choose the weakest compiling form that preserves the author's wording or return a diagnostic for author input. For MVP role and constraint inference, repair should add a marker only when the source context is very clear; otherwise it should leave a diagnostic.

### 6. Be Idempotent

Running repair twice on the same source, diagnostics, imports, standard library, and compiler schema should produce no further source changes after the first accepted repair.

This rule prevents repair from becoming an open-ended rewriting loop. Once a missing keyword, type, declaration, or `generated text` declaration has been added, future repair runs should treat that repaired source as stable.

Repair may change the file again only when one of its inputs changes:

- the author edits the source;
- imports or standard-library definitions change;
- compiler syntax, typing, or validation rules change;
- diagnostics change;
- the author explicitly requests regeneration or migration.

The deterministic compiler remains responsible for proving idempotence operationally: after accepting a repaired source file, re-running parse, resolution, inference, validation, and repair eligibility should produce no repairable diagnostics for that same input set.

### 7. Add Types Only When Needed

Glyph source may be duck-typed and inferred. The repair pass should add type annotations only when inference fails or the compiler reports ambiguity.

Example:

```glyph
max_attempts = 3
```

Could become:

```glyph
max_attempts: Int = 3
```

But only if the compiler needs that annotation.

### 8. Use Diagnostics Over Guesswork

The pass should be driven by compiler diagnostics. If a repair depends on intent that is not inferable from source, the pass should leave a diagnostic rather than silently choose.

Example unresolved question:

```text
Could not determine whether summarize_tradeoffs is a workflow step or an output contract.
Add an explicit step marker or output marker.
```

## Accepted Repairs

The repair pass may add:

- explicit role or constraint markers when context makes the intended role, strength, or polarity very clear;
- marker-plus-concept normalizations such as `avoid_unrelated_edits` to `avoid unrelated_edits`, with a notification;
- missing type annotations;
- local declarations for author-defined shorthand;
- stable `generated text` declarations for undefined bare or shorthand instructions;
- missing imports when the referenced library is obvious from available context;
- `export` on a block only when an importability diagnostic makes the author's intent clear and the repaired block still passes closure validation;
- missing block delimiters or indentation fixes;
- explicit section headers when the source already implies the section.

The repair pass may remove:

- duplicate declarations that make resolution impossible;
- syntax that is invalid and has a clear local correction.

The repair pass should not remove meaningful instructions.

## Validation Loop

Repair is iterative but bounded:

1. Run deterministic compiler stages.
2. If diagnostics are repairable, run the LLM repair pass.
3. Re-run deterministic compiler stages.
4. Accept repaired source only if it compiles.
5. If diagnostics remain after a bounded number of attempts, stop and return the unresolved issues.

The LLM repair pass should never be treated as proof of correctness. The deterministic compiler remains the authority.

## Relationship To Semantic Expansion

Repair and deterministic semantic expansion are separate.

Repair:

```glyph
root_cause_before_fix
```

may become:

```glyph
root_cause_before_fix

generated text root_cause_before_fix = """
    Identify the root cause before proposing or applying a fix.
"""
```

The LLM expansion of `root_cause_before_fix` happened during repair by creating the `generated text` declaration. Deterministic semantic expansion later resolves `root_cause_before_fix` from that stable definition into the IR or compiled output.

This separation preserves readability while still giving the compiler explicit structure.

## Important Remaining Gaps

The repair-pass design still needs decisions in these areas:

- **Diagnostic taxonomy.** Define which compiler errors are repairable, which require author input, and which must fail immediately.
- **Security and trust.** Prevent repair from adding imports, effects, exports, or generated text that broadens behavior beyond the author's apparent intent.

## Multi-File Repair

Repair may edit more than the current `.glyph.md` file only when diagnostics require changing those other files. For example, if an imported `.glyph.md` file itself has repairable diagnostics, repair may modify that imported file. Repair should not edit imported files merely because the current file references them; if the current file needs a local `generated text` declaration, append it to the current file.

Repair writes directly to source files, like any normal compiler-assisted source rewrite. The user can review those file changes afterward using normal editor or version-control workflows.

## Open Syntax Choices

The pass depends on syntax that is not finalized yet:

- declaration syntax for shorthand instructions;
- whether type annotations use `name: Type = value`, `let name: Type = value`, or another form.
