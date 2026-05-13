# LSP Architecture

The Glyph LSP is a thin wrapper around `glyph-core`'s deterministic
Phase 1 (Parse) and Phase 2 (Analyze) phases. It republishes existing
diagnostics and name resolution in LSP shape; it introduces no new
compiler behaviour. The compiler remains the single source of truth —
if a check is missing, it gets added to `glyph-core` and the LSP picks
it up automatically.

This document is for LSP maintainers. For user-visible behaviour see
[[glyph-lsp]]. For the framework
choice rationale see
[[0023-tower-lsp-over-lsp-server]];
for why the LSP depends on `glyph-core` rather than `glyph-cli` see
[[0024-lsp-shares-glyph-core]].

## Process Model

The LSP server is a single OS process launched by the editor.
Communication is JSON-RPC framed on stdin/stdout per the LSP spec.

```
                                                     ┌─────────────────┐
                                                     │  source files   │
                                                     │   on disk       │
                                                     │   (*.glyph)     │
                                                     └────────▲────────┘
                                                              │ read for
                                                              │ unsaved
                                                              │ imports
                                                              │
   ┌──────────┐   stdio     ┌──────────────────────┐     ┌────┴─────────┐
   │  editor  │ ◄────────►  │  glyph-lsp server    │ ──► │ glyph-core   │
   │  (nvim)  │  JSON-RPC   │                      │     │ (library)    │
   └──────────┘             │  ┌────────────────┐  │     │              │
                            │  │ DocumentStore  │  │     │  parse       │
                            │  │ (open buffers) │  │     │  analyze     │
                            │  └────────────────┘  │     │  diagnostic  │
                            │  ┌────────────────┐  │     │  ast/span    │
                            │  │ FileGraph      │  │     │              │
                            │  │ (resolved      │  │     └──────────────┘
                            │  │  imports cache)│  │
                            │  └────────────────┘  │
                            │  ┌────────────────┐  │
                            │  │ DiagSnapshot   │  │
                            │  │ (last-published│  │
                            │  │  per-URI bag)  │  │
                            │  └────────────────┘  │
                            └──────────────────────┘
```

**No worker pool.** All compiler work is synchronous on the request
handler. This is fine for kilobyte-scale `.glyph` files; if profiling
later shows latency, a single background worker channel is the
natural extension.

## State Model

The LSP keeps three caches.

### DocumentStore — open buffers

Map `Url → DocumentState`. Updated on every `didOpen` / `didChange` /
`didClose` / `didSave`.

```rust
struct DocumentState {
    text: String,                       // current contents (mutable across didChange)
    version: i32,                       // LSP didOpen/didChange version
    parsed: Option<ParsedView>,         // lazily populated; None when text is dirty
    last_published_diagnostics: Vec<lsp_types::Diagnostic>,
}

struct ParsedView {
    ast: ast::SourceFile,
    line_index: LineIndex,
    resolutions: Vec<Resolution>,       // M2+: populated by analyze_with_resolutions
    diag_bag: DiagBag,
}
```

### FileGraph — resolved imports cache

Optional but recommended. Cache the resolved import edges per file so
cross-file go-to-def avoids re-walking the DAG every request.

Invalidated on any `didSave` of a file that another file imports, or
when `didChange` mutates an `import` line in an open buffer (cheap
regex / parse-on-demand).

### DiagSnapshot — last-published diagnostics

Per-URI list of the diagnostics most recently sent. The server clears
stale URIs by publishing an empty list when a file is closed **and**
no longer transitively imported.

## Lifecycle

- `didOpen { uri, text }` → insert `DocumentState`; `parsed = None`;
  trigger parse + diagnostics publish.
- `didChange { uri, contentChanges, version }` → update `text` (full
  or incremental sync), bump version, set `parsed = None`. **v1 does
  not auto-republish on change.**
- `didSave { uri, text? }` → if `text` is provided, replace; trigger
  parse + diagnostics publish. For cross-file diagnostics, re-lint any
  open buffer that imports this URI.
- `didClose { uri }` → drop `DocumentState`. Publish empty diagnostics
  for the URI **only if** no other open buffer transitively imports it.

**Sync mode.** v1 advertises `TextDocumentSyncKind::Full`. Incremental
sync is optional and adds complexity; a full re-send of a kilobyte-
scale file every keystroke is fine bandwidth-wise.

**What invalidates a parse.** Any change to `text`. Re-parse on next
request or save — never speculatively in the background.

**What invalidates cross-file diagnostics.** A `didSave` of file `A`
invalidates the cached `ParsedView` of every open buffer with an
`import` line resolving to `A`.

**When does the server re-parse?** On the request handler for any LSP
method that needs an AST. Lazy: only when `parsed.is_none()`. Always
full-file — Glyph has no incremental parser.

**Workspace mode.** Workspace-wide indexing is not done eagerly. Files
become known to the server only when the editor opens them or when an
open buffer imports them (read on demand).

## `glyph-core` API Surface

Every existing or new `glyph-core` symbol the LSP touches. Where new
API is required, it is flagged `[NEW]`.

### Diagnostics — required for M1

Existing entry points the LSP reuses unchanged:

| Symbol | File | Notes |
|--------|------|-------|
| `pub fn check_source(source, file_id, file_label) -> DiagBag` | `lib.rs:122` | Phase 1 + Phase 2 on a single in-memory buffer. M1 entry point. |
| `pub fn check_source_with_effects(source, file_id, file_label, enable_effects) -> DiagBag` | `lib.rs:127` | Same, with `--enable-effects` gate. Exposed via initialization option. |
| `pub fn check_file(path) -> DiagBag` | `lib.rs:246` | Import-aware: walks the import DAG, returns diagnostics for the closure. M3. |
| `pub fn check_file_with_effects(path, enable_effects) -> DiagBag` | `lib.rs:250` | With effects gate. |

Diagnostic shape (unchanged):

| Type | File | Notes |
|------|------|-------|
| `pub struct Diagnostic { id, classification, message, span, related, hints }` | `diagnostic.rs:101` | Canonical structured diagnostic. |
| `pub enum Classification { Error, Repairable, Warning }` | `diagnostic.rs:26` | Three tiers from [[compiler-pipeline]] Phase 2. |
| `pub struct SourceSpan { file, start, end }` | `diagnostic.rs:64` | 1-indexed, **inclusive** end. |
| `pub struct LineCol { line: u32, col: u32 }` | `diagnostic.rs:55` | 1-indexed. |
| `pub struct DiagBag` with `iter`, `sorted`, `is_empty`, `has_error`, `has_repairable`, `exit_code` | `diagnostic.rs:135–207` | Accumulator. The LSP calls `sorted()` for deterministic per-URI publishes. |

Required addition:

| Symbol | Why |
|--------|-----|
| `pub fn check_source_with_imports(source, file_label, importer_path: Option<&Path>, enable_effects) -> DiagBag` `[NEW]` | M3 lints an *unsaved* buffer while resolving `import` lines against on-disk dependencies. Current split is binary: `check_source` (no imports) or `check_file` (reads disk). New helper stitches them. |

### Span / line-index utilities

| Symbol | File | Notes |
|--------|------|-------|
| `pub struct Span { file_id, start, end }` | `span.rs:10` | Half-open byte range. |
| `pub struct LineIndex` + `LineIndex::new` + `LineIndex::line_col` | `span.rs:38–67` | Byte-offset → 1-indexed (line, col). |
| `pub struct Spanned<T> { node, span }` | `span.rs:24` | Used on every top-level `ast::Decl`. |

Required addition:

| Symbol | Why |
|--------|-----|
| `LineIndex::byte_offset(line: u32, col: u32) -> u32` `[NEW]` | LSP receives 0-indexed `Position` and needs the inverse mapping. Trivial: `self.line_starts[(line - 1) as usize] + (col - 1)` with bounds checks. |

### AST — required for M2

| Type | File | Notes |
|------|------|-------|
| `pub struct SourceFile { decls: Vec<Decl> }` | `ast.rs:10` | Parser output. |
| `pub enum Decl { Skill, Text, ExportBlock, Block, Import }` | `ast.rs:15` | Every variant wraps `Spanned<…>` — declaration spans available. |
| `pub struct Param { name, default, span }` | `ast.rs:120–129` | Already has a span — sufficient for header parameter jumping. |
| `pub enum FlowStmt` | `ast.rs:148–169` | **Has no per-statement spans.** Central blocker for high-quality go-to-def. |
| `pub struct ConstraintMarker { marker, name }` | `ast.rs:131–137` | `name` has no span. |
| `pub struct ImportDecl { path, kind }` + `ImportName { name, alias }` | `ast.rs:30–53` | Per-name spans inside `Selective(Vec<ImportName>)` are absent. |

Required additions for M2:

| Change | Why |
|--------|-----|
| Wrap `FlowStmt::Call.target`, `FlowStmt::BareName(...)`, `ConstraintMarker.name`, `ContextEntry::NameRef(...)`, and `ImportName.name` with `Spanned<String>`. `[NEW]` | Without these, the LSP cannot know which identifier the cursor lands on. The parser already has the token `Span` at construction time. |
| Add `body_bare_names_with_spans: Vec<Spanned<String>>` on `Skill` and similar. `[NEW]` | `analyze.rs:418` already accepts the synthetic-fallback span loss as a known limitation; this fixes it. |
| Extend `slot::scan_slots` to return per-slot byte offsets within the string. `[NEW]` | Required for go-to-def inside `{name}` interpolation. |

These are additive — existing call sites continue to compile.

### Name resolution — required for M2 / M3

**There is no `(use_span → def_span)` table in `glyph-core` today.**
`analyze::analyze_with_diagnostics` emits diagnostics for unresolved
names but does not record successful resolutions. The compiler
resolves names a second time from scratch in Phase 4 (Lower).

This is fine for the compiler but not for an LSP that wants to answer
go-to-def from the AST alone without re-running Lower.

Required addition:

```rust
// glyph-core/src/analyze.rs (or a new module glyph-core/src/resolve.rs)

#[derive(Clone, Debug)]
pub struct Resolution {
    pub use_span: Span,
    pub def_span: Span,
    pub def_file: PathBuf,
    pub kind: ResolutionKind,
}

pub enum ResolutionKind {
    Skill,
    Block,
    ExportBlock,
    Text,        // includes ExportText
    Param,       // header parameter of the enclosing skill/block/export-block
    Import,      // the import alias / selective name itself
    Stdlib,      // @glyph/std member
}

pub fn analyze_with_resolutions(
    file: SourceFile,
    file_id: u32,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    enable_effects: bool,
) -> (SourceFile, Vec<Resolution>) { ... }
```

Implementation: every place existing analyze code checks a name
against `text_names` / `block_names` / `imported_texts` /
`imported_blocks` and is happy with the result is a place where we
push a `Resolution` onto the output vector.

For M3 (cross-file), the existing `check_file_recursive` (`lib.rs:275`)
already walks the DAG and knows which imported name came from which
file. The same recursion writes a per-file resolution list and the LSP
joins them by file URI.

### Other touched symbols

| Symbol | File | Use |
|--------|------|-----|
| `lib::resolve_import_path(importer, import_path)` private | `lib.rs:174` | Needs to be made `pub` so the LSP can answer "go to definition of an import path string". |
| `parse::parse_with_diagnostics_opts(...)` | `parse.rs:77` | Driven indirectly via `check_source_*`. |
| `slot::scan_slots(text: &str)` | `slot.rs` | After the extension above, used to map a cursor inside a flow string back to a `{name}` slot. |

### Summary of additions to `glyph-core`

| Item | Effort | Milestone |
|------|--------|-----------|
| `LineIndex::byte_offset` inverse lookup | trivial (5 LOC) | M1 |
| `pub fn check_source_with_imports` | refactor (~40 LOC) | M3 |
| `pub fn resolve_import_path` (made public) | trivial | M3 |
| Spans on `FlowStmt::Call.target` / `BareName` / `ConstraintMarker.name` / `ContextEntry::NameRef` / `ImportName.name` | mechanical (~80 LOC across `parse.rs` + `ast.rs`) | M2 |
| `slot::scan_slots` per-slot byte offsets | small (~20 LOC) | M2 |
| `analyze::analyze_with_resolutions` + `Resolution` types | refactor (~150 LOC) | M2 |
| Cross-file resolution list extension to `check_file_recursive` | refactor (~60 LOC) | M3 |

None of these change the compiler's observable behaviour — they
expose information that already exists at compile time, or restore
source provenance the AST currently drops.

## Diagnostic Mapping

The compiler emits `glyph_core::diagnostic::Diagnostic`; the LSP
translates to `lsp_types::Diagnostic`. Mapping is mechanical:

| `glyph-core` field | LSP `Diagnostic` field | Conversion |
|--------------------|------------------------|------------|
| `id: String` | `code: NumberOrString::String(id.clone())` | Verbatim. IDs are stable across compiler versions per [[docs/reference/diagnostics]]. |
| `classification: Classification` | `severity: DiagnosticSeverity` | `Error → Error`, `Repairable → Warning`, `Warning → Information`. Repairable is "the agent will likely fix this" — `Warning` matches that mental model. |
| `message: String` | `message: String` | Verbatim. |
| `span.start: LineCol { line, col }` (1-indexed) | `range.start: Position { line, character }` (0-indexed) | `Position { line: span.start.line - 1, character: span.start.col - 1 }`. |
| `span.end: LineCol { line, col }` (1-indexed, **inclusive**) | `range.end: Position` | `Position { line: span.end.line - 1, character: span.end.col }`. Asymmetry: glyph end is inclusive (`end.col` is the column *of the last character*), LSP end is exclusive — add 1, don't subtract 1. |
| `related: Vec<SourceSpan>` | `related_information` | Each becomes `DiagnosticRelatedInformation { location, message: "" }`. Empty vector → `None`. |
| `hints: Vec<String>` | (none directly) | LSP has no first-class "hints" field. v1 appends each hint to `message` as a new line. Post-MVP, surface via code actions. |
| (constant) | `source` | Always `Some("glyph".into())`. |
| (constant) | `tags` | `None` in v1. |

**Sort order.** The LSP publishes the bag's `sorted()` output, which
orders by `(file, byte_start, id)` — byte-stable for a given source.

**File attribution.** `glyph-core` diagnostics carry a `file` string
that the LSP sets as the URI's path component. M3 may produce
diagnostics whose `file` is a different URI than the buffer just
analyzed (a bug surfaced in an imported file). The server publishes
those under the imported file's URI.

**Empty-bag publish.** When a previously-published URI now has zero
diagnostics, the server publishes an empty array to clear stale
markers. Compare `last_published_diagnostics` to the new bag; publish
whenever they differ.

## Go-to-Definition Algorithm

Triggered by `textDocument/definition { textDocument: { uri },
position: { line, character } }`. Returns `null`, `Location`, or
`Vec<Location>`.

**Algorithm (same-file):**

1. **Resolve buffer.** Look up `DocumentState` for `uri`. If missing,
   return `null`.
2. **Ensure parse.** If `parsed.is_none()`, run `check_source_with_imports`
   (or `check_source` for M2) to populate `ast`, `line_index`,
   `resolutions`, `diag_bag`.
3. **Map cursor to byte offset.** `let off = line_index.byte_offset(
   position.line + 1, position.character + 1)`. Out-of-range → `null`.
4. **Find the identifier under the cursor.** Walk `parsed.resolutions`
   for the smallest `use_span` containing `off`. Resolutions are
   span-disjoint by construction. If no resolution covers the cursor,
   fall back to looking inside flow inline strings: scan slot list of
   the enclosing string; a `{name}` slot enclosing `off` resolves
   against the enclosing skill/block/export-block parameter list.
5. **Return the location.** Same file: `Location { uri, range: ... }`.
   Different file (M3): `Location { uri: Url::from_file_path(
   resolution.def_file), range: ... }`.

**Cross-file resolution.** `Resolution { def_file, def_span }` carries
the path. The LSP converts to a `Url` and replies. The def file need
not be open — `Url::from_file_path` works on any extant path.

**Stdlib targets (`@glyph/std`).** When `resolution.kind == Stdlib`,
there is no `.glyph` source to jump to. v1 returns `null`. Post-MVP
could open a synthetic markdown buffer.

**Inside `{name}` slots.** The cursor is at byte offset `off` inside
an inline flow string `s` with source-relative byte span
`[s_start, s_end)`. For each slot returned by `scan_slots(s.text)`,
compute its absolute span: `[s_start + slot.brace_open_offset,
s_start + slot.brace_close_offset + 1)`. If `off` falls inside a slot,
look up `slot.name` against the enclosing declaration's `Param` list.
Match → return the param's span; no match → `null`.

**Unresolvable identifiers.** Return `null`. The user already sees the
diagnostic; go-to-def cannot manufacture a definition.

**Complexity.** Per request: one parse (only if dirty), one linear
walk of `resolutions` (or binary search if sorted). Sub-millisecond
for typical files.

## Editor Integration

### Neovim (verified target)

```lua
local lspconfig = require("lspconfig")
local configs = require("lspconfig.configs")

if not configs.glyph then
  configs.glyph = {
    default_config = {
      cmd = { "glyph", "lsp" },
      filetypes = { "glyph" },
      root_dir = lspconfig.util.root_pattern(
        ".git",
        "Cargo.toml",
        "*.glyph"
      ),
      single_file_support = true,
      init_options = {
        enableEffects = false,
      },
      settings = {},
    },
  }
end

lspconfig.glyph.setup({})

vim.filetype.add({
  extension = {
    ["glyph"] = "glyph",
  },
})
```

Notes:

- `cmd = { "glyph", "lsp" }` matches the `glyph lsp` subcommand
  decision.
- `filetypes = { "glyph" }` must agree with the tree-sitter
  highlighting branch's filetype.
- `init_options.enableEffects` mirrors `--enable-effects`. Default
  `false` per [[docs/reference/cli]].
- `single_file_support = true` covers lone `.glyph` files outside
  project roots.

### VS Code (untested)

```jsonc
// package.json (extension)
{
  "name": "vscode-glyph",
  "displayName": "Glyph",
  "engines": { "vscode": "^1.85.0" },
  "main": "./out/extension.js",
  "contributes": {
    "languages": [{
      "id": "glyph",
      "extensions": [".glyph"],
      "aliases": ["Glyph"]
    }]
  }
}
```

```typescript
// extension.ts
import * as vscode from "vscode";
import { LanguageClient, ServerOptions, TransportKind, LanguageClientOptions } from "vscode-languageclient/node";

export function activate(ctx: vscode.ExtensionContext) {
  const serverOptions: ServerOptions = {
    command: "glyph",
    args: ["lsp"],
    transport: TransportKind.stdio,
  };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "glyph" }],
    initializationOptions: { enableEffects: false },
  };
  const client = new LanguageClient("glyph", "Glyph", serverOptions, clientOptions);
  ctx.subscriptions.push(client.start());
}
```

Flagged untested. Documented so a contributor can verify.

## References

- LSP crate: `crates/glyph-lsp/`
- User-visible behaviour: [[glyph-lsp]]
- Outstanding work: [[lsp-todos]]
- Diagnostic contract: [[docs/reference/diagnostics]]
