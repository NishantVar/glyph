---
# AUTO-GENERATED FILE -- DO NOT EDIT
# Source: .agents/commands/glyph/teach.md
# Regenerate: scripts/sync_commands_no_desc.sh
name: teach
---

## Parameters

- **target**. Required.

## Instructions

### Context

- **glyph-overview**

  Glyph is a small DSL for authoring agent skills. The author writes a
  structured `.glyph` source file. The Glyph compiler turns it into a
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

- **file-kinds**

  A Glyph source file is named `<basename>.glyph`. There are exactly two
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

- **layout-rules**

  Formatting rules for Glyph source files:

  - 4-space indentation, significant. No tabs (hard error). No braces, no
    `end` keywords.
  - No trailing colon on top-level declarations. Write `skill name()` not
    `skill name():`. Colons mark sub-section headers inside a body
    (`flow:`, `constraints:`, etc.).
  - Blank lines inside a body are visual separators only — they do not close
    the block.
  - Implicit line continuation only inside paired delimiters
    (`(...)`, `{...}`, `"""..."""`). No backslash continuation.
  - Line comments use `//`. No block comments. Comments are stripped from
    compiled output.

- **declarations**

  Top-level building blocks (column 0):

  - `skill <name>(<params>) [-> ReturnType]` — the public, compiled
    entrypoint (one per skill file). Parentheses always required. Return
    type is optional and folds into the closing sentence of the final Step
    in compiled output (no separate `### Returns` section). Only domain
    types are valid in `->` position; no primitive type names.

  - `block <name>(<params>) [-> Type]` — private callable helper, scoped to
    the file. Single-string shorthand: when a block body is exactly one
    instruction string and no other sub-sections, `flow:` may be omitted.

  - `export block <name>(<params>) [-> Type]` — importable, self-contained
    block. Hard rules:
      * A return type is required when the block produces a meaningful return
        value; omit `->` entirely when it does not (no `-> None`).
      * Every parameter must have a default. A required parameter without a
        default is a hard compile error (no LLM repair).
      * The block must end with an explicit `return`. Even instruction-only
        blocks should `return none`.
      * The block must be closed: behavior depends only on declared inputs, local
        bindings, explicit imports, same-file declarations, the standard
        library, and declared constraints/effects.

  - `const <name> = "..."` / `const <name> = 3` / `const <name> = 0.8`
    (and their `export` forms) — named compile-time constants. No
    parameters, no body, no return type. RHS may be a literal or a static
    reference to another constant of the same kind. String content may be
    inline `"..."` or block `"""..."""`. A bare string-valued constant
    in `flow:` without a marker (`context`/`require`/`avoid`/`must`) is an
    error — for instructions, use `block`.

  - `type <Name> = <"description">` (and `export type <Name> = <"description">`)
    — top-level decl that attaches a default description to a domain type.
    No body, no parameters, no return type. RHS uses `<"...">` (inline) or
    `<"""...""">` (block-string). The decl emits nothing on its own; it
    supplies the description used wherever `: Name` or `-> Name` appears,
    unless a per-param `<"...">` overrides at that slot. Type imports are
    selective only (`import "./types.glyph" { Foo }`); whole-module
    qualified type refs are not supported. A library file containing only
    `export type` decls is valid.

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

- **parameters**

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

  Per-parameter descriptions — append `<"...">` to a slot to set the
  parameter's text in the compiled `## Parameters` section. Four forms:

      x = <"description only — no default value">
      x = "foo" <"default with description">
      x: T = <"typed, no default, with description">
      x: T = "foo" <"typed, with default and description">

  Block-string form `<"""...""">` is accepted for multi-line descriptions.
  A per-param description wins over any description supplied by a
  `type Foo = <"...">` decl. When both are absent, the compiled bullet
  shows just the name + (optional) type + default-or-required marker.
  Per-param descriptions are author guidance for the compiler — `{name}`
  slots are not allowed inside them.

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

- **sub-sections**

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

- **calls-and-control-flow**

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
  - Headers are bare keyword + condition; a trailing colon is optional and
    accepted by the parser. Significant indentation marks the arm body.
  - Allowed conditions: boolean identifier or binding; boolean-returning
    call; single-level dot access (`ctx.has_tests`); `not`; equality
    (`==`) / inequality (`!=`); `and` / `or`; parenthesized grouping;
    block trigger predicate (`block_name.applies()`); named string
    predicate (a `const` whose body is the natural-language predicate,
    used bare in condition position — e.g.,
    `if complex_change_required`); inline string predicate (a string
    literal in condition position — e.g.,
    `if "the user has explicitly opted out of compile-on-save"`).
  - The two string-predicate forms compose with `not`/`and`/`or` like any
    other condition. A non-bool, non-string primitive in bare condition
    position (e.g., an integer `const`) is a hard error — use `==`.
  - Carve-out: when a string-kinded name appears as an `==` operand
    (`if risk == high_risk_const`), it is treated as a string equality
    comparison, not a predicate.
  - Use the named-const or inline-literal form when the predicate stands
    on its own. Use `.applies()` when the predicate ships bundled with a
    block body — it is the canonical form whenever the natural-language
    text is the `description:` of the block being dispatched to.
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
  - At most one `return` per `flow:`, and when present it must be the last
    statement at the top level (not inside `if`/`elif`/`else`).
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

- **values**

  Strings
  - Inline: `"..."` (double quotes only; no single quotes).
  - Block: `"""..."""` — multiline; common leading indentation
    stripped (Python-style dedent).
  - Escapes: `\"` and `\\` only. No `\n`, `\t`, no Unicode escapes.
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

- **names-and-types**

  Identifiers
  - Pattern: `[a-zA-Z_][a-zA-Z0-9_]*`. No hyphens.
  - Convention: `snake_case` for values and callables; `PascalCase` for
    types.
  - Case-normalized: `makePlan`, `make_plan`, `MakePlan`, `MAKE_PLAN` all
    resolve to the same name.
  - Dots are reserved for module-qualified access and single-level
    dot-property access on bound values.

  Reserved keywords (cannot be used as identifiers):
    skill, block, export, import, const, type, flow, call, if, elif,
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
    opaque tags, implicitly declared by first use in a `-> Type` or
    `: Type` position. An explicit `type Foo = <"description">` decl
    (and its `export type` form) is optional — it does not change
    nominal matching, it only attaches a default description that
    surfaces wherever `Foo` annotates a parameter or return.
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

- **stdlib**

  The standard library is compiler-embedded under the reserved virtual
  prefix `@glyph/`. Three entries:

  - `subagent(task) -> Agent` — author-facing. Spawns a delegated agent.
  - `send(agent: Agent, message)` — author-facing. Messages a running
    subagent and has no meaningful return value. Use UFCS for readability:
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

- **library-files-and-prefs**

  A library file is just a `.glyph` with no `skill`. It contains
  `import`, value bindings, `block`, and `export …` declarations.

  Preferences are ordinary constants
  - There is no `pref(...)` call form, no `reads_prefs` effect, no ambient
    lookup. A preference is just an `export const`.
  - The compiler infers the value kind from the literal. An RHS value is
    mandatory on every constant declaration in a library.
  - A consumer imports normally:
        import "./prefs.glyph" { preserve_existing_patterns }
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

- **compiled-output**

  The compiler emits one Markdown file per skill with this shape:

      ---
      name: <skill-name>
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

- **pitfalls**

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

- **worked-examples**

  Minimal skill (novice kernel):

      skill update_docs(scope = ".")
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
          require preserve_existing_patterns
          avoid unrelated_edits
          context:
              "The bug is assumed to be reproducible locally."

          flow:
              inspect_repo(scope) with "focus on the area where the bug was reported"

              if deep_investigation.applies()
                  "Trace symptoms across multiple subsystems."
                  "Gather extensive evidence from logs, tests, and code."
              else
                  identify_root_cause()

              if has_test_suite.applies()
                  "Run the existing test suite to establish a baseline."
              else
                  "Manually verify the fix by inspecting the changed code paths."

              patch_minimally()
              validate_fix()
              return summarize_changes()

      block deep_investigation()
          flow:
              "Map the full dependency chain of the affected code."
              "Identify every subsystem involved in the bug."
              "Create a minimal reproduction case."

  Multi-file skill with library and preferences:

      // prefs.glyph
      export const preserve_existing_patterns = "Prefer the repository's existing patterns and helpers."
      export const safety_first = "Never execute destructive operations without explicit confirmation."

      // repo_tools.glyph
      export block inspect_repo(scope = ".") -> RepoContext
          flow:
              "List directories and files under {scope}."
              "Identify source modules and their relationships."
              return <"summary of the repo layout">

      export block has_test_suite(scope = ".")
          flow:
              "Inspect {scope} for test configuration and existing tests."
              return none

      // fix_bug.glyph
      import "./prefs.glyph" { preserve_existing_patterns, safety_first }
      import "./repo_tools.glyph" { inspect_repo, has_test_suite }

      skill fix_bug(scope = ".")
          require preserve_existing_patterns
          must safety_first

          flow:
              ctx = inspect_repo(scope) with "focus on where the bug was reported"
              if has_test_suite.applies()
                  "Run tests to establish a baseline before any change."
              "Identify the root cause from {ctx} before proposing a fix."
              "Apply the smallest possible patch."
              "Verify the fix resolves the issue and runs the test suite cleanly."
              return <"short summary of what was changed and why">

  Subagent delegation:

      import "@glyph/std" { subagent, send }

      skill investigate(scope = ".")
          flow:
              researcher = subagent(scope) with "trace the failure end-to-end"
              researcher.send("Begin with the entrypoint and trace data flow downstream.")
              researcher.send("Surface every assumption you make.")
              return researcher

- **quick-reference**

  File:     <name>.glyph           — skill file (one `skill`) or library file (no `skill`)
  Indent:   4 spaces, significant; no tabs
  Comments: // line comments only
  Strings:  "inline"   """block"""   no interpolation; only `{name}` slots in instruction strings

  Top-level declarations:
      skill <name>(<params>) [-> Type]
      block <name>(<params>) [-> Type]
      export block <name>(<params>) [-> Type]   # default required on every param; explicit return required
      const <name> = "..." | <int> | <float> | bare-name | qualified-name
      export const <name> = "..." | <int> | <float> | bare-name | qualified-name
      type <Name> = <"description">              # default description for : Name / -> Name slots
      export type <Name> = <"description">       # importable type-with-description
      import "<path>" as <alias>                 # whole-module
      import "<path>" { name, name as alias }    # selective (also imports `export type` decls)
      import "@glyph/std" { subagent, send }     # stdlib

  Parameter slot forms:
      name                                       # untyped, no default
      name = "default"                           # untyped, with default
      name: Type                                 # typed, no default
      name: Type = "default"                     # typed, with default
      name = <"description">                     # plus per-param description (any of the four base forms)

  Sub-section headers (inside skill / block / export block body):
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
      bare_block_name                  # shorthand call; string constants need a marker
      "inline instruction"
      context <name|"string">          # context marker
      require / avoid / must … <name|"string">   # constraint marker
      if <cond>                         elif <cond>     else
      return <expr>                    # exactly one, top-level, last

  Conditions:
      is_valid | foo(ctx) | ctx.has_tests | not x | a == b | a != b |
      a and b | a or b | (a or b) and c | block_name.applies() |
      string_const_name                # named string predicate (const body is the predicate)
      "inline string predicate"        # inline string literal in condition position

  Stdlib type:  Agent
  Stdlib calls: subagent(task) -> Agent ;   send(agent: Agent, message)

  Values: "..."  """..."""  3  -1  0.8  true  false  none

### Steps

1. Read the user's request and identify the skill's purpose, the runtime parameters it will need, and any task-specific constraints. If {target} is an existing `.glyph` file, read it first and treat the task as an edit. Otherwise, plan to create a new file at {target}.
2. Decide the file kind: a skill file with exactly one `skill` declaration, or a library file with zero `skill` declarations and at least one `export block` or `export const`.
3. Write the declaration header. For `skill`, parameters may have no defaults (the agent extracts them from user context at runtime). For `export block`, every parameter must have a default. For return types, use a named domain type (`Plan`, `BranchName`, `Diagnosis`) or omit `->` entirely when there is no meaningful return value — never `String`/`Int`/`Float`/`Bool`/`None`. Decide each parameter's slot form: untyped `name`, defaulted `name = "x"`, typed `name: T`, or typed-and-defaulted `name: T = "x"`. Add a per-param description with `<"...">` (or block-string `<"""...""">`) when the parameter's role isn't obvious from the name. If two or more parameters across this file share a domain type, hoist the description into a top-level `type Foo = <"...">` decl (or `export type Foo = <"...">` when other files annotate the same type) instead of repeating per-param `<"...">` text. A per-param `<"...">` still wins over the type-level description on individual slots.
4. Add `description:` as a single-line routing string, or as a bare-name reference to a same-file `const` constant. No `{name}` slots inside `description:`. Choose constraints. Use `require` / `avoid` for soft rules and `must` / `must avoid` for genuinely non-negotiable ones. Each marker carries either a bare-name reference to a same-file `const` constant or an inline string. Add `context:` entries for background facts the agent should keep in mind at runtime — bare-name references to string-valued `const` constants, inline strings, or `context`-prefixed markers.
5. Follow the write-flow-section procedure below.
6. Promote any inline string that repeats into a `const` constant. Promote any instruction sequence that repeats into a `block` (or `export block` if another file needs it).
7. Scan every instruction string in `flow:` bodies. For any string longer than 10 words, extract it into a named `block` (or `export block` if it must be reachable from another file) and replace the inline string with a call to that block. Pick a verb-phrase name that describes the step's intent. Scan every inline string used as a marker body (`require`/`avoid`/`must`/`must avoid`/`context`) or as a `context:` entry. For any string longer than 10 words, extract it into a named `const` constant (or `export const` if another file imports it) and replace the inline string with a bare-name reference. Skip `description:` strings — leave them inline.
8. Follow the recognize-conditionals-as-branches procedure below.
9. Reorder top-level declarations in the file so that the single `skill` declaration appears first, every `block` and `export block` follows it, and every `const` and `export const` constant comes last. Preserve `import` statements at the very top of the file, above the `skill` declaration.
10. If the user prefers to delegate compilation to a sub-agent rather than run the pipeline inline:
   a. Spawn a sub-agent and instruct it to run the `/glyph:compile` slash command on the Glyph source file at {target}, drive the full Glyph pipeline (deterministic compile, repair loop, expand, validate, review), and return a structured compilation outcome — on success, list the emitted `.md` paths; on failure, list every diagnostic verbatim. The deterministic compiler alone is not sufficient for verification: only the full pipeline catches expand/validate/review failures. Once the sub-agent reports success, return the path to the authored or updated `.glyph` file at {target}.
   Otherwise:
   a. Load the compile skill content from `.agents/commands/glyph/compile.md` (resolve via the `glyph:compile` slash command if available) and execute its steps yourself, in this conversation, against the Glyph source at {target}. Drive the full pipeline inline — deterministic compile, repair loop, expand, validate, review — without delegating to a sub-agent, and report the compilation outcome by listing every emitted `.md` path on success or every diagnostic verbatim on failure. Once the inline pipeline succeeds, return the path to the authored or updated `.glyph` file at {target}.

### Constraints

- You must indent every body with exactly 4 spaces. Tabs are a hard error and there are no braces or `end` keywords.
- You must never use `String`, `Int`, `Float`, `Bool`, or `None` as a type annotation in author-facing source. Use named domain types (`BranchName`, `Severity`, `Confirmation`) instead.
- You must never build strings with `${...}`, `+` concatenation, or any other template syntax. The only template-like form is `{name}` parameter slots, legal solely inside instruction-bearing strings.
- You must never write the agent-facing Markdown prose by hand. The Expand pass produces the prose; the author writes structure and intent only.
- You must ensure every skill file contains exactly one `skill` declaration. Library files contain zero. Two `skill` declarations in one file is a hard error.
- You must place at most one `return` per `flow:`, and when present it must be the last statement at the top level — never inside an `if`/`elif`/`else` branch arm.
- You must give every parameter on an `export block` a default value. A required-without-default parameter is a hard error with no LLM repair.
- You must end every `export block` with an explicit `return` statement (use `return none` if there is no meaningful return value).
- You must never place `return` inside an `if`/`elif`/`else` branch arm. There is no early return in MVP — `return` is restricted to the top level of `flow:` and must be last.
- You must never use `{name}` parameter slots in `description:` bodies, parameter defaults, or any string that is not instruction-bearing. Slots are legal only inside `flow:` instruction strings, string-valued constant bodies, constraint texts, and string arguments to stdlib calls.
- Reach for the smallest viable surface — `skill`, `require`/`avoid`, `flow:`, inline strings, calls, and `with`. Promote to `block`, named `const`, or types only when they pay for themselves.
- When you do annotate a type, give it a domain name that tells an agent what role the value plays (`BranchName`, `Plan`, `Diagnosis`) — not a primitive-feeling label.
- Ensure any block consulted via `BLOCKNAME.applies()` carries a `description:` — that description is the predicate the agent matches against current context. Missing description on an imported block is a hard error.
- After running the compiler, review the source diff. Repair may have inserted `generated const` / `generated block` definitions, hoisted markers into `constraints:`, or generated a missing `description:`. Promote anything you want to harden by renaming `generated …` to `const` / `block`.
- Avoid writing a bare string-valued constant name in `flow:` without a marker. Wrap it with `context`/`require`/`avoid`/`must`, or convert it into a `block` if it really represents an instruction sequence.
- Avoid reaching for `must` / `must avoid` for everyday rules. Reserve hard markers for genuinely non-negotiable constraints; default to soft `require` / `avoid`.
- Avoid stacking nested calls (`apply(merge(base, overlay(ctx)))`). Nested calls are legal but read and visualize better as named intermediate bindings.
- Avoid starting an `avoid` or `must avoid` constraint's const body with a negation word (`do not`, `never`, `no`) — the polarity marker already supplies the negative, so a negation-leading body produces a double-negative bullet (`Avoid do not touch …`). Phrase the body as a noun or gerund phrase that completes `Avoid X` cleanly.

### Procedure: write-flow-section

1. Write `flow:` as an ordered sequence of instruction strings, bindings, calls (bare, qualified, or UFCS), and branches. Use `with "..."` to specialize a single call site.
2. Pick the right `if`/`elif` condition form for each branch: `BLOCKNAME.applies()` when the predicate is the description of an associated block body; a named string-`const` (`if predicate_name`) when the predicate stands on its own without a block; an inline string literal (`if "the user has explicitly opted out"`) for one-off predicates; otherwise the existing boolean / `==` / `!=` / `not`/`and`/`or` forms. Composition with `not`/`and`/`or` works for every form. Reach for `==` only for value comparison — a string-kinded name as an `==` operand compares as a string, not a predicate.
3. Reference parameters and local bindings inside instruction strings with `{name}` slots — only where the surrounding string is instruction-bearing.
4. End `flow:` with at most one top-level `return`, placed last (never inside a branch arm). Use `return <name>` or `return <"description">` only when the value is synthesized by the agent from prose; otherwise prefer a normal binding. For `export block`, the `return` must be explicit even when returning `none`.

### Procedure: recognize-conditionals-as-branches

1. Walk every `flow:` body in the file. For each instruction string, block call, or sequence that expresses conditional behavior in prose ("If X, do Y", "When Z, run W", "Only when …", "Otherwise …"), lift the condition out of the prose and rewrite the site as a Glyph `if` / `elif` / `else` branching statement. Each arm body becomes the actions the prose originally described — inline instructions, calls, or further branches.
2. Pick the right condition form: `BLOCKNAME.applies()` when the predicate is the `description:` of a same-file block (its arm body lives in that block); a named string-`const` (`if predicate_name`) when the predicate stands on its own without a block body and recurs; an inline string literal (`if "the predicate prose"`) for a one-off predicate that does not warrant a const; otherwise the standard boolean / `==` / `!=` / `not` / `and` / `or` forms over bindings or call results.
3. Recurse into each arm — nested conditional prose becomes nested `if` / `else` statements. Stop when no arm contains unconverted conditional prose.
4. Never place `return` inside an arm. If the original prose described a return-shaped outcome under a condition, keep `return` at the top level of `flow:` and let each arm bind the outgoing value beforehand (so the single trailing `return` references that binding).

