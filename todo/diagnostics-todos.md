# Diagnostics — TODOs and Deferred Work

Work-tracking items extracted from `design/diagnostics.md` during the design-folder reorg (2026-05-13). These are not stable contracts — they are gaps, deferred features, and forward-compat slots.

## Known Gaps (MVP)

### `G::parse::malformed-output-target` — unterminated descriptive form silently passes

An unterminated descriptive output-target form such as `<"…<EOF>` currently emits no structured diagnostic. The tokenizer's `UnterminatedString` path falls through silently rather than surfacing as a parse-error.

**Intended behavior:** the tokenizer should raise a generic `UnterminatedString` parse-error before the `malformed-output-target` rule is reached. Promoting that path to emit a structured diagnostic is a follow-up.

**Regression fence:** the `descriptive_form_unterminated_produces_no_structured_diagnostic` integration test pins the current silent behavior. When the gap is closed, the test must flip from pinning silence to asserting the structured diagnostic.

## RETIRED Diagnostics (Forward-Compat Slots)

Two diagnostic IDs were retired post-Phase-1 but reserved for forward-compat. They are not emitted today; their roles are covered by other diagnostics:

- `G::expand::missing-instructions` — RETIRED. Role covered by `extra-h2` and body H2 count checks (the `## Instructions` wrapper is gone).
- `G::expand::extra-h3` — RETIRED. With body sections at H2, the only legal H3 is `### Procedure: <name>` (which has its own dedicated diagnostics).

These IDs are kept in the catalog so they cannot be accidentally reassigned. Either delete them once the forward-compat horizon is clear, or repurpose them via the documented "deprecate and replace" rule.

## Deferred Repair Extension — Type Description Coherence

No compiler code exists for these yet. They are speculative diagnostic IDs reserved for a deferred Repair extension. All three are gated on the presence of a `type Foo = <"...">` declaration in the same compilation unit; without an anchor, the check does not fire. Each carries an author escape hatch via the line comment `// glyph-allow: <short-id>` (the short ID is the segment after `G::repair::`).

| ID | Classification | Trigger |
|---|---|---|
| `G::repair::type-description-conflict` | repairable | A `type Foo = <"X">` decl and a per-param `: Foo = <"Y">` description coexist; LLM judges whether `Y` specializes or contradicts `X` |
| `G::repair::default-violates-type-description` | repairable | A `type Foo = <"X">` decl and a `: Foo = literal` default coexist; LLM judges whether the literal satisfies `X` |
| `G::repair::return-description-conflict` | repairable | A `type Foo = <"X">` decl and a `-> Foo` block with `return <"Y">` coexist; LLM judges whether `Y` is consistent with `X` |

When this feature ships, move these entries to [[docs/reference/diagnostics]] and document the full contract (input shape, LLM judging protocol, escape-hatch comment syntax) in [[docs/architecture/repair]] or a dedicated section.

## Future Formalization (Possible)

The Repair pass internally distinguishes deterministic auto-fixes from LLM-assisted fixes. This is currently an implementation detail of the repair loop, not part of the diagnostic shape. If a future need arises to expose this distinction (e.g., for IDE quick-fix UX that wants to mark "instant" vs "LLM-bounded" fixes), it should be added as a separate field on `Diagnostic`, not by overloading `classification`.

This is a speculative item — no work is scheduled. Listed here to record the consideration so it isn't re-litigated from scratch.
