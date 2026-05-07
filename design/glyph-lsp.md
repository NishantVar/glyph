# Glyph LSP — v1 Design

This document specifies the v1 Language Server Protocol implementation for the Glyph language. It is the second of two editor-tooling tracks (the first, a tree-sitter grammar for highlighting, ships on a different branch). Implementation follows after user review.

The LSP is a thin wrapper around `glyph-core`'s existing deterministic phases — the design's job is to (a) decide where the wrapper goes, (b) enumerate exactly which `glyph-core` symbols it touches, and (c) call out the one place where `glyph-core` does not yet expose enough information (per-reference spans for go-to-definition).

Cross-references throughout to `design/diagnostics.md`, `design/pipeline.md`, `design/ir-and-semantics.md`, and `design/cli.md`. The LSP introduces no new compiler behaviour — it republishes existing diagnostics and existing name resolution, in LSP shape.

---

## 1. Goal and Scope

**v1 ships two LSP capabilities** for `.glyph` files:

1. `textDocument/publishDiagnostics` — push the diagnostics produced by `glyph-core`'s Phase 1 (Parse) + Phase 2 (Analyze) phases (the same engine `glyph check` runs). Triggered on `didOpen` and `didSave`. `didChange` debouncing is deferred (see §5).
2. `textDocument/definition` — go-to-definition for any identifier that resolves to a top-level declaration (`skill`, `block`, `export block`, `text`, `import`-introduced name, or a header parameter). Cross-file resolution follows `import` paths.

**Editor verification target:** Neovim (via `nvim-lspconfig`). VS Code config is documented but not tested in v1.

**Deferred (mention only):** hover, completion, document symbols, formatting, semantic tokens, code actions, rename, references, signature help, workspace-symbol search, inlay hints. Each of these would compose cleanly on top of the document-state cache described in §5, but none are in v1 scope.

The compiler is and remains the single source of truth for diagnostics and resolution. The LSP must never re-implement either; if a check is missing, it gets added to `glyph-core` and the LSP picks it up automatically.

---

## 2. Architecture Overview

```
                                                     ┌─────────────────┐
                                                     │  source files   │
                                                     │   on disk       │
                                                     │  (*.glyph)   │
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

**Process model.** The LSP server is a single OS process launched by the editor. Communication is JSON-RPC framed on stdin/stdout per the LSP spec.

**Document state cache (`DocumentStore`).** Map `Url → DocumentState`. `DocumentState` holds the current source text, its `LineIndex`, and (lazily) the parsed `ast::SourceFile`. Updated on every `didOpen` / `didChange` / `didClose` / `didSave`.

**Import graph cache (`FileGraph`).** Optional but recommended: cache the resolved import edges per file so cross-file go-to-def can avoid re-walking the DAG every request. Invalidated on any `didSave` of a file that another file imports, or when `didChange` mutates an `import` line in an open buffer (cheap regex / parse-on-demand).

**Last-published-diagnostics cache (`DiagSnapshot`).** Per-URI list of the diagnostics most recently sent. The server clears stale URIs by publishing an empty list when a file is closed *and* no longer transitively imported.

**No worker pool in v1.** All compiler work is synchronous on the request handler. This is fine for kilobyte-scale `.glyph` files; if profiling later shows latency, a single background worker channel is the natural extension. See §10 risk #4.

---

## 3. Framework Choice: `tower-lsp` vs `lsp-server`

**Pick: `tower-lsp` (v0.20.x).** Justification follows.

| Axis                           | `tower-lsp`                                                 | `lsp-server` (rust-analyzer's)                              |
| ------------------------------ | ----------------------------------------------------------- | ----------------------------------------------------------- |
| Layer                          | High-level: trait `LanguageServer` per-method               | Low-level: framed JSON-RPC + you own the dispatcher         |
| Async story                    | `tokio` + `async fn` per request                            | Sync; you build your own threadpool / event loop            |
| Boilerplate to first message   | ~50 LOC                                                     | ~300 LOC                                                    |
| Cancellation                   | Built-in via `tokio` task drop                              | Manual; rust-analyzer wires it through its salsa DB         |
| Incremental / cancellable work | Naïve (re-run from scratch per request)                     | Idiomatic if you adopt salsa/query-graph                    |
| Maintenance / community        | Active, used by many small/medium servers                   | Active, but more tightly tailored to rust-analyzer's needs  |
| Ergonomics for our scope       | Excellent — every request is `parse → analyze → respond`    | Overkill                                                    |

**Why `tower-lsp` wins for Glyph v1:**

1. **Our scope is "rerun the compiler per request."** Glyph source files are small (kilobytes, not megabytes). A full Parse + Analyze on save is sub-millisecond in release mode for any realistic skill file. We do not need an incremental query graph, salsa, or query cancellation. `tower-lsp`'s "request handler runs `glyph_core::check_source(...)` and replies" model is exactly the shape we want.
2. **Async fits the shape of LSP work.** Even without compute parallelism, async makes it easy to (a) await a single read of an unsaved-but-imported file from the `DocumentStore`, (b) push debounced diagnostics with `tokio::time::sleep` if we ever add it, and (c) compose with future capabilities (hover, completion) without a reactor rewrite.
3. **rust-analyzer chose `lsp-server` because they need salsa-style incremental computation across a 10M-LOC project.** That requirement does not exist for Glyph and likely never will — Glyph skill files are intentionally small.
4. **Trait-driven dispatch is honest documentation.** A `tower-lsp::LanguageServer` impl reads as a list of LSP methods we support — exactly the surface this design enumerates.

The single mild downside is the `tokio` runtime dependency. That is acceptable: the workspace already pulls `serde`/`serde_json`/`clap`, adding `tokio` and `tower-lsp` is a one-line bump.

**Reversibility.** If a future need (e.g., truly incremental cross-file reanalysis) outgrows `tower-lsp`, the swap to `lsp-server` is mostly mechanical because the `DocumentStore` and `glyph-core` interactions live below the framework layer.

---

## 4. `glyph-core` API Surface Enumeration

This section enumerates every existing or new `glyph-core` symbol the LSP touches, with file and line references into the worktree at `crates/glyph-core/src/`. **Read every reference here as concrete; nothing is invented.** Where new API is required, it is flagged `[NEW]` and justified.

### 4.1 Diagnostics — required for M1

Existing entry points the LSP reuses unchanged:

| Symbol                                                                  | File                          | Notes                                                                                                                                                       |
| ----------------------------------------------------------------------- | ----------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pub fn check_source(source: &str, file_id: u32, file_label: &str) -> DiagBag` | `lib.rs:122`                  | Runs Phase 1 + Phase 2 on a single in-memory buffer. **This is the M1 entry point** — what the LSP calls on every `didOpen` / `didSave` / (later) `didChange` for a buffer with no resolvable on-disk path or whose imports we ignore in M1. |
| `pub fn check_source_with_effects(source, file_id, file_label, enable_effects: bool) -> DiagBag` | `lib.rs:127`     | Same, with the `--enable-effects` gate. The LSP exposes this gate via an initialization option (see §9).                                                   |
| `pub fn check_file(path: &Path) -> DiagBag`                             | `lib.rs:246`                  | Import-aware: walks the import DAG starting at `path`, returning a bag of diagnostics for the whole closure. Used in M3 (cross-file diagnostics).         |
| `pub fn check_file_with_effects(path: &Path, enable_effects: bool) -> DiagBag` | `lib.rs:250`             | With effects gate.                                                                                                                                          |

Diagnostic shape (also unchanged):

| Type                                          | File              | Notes                                                                                                                                                   |
| --------------------------------------------- | ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pub struct Diagnostic { id, classification, message, span, related, hints }` | `diagnostic.rs:101` | The canonical structured diagnostic — see §6 for the LSP mapping.                                                                                       |
| `pub enum Classification { Error, Repairable, Warning }` | `diagnostic.rs:26` | Three tiers from `pipeline.md` Phase 2.                                                                                                                |
| `pub struct SourceSpan { file, start, end }`  | `diagnostic.rs:64` | 1-indexed, **inclusive** end (`diagnostics.md` §Span Semantics). `start.line` / `start.col` / `end.line` / `end.col` are all `u32`.                    |
| `pub struct LineCol { line: u32, col: u32 }`  | `diagnostic.rs:55` | 1-indexed line + column.                                                                                                                                 |
| `pub struct DiagBag` with `iter`, `sorted`, `is_empty`, `has_error`, `has_repairable`, `exit_code` | `diagnostic.rs:135–207` | The accumulator. The LSP calls `sorted()` so per-URI diagnostics are deterministic across publishes.                                            |

**Required addition:**

| Symbol                                                                                | Why                                                                                                                                                                                                                          |
| ------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pub fn check_source_with_imports(source, file_label, importer_path: Option<&Path>, enable_effects) -> DiagBag` `[NEW]` | M3 wants to lint an *unsaved* buffer while still resolving its `import` lines against on-disk dependencies. The current split is binary: `check_source` (no imports) or `check_file` (reads from disk). The new helper stitches them: parse the in-memory buffer, then run `check_file_recursive`-style logic to resolve imports starting at `importer_path.parent()`. Implementation is a refactor of the existing `check_file_recursive` body to accept either `(path, &str source)` or just `path` for already-on-disk dependencies. |

### 4.2 Span / line-index utilities

| Symbol                                       | File                | Notes                                                                                                                          |
| -------------------------------------------- | ------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `pub struct Span { file_id, start, end }`    | `span.rs:10`        | Half-open byte range. Used internally; the LSP rarely sees raw `Span`s but must understand them when consuming AST nodes.   |
| `pub struct LineIndex` + `LineIndex::new` + `LineIndex::line_col` | `span.rs:38–67` | Byte-offset → 1-indexed (line, col). The LSP needs the inverse — see §4.4 — for reverse-lookup at definition time.            |
| `pub struct Spanned<T> { node, span }`       | `span.rs:24`        | Used on every top-level decl in `ast::Decl`. Critical for M2: gives us declaration spans for free.                            |

**Required addition:**

| Symbol                          | Why                                                                                                                                                                                  |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `LineIndex::byte_offset(line: u32, col: u32) -> u32` `[NEW]` | The LSP receives a `Position { line, character }` (0-indexed) from the editor and needs to map it back to a byte offset to look up which AST node covers it. `LineIndex` has the forward mapping; we add the inverse. Trivial: `self.line_starts[(line - 1) as usize] + (col - 1)` with bounds checks. |

### 4.3 AST — required for M2

| Type                                                  | File             | Notes                                                                                                                                                  |
| ----------------------------------------------------- | ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `pub struct SourceFile { decls: Vec<Decl> }`          | `ast.rs:10`      | The parser's output. Held in `DocumentState` after each parse.                                                                                          |
| `pub enum Decl { Skill, Text, ExportBlock, Block, Import }` | `ast.rs:15`      | Every variant wraps `Spanned<…>` — declaration spans available out of the box. **Definition sites for top-level names are these spans.**            |
| `pub struct Param { name, default, span }`            | `ast.rs:120–129` | **Already has a span** (the only sub-decl currently with one) — sufficient to make header parameters jumpable in M2.                                  |
| `pub enum FlowStmt`                                   | `ast.rs:148–169` | **Has no per-statement spans.** Variants `InlineString`, `Call { target, args, … }`, `BareName`, `ConstraintMarker`, `ContextMarker`, `Branch`, `Return` carry only cooked text. This is the central blocker for high-quality go-to-def — see §10.A. |
| `pub struct ConstraintMarker { marker, name }`        | `ast.rs:131–137` | `name` has no span.                                                                                                                                    |
| `pub struct ImportDecl { path, kind }` + `ImportName { name, alias }` | `ast.rs:30–53` | The whole `ImportDecl` is `Spanned<>` (its enclosing `Decl::Import`), but per-name spans inside `Selective(Vec<ImportName>)` are absent.              |

**Required additions for M2 (high-quality go-to-def):**

| Change                                                                                       | Why                                                                                                                                                                                                          |
| -------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Wrap `FlowStmt::Call.target`, `FlowStmt::BareName(...)`, `ConstraintMarker.name`, `ContextEntry::NameRef(...)`, and `ImportName.name` with `Spanned<String>` (or add a sibling `_span: Span` field). `[NEW]` | Without these, the LSP cannot know which identifier the cursor lands on — only which declaration encloses the cursor. The parser already has the token `Span` at construction time; threading it through the AST is purely mechanical. |
| Add `body_bare_names_with_spans: Vec<Spanned<String>>` (or replace the existing `body_bare_names: Vec<String>` field on `Skill` and similar). `[NEW]` | Same reason. `analyze.rs:418` already accepts the synthetic-fallback span loss as a known limitation; this fixes it.                                                                                          |
| Add per-slot spans inside flow inline strings. `slot::scan_slots` in `slot.rs` returns `{name, …}` records; extend to also return the byte offset of the `{` and `}` within the string, plus the byte offset of the string within the source. `[NEW]` | Required for M2 go-to-def **inside** `{name}` interpolation slots (per the brief's specific call-out). Today the parser drops the source position of the string literal — we need to keep it.            |

These changes are additive: existing call sites that don't use the new spans continue to compile.

### 4.4 Name resolution — required for M2 / M3

**This is the most important read of the existing code.** The brief's framing ("Uses `glyph-core`'s name-resolution table (which Analyze already builds — verify by reading the code)") needs correction:

> **There is no `(use_span → def_span)` table in `glyph-core` today.** `analyze::analyze_with_diagnostics` (`analyze.rs:38`) and `analyze::analyze_with_imports` (`analyze.rs:153`) emit *diagnostics* for unresolved names but **do not record successful resolutions anywhere**. On a clean parse, Analyze returns `SourceFile` unchanged. The compiler resolves names a second time from scratch in Phase 4 (Lower) at `lower.rs` (see `resolve_block_body_text` `lower.rs:23` and the `texts` / `blocks` HashMaps used inside `lower::lower`).

This is fine for the compiler — Lower needs the data anyway, and the cost is negligible for kilobyte-scale files. It is **not** fine for an LSP that wants to answer go-to-def from the AST alone without re-running Lower.

**Required addition:**

```rust
// glyph-core/src/analyze.rs (or a new module glyph-core/src/resolve.rs)

/// A resolved name reference: the source position where a name was *used*,
/// and the declaration span where it was *defined*.
#[derive(Clone, Debug)]
pub struct Resolution {
    pub use_span: Span,
    pub def_span: Span,
    pub def_file: PathBuf, // None / same-file when the def is in this file; Some(...) when imported
    pub kind: ResolutionKind,
}

pub enum ResolutionKind {
    Skill,       // applies()-style block-trigger receiver
    Block,
    ExportBlock,
    Text,        // includes ExportText
    Param,       // header parameter of the enclosing skill/block/export-block
    Import,      // the import alias / selective name itself
    Stdlib,      // @glyph/std member
}

/// Run Phase 2 like `analyze_with_diagnostics`, but *also* return a flat list
/// of every resolved reference. The caller can index into this list (e.g.,
/// binary-search on `use_span.start`) to answer go-to-def queries.
pub fn analyze_with_resolutions(
    file: SourceFile,
    file_id: u32,
    file_label: &str,
    line_index: &LineIndex,
    bag: &mut DiagBag,
    enable_effects: bool,
) -> (SourceFile, Vec<Resolution>) { ... }
```

Implementation: every place the existing analyze code currently *checks* a name against `text_names` / `block_names` / `imported_texts` / `imported_blocks` and is happy with the result is a place where we push a `Resolution` onto the output vector. The diagnostic-only paths and the resolution-recording paths share the same matchers — this is a small, surgical refactor (`analyze.rs:440–501`, `analyze.rs:541–566`, etc., all already have the relevant scopes in hand).

For M3 (cross-file), the existing `check_file_recursive` (`lib.rs:275`) already walks the DAG and knows which imported name came from which file. The same recursion writes a per-file resolution list and the LSP joins them by file URI.

### 4.5 Other touched symbols

| Symbol                                                                | File         | Use                                                                                                |
| --------------------------------------------------------------------- | ------------ | -------------------------------------------------------------------------------------------------- |
| `lib::resolve_import_path(importer: &Path, import_path: &str)` private | `lib.rs:174` | Needs to be made `pub` (or wrapped in a public helper) so the LSP can answer "go to definition of an import path string" by jumping to file 1:1 of the imported file. |
| `parse::parse_with_diagnostics_opts(...)`                              | `parse.rs:77` | Driven indirectly via `check_source_*`; the LSP doesn't call it directly but inherits its diagnostics.                                                                |
| `slot::scan_slots(text: &str)`                                         | `slot.rs`    | After the §4.3 extension, the LSP uses this to map a cursor inside a flow string back to a `{name}` slot.                                                            |

### 4.6 Summary of additions to `glyph-core`

| Item                                                                           | Crate effort      | Required by milestone |
| ------------------------------------------------------------------------------ | ----------------- | --------------------- |
| `LineIndex::byte_offset` inverse lookup                                        | trivial (5 LOC)   | M1                    |
| `pub fn check_source_with_imports`                                             | refactor (~40 LOC) | M3                    |
| `pub fn resolve_import_path` (made public, or thin wrapper)                    | trivial           | M3                    |
| Spans on `FlowStmt::Call.target` / `BareName` / `ConstraintMarker.name` / `ContextEntry::NameRef` / `ImportName.name` | mechanical (~80 LOC across `parse.rs` + `ast.rs`) | M2 |
| `slot::scan_slots` returning per-slot byte offsets                             | small (~20 LOC)   | M2                    |
| `analyze::analyze_with_resolutions` + `Resolution` / `ResolutionKind` types    | refactor (~150 LOC, mostly mirroring existing branches) | M2 |
| Cross-file resolution list extension to `check_file_recursive`                 | refactor (~60 LOC) | M3                    |

None of these change the compiler's observable behaviour — they expose internal information that already exists at compile time, or they restore source provenance (spans) the AST is currently dropping.

---

## 5. Document State Model

**Per-buffer state** — keyed by LSP `Url`:

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
    resolutions: Vec<Resolution>,       // populated when M2+ analyze_with_resolutions is called
    diag_bag: DiagBag,
}
```

**Lifecycle:**

- `didOpen { uri, text }` → insert `DocumentState`; mark `parsed = None`; trigger a parse + diagnostics publish.
- `didChange { uri, contentChanges, version }` → update `text` (full or incremental sync — see below), bump version, set `parsed = None`. **In v1, no auto-republish on change** (we wait for `didSave`); see §10.D.
- `didSave { uri, text? }` → if `text` is provided, replace; trigger parse + diagnostics publish. For cross-file diagnostics (M3), also re-lint any open buffer that imports this URI.
- `didClose { uri }` → drop `DocumentState`. Publish empty diagnostics for the URI **only if** no other open buffer transitively imports it.

**Sync mode.** v1 advertises `TextDocumentSyncKind::Full` in capabilities. Incremental sync is optional and adds complexity (rebuilding `LineIndex` segments is fiddly); a full re-send of a kilobyte-scale skill file every keystroke is fine bandwidth-wise. If this becomes a problem, switching to `Incremental` is a `tower-lsp` capability flip plus a small text-edit applier.

**What invalidates a parse.** Any change to `text` for the buffer in question. Re-parse on next request or save — never speculatively in the background in v1.

**What invalidates cross-file diagnostics (M3).** A `didSave` of file `A` invalidates the cached `ParsedView` of every open buffer that has an `import` line resolving to `A`. The simplest implementation re-runs `check_source_with_imports` for those buffers eagerly; a quieter implementation defers until the next request that needs them.

**When does the server re-parse?**
- On the request handler for any LSP method that needs an AST (`textDocument/definition`, `textDocument/publishDiagnostics`).
- Lazy: only when `parsed.is_none()`.
- Always full-file. Glyph has no incremental parser and won't grow one for v1 — files are too small for the complexity to pay off.

**Workspace mode.** Workspace-wide indexing is **not** a v1 feature — we do not pre-walk the project on `initialize`. Files become known to the server only when the editor opens them or when an open buffer imports them (and we then read the import on demand). This keeps the server stateless on startup; the cost is that go-to-def from an unopened file isn't possible until it's opened, which matches the brief's "single-file-at-a-time vs workspace-wide" framing.

---

## 6. LSP Diagnostic Mapping

The compiler emits `glyph_core::diagnostic::Diagnostic`. The LSP must translate to `lsp_types::Diagnostic`. Mapping is purely mechanical:

| `glyph-core` field                             | LSP `Diagnostic` field           | Conversion                                                                                                                                                                                                                       |
| ---------------------------------------------- | -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id: String` (e.g., `"G::analyze::missing-required-arg"`) | `code: NumberOrString::String(id.clone())` | Verbatim. The ID is stable across compiler versions per `diagnostics.md` §ID Scheme — perfect as an LSP code.                                                                                                                |
| `classification: Classification`               | `severity: DiagnosticSeverity`   | `Error → Error`, `Repairable → Warning`, `Warning → Information`. **Rationale:** repairable is "the agent will likely fix this" — analogous to a clippy-style hint that an automated tool can address; it's not a hard error in the editor, but it's not a passive note either. `Warning` matches that mental model. (Pretty-printer in `glyph-cli/src/main.rs:493` makes the same call.) |
| `message: String`                              | `message: String`                | Verbatim.                                                                                                                                                                                                                        |
| `span.start: LineCol { line, col }` (1-indexed) | `range.start: Position { line, character }` (0-indexed) | `Position { line: span.start.line - 1, character: span.start.col - 1 }`.                                                                                                                                                     |
| `span.end: LineCol { line, col }` (1-indexed, **inclusive**) | `range.end: Position`            | `Position { line: span.end.line - 1, character: span.end.col }`. **Note the asymmetry**: glyph end is inclusive at the column level (`end.col` is the column *of the last character*), LSP end is exclusive — so we add 1 column, not subtract 1. Multi-line spans (`end.line > start.line`) work the same: bump `end.col` by 1. |
| `related: Vec<SourceSpan>`                     | `related_information: Option<Vec<DiagnosticRelatedInformation>>` | Each `SourceSpan` becomes a `DiagnosticRelatedInformation { location: Location { uri, range }, message: "" }`. Empty vector → `None`.                                                                                          |
| `hints: Vec<String>`                           | (none directly)                  | LSP has no first-class "hints" field. v1 appends each hint to `message` as a new line: `\n  hint: <hint>`. Post-MVP, surface hints via code-actions.                                                                            |
| (constant)                                     | `source: Option<String>`         | Always `Some("glyph".into())`.                                                                                                                                                                                                  |
| (constant)                                     | `tags: Option<Vec<DiagnosticTag>>` | `None` in v1. Future: `unused-import` could carry `DiagnosticTag::UNNECESSARY`.                                                                                                                                                |

**Sort order.** The LSP publishes the bag's `sorted()` output (`diagnostic.rs:196`), which orders by `(file, byte_start, id)`. This makes diagnostic publish payloads byte-stable for a given source — useful for editor caches and snapshot tests.

**File attribution.** `glyph-core` diagnostics carry a `file` string (`SourceSpan.file`) that the LSP set as the file URI's path component when calling `check_source_with_imports`. M3 may produce diagnostics whose `file` is a *different* URI than the one the LSP just analyzed (a bug surfaced in an imported file). The server publishes those diagnostics under the imported file's URI, not the originating buffer's. This is the LSP convention and matches `glyph check`'s multi-file behavior in `cli.md` §Multi-File Behavior.

**Empty-bag publish.** When a previously-published URI now has zero diagnostics, the server publishes an empty array to clear stale markers. Mechanically: compare `last_published_diagnostics` to the new bag; publish whenever they differ.

---

## 7. Go-to-Definition Algorithm

Triggered by `textDocument/definition { textDocument: { uri }, position: { line, character } }`. The server returns one of: `null` (no definition), `Location { uri, range }` (single), or `Vec<Location>` (multiple — rare in Glyph but possible for whole-module imports surfacing aliases).

**Algorithm (M2, single-file):**

1. **Resolve buffer.** Look up `DocumentState` for `uri`. If missing, return `null`.
2. **Ensure parse.** If `parsed.is_none()`, run `check_source_with_imports` (or `check_source` in M2) to populate `ast`, `line_index`, `resolutions`, `diag_bag`. (M2: parse-only is enough; M3: needs the import-aware flavor for cross-file.)
3. **Map cursor to byte offset.** `let off = line_index.byte_offset(position.line + 1, position.character + 1)` (1-indexed for `LineIndex`). Out-of-range → `null`.
4. **Find the identifier under the cursor.** Walk `parsed.resolutions` for the smallest `use_span` containing `off`. (Resolutions are span-disjoint by construction — every reference has exactly one resolution, so this is just `resolutions.iter().find(|r| r.use_span.contains(off))` or a binary search if we sort.) If no resolution covers the cursor, fall back to looking inside flow inline strings: scan the slot list of the enclosing flow string (per §4.3 extension); a `{name}` whose `{...}` brackets enclose `off` resolves against the enclosing skill/block/export-block parameter list (`Param.name` / `Param.span`). If still nothing: `null`.
5. **Return the location.**
   - `def_span` is in the same file: `Location { uri, range: span_to_lsp_range(def_span) }`.
   - `def_span` is in a different file: `Location { uri: file_uri(resolution.def_file), range: ... }`. (M3 only.)

**Cross-file resolution (M3).** The `Resolution { def_file, def_span }` already carries the path the def lives in (per §4.4). The LSP converts the path to a `Url` (`Url::from_file_path`) and replies. If the def file is not currently open, that's fine — `Url::from_file_path` works on any extant filesystem path.

**Stdlib targets (`@glyph/std`).** When `resolution.kind == Stdlib`, there is no `.glyph` source to jump to — these are compiler-embedded primitives (see `lib.rs:368-407`). v1 returns `null`. Post-MVP could open a synthetic markdown buffer describing the primitive (analogous to rust-analyzer's "go to definition on a built-in"). Out of scope here.

**Inside `{name}` slots.** Per the brief's specific call-out:

- The cursor is at byte offset `off` inside an inline flow string `s` whose source-relative byte span is `[s_start, s_end)` (per the §4.3 extension to `slot::scan_slots`).
- For each slot returned by `scan_slots(s.text)`, compute its absolute span: `[s_start + slot.brace_open_offset, s_start + slot.brace_close_offset + 1)`. If `off` falls inside any slot, we have a match.
- The slot resolves against the enclosing declaration's `Param` list (search by `slot.name`). If a `Param` with that name exists, return its `span` (already in the AST per `ast.rs:128`). If none — that's a `G::analyze::unknown-param-slot` — return `null`.

**Unresolvable identifiers.** If the cursor lands on a name that the compiler considers undefined (an `undefined-name` / `undefined-call` / `unknown-param-slot` diagnostic covers it), return `null`. The user already sees the diagnostic; go-to-def cannot manufacture a definition.

**Complexity.** Per request: one parse (only if dirty), one linear walk of `resolutions` (or one binary search if sorted by `use_span.start`). For typical Glyph files (<200 references) this is far under a millisecond.

---

## 8. Phased Implementation Plan

Three milestones, each landing as its own PR. Each PR is independently shippable: M1 alone produces a useful diagnostics-only LSP; M2 adds same-file go-to-def; M3 extends to cross-file.

### M1 — Diagnostics-only stdio server

**Scope.**

- New crate `crates/glyph-lsp` in the workspace (depends on `glyph-core`, `tower-lsp`, `tokio`, `lsp-types`, `serde`, `serde_json`, `url`).
- `glyph lsp` subcommand on the existing `glyph-cli` binary that calls into `glyph-lsp::run(stdin, stdout)`. Single binary, single distribution. (A separate `glyph-lsp` binary was considered and rejected: distribution complexity, plus most editors call `glyph lsp` once and forget it. The subcommand path keeps everything one cargo install away.)
- `LineIndex::byte_offset` reverse mapping in `glyph-core` (per §4.2).
- `tower-lsp`-based `Backend` impl exposing `initialize`, `initialized`, `shutdown`, `did_open`, `did_change`, `did_save`, `did_close`.
- `DocumentStore` with `DocumentState` per URI; `Full` text sync.
- Diagnostics computed via `glyph_core::check_source_with_effects(text, 0, &uri.path(), enable_effects)` and translated per §6.
- `--enable-effects` plumbed via `initializationOptions: { "enableEffects": bool }`. Default false (matches the CLI default).

**Deliverables.** `crates/glyph-lsp/`, `glyph lsp` subcommand, integration test that drives the server via stdio with a hand-crafted JSON-RPC stream.

**Exit criteria.**

1. `nvim-lspconfig` snippet in §9 attaches and shows the welcome message in `:LspInfo`.
2. Opening a `.glyph` file with a known parse error (e.g., `flow:` indented with tabs) shows the corresponding `G::parse::tab-indent` diagnostic in the editor with the correct line and column.
3. Saving a file with a fixed source clears the diagnostic.
4. Opening a clean file produces zero diagnostics in `:LspInfo`.
5. Server shuts down cleanly on `exit` notification (no orphan stdio processes).

### M2 — Single-file go-to-definition

**Scope.**

- `glyph-core` AST extensions per §4.3: spans on `FlowStmt::Call.target`, `FlowStmt::BareName`, `ConstraintMarker.name`, `ContextEntry::NameRef`, `ImportName.name`. Update the parser to populate them (the spans already exist at token-consumption time in `parse.rs`).
- `glyph-core` `slot::scan_slots` extension per §4.3.
- `glyph-core` `analyze::analyze_with_resolutions` per §4.4 returning `Vec<Resolution>` for same-file targets only.
- LSP `text_document_definition` handler implementing the §7 algorithm minus the cross-file branch.

**Deliverables.** Updated parser/AST, new `Resolution` types, new analyze entry point, LSP handler, integration tests.

**Exit criteria.**

1. Cursor on a `block` call target (e.g., `validate_plan()` inside `flow:`) jumps to the `block validate_plan` declaration in the same file.
2. Cursor on a `text` name in `require <name>` jumps to its `text <name>` declaration.
3. Cursor inside a `{param}` slot in a flow inline string jumps to the parameter declaration in the enclosing `skill` / `block` / `export block` header.
4. Cursor on an unresolvable identifier returns `null` (editor reports "no definition").
5. All M1 exit criteria still pass.

### M3 — Cross-file go-to-definition + import-aware diagnostics

**Scope.**

- `glyph-core` `check_source_with_imports` per §4.1.
- `glyph-core` `resolve_import_path` made public.
- `analyze_with_resolutions` extended to record cross-file resolutions during the existing import walk (`check_file_recursive` already knows the imported file path).
- LSP cross-file diagnostic publishing: when a buffer is checked, every URI present in the resulting `DiagBag` gets its own `publishDiagnostics` notification.
- LSP `definition` handler honours `Resolution.def_file` for non-local targets; converts to `Url::from_file_path`.
- LSP minimal `FileGraph` cache so opening file `B` after editing file `A` (which imports `B`) does not re-walk the entire project tree.

**Deliverables.** Import-aware single-buffer linting, cross-file def jumps, file-graph cache.

**Exit criteria.**

1. A buffer with `import "./prefs.glyph" { project_conventions }` and a use of `project_conventions` jumps to the `text project_conventions` declaration in `prefs.glyph`, opening that file in the editor.
2. Editing the importer to refer to a non-existent name produces a `G::analyze::import-private` (or related) diagnostic, with span on the `import` line.
3. Editing `prefs.glyph` and saving clears or updates the importer's diagnostics on the next request that needs them (no perceptible lag in nvim).
4. Stdlib references (`subagent`, `send`) do not jump (return `null`) but also do not error.
5. All M1 / M2 exit criteria still pass.

---

## 9. Editor Integration

### Neovim (verified target)

The following snippet drops into the user's `init.lua` (or a Neovim config file). It assumes `nvim-lspconfig` is already installed (the user has `nvim-treesitter` set up, so `nvim-lspconfig` is a sibling install).

```lua
-- ~/.config/nvim/lua/plugins/glyph.lua  (or wherever your LSP setup lives)

local lspconfig = require("lspconfig")
local configs = require("lspconfig.configs")

-- Register the glyph LSP if it isn't already registered.
if not configs.glyph then
  configs.glyph = {
    default_config = {
      cmd = { "glyph", "lsp" },                    -- requires `glyph` on PATH
      filetypes = { "glyph" },                     -- see Filetype below
      root_dir = lspconfig.util.root_pattern(
        ".git",
        "Cargo.toml",                              -- glyph projects often live in a workspace
        "*.glyph"                               -- fallback: dir containing any .glyph
      ),
      single_file_support = true,                  -- works on lone .glyph files
      init_options = {
        enableEffects = false,                     -- match CLI default; flip to true if your project uses effects:
      },
      settings = {},
    },
  }
end

lspconfig.glyph.setup({
  -- on_attach = your_on_attach,
  -- capabilities = your_capabilities,
})

-- Filetype detection: .glyph → filetype "glyph"
-- (The tree-sitter grammar branch ships a parser keyed off this same filetype.)
vim.filetype.add({
  extension = {
    ["glyph"] = "glyph",
  },
})
```

**Notes.**

- **`cmd = { "glyph", "lsp" }`** matches the M1 subcommand decision (§8). If the user prefers an explicit binary, swap to `{ "glyph-lsp" }` — but we don't ship one in v1.
- **`filetypes = { "glyph" }`** must agree with whatever the tree-sitter highlighting branch uses; both branches should land on the same filetype name.
- **`init_options.enableEffects`** mirrors the CLI's `--enable-effects` flag. Default `false` per `cli.md`.
- **`single_file_support = true`** means an `nvim` invocation that opens a single `.glyph` outside any project root still gets diagnostics (M1 covers this; M2/M3 may degrade gracefully when there's no on-disk import context).

**Verification.** With the above, `:LspInfo` after opening a `.glyph` shows `glyph` attached. `:lua vim.lsp.buf.definition()` (or `gd` if mapped on `LspAttach`) jumps to the def. `:lua vim.diagnostic.open_float()` shows the diagnostic at point.

### VS Code (untested)

A minimal extension manifest that points at the same binary:

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

**Flagged untested in v1.** We document the path so a contributor or the user can verify it; the team-lead should not block on it.

---

## 10. Open Questions and Risks

### A. **AST does not currently carry per-reference spans.** *(High impact, raised by reading the source.)*

The brief's framing ("Uses `glyph-core`'s name-resolution table (which Analyze already builds — verify by reading the code)") doesn't match the code:

- Analyze emits diagnostics on resolution *failure* but does not record successful resolutions.
- AST nodes for *uses* (call targets, constraint names, context name refs, import names, body bare names) carry only cooked strings, not source spans. The parser has the spans at token time and drops them.

§4.3 and §4.4 propose the additive changes to fix this. They are mechanical, not conceptual. **But this means M2 is a roughly 200–300 LOC `glyph-core` change, not a pure LSP-side feature.** The team-lead should weigh in on:

1. Is that `glyph-core` change OK to merge into `feature/glyph-lsp` or should it land on `main` first?
2. Is `analyze::analyze_with_resolutions` the right shape, or would they prefer a separate `resolve` module?
3. Should we use `Spanned<String>` or `(String, Span)` tuples in the AST? Existing `Param` uses a `pub span: Span` field next to `pub name`; consistency says do the same elsewhere. Confirming.

### B. **Inclusive-vs-exclusive end conversion is easy to get wrong.** *(Medium.)*

`glyph-core`'s `SourceSpan.end` is 1-indexed inclusive (`diagnostic.rs:64`). LSP's `Range.end` is 0-indexed exclusive. The mapping (§6) is `lsp_end_char = glyph_end_col` (no off-by-one — the +1 from converting to 0-indexed and the -1 from inclusive→exclusive cancel). M1 must include a unit test against a single-character span to lock this in.

### C. **`didChange` debouncing is deferred — is "publish on save only" acceptable?** *(Medium — UX call.)*

v1 republishes diagnostics on `didOpen` and `didSave` only. This means a user mid-edit doesn't see compiler diagnostics until they save. Tradeoff:

- **Pro (current plan):** simpler; no debouncing logic; matches `cargo check` workflow many Rust users expect; matches the one-error-at-a-time philosophy of `pipeline.md` Phase 1 (bail-at-first-parse-error becomes very chatty under live rechecking).
- **Con:** modern editors expect live errors. Glyph compilation is fast enough that a 200ms debounce on `didChange` would feel instantaneous.

Recommendation: ship M1 save-only; reconsider after dogfooding. Adding a 200ms `tokio::time::sleep` debounce on `didChange` is a 30-LOC follow-up; it does not block the release.

### D. **Stdlib targets (`@glyph/std::subagent`, etc.) have no source location.** *(Low.)*

§7 returns `null` for these. Post-MVP, we could expose a synthetic readonly markdown buffer ("Stdlib: `subagent` — see stdlib.md §..."). Worth noting because users will try `gd` on `subagent` and be momentarily confused.

### E. **Workspace-wide diagnostics are deferred.** *(Low — explicit non-goal.)*

The server only knows about open buffers and their (resolved-on-demand) imports. A user who never opens `lib_x.glyph` won't see diagnostics from it even though it's in the project. v1 is "what your editor shows you." A workspace symbol index / project diagnostics is a clean post-MVP follow-up — `compile_directory` already builds the DAG and can stream per-file diagnostics.

### F. **`tokio` dependency adds build-time weight.** *(Low.)*

`tower-lsp` brings `tokio` (multi-threaded runtime by default; we'd configure single-threaded `current_thread` flavor to match our serial work pattern). This adds ~5 seconds to a clean build. Acceptable for a tool the user installs once; flag if anyone's CI is sensitive.

### G. **Filetype name agreement with the tree-sitter branch.** *(Low — coordination.)*

The §9 nvim snippet uses `filetype = "glyph"`. The tree-sitter highlighting branch must use the same. Check before both branches merge.

---

## Summary

The Glyph LSP v1 is a thin `tower-lsp`-based wrapper over `glyph-core`'s existing Phase 1+2 phases, plus a small additive `glyph-core` change to expose per-reference source spans and a name-resolution table. M1 (diagnostics) is mostly LSP plumbing; M2 (same-file go-to-def) requires the AST/analyze additions called out in §4.3 and §4.4; M3 (cross-file go-to-def + import-aware diagnostics) reuses the existing `check_file_recursive` infrastructure and adds cross-file resolution recording. Editor verification is via `nvim-lspconfig`; VS Code is documented untested.

The principal open question for the team-lead is §10.A: confirm we're authorised to extend `glyph-core` with per-reference spans and the resolution table as part of M2, since the brief assumed that infrastructure already existed.
