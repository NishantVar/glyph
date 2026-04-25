# Glyph Diagnostics

This document defines the shape of structured diagnostics emitted by the Glyph compiler. It is the contract that `repair.md` §3 references as "structured diagnostics from earlier deterministic passes" and that `pipeline.md` Phase 2 (Analyze) produces.

## Diagnostic Shape

Every diagnostic the compiler emits is a structured record:

```text
Diagnostic {
  id:             String          // stable identifier, never changes across compiler versions
  classification: Classification  // see Classification below
  message:        String          // human-readable summary, one sentence
  span:           SourceSpan      // primary location in source
  related:        SourceSpan[]?   // other locations that contribute (e.g. the other side of a name collision)
  hints:          String[]?       // actionable suggestions for the author
}

SourceSpan {
  file:  String          // file path
  start: { line, col }   // 1-based
  end:   { line, col }   // 1-based, inclusive
}
```

## Classification

Diagnostics use the three-tier classification from `pipeline.md` Phase 2:

| Classification | Meaning | Compilation continues? |
|---|---|---|
| `error` | Hard stop, no repair possible | No |
| `repairable` | Phase 3 (Repair) can likely fix this | Paused until repair attempt |
| `warning` | Non-blocking observation | Yes |

Phase 3 internally distinguishes deterministic auto-fixes (3a) from LLM-assisted fixes (3b). That distinction is an implementation detail of the repair loop, not part of the diagnostic shape. See `todo.md` (Diagnostics section) for a possible future formalization.

## ID Scheme

Diagnostic IDs are namespaced by canonical pipeline phase, human-readable, and stable:

```
G::<phase>::<name>
```

- `G::parse::*` — Phase 1 (Parse)
- `G::analyze::*` — Phase 2 (Analyze)
- `G::validate::*` — Phase 5 (Validate)
- `G::repair::*` — Phase 3 notifications (e.g. "generated a definition")

Once an ID exists it is never renamed or reassigned. It can be deprecated and replaced by a new ID, but the old one keeps its meaning.

The `::` separator avoids collision with `.` (module access) and `/` (file paths).

## Catalog Completeness Rule

The catalog below is representative, not exhaustive. It grows as the compiler is implemented. The completeness meta-rule is:

1. **Every check in Phases 1–5 and 6b that can fail MUST have exactly one diagnostic ID.** No check may emit an unstructured error string, a bare exception, or a generic fallback diagnostic.
2. **The ID follows the `G::<phase>::<descriptive-name>` convention** defined above.
3. **Every diagnostic has the full shape** defined in §Diagnostic Shape (id, classification, message, span, optional related spans, optional hints).
4. **Classification is deterministic per ID.** Each diagnostic ID maps to exactly one of `error`/`repairable`/`warning`. The same ID never changes classification based on context.
5. **Implementers add IDs as checks are implemented**, following this convention. The representative catalog below serves as guidance and a naming reference.

## Examples

Representative diagnostics implied by the current design.

### Parse phase

| ID | Classification | Trigger |
|---|---|---|
| `G::parse::tab-indent` | repairable | Tabs used instead of 4-space indentation (`language-surface.md` §2.2) |
| `G::parse::mixed-indent` | repairable | Tabs and spaces on the same line (`language-surface.md` §2.2) |
| `G::parse::nested-flow` | error | `flow:` inside `flow:` (`data-flow.md`) |
| `G::parse::none-with-effects` | error | `effects: none, reads_files` — `none` alongside other keywords (`ir-and-semantics.md` §3). Detectable in Parse because the token sequence is unambiguous, but semantically an error — `none` exclusivity is a hard rule, not repairable. |
| `G::parse::multiple-with` | error | Chained `with ... with ...` on a single call (`data-flow.md`) |
| `G::parse::with-on-bare-name` | error | `with` modifier on a non-call statement (`data-flow.md`) |
| `G::parse::operator-in-expression` | repairable | Operator token (`+`, `-`, `*`, `/`, etc.) in expression position; MVP has no value-level operators (`values-and-names.md`) |

### Analyze phase

| ID | Classification | Trigger |
|---|---|---|
| `G::analyze::undefined-name` | repairable | Bare name doesn't resolve to any declaration; repair generates a definition (`repair.md` §5) |
| `G::analyze::undefined-call` | repairable | Parens-call doesn't resolve; repair generates a `generated block` (`repair.md` §5) |
| `G::analyze::name-collision` | error | Two names collide after case normalization (`values-and-names.md`) |
| `G::analyze::import-private` | error | Tried to import a non-exported declaration (`imports.md` §2) |
| `G::analyze::import-skill` | error | Tried to selectively import a skill (`imports.md` §2) |
| `G::analyze::circular-import` | error | Import cycle detected (`imports.md` §5) |
| `G::analyze::missing-file` | error | Import path doesn't resolve to a file (`imports.md` §1) |
| `G::analyze::duplicate-import` | repairable | Same file imported twice; merged (`imports.md` §6) |
| `G::analyze::unused-import` | repairable | Imported name never referenced; removed (`imports.md` §7) |
| `G::analyze::ambiguous-role` | repairable | Can't determine instruction role from context (`ir-and-semantics.md` §2) |
| `G::analyze::effects-under-declared` | error | Declared effects are a subset of inferred effects (`ir-and-semantics.md` §3) |
| `G::analyze::effects-over-declared` | warning | Declared effects include keywords not inferred from the body; non-blocking, surfaced for author cleanup (`ir-and-semantics.md` §3) |
| `G::analyze::nominal-mismatch` | error | Type name mismatch at a call boundary (`types.md`) |
| `G::analyze::lossy-coercion` | error | Lossy numeric conversion, e.g. `3.7` where integer expected (`values-and-names.md`) |
| `G::analyze::missing-return` | repairable | Export block lacks `return` on a code path (`language-surface.md` §3.3) |
| `G::analyze::closure-violation` | error | Export block depends on hidden caller context (`data-flow.md`) |
| `G::analyze::stdlib-missing-import` | repairable | `subagent()` used without importing `@glyph/std` (`stdlib.md`) |

### Repair notifications

| ID | Classification | Trigger |
|---|---|---|
| `G::repair::generated-text` | warning | A `generated text` was materialized for an undefined bare name (`repair.md` §5) |
| `G::repair::generated-block` | warning | A `generated block` was materialized for an undefined parens-call (`repair.md` §5) |

## Interaction With Repair

The repair pass (Phase 3) receives the full `Diagnostic[]` array as a snapshot, not a stream. After repair modifies the source, the compiler re-runs Parse + Analyze from scratch, producing a fresh diagnostic set. Repair is accepted when no `error` or `repairable` diagnostics remain. See `pipeline.md` Phase 3 for the bounded loop.

## Interaction With Compiled Output

Diagnostics are internal compiler artifacts. They do not appear in compiled `.md` files. Warning-level diagnostics (like `G::repair::generated-text`) are surfaced to the author through compiler CLI output or IDE integration, not through the compiled skill.

## Cross-References

- **Pipeline** (`pipeline.md`): Phase 2 emits diagnostics; Phase 3 consumes them.
- **Repair** (`repair.md` §3): Input contract references "structured diagnostics."
- **Values and names** (`values-and-names.md`): Name collision and shadowing rules.
- **Imports** (`imports.md`): Import-specific error conditions.
- **IR and semantics** (`ir-and-semantics.md`): Role inference and effect validation.
- **Standard library** (`stdlib.md`): Stdlib-specific resolution.
