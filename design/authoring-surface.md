# Glyph Authoring Surface

This document records the current design direction for how people write Glyph source files. It is about the human-facing language surface, not the final compiled agent instructions.

## Goals

Glyph source should optimize for:

- **Easy readability.** A reader should be able to scan a skill and see its structure, flow, constraints, and reusable instruction names quickly.
- **Easy maintenance.** Repeated instruction text should be named once, imported where needed, and expanded by the compiler instead of copied through many skills.
- **Forgiving authoring.** The authoring surface may be duck-typed and partially inferred. The compiler is responsible for turning that convenient source into a stricter IR.

The important split is: source can be ergonomic; IR and compiled output must be explicit.

If ergonomic source does not compile directly, Glyph may run an LLM repair pass that rewrites it into valid Glyph source while preserving shorthand and readability. That pass fixes compiler-blocking issues; it does not expand shorthand into prose or produce agent-facing output.

## Source Files And Compiled Output

Glyph source modules should use the `.glyph.md` extension.

The MVP compiler target is Markdown agent-instruction output. A single `.glyph.md` source file compiles to exactly one Markdown file by replacing the `.glyph.md` suffix with `.md`; for example, `skill.glyph.md` compiles to `skill.md`, and `x.glyph.md` compiles to `x.md`. Multi-file compiled output may be added later, but is not part of the MVP. A typed IR or JSON form may exist between source and output, but it is an internal compiler stage rather than the main user-facing artifact.

The minimum compiled Markdown shape is:

```md
---
name: <skill-name>
description: <when this skill should be used>
---

## Effects

<What the skill reads, writes, runs, or otherwise touches. Omitted when effects: none.>

## Inputs

<Expected inputs, scope, assumptions, or required context.>

## Instructions

<Compiled behavior, constraints, and flow.>

## Output

<What the agent should produce or report.>

## When To Use

<Detailed trigger guidance when description is not sufficient. Omitted when description is enough.>
```

`description` follows the existing skill frontmatter convention and owns the normal "when to use" role for current coding agents. A separate `## When To Use` section should be optional and emitted only when the source explicitly contains trigger guidance that does not fit cleanly in the frontmatter description.

## Source To IR Contract

Glyph source may contain shorthand, omitted annotations, text aliases, imported names, and inline natural-language instructions. These are authoring conveniences.

The compiler must resolve them before Markdown output generation:

1. Parse `.glyph.md` source into a loose source AST.
2. Run deterministic diagnostics for syntax, name resolution, role inference, constraint attribute inference, and type inference.
3. Run source-preserving LLM repair if deterministic passes report repairable diagnostics.
4. Re-parse and re-check the repaired source.
5. Resolve local names, imported libraries, and known standard names.
6. Infer instruction role, constraint strength, and constraint polarity when the source omits them.
7. Resolve generated definitions and deterministic semantic shortcuts. Undefined bare names should be materialized as stable generated definitions during the MVP repair pass rather than repeatedly guessed later.
8. Normalize values, instruction text, blocks, calls, and constraints into explicit IR nodes.
9. Type and validate the IR before producing the compiled Markdown output.

This lets Glyph feel closer to Python-style duck typing while preserving the analyzability needed for visualization, validation, and reliable compiled instructions.

## MVP Top-Level Declarations

The MVP source language should support these five top-level declarations:

- `import` for bringing in exported declarations from other `.glyph.md` files.
- `text` for reusable named instruction text, with `export text` as the importable variant.
- `export block` for importable, self-contained reusable blocks.
- `block` for private helper blocks inside the current file.
- `skill` for the public task definition that compiles to Markdown agent instructions.

Each MVP `.glyph.md` source file must contain exactly one `skill`. It may also contain imports, text declarations, exported text declarations, private blocks, and exported blocks that support that skill. This is the MVP declaration set, not the permanent ceiling. Later design may add declarations such as `agent`, `abstract agent`, or `trait`, but those additions should not weaken the closure rule for importable blocks.

## Authoring Forms

Within those declarations, Glyph should support at least five authoring forms.

### 1. Language Primitives

Authors can build skills out of defined primitives such as `skill`, `export block`, `block`, `flow`, `call`, `if`, and `return`. `if` is required in the MVP. `for_each` is useful but deferred beyond the MVP. Explicit role and constraint markers may exist as disambiguators; the MVP role vocabulary is defined in [ir-roles.md](ir-roles.md).

Example:

```glyph
skill update_docs
    preserve_existing_patterns

    flow:
        inspect_docs()
        return summarize_changes()

skill implement_feature(scope, risk = "medium")
    preserve_existing_patterns
    validate_before_success

    flow:
        ctx = inspect_repo(scope)
        plan = make_plan(ctx, risk)
        apply_changes(plan)
        validate(plan)
        return summarize(plan)
```

Primitive expansion should be mostly deterministic. A `flow` becomes an ordered sequence and a `return` becomes an output contract in the compiled form. When authors use explicit role or constraint markers, those markers directly set the IR role, strength, or polarity; otherwise the compiler infers them.

Function-like calls may pass variables and bind return values. The detailed contract is defined in [data-flow-and-calls.md](data-flow-and-calls.md).

`skill` is the user-facing unit that compiles into Markdown instructions. `export block` is the importable block form and must be self-contained. Ordinary `block` declarations are private to the current file.

Skill parameters are optional. A skill may declare no parameters, normal invocation parameters, or selected global preference parameters resolved from the configured user/project preference store. In the MVP, global preferences resolve at compile time and should compile as explicit skill inputs or instructions, not as hidden global state. Runtime preference injection through a loader or hook may be added later.

Private `block`s may be top-level declarations or nested inside a `skill`, Python-style, when nesting improves readability. Private blocks may rely on their enclosing skill context in the MVP. The exact static analysis model for that context dependency is intentionally left for later design.

## Inferred Instruction Roles

Source instructions should not need to carry compiler-shaped keywords everywhere. A bare name or inline string can compile into an inferred IR role depending on context and metadata.

Bare instruction names are allowed in the MVP. If a bare name is undefined, the MVP repair pass may use an LLM to materialize a stable generated definition for that name while preserving the bare-name use site. The generated definition becomes the deterministic local meaning for later compiler passes.

The closed MVP role set is defined in [ir-roles.md](ir-roles.md): `InputContract`, `Step`, `Constraint`, `Context`, and `OutputContract`. Hard positive rules, prohibitions, and preferences are all `Constraint` nodes with separate strength and polarity attributes rather than separate roles.

Example:

```glyph
skill fix_bug(scope)
    avoid unrelated_edits
    require preserve_existing_patterns
    output explain_tradeoffs

    flow:
        inspect_failure(scope)
        identify_root_cause()
        patch_minimally()
        validate_before_success
```

Possible normalized IR roles:

```text
Constraint(strength: required, polarity: avoid, text: unrelated_edits)
Constraint(strength: required, polarity: require, text: preserve_existing_patterns)
OutputContract(explain_tradeoffs)
Step(inspect_failure(scope))
Step(identify_root_cause())
Step(patch_minimally())
Step(validate_before_success)
```

Recommended source markers are:

```text
input     -> InputContract
output    -> OutputContract
flow      -> contains Step nodes
always    -> Constraint(strength: invariant, polarity: require)
require   -> Constraint(strength: required, polarity: require)
avoid     -> Constraint(strength: required, polarity: avoid)
prefer    -> Constraint(strength: preferred, polarity: require)
```

Composed constraint markers such as `always avoid` and `prefer avoid` set
constraint strength and polarity without adding new roles.

`context` may be available as an author-facing disambiguator, but authors should
not need to write it most of the time. Clearly non-normative informational text
may become `Context` in the IR or in an intermediate repaired form.

Inference inputs should include:

- Position in the skill, such as before `flow`, inside `flow`, or near `return`.
- Metadata from same-file bindings, imports, and standard-library entries.
- The natural meaning of the expanded instruction text.
- Explicit keywords when the author chooses to disambiguate.

If the compiler can infer a missing marker confidently, the repair pass should add the smallest explicit marker back into source before strict IR compilation. `require`, `avoid`, and `prefer` may be inferred when evidence is clear. Compound names such as `avoid_unrelated_edits` should repair to marker-plus-concept form such as `avoid unrelated_edits` and notify the author. `always` should be inferred only from invariant-level wording or trusted metadata because it means the strongest constraint strength and should stay rare. If the compiler cannot infer the role, strength, or polarity confidently, it should emit a diagnostic and ask the author to add an explicit marker or move the instruction into a clearer section.

### 2. Same-File Text Blocks

Authors can bind a name to reusable instruction text in the same file. Later uses of the name expand to that text.

Example:

```glyph
text preserve_existing_patterns = """
Prefer the repository's existing patterns, helper APIs, naming, and file organization
before introducing a new abstraction or style.
"""

skill implement_feature(scope)
    preserve_existing_patterns
```

These bindings are not arbitrary string interpolation. The compiler should treat them as named instruction resources and place them into the IR as resolved instruction content.

Text declarations are private by default. A text declaration must use `export text` before another `.glyph.md` file can import it.

### 3. Imported Glyph Modules And Libraries

Authors can define reusable text blocks, instruction names, or exported blocks in another `.glyph.md` file and import them.

MVP imports should be path-based. Package names, registries, and versioned imports are deferred, but the import model should leave room for them later.

Whole-module imports are supported in MVP and should allow aliases. A whole-module import exposes the source module's public skill entrypoint plus its explicitly exported `text` and `export block` declarations. Private `text` and ordinary private `block` declarations remain inaccessible.

Illustrative whole-module import:

```glyph
import "./repo_tools.glyph.md" as repo_tools

skill fix_bug(scope)
    avoid repo_tools.unrelated_edits
    require repo_tools.preserve_existing_patterns

    flow:
        ctx = repo_tools.inspect_repo(scope)
        repo_tools.validate_changes(ctx)
```

Named imports are also supported for explicitly exported declarations and should allow aliases.

Illustrative named import:

```glyph
import "./coding_agent_safety.glyph.md" {
    unrelated_edits,
    preserve_existing_patterns as existing_patterns,
    validate_before_success,
}

skill fix_bug(scope)
    avoid unrelated_edits
    require existing_patterns

    flow:
        validate_before_success
```

Imports let teams maintain shared vocabularies without repeating prose. Imported names should resolve deterministically from the referenced source path in the MVP. Later package-style imports may add registry and version resolution.

Only explicitly exported declarations should be importable. In MVP, imported blocks must be declared as `export block`, and imported text must be declared as `export text`, in their source file. Ordinary `block`s and non-exported `text` declarations are private implementation details and should be rejected if another file tries to import them.

The source module's `skill` is its public compiled entrypoint. Whole-module imports may reference that entrypoint so one skill can invoke another module's skill behavior, but the imported skill still compiles to its own same-basename Markdown file when compiled directly.

An exported block must be closed: it may depend only on its parameters, local bindings, explicit imports, same-file reusable text, standard primitives, declared constraints, declared outputs, and declared effects. Closed does not mean pure; an exported block may read files, write files, run tools, or produce artifacts if those effects are declared.

An `export block` may call imported `export block`s. It may also call private blocks from the same source file if the compiler can prove those private blocks are closed under the exported block's declared contract.

Every `export block` must end in an explicit `return`. Instruction-only exported blocks are allowed, but should still return `none` so callers have a clear contract.

Circular imports should be rejected in the MVP.

Example:

```glyph
export block inspect_failure(scope) -> FailureReport
    effects: reads_files, runs_commands

    flow:
        reproduce(scope)
        collect_logs(scope)
        return failure_report()
```

The initial effect vocabulary should stay small and extensible. MVP effects include `none`, `reads_files`, `reads_env`, `writes_files`, `runs_commands`, `uses_network`, `asks_user`, and `creates_artifacts`; future versions may add more specific effects through the additive-only extension policy without changing the rule that importable blocks must declare meaningful effects.

### 4. Semantic Shortcuts

Authors can write a small function-like or identifier-like instruction directly in the skill when the name is instructive enough to expand.

Example:

```glyph
skill debug_failure(scope)
    root_cause_before_fix
    reproduce_before_patch
    root_cause_trace()
```

Resolution order should be:

1. Same-file binding.
2. Explicit import.
3. Standard library vocabulary.
4. MVP repair materializes a stable generated definition for an unresolved bare name when the name and surrounding context make the intended meaning clear enough.

LLM-assisted expansion for undefined bare names happens during the MVP repair pass, not at runtime. Repair materializes a stable generated definition that is cached in source or otherwise reviewable and must be validated before compilation continues. Later semantic expansion resolves from that definition instead of regenerating prose from the bare name on every compile.

### 5. Inline Instructions

Authors can place one-off instruction text inline, likely with quoted strings for short cases and block strings for longer cases.

Example:

```glyph
skill update_docs(scope)
    "Do not change public behavior while updating documentation."

    flow:
        inspect_docs(scope)
        apply_doc_changes()
        "Mention any docs you could not verify locally."
```

Inline text is useful for one-off details that do not deserve a shared name. If the same text appears repeatedly, it should be promoted to a same-file text block or imported library entry.

## Maintenance Rules

- Prefer named text blocks for repeated instruction text.
- Prefer imports for team-wide or project-wide instruction vocabulary.
- Use path-based imports in the MVP; defer package-style and versioned imports.
- Import only explicitly exported declarations; keep ordinary `block`s and non-exported `text` private to their source file.
- Use aliases when an imported module or declaration would otherwise collide or read poorly.
- Make every `export block` self-contained by declaring its inputs, outputs, constraints, dependencies, and meaningful effects.
- Use semantic shortcuts when the name itself communicates the intended behavior clearly.
- Use inline quoted instructions for local, one-off guidance.
- Use explicit role or constraint markers only when inference would be unclear or when the author wants to override the default role, strength, or polarity.
- Prefer canonical marker-plus-concept source such as `avoid unrelated_edits` over compound names such as `avoid_unrelated_edits`; repair may normalize compound names and notify the author.
- Avoid ad hoc string concatenation; instruction content should resolve into structured IR nodes.
- The compiler should surface unresolved or ambiguous names as diagnostics rather than silently guessing when confidence is low.

## Open Syntax Choices

The semantic commitments above are stronger than the exact syntax. These details can still change:

- Whether text bindings use `text name = ...`, `let name = ...`, or another keyword.
- Whether semantic shortcuts use bare identifiers, function-like calls, or both.
- The exact quote style for inline and multiline instruction text.
- The exact spelling of role and constraint source markers.
- The exact path import syntax for whole-module imports, named imports, and aliases.
- How package-style, registry-backed, or versioned imports should work after the MVP.
