# Build-foundation todos

Working notes captured from the deleted `design/build-foundation.md`. The
durable rationale lives in `docs/adr/` (ADRs 0001–0007). This file holds
the implementation-flavoured residue: setup, module layout, agent
workflow steps, and post-MVP extraction triggers worth remembering when
they fire.

## Crate dependency inventory

Cargo.toml is the source of truth; this list is here only to keep
historical intent visible while the workspace is being scaffolded.

| Crate | Dependency | Why |
|---|---|---|
| `glyph-core` | `serde`, `serde_json` | IR JSON serialization for `--emit-ir`. |
| `glyph-cli` | `clap` | CLI arg parsing (subcommands, flags, help text). |
| `glyph-cli` | `codespan-reporting` | Pretty-printed diagnostic rendering on stderr. |

Total: 4 external crate dependencies (plus transitive deps). No async
runtime, no parser generator, no error-handling framework — see ADRs
0001, 0003, 0007.

## Tokenizer plan (hand-rolled, two phases)

Recorded so the implementation doesn't get reinvented.

**Phase A — line-oriented pre-processing:**

- Split source into lines.
- Compute indentation level per line (count leading spaces, divide by 4, flag remainders).
- Strip comments (`//`), preserve positions for repair.
- Build line-offset table for span-to-line/col conversion.

**Phase B — token-level scanning within lines:**

- Keywords: `skill`, `block`, `export`, `import`, `const`, `generated`, `if`, `elif`, `else`, `return`, `require`, `avoid`, `must`, `soft`, `hard`, `with`, `none`.
- Identifiers: `[a-zA-Z_][a-zA-Z0-9_]*`.
- Literals: quoted strings (`"..."`, `"""..."""`), integers, floats, booleans.
- Punctuation: `(`, `)`, `,`, `:`, `=`, `.`.
- Name slots: `{name}` inside instruction-bearing strings. These resolve to declared parameters (preserved as runtime slots in compiled output) or local bindings (resolved into prose by Expand Step 2). Curly braces (`{`, `}`) are not standalone punctuation in MVP — they appear only as name slot delimiters inside strings. The tokenizer recognises `{name}` as a single `NameSlot` token when scanning string content; the parameter-ref / local-ref distinction is resolved in Analyze.

Tokens carry byte-offset spans. Line/col is derived on demand from the
line-offset table.

Estimated size: ~1500 LOC total (tokenizer ~350, parser ~900, AST types ~200).

## Emit module inventory (`glyph-core::emit`)

The deterministic emitter is split into focused modules per [[docs/architecture/expand]] §3.5:

| Module | Role |
|---|---|
| `emit::scaffold` | Walks the resolved IR and builds the `Scaffold { chunks: Vec<Chunk> }` value. Owns section/list scaffolding, return-fold suffix selection, and the standalone-return path for return-only skills/procedures. |
| `emit::merger` | Substitutes span fills into the scaffold to produce the final Markdown string. Validates that every emitted span has a fill and rejects unknown span IDs. |
| `emit::stub_fill` | Today's deterministic span filler. Replaced or per-`SpanKind` overridden when the LLM Expand pass lands. |
| `emit::constraint` | Locked four-form constraint renderer (`hard avoid`, `soft avoid`, `hard require`, `soft require`) per [[docs/reference/compiled-output]] §Constraint Rendering. |
| `emit::branch` | Pure-`applies()` Branch projection — three sub-cases (single-arm, multi-arm, multi-arm with `else`) — and the mixed-condition `BranchCondition` span emission. |
| `emit::templates` | Locked text helpers shared across the emitter and `validate_output`: `append_identifier_suffix`, `append_description_suffix`, `standalone_return_identifier`, `standalone_return_description`, `external_file_step`, `kebab_case`. |

`Scaffold`, `Chunk`, `SpanRef`, `SpanKind`, and `SpanPayload` are internal to
`emit::scaffold` and are **not** exposed via `--emit-ir` or any public API.

## Pipeline stop behaviour

The compiler stops after Phase 2 (Analyze) if repairable diagnostics
exist. It does **not** continue to Lower/Validate/Emit on a dirty AST.
Each re-invocation after repair runs the full pipeline from scratch.
This guarantees that diagnostics are always accurate — later phases
never see broken input from earlier phases.

## Agent workflow

The compiler is a phase-granular toolkit invoked by an agent skill. The
full compilation workflow:

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

Multi-file builds follow [[compiler-pipeline]] §Partial Failure Policy: exit `0`
only if every file succeeds; `1` wins over `2` per-build.

## Post-MVP extraction triggers (keep, don't act yet)

- **`glyph-llm`** — extract if an embedded LLM mode is added inside the compiler binary.
- **`glyph-diagnostics`** — extract if a language server needs shared diagnostic types with the compiler.
- **`glyph-emit`** — extract if Emit grows multi-format output (something beyond Markdown).

None of these should be created up front. The decision lives in ADR 0002.
