// Expand pass — Step 2 LLM responsibilities for the Glyph compiler.
// Encodes llm_expand_pass.md: per-node prose generation, whole-skill
// calibration, and the discipline rules a span-filling LLM must follow.

skill expand_prose(resolved_ir: ResolvedIR, scaffold: ScaffoldedMarkdown)
    description: "Fill marked prose spans in a scaffolded compiled skill file using its resolved IR, preserving every deterministic structure the emitter laid down."

    context:
        scaffold_authority
        whole_skill_view

    must preserve_param_refs
    must concise_step_prose
    must concise_constraint_prose
    must avoid invented_param_refs
    must avoid extra_or_reordered_content
    must avoid leaked_modifier_text
    must avoid surviving_local_refs
    must avoid leaked_output_target_tokens
    must avoid authoring_artifacts
    must avoid frontmatter_or_commentary
    require single_clause_param_description
    avoid html_tables_or_code_blocks

    flow:
        "Read the resolved IR from {resolved_ir} and the scaffolded markdown from {scaffold}; you will reshape only the marked prose spans, leaving every other structural element untouched."
        weave_site_modifiers()
        weave_scoped_constraints()
        resolve_local_binding_refs()
        paraphrase_description_outputs()
        prose_branch_conditions()
        write_param_descriptions()
        calibrate_across_steps()

        return <"the scaffolded compiled file with every marked prose span filled">


block weave_site_modifiers()
    """
    For each Call carrying a site_modifier (the `with "..."` clause), fold the
    modifier's intent into the Step's prose. The literal modifier string must
    never appear verbatim in the output — it is consumed by being woven in.
    """

block weave_scoped_constraints()
    """
    For each Call whose scoped_constraints field is non-empty, either fold each
    constraint into the Step's prose or prepend a localized framing sentence
    that scopes the constraint to the inlined region. Apply strength and
    polarity wording: hard renders as non-negotiable, soft renders as standard,
    require renders as a positive obligation, and avoid renders as a
    prohibition. Never emit a scoped constraint as a bullet under
    `### Constraints` — those are top-level only.
    """

block resolve_local_binding_refs()
    """
    For each curly-brace placeholder in a Step's resolved body whose name
    appears in the Call's local_refs array, replace the placeholder with a
    natural-language cross-reference to the producing step (for example, "the
    diagnosis from your earlier analysis" or "the diagnosis identified in
    step 1"). The literal placeholder token must not survive in the output —
    only declared-parameter references survive verbatim.
    """

block paraphrase_description_outputs()
    """
    For an OutputContract whose form is the Description variant, paraphrase
    the description into a Step-shaped sentence and fold it into the final
    Step's prose. The angle-bracket-quoted token, the surrounding angle
    brackets, and the verbatim quoted text must all be absent from the output.
    The Identifier variant is handled deterministically and is not your
    responsibility.
    """

block prose_branch_conditions()
    """
    For each Branch whose condition is a code-shaped expression (for example
    `x > 5 and not is_dry_run`), convert the expression into natural-language
    prose suitable for an `If <prose>:` arm header. Use applies_descriptions
    from the IR side-map for any embedded BLOCKNAME.applies() sub-expressions,
    weaving them into the larger condition prose. Pure-applies() Branches and
    the `Otherwise:` arm header are emitted deterministically and are not your
    responsibility.
    """

block write_param_descriptions()
    """
    For each Param in the skill's InputContract, generate a brief description
    from the parameter's name, type, default value, and how it is referenced
    in the body. Fill only the prose slot inside the deterministically-
    scaffolded `## Parameters` bullet — the bold name, the type fragment, and
    the `(default: ...)` or `(required)` trailer are not your responsibility.
    """

block calibrate_across_steps()
    """
    Adjust wording so consecutive Steps and constraints read as a connected
    workflow rather than isolated sentences, using the visibility you have
    over the full resolved IR. Do not violate any preservation, length, or
    no-invention rule listed in the Constraints section.
    """


const scaffold_authority = """
The deterministic emitter owns section headings, numbered Step ordering,
sub-step lettering, `### Context` bullets, InlineInstruction and
InstructionRef text, pure-applies() decision-frame headers, the
`If <condition>:` and `Otherwise:` arm structure, the `## Parameters` bullet
structure, every `### Constraints` bullet, the locked external-file Step
template, and the OutputContract Identifier return-fold suffix. If a scaffold
span looks malformed, do not edit it — that is a deterministic-emitter bug
and Phase 6b will flag it.
"""

const whole_skill_view = """
You see the full resolved IR for the skill in a single prompt. Use that
visibility to calibrate prose so the sequence reads as a single connected
workflow, not as isolated sentences.
"""

const preserve_param_refs = """
Every parameter placeholder for a declared InputContract parameter must
survive verbatim into the output. If Step 1's resolved body contained a
parameter placeholder, re-introduce it in the output prose — silent dropping
is forbidden.
"""

const invented_param_refs = "Writing a parameter placeholder for any name not declared in the skill's InputContract."

const extra_or_reordered_content = "Adding, merging, splitting, or reordering Steps, sub-steps, constraints, sections, or commentary relative to the IR's flow order."

const leaked_modifier_text = "Quoting a `with` modifier string verbatim in the output — modifiers must be consumed by being woven into prose, never echoed."

const surviving_local_refs = "Leaving any local_ref placeholder token unresolved in the compiled output."

const leaked_output_target_tokens = "Letting an output-target token, its surrounding angle brackets, or its verbatim quoted text survive in the compiled Markdown."

const authoring_artifacts = "Letting `generated` markers, import paths, IR field names, IR node IDs, or raw condition expressions appear in the output."

const concise_step_prose = "Each non-conditional Step and each Branch sub-step is at most three sentences, typically one or two."

const concise_constraint_prose = "Each `### Constraints` bullet is a single sentence."

const single_clause_param_description = "A parameter description is a single short clause that complements the deterministic name/type/default fragment, not a paragraph."

const html_tables_or_code_blocks = "Using HTML, tables, or fenced code blocks inside a Step's body — inline emphasis is fine, but structural Markdown is the deterministic emitter's job."

const frontmatter_or_commentary = "Emitting YAML frontmatter, JSON, IR, or commentary in the output channel — the output is Markdown text only."
