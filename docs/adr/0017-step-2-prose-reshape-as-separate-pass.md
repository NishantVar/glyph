# ADR 0017: Step 2 Prose Reshape as a Separate Pass

## Status

Accepted (v0). Orchestrator will renumber if a sibling ADR uses this slot.

## Context

Glyph compiles `.glyph` source to Markdown that a coding agent later reads as a skill. The Markdown has two kinds of content:

1. **Mechanically fixed structure.** YAML frontmatter, peer-level H2 headings (`## Parameters`, `## Context`, `## Steps`, `## Constraints`), numbered Step lists, the locked four-form constraint template, `### Procedure: <name>` sections, and `{param}` references. These shapes are dictated by the IR and must not vary.
2. **Prose the agent reading the skill needs to be *good*.** Parameter descriptions, Step bodies for `Call` nodes that expand a callee's body into natural language, `with` modifiers woven into Step wording, mixed-condition Branch headers, and `Description`-form return folds. Mechanically-emitted prose here reads as machine-generated and degrades the skill's usefulness.

If the compiler tries to do both — emit structure *and* write good prose — it either (a) ships a templating system that produces stilted output, or (b) embeds an LLM client and loses its determinism guarantee (see ADR 0014).

If the agent does both — generates structure *and* prose from the IR — the structural contract becomes a paraphrasing target. Empirically: LLMs renumber lists, drop sections, invent parameter references, and rephrase locked constraint templates when asked to "produce the compiled Markdown."

## Decision

Split Phase 6 Expand into two passes with disjoint responsibilities:

- **Step 1 (deterministic, in compiler).** Emit `foo.md` with frontmatter, all headings, all list scaffolding, the locked four-form constraint template, pure-`applies()` Branch headers, the external-file Call Step template, the `Identifier`-form return-fold suffix, and the `## Parameters` skeleton (names, types, `(required)` markers or defaults). Where the agent is responsible for prose, the emitter inserts **typed spans** — `ParamDescription`, `DescriptionReturnFold`, `BranchCondition`, `CallBodyShape` — that mark exactly which characters the LLM may rewrite.
- **Step 2 (LLM, in agent).** Read `foo.ir.json` and `foo.md`. Rewrite span contents **in place**. Preserve every literal chunk the deterministic emitter wrote. Do not regenerate the Markdown from scratch.
- **Phase 6b (deterministic, in compiler).** `glyph validate-output foo.ir.json foo.md` re-parses the agent's output and rejects any structural drift: extra H2/H3 headings, mismatched step/constraint/sub-step counts, missing or duplicate procedure sections, invented or dropped `{param}` references, unresolved `local_ref` slots, leaked `with` modifier strings, frontmatter returned by the agent.

Step 2 retries use **revise-with-feedback**, not regenerate-from-scratch: the retry prompt includes the previous failed Markdown plus the `validate-output` diagnostics, so the LLM fixes the specific violations rather than re-paraphrasing the whole document.

## Consequences

- **The structural contract survives the LLM.** Agents cannot accidentally rename a parameter, drop a constraint, or rephrase the locked template — the validator hard-fails the build before the user sees the output.
- **The prose contract survives the compiler.** Mechanical emit doesn't have to fake natural language. The compiler emits placeholders; the agent fills them.
- **Step 2 is non-idempotent.** Re-running Step 2 on already-filled spans would re-LLM-rewrite finished prose. The agent skill must distinguish "compiler just emitted, spans empty" from "agent already filled spans" — typically by checking that span markers are still present.
- **The IR JSON is a hard contract.** Step 2 needs the IR to know which Call nodes have `same_file_procedure` projection, what `with` modifiers attach to which Steps, etc. `--emit-ir` and `validate-output` use the same schema; [[ir-json]] is the authoritative version.
- **Validation surface is large but enumerable.** 25 `G::expand::*` diagnostic IDs cover section shape, role-count preservation, procedure sections, parameter references, and content shape. Each maps to a structural property that is cheap to check from a thin line-based Markdown parser (no full CommonMark needed).
- **On exhaustion the failed Markdown stays on disk.** Two retries is the budget. After exhaustion the agent surfaces validator diagnostics to the user and leaves the last failed `foo.md` in place — the user needs to see the prose to diagnose the persistent mismatch, not the mechanical fallback.
