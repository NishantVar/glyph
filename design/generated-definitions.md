# Glyph Generated Definitions

This document defines the MVP source syntax for repair-materialized generated definitions: the `generated text` declaration form.

## Status

MVP Tier 3. Formalizes the generated-definition sketch in `llm-repair-pass.md:136-169` and the inlining contract in `compiled-output.md:162`.

## Purpose

When the LLM repair pass encounters an undefined bare name, it materializes a stable definition so the deterministic compiler can resolve the name. The `generated text` declaration is the source form for that materialized definition. It is structurally identical to `text` (`declaration-headers.md:99-116`) with a `generated` prefix that marks it as machine-created.

## Declaration Header

### Grammar

```
generated text <name> = <string-literal>
```

### Examples

```glyph
generated text root_cause_before_fix = """
    Identify the root cause before proposing or applying a fix.
"""

generated text validate_before_success = "Run the full validation suite and confirm all checks pass before reporting success."
```

### Rules

- **Same shape as `text`.** The header follows the `text` grammar from `declaration-headers.md:104`: `generated` prefix, then `text <name> = <string-literal>`. No parameters, no return type, no body with sub-sections.
- **String literals follow `values-and-literals.md`.** Inline `"..."` or block `"""..."""`. No interpolation.
- **`generated` is already reserved** (`values-and-literals.md:113`). No new reserved words are introduced.
- **Not a callable.** A `generated text` declaration is a named constant, identical to `text`. A bare name resolves to its string content; a parenthesized form would be a compile error (no such block exists).

## Repair-Only Authorship

Only the LLM repair pass emits `generated text` declarations. Authors do not hand-write them. Authors who want to define bare names manually use `text` declarations (`declaration-headers.md:99-116`).

This preserves a clean separation: `generated` means machine-created, `text` means author-created.

## Placement Policy

All `generated text` declarations must appear after all non-generated top-level declarations in the source file. The compiler enforces this ordering rule.

No comment divider or section marker is required. The repair pass appends generated declarations to the end of the file (`llm-repair-pass.md:160`), and the ordering rule keeps them there.

Example file structure:

```glyph
import "./repo_tools.glyph.md" { unrelated_edits }

text short_note = "Keep changes minimal."

skill fix_bug(scope)
    root_cause_before_fix
    avoid unrelated_edits

    flow:
        inspect_failure(scope)
        return summarize_changes()

generated text root_cause_before_fix = """
    Identify the root cause before proposing or applying a fix.
"""
```

## Usage Site Behavior

The repair pass never inlines generated text at the use site. The bare name stays untouched in the skill or block body (`llm-repair-pass.md:138-139`, `llm-repair-pass.md:158`). The name resolves to the `generated text` declaration through normal name resolution (`values-and-literals.md:119-128`).

## Idempotence Detection

Repair does not regenerate an existing definition. Detection uses name resolution: if a bare name already resolves to any declaration — `text`, `generated text`, import, parameter, or local binding — repair skips it (`llm-repair-pass.md:171-181`).

No fingerprinting, hashing, or version tracking. The mechanism is: "does this name resolve to something?" If yes, do not regenerate.

## No-Shadowing Rule

`generated text` participates in the no-shadowing rule (`values-and-literals.md:139-151`). If both `text foo` and `generated text foo` exist in the same file, the compiler emits a warning and deletes the `generated text` declaration, keeping the author-written `text`.

This is the only case where the compiler auto-deletes a declaration. The rationale: the author's explicit `text` supersedes the machine-generated version.

## Author Editability

Authors may interact with `generated text` declarations in three ways:

- **Edit the string body.** Change the content directly. The declaration stays `generated text`. The next repair run sees the name is defined and skips it. The author's edit becomes the new stable meaning.
- **Promote to `text`.** Delete the word `generated`. The declaration becomes a regular `text` declaration. The author may then move it above the generated section or anywhere in the file.
- **Promote to imported library.** Move the content into another `.glyph.md` file as `export text`, import it back, and delete the `generated text` line. The name now resolves via import.

No special compiler behavior is needed for any of these. They all work through existing name resolution and the idempotence rule.

## Not Exportable

`export generated text` is not a valid declaration form. A generated definition is local to the file where repair created it. To share a definition across files, the author must first promote it to `export text`.

## Compile-Time Behavior

`generated text` compiles identically to `text` (`compiled-output.md:162`):

- At the usage site, the bare name is replaced by the string content.
- The `generated text` declaration itself produces nothing in compiled output.
- The `generated` marker is erased. No provenance marker, no `<!-- generated -->` comment.
- The compiled `.md` file is indistinguishable from one whose source used `text` instead of `generated text`.

## Interaction With Other Declarations

### With `declaration-headers.md`

`generated text` is a new top-level declaration form. Its header grammar is the `text` grammar prefixed by `generated`. It follows the same rules: no trailing colon, no parentheses, `=` separates name from value.

The MVP top-level declaration set becomes: `import`, `text`, `export text`, `block`, `export block`, `skill`, and `generated text`.

### With `block-structure.md`

`generated text` declarations at level 0 (column 0) like all top-level declarations. Block strings (`"""..."""`) follow the existing continuation and dedent rules.

### With `values-and-literals.md`

`generated text` appears in the name resolution table (`values-and-literals.md:128-134`) as "a repair-generated definition." The `generated` prefix is what makes this visible to the compiler's warning diagnostic for generated definitions (`values-and-literals.md:152-154`).

### With `compiled-output.md`

Per `compiled-output.md:162`: "Generated definitions resolve and inline. The `generated definition` metadata (`summary:`, the `generated` marker) is stripped. Only the instruction content appears." With `summary:` dropped, only the `generated` marker is stripped. The inlining contract is unchanged.

### With `llm-repair-pass.md`

This document formalizes the syntax sketched in `llm-repair-pass.md:136-169`. The repair pass creates `generated text` declarations; this document defines their exact source shape.

## Deferred

- Whether the compiler should limit the number of `generated text` declarations per file (to prevent repair from over-generating).
- Whether `generated text` should carry a compiler-generated hash for migration detection when language rules change (`llm-repair-pass.md:178`).
- Tooling support: IDE highlighting, gutter markers, or quick-fix actions for promoting `generated text` to `text`.
