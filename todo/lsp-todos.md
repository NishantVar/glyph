# LSP — Outstanding Work

Work tracking extracted from the original [[glyph-lsp]] when
that file was split into user-visible behaviour ([[glyph-lsp]])
and implementation architecture ([[lsp]]). This file
holds the milestone plan and open questions.

## Milestone Plan

Three milestones, each landing as its own PR. Each is independently
shippable: M1 is a useful diagnostics-only LSP; M2 adds same-file
go-to-def; M3 extends to cross-file.

### M1 — Diagnostics-only stdio server

Scope:

- New crate `crates/glyph-lsp` in the workspace (depends on
  `glyph-core`, `tower-lsp`, `tokio`, `lsp-types`, `serde`,
  `serde_json`, `url`).
- `glyph lsp` subcommand on the existing `glyph-cli` binary that calls
  into `glyph-lsp::run(stdin, stdout)`. Single binary, single
  distribution. (A separate `glyph-lsp` binary was rejected:
  distribution complexity; most editors call `glyph lsp` once and
  forget it.)
- `LineIndex::byte_offset` reverse mapping in `glyph-core`.
- `tower-lsp`-based `Backend` impl exposing `initialize`,
  `initialized`, `shutdown`, `did_open`, `did_change`, `did_save`,
  `did_close`.
- `DocumentStore` with `DocumentState` per URI; `Full` text sync.
- Diagnostics computed via `glyph_core::check_source_with_effects(
  text, 0, &uri.path(), enable_effects)` and translated per the
  diagnostic mapping in [[lsp]].
- `--enable-effects` plumbed via
  `initializationOptions: { "enableEffects": bool }`. Default `false`.

Deliverables: `crates/glyph-lsp/`, `glyph lsp` subcommand,
integration test driving the server via stdio with a hand-crafted
JSON-RPC stream.

Exit criteria:

1. `nvim-lspconfig` snippet attaches and shows the welcome message in
   `:LspInfo`.
2. Opening a `.glyph` file with a known parse error (e.g., `flow:`
   indented with tabs) shows the corresponding `G::parse::tab-indent`
   diagnostic at the correct line and column.
3. Saving a file with a fixed source clears the diagnostic.
4. Opening a clean file produces zero diagnostics in `:LspInfo`.
5. Server shuts down cleanly on `exit` notification (no orphan stdio
   processes).

### M2 — Single-file go-to-definition

Scope:

- `glyph-core` AST extensions: spans on `FlowStmt::Call.target`,
  `FlowStmt::BareName`, `ConstraintMarker.name`,
  `ContextEntry::NameRef`, `ImportName.name`. Update the parser to
  populate them.
- `glyph-core` `slot::scan_slots` extension for per-slot byte offsets.
- `glyph-core` `analyze::analyze_with_resolutions` returning
  `Vec<Resolution>` for same-file targets.
- LSP `text_document_definition` handler implementing the go-to-def
  algorithm minus the cross-file branch.

Deliverables: updated parser/AST, new `Resolution` types, new analyze
entry point, LSP handler, integration tests.

Exit criteria:

1. Cursor on a `block` call target (e.g., `validate_plan()` inside
   `flow:`) jumps to the `block validate_plan` declaration in the
   same file.
2. Cursor on a `text` name in `require <name>` jumps to its `text
   <name>` declaration.
3. Cursor inside a `{param}` slot in a flow inline string jumps to
   the parameter declaration in the enclosing header.
4. Cursor on an unresolvable identifier returns `null`.
5. All M1 exit criteria still pass.

### M3 — Cross-file go-to-definition + import-aware diagnostics

Scope:

- `glyph-core` `check_source_with_imports`.
- `glyph-core` `resolve_import_path` made public.
- `analyze_with_resolutions` extended to record cross-file
  resolutions during the existing import walk.
- LSP cross-file diagnostic publishing: when a buffer is checked,
  every URI present in the resulting `DiagBag` gets its own
  `publishDiagnostics` notification.
- LSP `definition` handler honours `Resolution.def_file` for non-
  local targets; converts to `Url::from_file_path`.
- LSP minimal `FileGraph` cache so opening file `B` after editing file
  `A` (which imports `B`) does not re-walk the entire project tree.

Deliverables: import-aware single-buffer linting, cross-file def
jumps, file-graph cache.

Exit criteria:

1. A buffer with `import "./prefs.glyph" { project_conventions }` and
   a use of `project_conventions` jumps to the `text
   project_conventions` declaration in `prefs.glyph`, opening that
   file in the editor.
2. Editing the importer to refer to a non-existent name produces a
   `G::analyze::import-private` (or related) diagnostic, with span on
   the `import` line.
3. Editing `prefs.glyph` and saving clears or updates the importer's
   diagnostics on the next request that needs them.
4. Stdlib references (`subagent`, `send`) do not jump (return `null`)
   but also do not error.
5. All M1 / M2 exit criteria still pass.

## Open Questions and Risks

### A. AST does not currently carry per-reference spans (High)

The original brief assumed `glyph-core` already exposed a
name-resolution table. The code does not — `Analyze` emits
diagnostics on resolution failure but does not record successful
resolutions, and AST nodes for uses (call targets, constraint names,
context name refs, import names, body bare names) carry only cooked
strings, not source spans. The parser has the spans at token time and
drops them.

The architecture doc lists the additive changes to fix this. They are
mechanical, not conceptual — but this means M2 is a roughly
200–300 LOC `glyph-core` change, not a pure LSP-side feature.

Team-lead decisions needed:

1. Is that `glyph-core` change OK to merge into `feature/glyph-lsp` or
   should it land on `main` first?
2. Is `analyze::analyze_with_resolutions` the right shape, or should
   we have a separate `resolve` module?
3. Should we use `Spanned<String>` or `(String, Span)` tuples in the
   AST? Existing `Param` uses a `pub span: Span` field next to `pub
   name`; consistency says do the same elsewhere.

### B. Inclusive-vs-exclusive end conversion is easy to get wrong (Medium)

`glyph-core`'s `SourceSpan.end` is 1-indexed inclusive. LSP's
`Range.end` is 0-indexed exclusive. The mapping is `lsp_end_char =
glyph_end_col` — no off-by-one because the +1 from converting to
0-indexed and the −1 from inclusive→exclusive cancel. M1 must include
a unit test against a single-character span to lock this in.

### C. `didChange` debouncing is deferred — publish-on-save acceptable? (Medium)

v1 republishes diagnostics on `didOpen` and `didSave` only. Mid-edit
the user doesn't see diagnostics until save. Pros: simpler; matches
`cargo check` workflow; matches the one-error-at-a-time philosophy of
Phase 1 (chatty under live rechecking). Cons: modern editors expect
live errors; Glyph compilation is fast enough that a 200 ms debounce
on `didChange` would feel instantaneous.

Recommendation: ship M1 save-only; reconsider after dogfooding.
Adding a 200 ms `tokio::time::sleep` debounce on `didChange` is a
~30 LOC follow-up.

### D. Stdlib targets have no source location (Low)

Go-to-def returns `null` for `@glyph/std::subagent` etc. Post-MVP,
could expose a synthetic read-only markdown buffer ("Stdlib:
`subagent` — see stdlib.md §…"). Worth noting because users will try
`gd` on `subagent` and be momentarily confused.

### E. Workspace-wide diagnostics are deferred (Low — explicit non-goal)

The server only knows about open buffers and their resolved-on-demand
imports. A user who never opens `lib_x.glyph` won't see diagnostics
from it even though it's in the project. v1 is "what your editor
shows you." A workspace symbol index / project diagnostics is a clean
post-MVP follow-up — `compile_directory` already builds the DAG and
can stream per-file diagnostics.

### F. `tokio` dependency adds build-time weight (Low)

`tower-lsp` brings `tokio` (multi-threaded runtime by default; we
configure single-threaded `current_thread` flavor). Adds ~5 s to a
clean build. Acceptable for a tool installed once; flag if CI is
sensitive.

### G. Filetype name agreement with the tree-sitter branch (Low — coordination)

Both LSP and tree-sitter integrations must use `filetype = "glyph"`.
Confirm before both branches merge.
