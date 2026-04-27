# Glyph First Principles

Candidate reduction of `foundations.md` into a smaller set of bedrock principles.
These are meant to be the reasons the rest of the language exists, not a restatement
of every current design choice.

## 1. Novice Learnability By Inspection

A complete beginner should be able to read a short GitHub introduction, see a few
examples, and write a useful Glyph skill without prior programming knowledge.
Glyph should minimize both syntax burden and conceptual burden.

## 2. Users Express Intent; Glyph Bears Formalization

Authors should mostly say what they want, what matters, and what to avoid, in terms
close to natural language. The system may infer omitted structure, but Glyph must
bear the burden of turning that intent into explicit formal structure.

## 3. Human Authoring And Agent Execution Optimize For Different Readers

Source Glyph is for humans; compiled output is for agents. The authoring form should
stay compact, intuitive, and easy to edit, while the compiled form may become flatter,
more repetitive, and more explicit if that improves agent reliability.

## 4. Inference Is Allowed At The Surface, But Semantics Must Become Explicit

Glyph may use inference, repair, shorthand expansion, and contextual guessing to help
non-expert authors write successfully. But before execution, every meaningful part of
the skill must become explicit, typed, and checkable in the IR: control flow,
constraints, inputs, outputs, and effects cannot remain implicit.

## 5. Trust Comes From Deterministic Boundaries

LLMs may assist authoring, repair, and elaboration, but they do not define the
language's semantics. Trust comes from deterministic parsing, normalization,
validation, and evaluation that bound any probabilistic step and reject ambiguous or
unsafe interpretations when confidence is too low.

## Consequences

- The core mental model must be tiny.
- Safe inference is a product requirement, not a convenience feature.
- Low-confidence interpretations need paraphrase, ranking, or clarification.
- Features that are hard to explain, visualize, validate, or test do not belong in the core language.
- Syntax choices are downstream decisions; novice success and explicit semantics are upstream constraints.
