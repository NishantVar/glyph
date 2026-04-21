# Glyph Effect Vocabulary

This document defines the MVP effect vocabulary, syntax, semantics, and extension policy for Glyph. Effects describe what a skill, block, or exported block does beyond computing values.

## Goals

Effects in Glyph should be:

- explicit enough for analysis, visualization, and import contracts;
- coarse enough for the MVP without premature granularity;
- extensible without breaking existing import contracts;
- inferable by the compiler from the call graph.

## MVP Effect Keywords

The MVP defines eight effect keywords using `verb_noun` snake_case:

- `none` — no meaningful effects. Pure or near-pure computation.
- `reads_files` — inspects files, repository contents, source code, logs, or other local file-system artifacts.
- `reads_env` — reads environment variables, system state, git metadata, or project configuration that is not file content.
- `writes_files` — creates or modifies files such as source code, configuration, or data files.
- `runs_commands` — invokes shell commands, test runners, formatters, linters, package managers, or similar tools.
- `uses_network` — accesses web resources, downloads packages, calls remote APIs, or contacts external services.
- `asks_user` — pauses execution to request human input, approval, or clarification.
- `creates_artifacts` — produces durable outputs such as reports, generated assets, compiled Markdown files, archives, or exported data. Distinct from `writes_files`: artifact creation is the skill's purpose, not a side-effect file edit.

This list is intentionally small. See Extension Policy below for how new effects are added.

## Syntax

The `effects:` clause may appear on `skill`, `block`, and `export block` declarations.

Two syntactic forms are allowed; the compiler normalizes both to the same IR representation.

Inline comma-separated (preferred for short lists):

```glyph
export block inspect_failure(scope) -> FailureReport
    effects: reads_files, runs_commands

    flow:
        reproduce(scope)
        collect_logs(scope)
        return failure_report()
```

Indented bullet list (preferred for longer lists):

```glyph
skill implement_feature(scope, risk = "medium")
    effects:
        - reads_files
        - reads_env
        - writes_files
        - runs_commands
        - creates_artifacts

    flow:
        ctx = inspect_repo(scope)
        plan = make_plan(ctx, risk)
        apply_changes(plan)
        validate(plan)
        return summarize(plan)
```

## `none` Semantics

`none` is a keyword representing the empty effect set:

- Omitting `effects:` entirely is equivalent to `effects: none`.
- Writing `effects: none` explicitly is allowed for documentation and clarity.
- `none` must not appear alongside other effect keywords. `effects: none, reads_files` is a compile error.

## Effect Inference And Propagation

The compiler infers effects by walking the call graph:

- Each primitive call or block call contributes its declared or inferred effects.
- A block's inferred effect set is the union of its own direct effects and the effects of every block it calls.
- Skills, exported blocks, and private blocks all participate in inference.

Authors may optionally write `effects:` for readability and documentation. When an author declares effects explicitly, the compiler validates that the declared set is a superset of the inferred set. If the declared set is smaller than the inferred set, that is a compile error (the declaration is lying about what the block does).

Import contracts are satisfied through the compiler's output: the IR and compiled Markdown always contain the full inferred effect set, regardless of whether the author wrote `effects:` in source.

## Effect Set Semantics

Effects combine as set union:

- If block A has `reads_files` and calls block B which has `runs_commands`, then A's inferred effect set is `{reads_files, runs_commands}`.
- There is no effect subtraction or masking in the MVP.
- Effect sets are unordered; the compiler may sort them alphabetically or by declaration order in output.

## Compiled Output

Effects surface in the compiled Markdown as a dedicated `## Effects` section. Each effect appears as a bullet with a human-readable expansion:

```md
## Effects
- Reads files (repository structure, source code, logs)
- Runs commands (git, test runners)
```

The exact placement and formatting of this section is owned by the compiled-output design. The effect vocabulary design requires only that the section exists and that every inferred effect is represented.

## Extension Policy

New effects are introduced through additive-only changes to the flat global keyword list:

- New effect keywords may be added in future versions (e.g., `reads_database`, `sends_messages`).
- Existing effect keywords are never renamed or removed once stabilized.
- Old skills that do not use a new effect are unaffected; their import contracts remain valid.
- No namespacing is required in MVP. If the flat namespace becomes crowded in the future, a namespacing scheme may be introduced as a backwards-compatible addition.
- New effects should follow the established `verb_noun` snake_case naming convention.

## Interaction With Other Design Areas

- **Literals (teammate scope):** Effect keywords are identifiers, not string literals. Their exact lexical rules depend on the identifier specification.
- **Compiled output (teammate scope):** The `## Effects` section placement and formatting within the compiled Markdown is owned by the output-shape design.
- **Import contracts (principle 19):** Exported blocks carry their full inferred effect set in the IR. Callers importing an exported block inherit its effects through union propagation.
- **Visualization (principle 17):** Effects are annotations on call nodes in the data-flow graph. The effect set for each node should be available for rendering.

## Open Questions

- Whether the compiler should warn when an author declares effects that are broader than what inference finds (over-declaration). This is not an error but may indicate stale annotations.
- Whether effect annotations on individual calls within a flow (as opposed to block-level declarations) are useful for the MVP or should be deferred.
- How standard-library primitives declare their effect signatures.
