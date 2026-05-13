# IR Semantics — TODOs

Work-tracking items extracted from the previous [[ir-and-semantics]].

## Effects gate is temporary

The entire effects subsystem (parsing, inference, validation, repair
auto-fill, output emission) is gated behind `--enable-effects` (default
off). When the flag is off the parser rejects any `effects:` sub-section
with `G::parse::effects-disabled` (error). The gate is temporary until
effect inference can handle skills without a call graph (cross-reference
the entry in [[todo]]).

Lift the gate once inference is robust on call-graph-less skills.

## Per-call effect annotations (deferred)

MVP does not support attaching an `effects:` clause to an individual call
site. Effects are declared only at the declaration level (`skill`, `block`,
`export block`); call-site effects are inferred and stored on the `Call`
IR node by the compiler, not author-writable.

Adding per-call effect annotations later is backwards-compatible. Open a
follow-up issue if/when call-site effect declarations become useful.
