//! Phase 6 Step 1 (Expand, deterministic) — walking-skeleton passthrough.
//!
//! Per `design/build-foundation.md` §A4 and `design/pipeline.md` §Phase 6, Step 1 is
//! deterministic. The slice 1 walking skeleton has no calls, no `with` modifiers, and
//! no parameter slots, so Step 1 reduces to a pass-through on the IR — Emit will read
//! the resolved-but-unmodified text directly.
//!
//! This module exists as a named seam so future slices can plug in resolution, slot
//! tagging, and projection-tier assignment without restructuring the pipeline.

use crate::ir::IrArena;

pub fn expand_step1(arena: IrArena) -> IrArena {
    // Walking skeleton: no transformations. Constraint and Step text on IR nodes is
    // already resolved (text refs were inlined during Lower).
    arena
}
