# Output Target Expression Draft

## Problem

Glyph blocks read like functions: they take parameters, run a `flow:`, and may declare a return type such as `-> Confirmation` or `-> BranchName`.

Current examples sometimes express the returned value as a placeholder string:

```glyph
block ask_user_to_confirm(candidates) -> Confirmation
    flow:
        "Show the candidate list and ask for confirmation."
        return "<boolean confirmation>"
```

That is semantically wrong. The return expression is a string literal, not a confirmation value, and `<...>` is only an informal convention inside the string.

The source needs a visually distinct way to say: "the agent must synthesize this output value."

## Proposed Answer

Formalize an **output target expression** in two forms — identifier and descriptive:

```glyph
return <confirmed>                                          // identifier form
return <"whether the user confirmed the candidate list">    // descriptive form
```

or:

```glyph
return <current_branch>
```

Meaning:

- `<name>` is not a string.
- `<name>` is not Markdown.
- `<name>` is not interpolation.
- `<name>` is an agent-produced output target (identifier form).
- `<"description">` is a quoted descriptive string that tells the agent what to synthesize (descriptive form).
- In `return <name>` or `return <"description">`, the output type comes from the enclosing declaration's return annotation.

The `-> DomainType` on the header serves as the compiler contract (nominal matching). The `<"description">` serves as agent guidance (what to synthesize). These are complementary:

```glyph
export block diagnose_issue(scope) -> Diagnosis
    flow:
        inspect_repo(scope)
        return <"root cause analysis including affected files and severity">
```

The important design point is broader than `return`: `<name>` names a value the agent is expected to produce from instructions. `return <name>` is the smallest and most obvious use because return statements are where the ambiguity first appeared.

Example:

```glyph
block ask_user_to_confirm(candidates) -> Confirmation
    flow:
        "Show the candidate list and ask for confirmation. Treat only an unambiguous affirmative as true."
        return <confirmed>

block read_current_branch(repo_path) -> BranchName
    flow:
        "Run `git rev-parse --abbrev-ref HEAD` in {repo_path} and capture the branch name."
        return <current_branch>
```

## Name Forms

This gives Glyph four visually distinct name forms:

```glyph
{name}              // prose slot or reference inside instruction text
name                // ordinary identifier resolving to an existing value or declaration
<name>              // output target the agent must produce (identifier form)
<"description">     // output target with descriptive guidance (descriptive form)
```

This distinction is the core value of the design.

## Broader Concept

The construct is not fundamentally "return syntax." It is a way to mark an **agent-produced value** in source.

Glyph already has normal dataflow for values produced by calls:

```glyph
selected_candidate = ask_user(candidates)
```

That should remain the preferred form whenever there is a real producer expression.

The gap appears when the producer is a prose instruction, a judgement, a human interaction, an extraction, or a synthesis task. In those cases, the source needs a way to name the value without pretending it already exists.

Examples of values that may need output-target syntax:

- a confirmation decision produced after asking the user;
- a current branch name extracted from command output;
- a diagnosis synthesized from repository inspection;
- a classification label chosen from evidence;
- a report or summary assembled from multiple prior steps;
- an artifact identity or handle produced by an instruction.

`return <name>` is one use of that broader concept: the final output target.

## Why Not Bare `return current_branch`

Bare names already mean ordinary identifier references. They should resolve to existing bindings, parameters, imports, or declarations.

`return current_branch` is visually ambiguous: it looks like returning a value that already exists.

`return <current_branch>` makes the agent-produced nature of the value visible in source.

## Why Not `return output.current_branch`

`output.current_branch` can work as a pseudo-namespace design, but it adds an abstract compiler-known object named `output`.

`<current_branch>` is more direct. It marks the value itself as an output target without introducing a pseudo-binding.

## Why Not Keep Placeholder Strings

```glyph
return "<current_branch>"
```

This must remain a string literal. It should not mean "return the current branch." When the return type is a domain type, this should become a repairable diagnostic that suggests:

```glyph
return <current_branch>
```

## Proposed Rules

- **Identifier form:** Syntax is exactly `<IDENTIFIER>`.
  - No spaces: `<current_branch>` is valid; `<current branch>` is not.
  - The target name should not collide with an existing visible value. If `name` is already bound, use `return name`.
  - `return <Diagnosis>` or other type-looking names should be diagnostic-worthy; output target names should follow value naming conventions such as `snake_case`.
- **Descriptive form:** Syntax is `<"quoted string">`.
  - The string describes what the agent should synthesize as the return value.
  - Descriptive form is **terminal-return-only in MVP**. Mid-flow output targets (if added later) should use the identifier form, not the descriptive form.
- No expressions: `<foo()>` and `<a.b>` are invalid.
- Initially, allow output targets only in terminal `return`.
- `return <name>` and `return <"description">` do not introduce a normal local binding.
- The compiled Markdown should never contain the literal `<name>` or `<"description">` token. Expand/Emit should turn it into natural output prose.
- **Complementary with `-> Type`:** The `-> DomainType` on the header is the compiler contract (nominal matching at call boundaries). The `<"description">` is agent guidance (what to synthesize). Both may be present on the same block.

## Semantics

`return <current_branch>` lowers to an output contract, not a literal value:

```text
OutputContract {
  target_name: "current_branch",
  type: "BranchName",
  source: "synthesized_by_agent"
}
```

For `-> Confirmation`:

```glyph
return <confirmed>
```

the compiled prose can say the final result should be whether the user confirmed.

For `-> BranchName`:

```glyph
return <current_branch>
```

the compiled prose can say the final result should be the current branch name.

For the descriptive form, `return <"root cause analysis including affected files and severity">` lowers to:

```text
OutputContract {
  description: "root cause analysis including affected files and severity",
  type: "Diagnosis",
  source: "synthesized_by_agent"
}
```

## Relationship To Bindings

Normal bindings remain the right answer when there is a real producer expression:

```glyph
selected_candidate = ask_user(candidates)
apply_candidate(selected_candidate)
return selected_candidate
```

The output target form is for values produced by prose-guided agent work rather than by a callable expression:

```glyph
flow:
    "Inspect the repository and identify the current branch."
    return <current_branch>
```

This is the use case that placeholder strings were trying to express.

## Future Use Cases Beyond Return

The same concept could support agent-produced intermediate values, but that should be designed deliberately.

Possible future shape:

```glyph
flow:
    "Inspect the repository and produce a concise diagnosis." -> <diagnosis>
    choose_fix(<diagnosis>)
    return <fix_summary>
```

or:

```glyph
produce <diagnosis>
```

These should not be added casually. Once `<name>` appears outside `return`, it becomes part of dataflow and the compiler must track where each output target is introduced and consumed.

For now, the conservative rule is:

```glyph
return existing_binding      // return an existing value
return some_call()           // return a callee result
return none                  // no meaningful value
return <output_name>         // synthesize and return a named output (identifier form)
return <"what to produce">   // synthesize with descriptive guidance (descriptive form)
```

## Design Work Needed

If this is promoted into the main design, update:

- `design/values-and-names.md` — define `<IDENTIFIER>` and distinguish `{name}`, `name`, and `<name>`.
- `design/data-flow.md` — add `return <name>` to return semantics.
- `design/ir-schema.md` and `design/ir-json-schema.md` — add an `OutputTarget` / `OutputTargetExpr` variant.
- `design/compiled-output.md` — define return folding for output targets.
- `design/expand.md` — ensure Step 2 turns output targets into natural output prose and does not preserve `<name>`.
- `design/repair.md` — repair placeholder-string returns to output targets where appropriate.
- `design/diagnostics.md` — add diagnostics for malformed output targets and placeholder-string returns.
- `design/types.md` — state that `return <name>` inherits type from the enclosing return annotation.
