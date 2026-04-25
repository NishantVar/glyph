# Glyph Repair Pass and Generated Definitions

This document is the single authoritative reference for the LLM repair pass and repair-materialized generated definitions. Consolidates the former llm-repair-pass, generated-definitions, and comments design documents.

## 1. Purpose

The repair pass is a source-to-source pass that turns invalid or under-specified Glyph source into valid, still-readable Glyph source before deterministic IR compilation.

Repair is not just a safety net for experienced authors — it is the **primary content generation mechanism for novice authors**. A novice using only the kernel surface (`skill`, `require`/`avoid`, `flow:`, quoted strings, calls with parens, `with` modifier) writes source that contains many undefined bare names and parens-calls. Repair materializes these as `generated text` and `generated block` declarations so the source compiles; those generated definitions are the novice's effective "library" until they promote entries to hand-written `text` or `block`. This is why repair emits **one-sentence** generated bodies — short enough to minimize drift from author intent, reviewable at a glance, and easy to promote.

```text
loose or invalid Glyph source
    -> parse / resolve / infer diagnostics
    -> LLM repair pass, when needed
valid Glyph source
    -> deterministic source-to-IR compiler
typed IR
    -> compiled agent instructions
```

The repair pass is not the compiler. It fixes compiler-blocking issues so the normal compiler can continue. Deterministic semantic expansion is a later pass; repair makes source valid but does not flatten source into instructions.

## 2. Non-Goals

The repair pass must not:

- replace readable aliases with long generated text or inline shorthand instruction names as full prose at use sites;
- reinterpret the skill's purpose;
- reorder workflow steps unless the source is structurally invalid and no smaller repair exists;
- silently invent behavior that was not implied by the source;
- make a private `block` importable unless diagnostics clearly establish that the author intended an `export block`;
- produce compiled agent instructions directly.

## 3. Input / Output Contract

### Input

The pass receives:

- the original Glyph source;
- structured diagnostics from earlier deterministic passes;
- known local declarations, imports, and standard-library entries;
- the partial source AST when parsing succeeded far enough to produce one;
- compiler rules for valid syntax, role and constraint markers, type annotations, and declaration forms.

The LLM should repair against diagnostics, not free-form guess from scratch.

### Output

The pass returns:

- repaired Glyph source;
- a concise list of changes made;
- any unresolved questions or diagnostics that still need author input;
- a confidence level or equivalent repair status.

The compiler must re-run deterministic parsing, resolution, role inference, typing, and validation on the repaired source. The repair is accepted only if the deterministic compiler accepts it.

## 4. Repair Rules

### 4.1 Preserve Readability

The repaired file should still look like the author's Glyph file. The pass preserves:

- names and shorthand identifiers;
- comments (repair may insert new code around comments but must not delete, move, or rewrite comment text);
- ordering and section structure;
- indentation style where possible;
- inline text and string content;
- imports and local text blocks unless a diagnostic requires changing them.

### 4.2 Prefer Minimal Syntax

When a missing annotation blocks compilation, add the smallest disambiguating syntax. For instruction roles and constraints, add only the marker needed to make role, strength, and polarity deterministic.

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

### 4.3 No Inlining at Use Sites

The repair pass never inlines generated or expanded text at the use site. The bare name stays untouched in the skill or block body. The name resolves to its declaration through normal name resolution.

This is the core readability contract: repair keeps shorthand names readable while making future compilation deterministic. The LLM expansion happens once, during repair, by creating a generated definition; later compiler passes resolve from that stable definition.

### 4.4 Follow Intent Potency

Repair may make existing author intent explicit, but it must not make the intent stronger than the source supports.

- Repair may add syntax that clarifies an already-present instruction.
- Repair may add a generated definition whose meaning is implied by the shorthand name and local context.
- Repair may choose an explicit role or constraint marker when diagnostics and wording make the role, strength, and polarity clear.
- Repair must not upgrade a weak instruction into a hard requirement without evidence.
- Repair must not add new obligations, effects, imports, exports, or safety claims merely because they seem useful.

Acceptable: `unrelated_edits` may become `avoid unrelated_edits` because the context already carries avoid-like intent. Unacceptable: `think_about_tests` must not become `require add_full_test_suite` because that changes a weak consideration into a strong behavioral obligation.

When potency is ambiguous, repair should either choose the weakest compiling form that preserves the author's wording or return a diagnostic for author input.

### 4.5 Be Idempotent

Running repair twice on the same source, diagnostics, imports, standard library, and compiler schema produces no further source changes after the first accepted repair.

Detection mechanism: if a bare name already resolves to any declaration -- `text`, `generated text`, import, parameter, or local binding -- repair skips it. No fingerprinting, hashing, or version tracking; the mechanism is: "does this name resolve to something?" If yes, do not regenerate.

Repair may change the file again only when one of its inputs changes:

- the author edits the source;
- imports or standard-library definitions change;
- compiler syntax, typing, or validation rules change;
- diagnostics change;
- the author explicitly requests regeneration or migration.

The deterministic compiler remains responsible for proving idempotence operationally: after accepting a repaired source file, re-running parse, resolution, inference, validation, and repair eligibility should produce no repairable diagnostics for that same input set.

### 4.6 Add Types Only When Needed

Glyph source may be duck-typed and inferred. The repair pass adds type annotations only when inference fails or the compiler reports ambiguity.

```glyph
max_attempts = 3
```

becomes `max_attempts: Int = 3` only if the compiler needs that annotation.

### 4.7 Use Diagnostics Over Guesswork

The pass should be driven by compiler diagnostics. If a repair depends on intent that is not inferable from source, the pass should leave a diagnostic rather than silently choose.

Example unresolved question:

```text
Could not determine whether summarize_tradeoffs is a workflow step or an output contract.
Add an explicit step marker or output marker.
```

### 4.8 Compound Names

Compound names like `avoid_unrelated_edits` are valid identifiers and are **not** forcibly split into marker-plus-concept form. Both `avoid_unrelated_edits` (single identifier) and `avoid unrelated_edits` (marker keyword + concept name) are accepted authoring styles.

When a compound name resolves to a declaration (`text`, `generated text`, import, etc.), the compiler infers role, strength, and polarity from the declaration's text content, with the name prefix (`avoid_*`, `must_*`) as supporting evidence. No splitting or renaming occurs.

When a compound name is unresolved, repair generates a definition under the full compound name with the full semantics baked into the text body. For example, an unresolved `avoid_unrelated_edits` produces:

```glyph
generated text avoid_unrelated_edits = "Do not make changes outside the requested scope."
```

The definition carries the polarity in its text. No splitting, no renaming.

## 5. Generated Definitions

Repair materializes two kinds of generated declarations: `generated text` for undefined bare names, and `generated block` for undefined parens-calls. Both follow the same stability, placement, promotion, and idempotence rules.

### 5.1 Syntax

**`generated text`** — for undefined bare names (no parens at the use site):

```
generated text <name> = <string-literal>
```

Examples:

```glyph
generated text root_cause_before_fix = """
    Identify the root cause before proposing or applying a fix.
"""

generated text validate_before_success = "Run the full validation suite and confirm all checks pass before reporting success."
```

**`generated block`** — for undefined parens-calls (the use site has parentheses, with or without arguments):

```
generated block <name>(<params>)
    <one-sentence-body>
```

Examples:

```glyph
generated block inspect_failure(area)
    "Inspect the failure in {area} and identify what is failing."

generated block summarize_changes()
    "Summarize what was changed and why."
```

Rules common to both:

- `generated` is already reserved (`values-and-names.md`, Reserved Words section). No new reserved words.
- String literals follow `values-and-names.md`: inline `"..."` or block `"""..."""`, no interpolation.
- The repair pass picks the kind from the use site: parens-call → `generated block`; bare name → `generated text`. Never both for the same name.

Rules specific to `generated text`:

- Same shape as `text`. No parameters, no return type, no body with sub-sections.
- Not a callable. A bare name resolves to its string content; a parenthesized form is a compile error.

Rules specific to `generated block`:

- Minimal `block` shape with a `generated` prefix. Parameters are allowed (inferred from the use site); the generated form has no explicit return type annotation.
- The body is exactly one inline or block string — a single sentence. This is the **one-sentence rule**: generated bodies stay close to the name's meaning and leave room for the `with` modifier and downstream passes to shape the final instruction. If the name implies a multi-step workflow, repair emits one summarizing sentence and optionally leaves a diagnostic suggesting the author promote it to a hand-written `block` with a `flow:`.
- The body may reference parameters by name (e.g. `"{area}"`); the expand pass substitutes them with concrete values. No other interpolation semantics in MVP.

### 5.2 Repair-Only Authorship

Only the LLM repair pass emits `generated text` and `generated block` declarations. Authors do not hand-write them. Authors who want to define names manually use `text`, `block`, or `export block`.

This preserves a clean separation: `generated` means machine-created; `text`/`block` means author-created.

### 5.3 Placement

All generated declarations (both `generated text` and `generated block`) must appear after all non-generated top-level declarations in the source file. The compiler enforces this ordering rule. The repair pass appends generated declarations to the end of the file.

Example file structure:

```glyph
import "./repo_tools.glyph.md" { unrelated_edits }

text short_note = "Keep changes minimal."

skill fix_bug(scope)
    avoid unrelated_edits
    require preserve_existing_patterns

    flow:
        inspect_failure(scope) with "focus on auth boundaries"
        return summarize_changes()

generated text preserve_existing_patterns = "Follow the repository's existing patterns before introducing new abstractions."

generated block inspect_failure(area)
    "Inspect the failure in {area} and identify what is failing."

generated block summarize_changes()
    "Summarize what was changed and why."
```

### 5.4 Stability

Generated definitions are stable once created. Future compiles reuse the same definition and do not ask an LLM to regenerate it unless:

1. the shorthand name changes;
2. the generated definition is deleted;
3. the author explicitly asks to regenerate it;
4. the compiler schema requires a migration;
5. the generated definition no longer validates against the current language rules.

Detection uses the same name-resolution mechanism as idempotence (section 4.5): if the name already resolves to any declaration, repair skips it.

This turns LLM materialization of undefined names into a one-time source repair rather than repeated semantic guessing.

### 5.5 No-Shadowing Rule

Both `generated text` and `generated block` participate in the no-shadowing rule (`values-and-names.md`, No Shadowing section). If an author-written declaration (`text`, `block`, or `export block`) exists with the same name as a generated one in the same file, the compiler emits a warning and deletes the generated declaration, keeping the author-written version.

This is the only case where the compiler auto-deletes a declaration. The author's explicit declaration always supersedes the machine-generated version.

### 5.6 Promotion Paths

Authors may interact with generated declarations in three ways. All work through existing name resolution and the idempotence rule; no special compiler behavior is needed.

- **Edit the body.** The declaration stays `generated text` / `generated block`. Repair sees the name is defined and skips it. For `generated block`, edits are still constrained to the one-sentence body until promoted.
- **Promote to `text` or `block`.** Delete the word `generated`. For a promoted `block`, the author may then add `flow:`, `effects:`, `constraints:`, and a proper body with multiple steps. The declaration may also be moved anywhere in the file.
- **Promote to imported library.** Move the content into another `.glyph.md` file as `export text` or `export block`, import it back, and delete the local `generated` declaration.

### 5.7 Not Exportable

Neither `export generated text` nor `export generated block` is a valid declaration form. A generated definition is local to the file where repair created it. To share across files, the author must first promote it to `export text` or `export block`.

### 5.8 Compile-Time Behavior

Generated declarations compile identically to their hand-written counterparts:

- `generated text`: at the usage site, the bare name is replaced by the string content.
- `generated block`: at the usage site, the call expands to the one-sentence body, with `{param}` references preserved as named slots and the optional `with` modifier applied by the expand pass.
- The declaration itself produces nothing in compiled output. The `generated` marker is erased. No provenance marker appears in the compiled `.md` file.

## 6. Comment Syntax

Glyph uses `//` (double slash) for line comments. Block comments and doc-comments are deferred beyond the MVP.

- `//` may appear at the start of a line (whole-line comment) or after code on the same line (trailing comment).
- `//` inside a string literal (`"..."` or `"""..."""`) is not a comment.
- Comment-only lines are invisible to the indentation parser.
- Trailing comments do not affect indentation measurement.
- Blank lines around comments do not close blocks.
- Comments are stripped during compilation and do not appear in the compiled `.md` file.

## 7. Accepted Repairs

The repair pass may add:

- explicit role or constraint markers when context makes the intended role, strength, and polarity very clear;
- `generated text` definitions for unresolved compound names (e.g. `avoid_unrelated_edits`), with full semantics baked into the text body;
- missing type annotations;
- local declarations for author-defined shorthand;
- stable `generated text` definitions for undefined bare names;
- stable `generated block` definitions for undefined parens-calls (one-sentence bodies);
- missing imports when the referenced library is obvious from available context (deferred from MVP — see `todo.md`);
- `export` on a block only when an importability diagnostic makes the author's intent clear;
- missing block delimiters or indentation fixes;
- explicit section headers when the source already implies the section.

The repair pass may remove:

- duplicate declarations that make resolution impossible;
- syntax that is invalid and has a clear local correction.

The repair pass should not remove meaningful instructions.

## 8. Validation Loop

Repair is iterative but bounded:

1. Run deterministic compiler stages.
2. If diagnostics are repairable, run the LLM repair pass.
3. Re-run deterministic compiler stages.
4. Accept repaired source only if it compiles.
5. If diagnostics remain after a bounded number of attempts, stop and return the unresolved issues.

The LLM repair pass is never treated as proof of correctness. The deterministic compiler remains the authority.

## 9. Multi-File Repair

**MVP: repair only edits the current file.** All repairs — generated definitions, marker additions, indentation fixes, section reordering — are local to the file being compiled. If a diagnostic requires changes to another file (e.g., an imported block is not exported), repair emits a non-repairable diagnostic for the author to fix manually. Repair does not add `export` to another file's declarations and does not discover or add new `import` statements pointing to files the author did not already import.

This restriction eliminates cross-file trigger propagation: one file's repair cannot force another file to re-run from Phase 1. Each file's repair loop is self-contained.

**Post-MVP:** cross-file repair (editing other `.glyph.md` files when diagnostics require it) and auto-import discovery (adding imports to files the author did not reference) are deferred. See `todo.md`.

## 10. Argument-Agnosticism Invariant

**Repair is argument-agnostic.** It operates on authored source without any concrete argument values. It does not receive, inspect, or depend on concrete argument values. (Since compilation is parameterless, no phase receives concrete argument values — parameters appear as `{param}` slots in compiled output, resolved by the consuming LLM at runtime.) This property holds for three structural reasons:

1. **Nominal-only types.** The MVP type system (`types.md`) uses opaque name tags with no union types, generics, or conditional types. No type can narrow based on a concrete argument value, so no type diagnostic is hidden from Repair by the absence of arguments.

2. **Branch conditions are structural, not evaluated.** `if`/`elif`/`else` blocks are checked exhaustively — Repair resolves names and assigns roles in every branch regardless of the condition. Conditions are preserved as text through Lower and flattened into prose by Expand; no phase evaluates them.

3. **Topological compilation order.** An importing file cannot enter Phase 2 (Analyze) until the imported file has passed Phase 5 (Validate) — see `pipeline.md` §Multi-File Compilation Order. Repair always sees dependencies in post-repair, post-validate form.

This invariant is what enables the cache-key-by-post-repair-source-hash strategy (`pipeline.md` §Cacheability): Phases 1-5 produce a validated IR that is independent of invocation arguments.

**Post-MVP consideration:** If the type system gains union types, structural narrowing, or value-dependent type features, this invariant must be re-examined.

## 11. Open Questions

- **Diagnostic taxonomy.** The diagnostic shape and classification tiers are defined in [diagnostics.md](diagnostics.md). The full catalog of individual diagnostics will be built out as the compiler is implemented.
- **Security and trust.** Prevent repair from adding imports, effects, exports, or generated text that broadens behavior beyond the author's apparent intent.
- **Generation limits.** Whether the compiler should limit the number of `generated text` declarations per file.
- **Migration hashing.** Whether `generated text` should carry a compiler-generated hash for migration detection when language rules change.
- **Tooling.** IDE highlighting, gutter markers, or quick-fix actions for promoting `generated text` to `text`.
