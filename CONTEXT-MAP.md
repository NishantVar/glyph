# CONTEXT-MAP.md

Mandatory lightweight context index for the Glyph repo. Every agent uses this as
the one lookup path for semantic context: which context a path belongs to, who
owns it, where its glossary lives, and where its ADRs go. Map presence alone does
not imply multiple contexts; the entries below define the topology. Context
resolution is path-first — the most specific path match wins, with the default
context catching unmatched paths.

## Default Context
- default: true
- scope: whole repo unless a more specific context is added later
- paths: /**
- primary owner: Language Designer (language-designer-agent)
- glossary: GLYPH_LANGUAGE_GUIDE.md (current language/vocabulary contract);
  docs/glossary.md is the future dedicated home if a separate glossary is needed
- compatibility context: no current CONTEXT.md shim
- ADRs: docs/adr/ (existing; numbered immutable records)
- ADR state: ADRs present (0001+)

Add more entries only when repo evidence shows a real semantic boundary
(different vocabulary, ADR authority, owner authority, service/package
lifecycle, or existing local context artifacts). Folder categories alone are not
enough. The named context owner keeps an entry accurate over time.

Note: `.agents/**` is Glyph product/dogfood artifact surface; the visible
`agents/**` tree is the Flux org scaffold. They are separate contexts of
authorship even though both fall under the default context for routing.
