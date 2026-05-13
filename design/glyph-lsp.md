# Glyph LSP — Editor Behaviour

This document describes what a Glyph author sees from the Language
Server: when diagnostics appear, what go-to-definition does, and what
is intentionally not yet supported.

The LSP is a thin wrapper over the compiler — every diagnostic and
every resolved name comes from `glyph-core`. If the compiler doesn't
know something, the LSP doesn't either. For implementation details
see [[lsp]]; for
work in progress see [[lsp-todos]].

## Supported Capabilities (v1)

Two capabilities ship in v1 for `.glyph` files:

1. **Diagnostics on open and on save.** The same diagnostics
   `glyph check` produces appear inline in the editor, with the
   correct line and column. Saving a file with a fix clears the
   diagnostic.
2. **Go-to-definition** for any identifier that resolves to a
   top-level declaration (`skill`, `block`, `export block`, `const`,
   an `import`-introduced name, or a header parameter). Cross-file
   resolution follows `import` paths. Jumping into a parameter slot
   (`{name}` inside a flow string) lands on the parameter declaration
   in the enclosing header.

**Verified editor.** Neovim (via `nvim-lspconfig`). VS Code is
documented but not formally verified in v1.

## Diagnostic Behaviour

- **When they appear.** On `didOpen` and on `didSave`. v1 does not
  republish on every keystroke — saving is the trigger. Live
  rechecking on edit is a deferred follow-up.
- **Severity mapping.** Hard errors render as Error; repairable
  diagnostics render as Warning (an automated agent will likely fix
  these); style warnings render as Information.
- **Hints.** Each compiler hint is appended to the diagnostic message
  as a new line. v1 does not yet expose hints as code actions.
- **Cross-file.** Editing a file that another open buffer imports
  invalidates the importer's diagnostics; they refresh on the next
  request that needs them. A buffer can show diagnostics whose source
  is in a different file (an imported file with a bug); the diagnostic
  is published under the imported file's URI.
- **Clearing.** Closing a file clears its diagnostics unless another
  open buffer transitively imports it.

## Go-to-Definition Behaviour

- **Local names.** Cursor on a call target, a constraint or context
  marker name, a binding LHS, or a parameter usage jumps to the
  defining declaration in the same file.
- **Imported names.** Cursor on an imported name jumps to the
  declaration in the imported file (opening it in the editor if it
  isn't already).
- **Parameter slots.** Cursor inside a `{param}` slot in a flow inline
  string jumps to the parameter declaration in the enclosing `skill` /
  `block` / `export block` header.
- **Stdlib references.** Cursor on a stdlib symbol (e.g., `subagent`,
  `send`) returns no definition in v1 — the editor reports "no
  definition." This is deliberate: stdlib primitives have no `.glyph`
  source.
- **Unresolvable names.** Cursor on a name the compiler cannot resolve
  also returns no definition. The diagnostic on the same span tells
  the user why.

## Deferred Capabilities

The following are intentionally not in v1. Each composes cleanly onto
the v1 architecture; none changes the language contract.

- Hover
- Completion
- Document symbols (outline view)
- Formatting
- Semantic tokens
- Code actions (including surfacing repair hints)
- Rename
- References
- Signature help
- Workspace-symbol search
- Inlay hints
- Workspace-wide indexing (the server only knows about open buffers
  and their on-demand imports)
- Live diagnostics on every keystroke (currently save-triggered)

## Configuration

- **Filetype.** `.glyph` → `glyph` filetype. The same filetype name is
  used by the tree-sitter highlighting integration; both must agree.
- **Effects gate.** `initializationOptions.enableEffects = true` opts
  in to effect-related diagnostics. Default `false`, matching the CLI.
- **Single-file mode.** A `.glyph` file opened outside any project
  root still gets diagnostics. Cross-file features degrade gracefully
  when there is no on-disk import context.

## Relationship to the Compiler

The compiler is the source of truth. The LSP never re-implements
diagnostics or name resolution: if a check is missing, it is added to
`glyph-core` and the LSP picks it up automatically. This means LSP
behaviour evolves with the compiler — the same source that compiles
cleanly under `glyph check` produces no LSP diagnostics, and the same
identifier that the compiler resolves is the identifier the LSP can
jump to.
