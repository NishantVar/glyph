// teach_glyph_context.glyph.md
//
// Bundles the Glyph language reference as a single context-only skill.
// Importers reference `glyph_language_context` under their `context:` section
// to inject the full reference. The individual `const` constants below are
// the bundle's contents; they remain `export` so files that only need a
// subset can still import them by name.

skill glyph_language_context()
    description: "Bundle of Glyph language reference material to inject as context into any skill that authors or edits Glyph source."

    context:
        glyph_overview
        file_kinds
        layout_rules
        declarations
        parameters
        sub_sections
        calls_and_control_flow
        values
        names_and_types
        stdlib
        library_files_and_prefs
        compiled_output
        pitfalls
        worked_examples
        quick_reference

export const glyph_overview = """
Glyph is a small DSL for authoring agent skills. The author writes a
structured `.glyph.md` source file. The Glyph compiler turns it into a
flat, explicit Markdown skill (`.md`) that a coding agent can follow at
runtime.

- The source form is for humans: structured, readable, like a tiny program.
- The compiled form is for agents: explicit prose with sections like
  `## Parameters`, `### Context`, `### Steps`, `### Constraints`.
- The author never writes the agent-facing prose by hand. They describe
  structure and intent; the compiler produces the prose.
- Glyph is a language with a compiler, not a runtime. There is no agent
  execution at compile time — the compiler emits instructions for an agent
  to follow later.

Two things to internalize:
1. Source can be ergonomic; the IR (and compiled output) is strict. Authors
   may omit annotations, skip type names, write inline strings, and leave
   names undefined; the compiler (with a bounded LLM repair pass) will
   normalize and fill in. Do not over-decorate source.
2. There is no string interpolation. Values flow through parameters and
   call arguments. A `{name}` token in instruction strings is a name
   reference (parameter or local binding), not template substitution.
"""

export const file_kinds = """
A Glyph source file is named `<basename>.glyph.md`. There are exactly two
file kinds:

- Skill file: contains exactly one `skill` declaration plus optional
  supporting declarations. Compiles to one `<basename>.md` skill.
- Library file: contains zero `skill` declarations; only `import`, value
  bindings, `block`, and `export …` declarations. Compiles to zero or more
  procedure `.md` files (one per qualifying `export block`); constants are
  inlined into consumers.

Rules:
- A file may not contain two `skill` declarations.
- A library file must contain at least one `export` declaration.
- A file may not be empty (whitespace/comments only).
- A skill body must contain at least one of `flow:` or `constraints:`
  (an empty skill is rejected).
"""

export const layout_rules = """
- 4-space indentation, significant. No tabs (hard error). No braces, no
  `end` keywords.
- No trailing colon on top-level declarations. Write `skill name()` not
  `skill name():`. Colons mark sub-section headers inside a body
  (`flow:`, `constraints:`, etc.).
- Blank lines inside a body are visual separators only — they do not close
  the block.
- Implicit line continuation only inside paired delimiters
  (`(...)`, `{...}`, `\"\"\"...\"\"\"`). No backslash continuation.
- Line comments use `//`. No block comments. Comments are stripped from
  compiled output.
"""

export const declarations = """
Top-level building blocks (column 0):

- `skill <name>(<params>) [-> ReturnType]` — the public, compiled
  entrypoint (one per skill file). Parentheses always required. Return
  type is optional and folds into the closing sentence of the final Step
  in compiled output (no separate `### Returns` section). Only domain
  types are valid in `->` position; no primitive type names.

- `block <name>(<params>) [-> Type]` — private callable helper, scoped to
  the file. Single-string shorthand: when a block body is exactly one
  instruction string and no other sub-sections, `flow:` may be omitted.

- `export block <name>(<params>) -> Type` — importable, self-contained
  block. Hard rules:
    * Return type required when the block produces a meaningful return
      value; omit `->` entirely when it does not (no `-> None`).
    * Every parameter must have a default. A required parameter without a
      default is a hard compile error (no LLM repair).
    * Must end with an explicit `return`. Even instruction-only blocks
      should `return none`.
    * Must be closed: behavior depends only on declared inputs, local
      bindings, explicit imports, same-file declarations, the standard
      library, and declared constraints/effects.

- `const <name> = "..."` / `const <name> = 3` / `const <name> = 0.8`
  (and their `export` forms) — named compile-time constants. No
  parameters, no body, no return type. RHS may be a literal or a static
  reference to another constant of the same kind. String content may be
  inline `"..."` or block `\"\"\"...\"\"\"`. A bare string-valued constant
  in `flow:` without a marker (`context`/`require`/`avoid`/`must`) is an
  error — for instructions, use `block`.

- `import "<path>" as <alias>` — whole-module import. Exposes the file's
  `skill` (via `M.skill_name`) plus all `export …` declarations.

- `import "<path>" { name, name as alias }` — selective import. Imports
  only explicitly exported declarations. Path is always quoted; relative
  paths only (`./...`, `../...`). No re-exporting. No circular imports.

- The `@glyph/` prefix is reserved for compiler-shipped modules.
  `@glyph/std` is the standard library.

- `generated const` / `generated block` — produced by the repair pass when
  the source uses an undefined name and the compiler can confidently
  materialize a definition. The author does not write these by hand;
  review them and promote (rename to `const`/`block`) if they should be
  hand-authored.
"""

export const parameters = """
Parameter forms inside parentheses on `skill`, `block`, `export block`:

    name                          // untyped, no default
    name = "default"              // untyped, with default
    name: Type                    // typed, no default
    name: Type = default_value    // typed, with default

Defaults can be a literal (string, int, float, bool, `none`) or a
name-reference to an in-scope constant. They cannot be a parameter
reference, a block reference, an arbitrary expression, or a call.

Type annotations use domain types only — `name: Plan`,
`name: BranchName`, `name: Severity`. There are no primitive type names
in author-facing source. Annotations are optional in MVP.

Default-availability by declaration:
- `skill`         — parameters without defaults are allowed; they become
                    runtime-required inputs the agent must extract.
- `export block`  — every parameter must have a default (hard error).
- `block` (priv)  — parameters without defaults are allowed; the caller
                    supplies the argument at the call site.

Parameter slots — `{name}` inside an instruction-bearing string:
- Strict grammar: `{IDENTIFIER}` only. Anything else with braces is
  treated as literal text.
- Legal only inside instruction-bearing strings: string-valued constant
  bodies, inline strings inside `flow:`, constraint texts, and string
  arguments to stdlib calls. Illegal inside `description:`, parameter
  defaults, etc.
- A `{name}` that doesn't resolve to a parameter or local binding in
  scope is a hard error.
- Parameter references survive into compiled Markdown as literal `{name}`
  slots — the consuming agent fills them at runtime.
- Local-binding references (e.g., `{diagnosis}` after `diagnosis = …`)
  are rewritten by the compiler into natural-language cross-references
  in compiled prose.
"""

export const sub_sections = """
A `skill`, `block`, or `export block` body may contain these
colon-terminated sub-sections (each at most once per body):

- `description:` (singular) — one-line summary; goes to compiled YAML
  frontmatter. Body must be exactly one quoted string literal or a
  bare-name reference to a same-file `const` / `export const`. No
  `{param}` slots inside `description:`. On a `skill`, omitting it
  triggers repair. On a `block`/`export block`, optional unless the
  block is consulted via `BLOCKNAME.applies()`.

- `effects:` (plural) — declared effect keywords. Gated behind
  `--enable-effects`; off by default.

- `context:` (singular, set-like) — informational background. Does not
  direct action. Body grammar: bare-name references to string-valued
  constants, inline strings, or `context`-prefixed markers. Multiple
  entries allowed. No `{param}` slots inside `context:`.

- `constraints:` (plural) — constraint markers. Four forms composed from
  three keywords:
    require          — soft positive (do this)
    avoid            — soft negative (don't do this)
    must             — hard positive
    must avoid       — hard negative
  Each marker carries either a bare-name reference (to a same-file const
  constant or generated definition) or an inline string. Two surface
  styles are valid: marker-plus-concept (`avoid unrelated_edits`, with a
  polarity-neutral concept name) and compound name (`avoid_unrelated_edits`,
  where the name carries the semantics).

- `flow:` (singular) — ordered workflow steps (see calls_and_control_flow).

Order is permissive in source; `glyph fmt` rewrites them on disk into the
canonical order: description → effects → context → constraints → flow.

Long form vs short form for list-shaped sections (effects, etc.) — both
accepted, identical IR:
    effects:
        - reads_files
        - runs_commands
    // or
    effects: reads_files, runs_commands

Constraint and context markers may appear:
1. inside `constraints:` / `context:`,
2. directly at the body level (no wrapper), or
3. as a flow statement inside `flow:` (including inside an `if`/`elif`/
   `else` arm).
Top-level markers are hoisted into the corresponding sub-section by the
compiler. Markers inside a branch arm stay inline and render as part of
the conditional Step prose.

A bare string in a body is always a Step. It is never context or
background. For background, use `context:` or `description:`. For named
string constants, use `const`.
"""

export const calls_and_control_flow = """
Inside `flow:`:

Calls
- Positional and named arguments. Positional must precede named. A named
  arg cannot duplicate a parameter already filled positionally. Trailing
  commas allowed; multi-line argument lists are common.
- Qualified callees from a whole-module import: `repo_tools.inspect_repo(scope)`.
- UFCS — `value.method(args)` desugars to `method(value, args)`. Pure
  syntactic sugar; there is no method dispatch.
- The `with` modifier — a trailing `with "..."` clause specializes one
  call site. Shapes the wording of the expanded Step. One per call site,
  no chaining. Works with bare calls, qualified calls, UFCS calls, and
  bound calls. Does not apply to bare-name statements.
- Nested calls are legal but read better with intermediate named bindings.

Branching — `if` / `elif` / `else`
- Python-style colon-terminated headers, significant indentation.
- Allowed conditions: boolean identifier or binding; boolean-returning
  call; single-level dot access (`ctx.has_tests`); `not`; equality
  (`==`) / inequality (`!=`); `and` / `or`; parenthesized grouping;
  block trigger predicate (`block_name.applies()`).
- Standard precedence: `not` > `and` > `or`. No `<`, `>`, `<=`, `>=`,
  no arithmetic, no `in`. Bind a boolean call result instead.
- Branch bodies may contain any flow statement form except `return`.

Block trigger predicate — `BLOCKNAME.applies()`
- Special form for description-driven dispatch inside an `if`/`elif`
  condition. Receiver must resolve to a `block` / `export block` (or
  `module_alias.block_name`) carrying a `description:`. The description
  is the natural-language predicate the consuming agent matches.
- Name `applies` and the empty parens are fixed. `.applies(arg)` and
  `.applies` (no parens) are errors.
- Only valid inside an `if`/`elif` condition. Cannot bind, return, or
  pass as an argument. Composes with `and`/`or`/`not`.
- A block consulted via `.applies()` must have `description:`. If
  missing on a same-file block, repair generates one. If missing on an
  imported block, hard error.

`return`
- Exactly one `return` per `flow:`, and it must be the last statement at
  the top level (not inside `if`/`elif`/`else`).
- `return` is forbidden inside branch arms — there is no early return
  in MVP.
- `export block` requires an explicit `return` (even `return none`). For
  `skill` and private `block`, omitting `return` is fine; the compiler
  implicitly returns `none`.
- The return type annotation is advisory (used to shape compiled prose
  and for nominal type matching at call boundaries). No runtime
  enforcement.
- Forms: `return <expr>` | `return` (≡ `return none`) | `return <name>`
  (output target identifier form) | `return <"description">` (output
  target descriptive form).

Output targets — `<name>` and `<"description">`
- Use when the return value is synthesized by the agent from prose
  rather than produced by a callable. Identifier form: `<current_branch>`
  (no spaces; snake_case; not a type-looking name). Descriptive form:
  `<"root cause analysis including affected files and severity">`.
- The compiler does NOT resolve the angle-bracket name to an existing
  binding. Output targets are terminal-return-only in MVP and do not
  introduce a local binding.
- The `-> DomainType` is the compiler contract; the `<"...">` is agent
  guidance. Both may co-exist.
- Compiled output never contains a literal `<name>` or `<"…">` token —
  Expand turns it into natural prose.

When in doubt, prefer a normal binding. Reach for output targets only
when the producer is the prose itself.
"""

export const values = """
Strings
- Inline: `"..."` (double quotes only; no single quotes).
- Block: `\"\"\"...\"\"\"` — multiline; common leading indentation
  stripped (Python-style dedent).
- Escapes: `\\"` and `\\\\` only. No `\\n`, `\\t`, no Unicode escapes.
- No interpolation, no concatenation. No `${...}`. No `+` on strings.
- The only template-like feature is `{name}` parameter slots inside
  instruction-bearing strings.

Integers and floats
- Integers: standard decimals. No leading zeros. Negative literals OK.
- Floats: digits required on both sides of the point. `0.5` valid;
  `.5` and `3.` not. No scientific notation.
- Numeric coercion at call boundaries is automatic and lossless: `3.0`
  to Int becomes `3`; `3` to Float becomes `3.0`; `3.7` to Int is a
  compile error.

Booleans and `none`
- `true` and `false`. Source is case-insensitive; IR normalizes to
  lowercase.
- `none` is a reserved keyword for absence of value. Usable wherever a
  value is expected: `return none`, `result = none`, `effects: none`.

No value-level operators
- MVP expressions support only literals, bindings, calls, and dot
  access. No arithmetic. No comparisons in arbitrary expressions (only
  inside `if` conditions, with the limited operator set).
- To combine context with a call, use `with`. For a derived boolean,
  bind the result of a call.
"""

export const names_and_types = """
Identifiers
- Pattern: `[a-zA-Z_][a-zA-Z0-9_]*`. No hyphens.
- Convention: `snake_case` for values and callables; `PascalCase` for
  types.
- Case-normalized: `makePlan`, `make_plan`, `MakePlan`, `MAKE_PLAN` all
  resolve to the same name.
- Dots are reserved for module-qualified access and single-level
  dot-property access on bound values.

Reserved keywords (cannot be used as identifiers):
  skill, block, export, import, const, flow, call, if, elif,
  else, return, true, false, none, effects, constraints, inputs, outputs,
  when_to_use, description, as, generated, input, output, must, require,
  avoid, context, and, or, not.

No shadowing
- If the same normalized name is visible from multiple sources in
  overlapping scopes, it is a hard error. Applies across parameter vs
  same-file constant, local binding vs parameter, and import vs
  same-file declaration.

Bare-name resolution order
1. a constant declaration in the current file,
2. a parameter of the enclosing skill or block,
3. a local binding,
4. an imported name (selectively-imported `@glyph/std` entries enter
   the namespace at this level — they require an explicit import),
5. a repair-generated definition (`generated const` / `generated block`).

A parenthesized form (`foo()` or `foo(x)`) is always a callable. A bare
`foo` is never a call. If a bare name in `flow:` is undefined, the
compiler treats it as an intended callable and materializes a
`generated block`. Bare names that resolve to a string-valued constant
are an error in `flow:` unless prefixed with a marker.

Types
- Types in MVP are semantic labels for an LLM reading the compiled
  output, not enforced structural contracts.
- There are NO primitive type names in author-facing source. Never
  write `String`, `Int`, `Float`, `Bool`, or `None` as type
  annotations. The compiler tracks primitive kinds internally but never
  surfaces them.
- Use named domain types — `RepoContext`, `Plan`, `FailureReport`,
  `BranchName`, `Severity`, `Confirmation`, `Diagnosis`. They are
  opaque tags, implicitly declared by first use in a `-> Type`
  position; no separate `type Foo` declaration.
- The compiler does nominal matching at call boundaries: matching
  names = compatible; differing names = error; either side untyped =
  no check.
- The one compiler-known type name is `Agent` (returned by
  `subagent()`). Behaves like any other domain type.
- Type position is determined by syntax: after `:` in a parameter
  declaration; after `->` in a return type.
- Omit `->` entirely when there is no meaningful return value. There
  is no `-> None` annotation. The `none` value keyword stays in value
  positions.
"""

export const stdlib = """
The standard library is compiler-embedded under the reserved virtual
prefix `@glyph/`. Three entries:

- `subagent(task) -> Agent` — author-facing. Spawns a delegated agent.
- `send(agent: Agent, message) -> None` — author-facing. Messages a
  running subagent. Use UFCS for readability:
  `agent.send("...")` desugars to `send(agent, "...")`.
- `load` — compiler-internal; not for author use.

Stdlib names are not auto-available. Import explicitly:
    import "@glyph/std" { subagent, send }

The `Agent` type
- Compiler-known. No literal form — the only way to create one is
  `subagent(...)`.
- Participates in nominal matching like any other domain type.
- A block declaring `-> Agent` must transitively obtain its return
  value from a `subagent` call (directly, through an imported callee,
  or through an `Agent` parameter).
- An `Agent` returned from a skill represents the handle to the spawned
  agent (not the agent's findings). To get findings, pass an instruction
  string instead.
- No identity equality, no termination primitive, no await — opaque
  handles only.

Effect boundary at subagent spawns
- A skill that spawns a subagent declares `spawns_agent`. It does NOT
  inherit the spawned skill's effects — the spawned skill is a separate
  compilation unit with its own effect surface.
"""

export const library_files_and_prefs = """
A library file is just a `.glyph.md` with no `skill`. It contains
`import`, value bindings, `block`, and `export …` declarations.

Preferences are ordinary constants
- There is no `pref(...)` call form, no `reads_prefs` effect, no ambient
  lookup. A preference is just an `export const`.
- The compiler infers the value kind from the literal. Default values
  are mandatory on every constant declaration in a library.
- A consumer imports normally:
      import "./prefs.glyph.md" { preserve_existing_patterns }
- Preferences may also serve as parameter defaults (resolved at compile
  time; the literal value appears in the compiled `## Parameters`
  section).
- When a preference value changes, recompile the consuming skills.

Across files
- Whole-module imports expose the file's `skill` (via `M.skill_name`)
  plus all `export …` declarations.
- Selective imports bring in only explicitly exported declarations.
- A consumer must import directly from the defining file. No
  re-exporting. No circular imports — refactor shared content into a
  third file.
"""

export const compiled_output = """
The compiler emits one Markdown file per skill with this shape:

    ---
    name: <skill-name>
    description: <one line>
    effects: [<keyword>, <keyword>]   # only when --enable-effects AND set is non-empty
    ---

    ## Parameters
    - **scope**: description (default: ".")
    - **target**: description (required)

    ## Instructions

    ### Context
    - Background point 1.
    - Background point 2.

    ### Steps
    1. First step prose.
    2. Second step prose. {scope} survives as a runtime slot.
    3. If the risk is high and tests exist:
       a. Run the full test suite.
       b. Request a code review.
       Otherwise:
       a. No action needed.

    ### Constraints
    - Strong: must avoid breaking the public API.
    - Soft: prefer existing patterns.

Notes:
- Frontmatter always has `name` (taken from the `skill` declaration) and
  `description`. There is no `# <Skill Name>` heading — the frontmatter
  `name` is the authoritative title.
- `## Parameters` is only present if the skill declares parameters.
- `### Context` only if there is a `context:` section or context
  markers.
- `### Constraints` only if any unconditional constraints exist.
- `### Steps` is omitted only for pure constraint-only skills (no
  `flow:` at all). At least one of `### Steps` or `### Constraints` is
  always present.
- Branches project to a single numbered Step with lettered sub-steps
  per arm. Letters reset per arm.
- The `return` expression folds into the final Step's closing
  sentence. There is no `### Returns` section.
- Imports compile away — no import paths or module names appear in the
  output.
"""

export const pitfalls = """
Common compile errors and their fixes:

- `tabs not allowed`: tabs in indentation → use 4 spaces.
- `multiple-skills`: two `skill` declarations in one file → factor into
  separate files.
- `empty-skill-body`: skill with no `flow:` and no `constraints:` → add
  at least one.
- `empty-flow`: `flow:` header present but body has zero statements →
  remove the header (constraint-only skill) or add a statement.
- `no-exports-in-library`: library file has zero `export` declarations
  → add at least one `export block` or `export const`.
- `const-in-flow`: a string-valued constant name appears bare in
  `flow:` without a marker → wrap with `context`/`require`/`avoid`/
  `must`, or convert to `block`.
- `missing-param-default` (export block): an `export block` parameter
  has no default → add an explicit default.
- `missing-return` (export block): `export block` body has no `return`
  → repairable (Phase 3 inserts `return none`); prefer to write it
  explicitly.
- `import-skill`: tried to selectively import a `skill` from another
  file → only `export …` declarations are importable; refactor into an
  `export block`.
- `applies-on-undescribed-block` (imported): `BLOCKNAME.applies()` on
  an imported block lacking `description:` → add `description:` in the
  source library.
- `unknown-param-slot`: `{name}` references a parameter or binding not
  in scope → rename, declare, or remove the slot.
- `param-slot-in-non-instruction-string`: `{name}` inside
  `description:` or a parameter default → move the slot into
  instruction text.
- `circular-import`: files import each other in a cycle → extract
  shared content into a third file.
- `effects-under-declared` (when effects gated on): declared `effects:`
  is missing keywords the call graph implies → add the missing
  keyword(s), or omit `effects:` to let inference fill it in.
- `no-shadowing` collision: same name from two sources in overlapping
  scope → rename one, or alias on import.
"""

export const worked_examples = """
Minimal skill (novice kernel):

    skill update_docs(scope = ".")
        description: "Update repository documentation to match current code."
        require accuracy
        avoid stale_references

        flow:
            "Scan {scope} for documentation files."
            "Compare each document against the current code."
            "Update outdated or incorrect sections."
            "Verify all cross-references and links are valid."

    const accuracy = "Ensure all documentation accurately reflects the current code."
    const stale_references = "Avoid leaving references to removed or renamed symbols."

With branching, blocks, and `.applies()`:

    skill fix_bug(scope = ".")
        description: "Debug and fix a bug with minimal, targeted changes."
        require preserve_existing_patterns
        avoid unrelated_edits
        context:
            "The bug is assumed to be reproducible locally."

        flow:
            inspect_repo(scope) with "focus on the area where the bug was reported"

            if deep_investigation.applies():
                "Trace symptoms across multiple subsystems."
                "Gather extensive evidence from logs, tests, and code."
            else:
                identify_root_cause()

            if has_test_suite.applies():
                "Run the existing test suite to establish a baseline."
            else:
                "Manually verify the fix by inspecting the changed code paths."

            patch_minimally()
            validate_fix()
            return summarize_changes()

    block deep_investigation()
        description: "The bug spans multiple subsystems or layers."
        flow:
            "Map the full dependency chain of the affected code."
            "Identify every subsystem involved in the bug."
            "Create a minimal reproduction case."

Multi-file skill with library and preferences:

    // prefs.glyph.md
    export const preserve_existing_patterns = "Prefer the repository's existing patterns and helpers."
    export const safety_first = "Never execute destructive operations without explicit confirmation."

    // repo_tools.glyph.md
    export block inspect_repo(scope = ".") -> RepoContext
        description: "Inspect the repository structure and identify key files."
        flow:
            "List directories and files under {scope}."
            "Identify source modules and their relationships."
            return "A summary of the repo layout."

    // fix_bug.glyph.md
    import "./prefs.glyph.md" { preserve_existing_patterns, safety_first }
    import "./repo_tools.glyph.md" { inspect_repo, has_test_suite }

    skill fix_bug(scope = ".")
        description: "Debug and fix a bug with minimal, targeted changes."
        require preserve_existing_patterns
        must safety_first

        flow:
            ctx = inspect_repo(scope) with "focus on where the bug was reported"
            if has_test_suite.applies():
                "Run tests to establish a baseline before any change."
            "Identify the root cause from {ctx} before proposing a fix."
            "Apply the smallest possible patch."
            "Verify the fix resolves the issue and runs the test suite cleanly."
            return "A short summary of what was changed and why."

Subagent delegation:

    import "@glyph/std" { subagent, send }

    skill investigate(scope = ".")
        description: "Delegate investigation of a code area to a subagent."
        flow:
            researcher = subagent(scope) with "trace the failure end-to-end"
            researcher.send("Begin with the entrypoint and trace data flow downstream.")
            researcher.send("Surface every assumption you make.")
            return researcher
"""

export const quick_reference = """
File:     <name>.glyph.md           — skill file (one `skill`) or library file (no `skill`)
Indent:   4 spaces, significant; no tabs
Comments: // line comments only
Strings:  "inline"   \"\"\"block\"\"\"   no interpolation; only `{name}` slots in instruction strings

Top-level declarations:
    skill <name>(<params>) [-> Type]
    block <name>(<params>) [-> Type]
    export block <name>(<params>) -> Type     # default required on every param; explicit return required
    const <name> = "..." | <int> | <float> | bare-name | qualified-name
    export const (same RHS forms; default required)
    import "<path>" as <alias>                 # whole-module
    import "<path>" { name, name as alias }    # selective
    import "@glyph/std" { subagent, send }     # stdlib

Sub-section headers (inside skill / block / export block body):
    description:   one-line string or const-name reference
    effects:       list / inline list (gated by --enable-effects)
    context:       bare names, inline strings, or `context "..."` markers
    constraints:   require / avoid / must / must avoid markers
    flow:          ordered statements (only one per body)

Constraint markers:
    require <name|"string">          # soft positive
    avoid   <name|"string">          # soft negative
    must    <name|"string">          # hard positive
    must avoid <name|"string">       # hard negative

Flow statement forms:
    x = call(args)                   # binding
    call(args)                       # statement call
    receiver.method(args)            # UFCS desugars to method(receiver, args)
    Alias.callee(args)               # qualified call
    call(args) with "modifier"       # site modifier
    bare_name                        # name reference (resolves to const/block/import/binding)
    "inline instruction"
    context <name|"string">          # context marker
    require / avoid / must … <name|"string">   # constraint marker
    if <cond>:                        elif <cond>:    else:
    return <expr>                    # exactly one, top-level, last

Conditions:
    is_valid | foo(ctx) | ctx.has_tests | not x | a == b | a != b |
    a and b | a or b | (a or b) and c | block_name.applies()

Stdlib type:  Agent
Stdlib calls: subagent(task) -> Agent ;   send(agent: Agent, message) -> None

Values: "..."  \"\"\"...\"\"\"  3  -1  0.8  true  false  none
"""
