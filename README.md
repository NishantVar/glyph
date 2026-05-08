# Glyph

<p align="center">
  <img src="assets/Glyph%20Bottom%20Bar%20README.png" alt="Glyph — A compiler for agent behavior" width="800" />
</p>

<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-orange?style=flat-square&logo=rust" />
  <img src="https://img.shields.io/badge/license-Apache%202.0-blue?style=flat-square" />
  <img src="https://img.shields.io/badge/status-early%20design-blue?style=flat-square" />
</p>

Glyph is a small language for authoring agent skills. You write structured source — parameters, constraints, control flow — and the compiler turns it into flat, explicit Markdown that agents can follow. The source form is for humans. The compiled output is for agents.

## Example

```glyph
import "./prefs.glyph" { house_style }

// const: named value (string, int, or float). Reusable across the file.
const tone = """
Be concise. Write for engineers. Use past tense.
"""

// block: private helper callable from this file.
block format_entry(change, style = "brief") -> Entry
    flow:
        "Read the full context of {change}."
        return write_entry(change, style)

// skill: public entrypoint. One per file.
// Parameters without defaults are required at runtime.
// -> ReturnType uses domain types — no String/Int, name the role instead.
skill write_changelog(scope = ".", version) -> Changelog
    description: "Generate a changelog entry for a new version."

    context: 
	    tone

    // require/avoid: soft (strong guidance). must/must avoid: hard (absolute).
    constraints:
        require house_style
        avoid marketing_language
        must "Never include changes not present in the diff."

    flow:
        changes = read_diff(scope)
        entry   = format_entry(changes)                               // block: normal call
        "Confirm the entry covers all changes in {version}."          // {x} = runtime slot, not interpolation
        if changes == "breaking":
            format_entry(changes) with "highlight breaking changes first"  // with: specializes this call site
        return write_file(entry)
```

## Skills

| Skill              | When to use                                                                                                                                                                                                                                 |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `/glyph`           | Entry point — describe what you want to do and it routes to the right sub-skill. Useful for agents that don't support slash commands.                                                                                                       |
| `/glyph:compile`   | Compile a `.glyph` source file into a finished `.md` skill — runs the full pipeline (fmt, LLM repair loop, constraint conflict scan, prose reshape, validation).                                                                            |
| `/glyph:decompile` | Convert an existing compiled `.md` skill back into a `.glyph` source file for editing.                                                                                                                                                      |
| `/glyph:teach`     | Author or edit a `.glyph` source file. Use when writing a new skill from scratch or making changes to an existing `.glyph` file.                                                                                                            |
| `/glyph:icompile`  | Apply a small targeted change to both the `.glyph` source and its compiled `.md` in tandem, without re-running the full pipeline. Use for localised wording or value swaps; fall back to `/glyph:compile` if prose needs to be regenerated. |
| `/install_glyph_editor_extension` | Build the Glyph VS Code extension and install it into every VS Code-compatible IDE detected on your machine (VS Code, Cursor, Antigravity, Windsurf, VSCodium). Idempotent — re-running uninstalls and reinstalls cleanly. |

> **Work in progress.** The compiler may surface errors or incomplete output. Your agent should be able to recover from these — treat compiler feedback as guidance, not a hard stop.

## The Five Primitives

Every construct in Glyph decomposes into one or more of five semantic primitives:

| Primitive | What it means | Where you see it |
|---|---|---|
| **Instruction** | Tell the agent to do something | Calls, inline strings, and blocks in `flow:` |
| **Constraint** | Bound the agent's behavior | `require`, `avoid`, `must`, `must avoid` markers |
| **Context** | Frame the agent's understanding | `context:` entries — informational, not directives |
| **Interface** | Define a callable's contract | Parameters, return type, `description:`, `effects:` |
| **Binding** | Introduce an addressable name | Declarations, local assignments, imports |

## Syntax Notes

- **Constraint strength has two levels.** `require`/`avoid` are soft — strong guidance. `must`/`must avoid` are hard — absolute rules. Reserve `must` for things that genuinely cannot be violated.

- **Return types are domain types, not primitives.** Write `-> Entry`, `-> Plan`, `-> BranchName`. There is no `String` or `Int` in the author-facing surface. If the value is really a plain string, name the role it plays.

- **`{name}` in strings is a runtime slot, not interpolation.** The name must be a declared parameter or local binding — you cannot invent a slot name that isn't in scope. Parameter slots survive into the compiled `## Parameters` section for the agent to fill at invocation time. Local binding references are rewritten into natural-language cross-references in the compiled prose.

- **`with` applies a modifier to a single call site.** `validate(plan) with "focus on security"` tells the expand pass to apply that modifier when expanding this invocation into prose — think of it as instructing the LLM to run the call with that extra lens. The callee's contract is unchanged; `with` affects only the wording of that one step in compiled output.

- **`context:` is not an instruction.** It is background framing the agent should understand while executing — not a directive and not a step. Instructions go in `flow:` as strings or calls.

- **`description:` is the routing key.** Agents read it to decide when to invoke the skill. Write it as a trigger condition, not a summary. Blocks support `description:`, `context:`, `constraints:`, and `flow:` too — the same sub-sections as a skill.

- **Undefined names are auto-materialized.** If a name appears as a call (`foo()`) with no definition, the repair pass creates a `generated block`. If it appears in a constraint or context position (`require preserve_patterns`), the repair pass creates a `generated const` with a string definition. You can leave stubs and let the compiler fill them in — or define them yourself.

- **Output targets use angle brackets.** `return <current_branch>` means "the agent must produce a value called current_branch from the prose." `return <"root cause including severity">` is the descriptive form. Both are distinct from returning an existing binding — use them when the producer is prose or judgement, not a callable.

- **Identifiers are case-normalized.** `make_plan`, `makePlan`, and `MakePlan` all resolve to the same name. Convention is `snake_case` for values and `PascalCase` for types — but the compiler treats them as equivalent.

- **One skill per file.** A file with a `skill` declaration is a skill file. A file with only `block`/`const`/`export ...` declarations is a library file. Two skills in one file is an error.

- **`export block` has strict rules.** Must end with an explicit `return`. Must be self-contained — behavior depends only on declared inputs, imports, and same-file declarations. No hidden context.

- **The compiler writes the prose.** You describe structure and intent. The compiler expands it into explicit, agent-followable instructions under `## Parameters`, `### Steps`, `### Constraints`. Don't try to write that layer by hand.

## LLM Passes

The full `/glyph:compile` pipeline runs four LLM passes after the deterministic compiler:

- **Repair** — triggered only when the compiler emits repairable diagnostics (unresolved names, missing `description:`, ambiguous constraint roles). Rewrites source, then re-parses. Skipped entirely if the source is already valid.
- **Semantic validation** — scans each declaration's constraint set for contradictions and tensions. Skipped for declarations with fewer than two constraints.
- **Expand** — needed to apply complex modifiers like with statement.
- **Review** — reads the finished `.md` against the IR and auto-fixes minor wording issues; flags anything structural for the author. Feel free to skip if it's a small skill or change.

## Planned Features

- Incremental Compilation
- Agents & Inheritance
- Standard Library
- Effects Inference
- Goal & Richer Output Contracts
- Freeform Sections
- Modular Testing & Evals
- Expanded Semantic Validation
- Fully Deterministic Compiler

## License

Apache 2.0 — see [LICENSE](LICENSE).
