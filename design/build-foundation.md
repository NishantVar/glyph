# Glyph Build Foundation

Locked implementation decisions for the Glyph MVP compiler. These gate the first commit and are not revisited unless a concrete problem surfaces during implementation.

## A1 — Crate Layout

**Decision: Two-crate Cargo workspace.**

| Crate | Role |
|---|---|
| `glyph-cli` | Binary. CLI arg parsing (`clap`), orchestrates the pipeline, renders pretty diagnostics to stderr (`codespan-reporting`), writes `.md` and `.ir.json` to disk. |
| `glyph-core` | Library. All deterministic compiler phases (1, 2, 4, 5, 6-Step1, 7). IR types, AST types, diagnostic types, arena, span. Each phase is a public function. IR types derive `serde::Serialize` for JSON output. |

**No `glyph-llm` crate.** The compiler is fully deterministic. LLM phases (Repair = Phase 3, Expand Step 2 = Phase 6) are implemented as an agent skill that invokes the CLI and handles LLM work between invocations. There is no API key, no HTTP client, no async runtime in the compiler.

**Extraction triggers (post-MVP):** Extract `glyph-llm` if an embedded LLM mode is added. Extract `glyph-diagnostics` if a language server needs shared diagnostic types. Extract `glyph-emit` if Emit grows multi-format support. Not before.

**Rationale:** The Safety Sandwich architecture divides the pipeline into deterministic and LLM-assisted phases. For MVP, the compiler owns only the deterministic phases. The two-crate split (binary + library) is the minimum structure — it lets integration tests call the library directly and keeps the binary thin.

### Emit module inventory (`glyph-core::emit`)

The deterministic emitter is split into focused modules per `expand.md` §3.5:

| Module | Role |
|---|---|
| `emit::scaffold` | Walks the resolved IR and builds the `Scaffold { chunks: Vec<Chunk> }` value. Owns section/list scaffolding, return-fold suffix selection, and the standalone-return path for return-only skills/procedures. |
| `emit::merger` | Substitutes span fills into the scaffold to produce the final Markdown string. Validates that every emitted span has a fill and rejects unknown span IDs. |
| `emit::stub_fill` | Today's deterministic span filler. Replaced or per-`SpanKind` overridden when the LLM Expand pass lands. |
| `emit::constraint` | Locked four-form constraint renderer (`hard avoid`, `soft avoid`, `hard require`, `soft require`) per `compiled-output.md` §Constraint Rendering. |
| `emit::branch` | Pure-`applies()` Branch projection — three sub-cases (single-arm, multi-arm, multi-arm with `else`) — and the mixed-condition `BranchCondition` span emission. |
| `emit::templates` | Locked text helpers shared across the emitter and `validate_output`: `append_identifier_suffix`, `append_description_suffix`, `standalone_return_identifier`, `standalone_return_description`, `external_file_step`, `kebab_case`. |

The `Scaffold`, `Chunk`, `SpanRef`, `SpanKind`, and `SpanPayload` types are internal to `emit::scaffold` and are **not** exposed via `--emit-ir` or any public API surface.

## A2 — Parser

**Decision: Hand-rolled recursive descent parser on a hand-rolled tokenizer.**

No parser generator dependency (`pest`, `lalrpop`, `nom`, `winnow`, `chumsky`). Zero parsing dependencies beyond `std`.

### Rationale

Glyph's grammar has three properties that fight parser generators:

1. **Indentation significance.** 4-space units determine nesting. Grammar-based parsers need synthetic INDENT/DEDENT tokens — an extra layer for no gain.
2. **Context-sensitive keywords.** `require`, `avoid`, `must` are constraint markers in some positions and bare names in others. `flow:`, `description:`, etc. are sub-section headers only in specific contexts. Hand-rolled state handles this naturally.
3. **Span tracking.** The spec demands rich diagnostics with precise spans. Hand-rolled parsers give full control over span emission. The MVP parser bails at the first parse error per `pipeline.md` §Phase 1; recovery and multi-diagnostic collection are deferred.

### Tokenizer Design

Two-phase approach:

**Phase A — Line-oriented pre-processing:**
- Split source into lines.
- Compute indentation level per line (count leading spaces, divide by 4, flag remainders).
- Strip comments (`//`), preserve positions for repair.
- Build line-offset table for span-to-line/col conversion.

**Phase B — Token-level scanning within lines:**
- Keywords: `skill`, `block`, `export`, `import`, `const`, `generated`, `if`, `elif`, `else`, `return`, `require`, `avoid`, `must`, `soft`, `hard`, `with`, `none`.
- Identifiers: `[a-zA-Z_][a-zA-Z0-9_]*`.
- Literals: quoted strings (`"..."`, `"""..."""`), integers, floats, booleans.
- Punctuation: `(`, `)`, `,`, `:`, `=`, `.`.
- Name slots: `{name}` inside instruction-bearing strings. These resolve to declared parameters (preserved as runtime slots in compiled output) or local bindings (resolved into prose by Expand Step 2). Curly braces (`{`, `}`) are not standalone punctuation in MVP — they appear only as name slot delimiters inside strings. The tokenizer recognizes `{name}` as a single `NameSlot` token when scanning string content; the distinction between parameter refs and local refs is resolved later in Analyze.

Tokens carry byte-offset spans. Line/col is derived on demand from the line-offset table.

### AST Shape

Loose AST — names unresolved, types unchecked, roles unassigned. Purely structural. Every node is `Spanned<T>`.

### Error Recovery

Declaration-boundary recovery only. If a declaration is broken, emit a diagnostic and skip to the next top-level declaration. No fine-grained recovery inside `flow:` bodies. The agent repair loop handles the rest.

No incremental parsing. Full reparse on every invocation.

### Estimated Size

~1500 LOC total (tokenizer ~350, parser ~900, AST types ~200).

## A3 — Span Type

**Decision: Plain struct, 12 bytes, no bit packing.**

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub file_id: u32,
    pub start: u32,   // byte offset, 0-indexed
    pub end: u32,     // byte offset, exclusive (half-open range)
}
```

**`Spanned<T>` wrapper** on every AST and IR node:

```rust
#[derive(Clone, Debug)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}
```

**`LineIndex` per file** for on-demand line/col conversion:

```rust
pub struct LineIndex {
    line_starts: Vec<u32>,  // byte offsets of each line start
}
```

Converts byte offsets to 1-indexed `{line, col}` for diagnostic output per `diagnostics.md`. Built once during tokenization, queried only when rendering diagnostics.

**`codespan-reporting`** for pretty-printed stderr output. Our `Span` converts to its `Range<usize>` at the rendering boundary.

### Rationale

The packed `u64` span (recommended starting position) saves 4 bytes per span at the cost of bit manipulation on every access and artificial file-size limits. For a project where individual source files are measured in kilobytes and ASTs have hundreds to low thousands of nodes, the savings are irrelevant. The plain struct is readable, debuggable, and has no artificial limits. Extract and pack later if profiling shows span size matters (it won't).

Half-open ranges are the Rust convention (`Range<T>`). Internally, a single-character span has `end = start + 1`. Diagnostic JSON output converts to inclusive 1-indexed `{line, col}` per `diagnostics.md` (where a single-character span has `start == end`). The `LineIndex::to_source_span()` method owns this conversion: it maps `(byte_start, byte_end)` half-open to `({line, col}, {line, col})` inclusive, matching the contract in `diagnostics.md` §Span Semantics.

## A4 — IR Representation

**Decision: Hand-rolled single arena per file.**

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub struct NodeId(pub u32);  // maps to spec's n0, n1, ...

pub struct IrArena {
    nodes: Vec<IrNode>,
}
```

`IrNode` is an enum over all node types from `ir-schema.md` — `Call`, `InlineInstruction`, `InstructionRef`, `Branch`, `Return`, `Constraint`, `ContextNode`, etc. Children are referenced by `NodeId`. All IR types derive `serde::Serialize` for the `--emit-ir` JSON output.

**Arena is IR-only.** The AST (Phases 1–2) uses plain owned trees (`Vec<Decl>` with nested `Vec<Stmt>`). The arena is built during Lower (Phase 4) with pre-order source-traversal allocation, exactly as `ir-schema.md` §Node Identifiers specifies.

**One arena per file, counter resets to 0.** Matches the spec's per-file scope with no global uniqueness.

**No `id-arena` dependency.** The abstraction is ~30 LOC and maps directly to the spec's `n<int>` ID scheme. A single `NodeId` type across all node kinds (the spec uses a single namespace).

### IR JSON Serialization

The `--emit-ir` flag outputs the **post-Step-1 resolved IR** as a JSON file (`foo.ir.json`) alongside the compiled `.md`. This is the IR after Expand Step 1 (deterministic resolution) — it includes `resolved_body_text`, `projection_mode`, `site_modifier`, and other resolved fields from `ir-schema.md` §Resolved IR. The agent needs the resolved IR (not the raw post-Phase-5 validated IR) because Step 2 reshaping operates on resolved content.

The JSON uses a **nested tree** shape (children inlined under parents) rather than a flat arena dump — this is natural for the agent to read and reason about during Expand Step 2. Each node carries its `node_id` as an attribute. This requires a custom serialization pass that walks the arena and builds the nested tree, rather than relying on derived `Serialize` on the arena directly.

The agent reads the IR JSON, performs LLM reshaping (Step 2) with full structural context — including `with` modifiers, roles, constraint attributes — and writes the final polished `.md`.

### JSON Determinism (Project-Wide Invariant)

Any JSON the compiler writes — IR JSON (`.ir.json`) and diagnostic JSON on stdout — must be byte-stable across runs over identical input. Two rules implement this and apply to every map- or list-shaped JSON output anywhere in the compiler:

1. **Map-shaped output uses `BTreeMap`** (or an equivalent sorted-keys serialization). `HashMap` is forbidden in any type whose `Serialize` impl is reachable from a CLI output path. This covers IR node fields, parameter sets, effect sets recorded as maps, and any future map-shaped diagnostic field.
2. **Diagnostic arrays are sorted by `(file, span.start.byte, id)`**, in that lexicographic order. This is the canonical ordering used by both pretty-printed stderr output and the NDJSON stdout stream. Files within a multi-file build are emitted in topological compile order (per `pipeline.md` §Multi-File Compilation Order); the per-file diagnostic array is sorted internally.

These two rules together are what makes Bar 2 in `mvp-acceptance.md` (byte-identical output across runs) achievable for `.ir.json` and diagnostic JSON, not just `.md`.

### Rationale

The spec requires stable node IDs for Phase 5 validation errors, Phase 6b retry feedback, and diagnostics. With arena allocation, the ID *is* the storage index — zero bookkeeping. Owned trees would require a separate ID assignment and mapping layer.

No `id-arena` because the spec defines a single `n<int>` namespace across all node types. A typed `Id<T>` that prevents mixing different arenas adds complexity without benefit when all nodes share one arena.

## A5 — Async Strategy

**Decision: Fully synchronous. No async runtime.**

No `tokio`, no `async-std`, no async runtime. Standard library `std::fs` for file I/O, `std::io` for stdout/stderr.

The compiler is a short-lived process: read source files, run deterministic phases, write output files. There is no I/O concurrency, no HTTP calls, no parallelism. Every phase is a pure function from its input to its output plus diagnostics.

**Multi-file builds are strictly serial.** A `glyph compile dir/` invocation compiles each `.glyph` file one at a time, in topological order over the import DAG. There is no threadpool, no `rayon`, no async fan-out across files. This is a direct consequence of the sync-only decision: independent files in the DAG are not parallelised. See `pipeline.md` §Multi-File Compilation Order for the topological ordering contract that this serial execution model satisfies. Parallelism across files is a post-MVP optimisation.

### Rationale

The original recommendation was "sync core, tokio at the LLM boundary." Since the compiler has no LLM boundary (LLM phases are handled by the external agent skill), there is no async boundary at all. Adding an async runtime for synchronous file I/O would be pure overhead.

If a future embedded LLM mode or language server requires async, introduce it then.

## A6 — Error Style

**Decision: Two separate channels — diagnostics and compiler errors.**

### Diagnostics (user-facing)

```rust
pub struct DiagBag {
    diagnostics: Vec<Diagnostic>,
}
```

Accumulated across all pipeline phases (1, 2, 4, 5 inside the compiler; Phase 3 and Phase 6 Step 2 diagnostics are emitted by the agent skill but follow the same `Diagnostic` shape). Each `Diagnostic` follows the shape from `diagnostics.md`: `id`, `classification`, `message`, `span`, optional `related` spans, optional `hints`.

Diagnostics are expected compiler output — "your source has an undefined name on line 5." They flow through the pipeline and are rendered as pretty stderr output or JSON stdout output.

### Compiler Errors (internal)

```rust
#[derive(Debug)]
pub enum CompileError {
    Io { path: PathBuf, source: std::io::Error },
    Internal(String),  // invariant violations, bugs
}
```

Hand-rolled `Display` impl (~15 lines). These mean the compiler itself is broken — file not found, arena index out of bounds, a phase received input that a previous phase should have rejected. Not user-facing diagnostics.

**No `thiserror`.** The error enum has 3–4 variants; hand-rolling `Display` is trivial and avoids a proc-macro dependency chain.

**No `anyhow`.** The error surface is small and well-defined. No need for dynamic error chaining.

### CLI Output Contract

| Channel | Content | Format |
|---|---|---|
| **stdout** (`--format json`) | Per-file diagnostic + emission records | NDJSON (one JSON object per line; see `cli.md` §JSON format shape) |
| **stderr** (always) | `warning` diagnostics + fatal compiler errors | Pretty-printed via `codespan-reporting` |
| **disk** | `foo.md` (compiled output) + `foo.ir.json` (validated IR, if `--emit-ir`) | Markdown + JSON |

### Exit Codes

| Code | Meaning | Agent action |
|---|---|---|
| `0` | Success. `.md` (and `.ir.json` if `--emit-ir`) written. | Proceed to Expand Step 2. |
| `1` | Hard errors. Cannot compile. | Surface diagnostics to author. Do not attempt repair. |
| `2` | Repairable diagnostics only. Pipeline stopped after Phase 2. | Agent performs LLM repair on source, re-invokes. |
| `3` | Invocation error. Bad flags, missing path, permission denied, IO failure. | Surface error to user. Stop. |

**`1` wins over `2`.** If both hard errors and repairable diagnostics exist, exit `1`. No point repairing if a hard error blocks compilation anyway.

### Pipeline Stop Behavior

The compiler stops after Phase 2 (Analyze) if repairable diagnostics exist. It does **not** continue to Lower/Validate/Emit on a dirty AST. Each re-invocation after repair runs the full pipeline from scratch. This guarantees that diagnostics are always accurate — later phases never see broken input from earlier phases.

## Dependencies

| Crate | Dependency | Justification |
|---|---|---|
| `glyph-core` | `serde`, `serde_json` | IR JSON serialization for `--emit-ir` agent workflow |
| `glyph-cli` | `clap` | CLI arg parsing (subcommands, flags, help text) |
| `glyph-cli` | `codespan-reporting` | Pretty-printed diagnostic rendering on stderr |

Total: 4 external crate dependencies (plus their transitive deps). No async runtime, no parser generator, no error-handling framework.

## Agent Workflow Summary

The compiler is a phase-granular toolkit invoked by an agent skill. The full compilation workflow:

```
Agent                                    Compiler (glyph compile)
  │                                        │
  ├─ glyph compile foo.glyph ──────────►│─ Phase 1 (Parse)
  │        --format json --emit-ir         │─ Phase 2 (Analyze)
  │                                        │
  │  (one of three outcomes)               │
  │                                        │
  │  ◄─── exit 1 + diagnostics (JSON) ────│  (hard errors — cannot compile)
  │  └─ Surface to author, stop            │
  │                                        │
  │  ◄─── exit 2 + diagnostics (JSON) ────│  (repairable diagnostics only)
  │  ├─ LLM repair: edit foo.glyph     │
  │  └─ Re-invoke glyph compile ──────────►│  (loop until exit 0 or 1)
  │                                        │
  │  ◄─── exit 0 ─────────────────────────│  (clean through Phase 2)
  │                                        │─ Phase 4 (Lower)
  │                                        │─ Phase 5 (Validate)
  │                                        │─ Phase 6 Step 1 (Expand, deterministic)
  │                                        │─ Phase 7 (Emit)
  │                                        │
  │  ◄─── foo.md + foo.ir.json on disk ───│  (success)
  │                                        │
  ├─ Read foo.ir.json                      │
  ├─ LLM Expand Step 2: reshape prose     │
  ├─ Overwrite foo.md with polished output │
  │                                        │
  └─ Done                                  │
```

**Multi-file builds:** Exit code semantics follow `pipeline.md` §Partial Failure Policy. Exit `0` only if every file succeeds. If any file has hard errors or is skipped due to a failed dependency, exit `1`. The `1`-wins-over-`2` rule applies per-build: if any file has hard errors, the entire build exits `1` even if other files only have repairable diagnostics.

## Cross-References

- **Pipeline:** `pipeline.md` — canonical 7-phase pipeline this document implements.
- **IR Schema:** `ir-schema.md` — node types, enums, node ID spec that A4 implements.
- **Diagnostics:** `diagnostics.md` — diagnostic shape and catalog that A6 implements.
- **Foundations:** `foundations.md` — #1 (compiler not runtime), #18 (deterministic passes own correctness).
