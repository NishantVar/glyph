# MVP Acceptance Contract

This document is the user-facing contract that the Glyph compiler MVP must
satisfy. It does not describe how the compiler is built; it describes what
a Glyph skill author can rely on at MVP.

## Compilation completes for valid source

A Glyph source file that uses only MVP-supported features must compile
end to end with exit code `0` and produce a `.md` file whose shape is
fixed by the compiled-output design document. The walking-skeleton
example in [[mvp-walking-skeleton]] shows the minimum
end-to-end behavior.

The author may rely on the following features compiling cleanly at MVP:

- Skill declarations with parameters and defaults.
- `block` and `export block` declarations, with multi-statement `flow:`
  bodies, return values, and constraint markers.
- `const` and `export const` declarations.
- `require` / `avoid` constraint markers (4-form model with `soft`/`hard`).
- `effects:` declarations on skills and blocks.
- `if` / `elif` / `else` branching in `flow:` bodies, including
  string-const predicates, inline-literal predicates, and mixed
  predicate-plus-boolean conditions.
- Selective imports (`import "./path" { name }`) across files in the
  same project.
- Library files (no skill, only `export const` or `export block`).
- `with` modifiers on call sites (stored in IR; applied to prose when
  Expand Step 2 runs).

## Output is byte-identical across runs

For identical input, every Glyph compiler output is byte-stable:

- The compiled `.md` file.
- The `--emit-ir` JSON output (`foo.ir.json`).
- The diagnostic JSON stream (`--format json` on stdout).

The byte-stability guarantee covers the deterministic emitter's
scaffolded portions (section headers, list numbering, the locked
four-form constraint template, return-fold suffixes, pure-`applies()`
branch projection, external-file call-step template, `## Parameters`
skeleton). Span-filled prose content is byte-stable only while the
deterministic stub filler is in use; once the LLM filler is wired in,
span content is no longer byte-stable, but scaffolded portions remain
so.

The JSON byte-stability invariant — `BTreeMap` over `HashMap` for any
map-shaped JSON, plus diagnostic arrays sorted by
`(file, span.start.byte, id)` — is captured in
[[0006-json-output-determinism]].

## Multi-file builds compile in topological order

A directory build (`glyph compile dir/`) compiles every `.glyph` file in
topological order over the import DAG. Cross-file name resolution works
for selective imports. Libraries with no skill declaration compile with
exit code `0` and emit no `.md`.

Authors may rely on:

- Libraries compiling before their consumers.
- Names imported via `import "./path" { name }` resolving to the
  exported declaration in the target file.
- Standalone (no-import) skills compiling alongside import-heavy
  siblings in the same build.

## `--strict` mode

`--strict` promotes `repairable` diagnostics to hard errors: any
`repairable` diagnostic in any file causes exit code `1` instead of `2`.
Source that compiles cleanly without `--strict` continues to compile
cleanly with `--strict`. The promotion only changes the exit code on
already-`repairable` source.

## Diagnostic IDs

Every diagnostic ID the compiler emits at MVP is listed in the
diagnostics reference doc. Each ID has at least one triggering test in
the test corpus. The IDs and their classifications (`error`,
`repairable`, `warning`) are part of the public diagnostic contract.

## Exit codes

The MVP exit-code contract is defined in the CLI reference doc:

- `0` — success.
- `1` — hard errors. Errors win over repairables, so the presence of
  both in a build still exits `1`.
- `2` — repairable diagnostics only. The pipeline stopped after Phase 2.
- `3` — invocation error (bad flags, missing path, permission denied,
  IO failure).

Per-file behavior in a multi-file build follows the partial-failure
policy in the compiler-pipeline architecture doc.
