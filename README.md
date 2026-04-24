# Glyph

A human-readable, visualizable DSL for authoring reusable agent skills that compiles into explicit, task-specific instructions for general-purpose agents, especially current coding agents.

## The Problem

Agent skills today are written as long, unstructured prompt text. They're hard to read, hard to reason about, hard to reuse, and impossible to visualize. As skills grow in complexity, prompt-based authoring breaks down.

## The Idea

**Write skills like code. Compile them for agents.**

Glyph separates the authoring form (optimized for humans) from the execution form (optimized for agents). You write structured, readable skill definitions. A compiler turns them into flatter, more explicit, agent-optimized instructions.

This is not a prompt template system. It's a language with a compiler.
Glyph is a language for skill definition and compilation, not a runtime for long-lived agent orchestration.

## Example

```
skill implement_feature(scope, framework, risk="medium") -> result
    constraints:
        must avoid_unrelated_edits
        must preserve_existing_patterns
        must validate_before_success

    flow:
        ctx = inspect_repo(scope)
        plan = make_plan(ctx)
        apply_changes(plan)
        validate(plan, risk)
        return summarize(plan)
```

## Key Properties

1. **Human-readable like code** -- Skills look like small structured programs, not prose. Hierarchy, flow, and constraints are obvious at a glance.

2. **Skill-oriented** -- The unit of abstraction is a skill, not a model call. Skills have parameters, sub-blocks, control flow, constraints, validation, and output contracts.

3. **Separate authoring from execution** -- The source is for humans. The compiled output is for agents. The compiler inlines, resolves defaults, removes irrelevant branches, expands constraints, and generates target-specific instructions.

4. **Visualizable** -- Skills can be viewed as code, as a graph/workflow, or as compiled agent output. Structured flows are easier to scan than walls of text.

5. **Small syntax** -- A limited set of primitives (`skill`, `block`, `call`, `if`, `require`/`prefer`/`avoid`/`must`, `return`) keeps things expressive yet constrained.

6. **Hybrid compilation** -- Deterministic parsing, validation, and normalization combined with LLM-assisted semantic expansion where needed. Compiles through an intermediate representation (IR).

7. **Modular and testable** -- Skills can compose from smaller blocks. Structure encourages breaking skills into pieces that can be tested individually -- running a block against sample inputs, verifying its compiled output, or validating constraints in isolation. Not everything will be modular, but the language makes it possible where it matters.

8. **Agent reliability first** -- The compiled output prioritizes concrete, followable instructions over elegance. Repetitive and explicit beats concise and ambiguous.

## Architecture (Planned)

```
Source (.glyph.md)
  -> 1. Parse      (deterministic)
  -> 2. Analyze    (deterministic)
  -> 3. Repair     [LLM, bounded loop]
  -> 4. Lower      (deterministic)
  -> 5. Validate   (deterministic)
  -> 6. Expand     [LLM, per-invocation]
  -> 7. Emit       (deterministic)
Output (.md)
```

A 7-phase hybrid compiler with a "Safety Sandwich" pattern -- deterministic passes bound the two LLM-assisted passes (Repair and Expand) to maintain reliability. See [design/pipeline.md](design/pipeline.md) for the canonical reference.

## How It Differs

| System | Focus | Glyph's difference |
|---|---|---|
| DSPy | Optimizing LLM pipelines via signatures/modules | Glyph targets skill *authoring* and *visualization*, not pipeline optimization |
| LangGraph | Stateful graph execution for multi-step agents | Glyph is a *language* that compiles to instructions, not a runtime |
| Prompt templates (Jinja/Handlebars) | String interpolation for prompts | Glyph has real structure: control flow, constraints, typed parameters |
| LMQL/Guidance/SGLang | Constrained generation at inference time | Glyph operates at the skill-definition layer, not the generation layer |
| CrewAI/AutoGen | Multi-agent orchestration | Glyph centers skill authoring and compilation first; coordination may exist around it later, but orchestration is not the primary abstraction |

## Status

Early research and design phase. The `research/` directory contains the founding design vision and exploration of the design space across syntax, IR, compiler architecture, visualization, and 15+ existing systems.

## License

TBD
