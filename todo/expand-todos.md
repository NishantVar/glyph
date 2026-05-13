# Expand — Outstanding Work

Deferred Phase 6b and Expand-related items extracted from [[docs/architecture/expand]]. These are work-tracking notes, not design decisions.

## Phase 6b Validation — Deferred Checks

From [[docs/architecture/expand]] §4.3:

- **Full Markdown well-formedness via a real Markdown parser.** Today Phase 6b uses lightweight structural checks plus `G::expand::malformed-markdown`. A pull-parser-based pass would catch a broader class of malformation but is not required for MVP.
- **No-embedded-HTML scan.** A scan rejecting raw HTML in Step 2 output is deferred. False-positive risk on legitimate constraint prose mentioning HTML tags is real, and consuming LLMs treat the file as text. Worth revisiting if HTML leakage shows up in practice.
- **Predicate-framing verbatim check.** From [[docs/architecture/expand]] §3.3: the pure-predicate Branch framing sentences (`Decide whether <…> applies and, if so:` / `Decide which of the following applies and follow only that path:` / `Otherwise:`) are not checked for verbatim match today. If drift becomes a problem, add a structural check keyed on `resolved_predicates` shape.

## Step 2 — Open Questions

(None currently extracted beyond the deferred 6b checks above. Add new TODOs here as they come up.)
