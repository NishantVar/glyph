# ADR 0011: Strictly Serial Multi-File Compilation In MVP

## Status

Accepted for MVP. Parallelism deferred post-MVP.

## Context

The Glyph compiler operates on a DAG of `.glyph` files connected by `import` statements. The DAG admits parallelism in principle:

- Independent leaves can compile in parallel.
- A dependency's Phase 6 (Expand) + Phase 7 (Emit) are architecturally independent of an importing file's Phases 1–5: importers only need the dependency's validated IR (Phase 5 output), not its compiled output.

Realistic Glyph projects in MVP scope have small file counts (single-digit to low-double-digit `.glyph` files per project). The compiler is also designed as a sync-only Rust binary (no async runtime — see the corresponding decision in build-foundation rationale).

## Decision

The MVP compiler compiles files **strictly serially**, in topological order. There is no threadpool, no `rayon`, no async fan-out. Independent leaves compile one at a time. A dependency's Expand/Emit cannot run concurrently with its importer's earlier phases.

The dependency-readiness gate is still enforced: an importer cannot enter Phase 2 (Analyze) until its dependency has passed Phase 5 (Validate). This gate operates serially.

## Consequences

- The implementation is simpler: no fork-join machinery, no inter-file synchronization, no double-checked invalidation of shared caches.
- The build is deterministically ordered: same DAG produces the same compilation order every run, simplifying diagnostic ordering and on-disk write ordering.
- For small projects (the MVP target), serial compilation is fast enough that parallelism would not pay back its complexity cost.
- The architectural independence between a dependency's Expand/Emit and an importer's Parse/Analyze/Lower/Validate is preserved in the design. A future post-MVP optimization can introduce parallelism along that boundary without redesigning the pipeline.
- The repair iteration counter is per-file, not per-build. Serial compilation makes this natural; under parallelism the counter would still be per-file but the accounting would need explicit cross-thread coordination.
