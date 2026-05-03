# Glyph Examples

Five annotated examples covering the full authoring surface. Each is a complete `.glyph.md` file you could drop into a project; comments call out which form is being demonstrated.

---

## 1. Small skill: minimal `flow:` and constraints

A single-purpose skill with parameters, body-level constraints, and a short flow.

```glyph
// File: update_docs.glyph.md
skill update_docs(scope = ".")
    description: "Use when the user asks to refresh documentation in a scope."

    require preserve_existing_patterns
    avoid unrelated_edits

    effects: reads_files, writes_files

    flow:
        inspect_docs(scope)
        apply_doc_changes()
        "Mention any docs you could not verify locally."
```

Notes:

- The `description:` block is for the skill selector — not runtime context.
- `preserve_existing_patterns` and `unrelated_edits` resolve to `const` declarations (in this file or imported). They could equally be inline strings: `require "Prefer existing helpers and naming."`.
- The trailing inline string in `flow:` is an instruction (`Step`), not context.

---

## 2. Skill with calls, imports, locals, and a `with` modifier

Demonstrates `import`, locals, named arguments, and call-site framing.

```glyph
// File: fix_bug.glyph.md
import "./repo_tools.glyph.md" as repo_tools
import "./prefs.glyph.md" { validation_strictness }

const root_cause_before_fix = """
Identify the root cause before proposing or applying a fix.
"""

skill fix_bug(scope = ".", risk = "medium")
    require root_cause_before_fix
    must avoid skipping_or_xfailing_tests

    effects: reads_files, writes_files, runs_commands

    flow:
        ctx = repo_tools.inspect_repo(scope)
        plan = make_plan(ctx, risk = risk)
        apply_changes(plan) with "stay surgical — touch only the failing path"
        validate(plan)
        return summarize(plan)

block make_plan(ctx, risk = "medium") -> Plan
    flow:
        analyze(ctx)
        return draft_plan(ctx, risk)
```

Notes:

- `repo_tools.inspect_repo(scope)` is a qualified callee — `repo_tools` is a whole-module alias.
- `risk = risk` is a named argument; the right side is the parameter binding from the skill header.
- `with "stay surgical ..."` shapes only this one `apply_changes` invocation.
- `must avoid` is a hard prohibition; it compiles to stronger language than `avoid`.
- `make_plan` is a private `block` — file-scoped, no `effects:` required because its only effects are inferred from inner calls.

---

## 3. Skill with branching and branch-scoped constraint

Demonstrates `if`/`elif`/`else`, condition forms, and a constraint that applies only inside one branch.

```glyph
// File: review_pr.glyph.md
skill review_pr(pr_id, depth = "standard")
    effects: reads_files, runs_commands

    flow:
        ctx = load_pr(pr_id)
        risk = assess_risk(ctx)

        if risk == "high" and ctx.has_tests:
            run_full_suite(ctx)
            must request_review
        elif risk == "high":
            "Flag for manual review — no test suite available."
        elif depth == "standard":
            run_smoke_tests(ctx)
        else:
            "Light review only; skip executable checks."

        return summarize(ctx)
```

Notes:

- `must request_review` lives inside the `if` body — it is **branch-scoped**. It governs only the high-risk-with-tests path, not the whole skill.
- Conditions use `==`, `and`, and a single-level dot access (`ctx.has_tests`). No `<` / `>`.
- `return` is at the top level of `flow:`, after the conditional.

---

## 4. Library file: `export const`, `export block`

A library file (zero `skill`s) shipping reusable constants and a procedure.

```glyph
// File: prefs.glyph.md
export const safety_rules = """
Never execute destructive operations without confirmation.
"""

export const preserve_existing_patterns = """
Prefer the repository's existing patterns, helper APIs, naming, and file
organization before introducing a new abstraction or style.
"""

export const default_max_attempts = 3

export block validate_changes(scope = ".", strict = true)
    description: "Use to verify changes after editing source."
    effects: reads_files, runs_commands

    flow:
        run_typecheck(scope)
        run_tests(scope) with "fail loudly on any new failure"
        if strict:
            run_linter(scope)
        return none
```

Notes:

- Library file: zero `skill` declarations, ≥1 `export` declaration. ✓
- One `const` keyword for both string (`safety_rules`) and integer (`default_max_attempts`) constants — the compiler infers the kind from the literal. The old `export text`/`export int`/`export float` keywords are gone.
- `export block` parameters **must** have defaults (`scope = "."`, `strict = true`). ✓
- `validate_changes` has no meaningful return value, so its header **omits `->`** entirely. The body still ends with explicit `return none` — that is required on `export block`. (Do not write `-> None`; that form is gone.)
- The `if strict:` inside `flow:` does not break the single-`return` rule because the `return` is at top level *after* the conditional.

---

## 5. Subagent skill using `@glyph/std`

Demonstrates spawning a subagent and following up via UFCS.

```glyph
// File: investigate.glyph.md
import "@glyph/std" { subagent, send }

skill investigate(area)
    description: "Use when the user wants a focused investigation of a code area."

    effects: spawns_agent

    flow:
        researcher = subagent(area) with "investigate {area} thoroughly"
        researcher.send("Pay special attention to authentication flows in {area}.")
        researcher.send("Also check input validation at every boundary.")
        return researcher
```

Notes:

- `area` is a runtime-required parameter — no default — so the LLM must extract it from the user's request.
- `effects: spawns_agent` is mandatory because the body calls stdlib functions that carry that effect.
- `researcher = subagent(...) with "..."` binds the returned `Agent`; the modifier shapes the spawn prose at this one site.
- Both follow-ups use UFCS: `researcher.send(msg)` reads as "tell the researcher ..." and desugars to `send(researcher, msg)`.
- Returning `researcher` returns the **handle**, so the caller can keep interacting with the agent. If you want to return the agent's findings instead, return an inline string describing them.

---

## 6. `{name}` slots and `<name>` / `<"description">` output targets

Demonstrates the four name forms working together: `{name}` for in-string references, `name` for ordinary identifiers, and `<name>` / `<"description">` for outputs the agent must synthesize from prose-guided work.

```glyph
// File: cleanup_branches.glyph.md
const stale_age_days = 60

skill cleanup_branches(repo_path, dry_run = true)
    description: "Use when the user asks to clean up local git branches merged into main."

    effects: reads_files, runs_commands, asks_user

    must avoid "deleting any branch named `main` or `master`"
    must avoid "deleting the currently checked-out branch"

    flow:
        "Verify {repo_path} is a git working tree by running `git -C {repo_path} rev-parse --is-inside-work-tree`."

        current_branch = read_current_branch(repo_path)
        candidates = list_stale_merged(repo_path, current_branch)

        if dry_run:
            "Print the candidates in {candidates} as a DRY RUN report. Do not delete."
        else:
            confirmed = ask_user_to_confirm(candidates)
            if confirmed:
                delete_branches(repo_path, candidates, current_branch)
            else:
                "Report that the user declined and exit."

block read_current_branch(repo_path) -> BranchName
    flow:
        "Run `git -C {repo_path} rev-parse --abbrev-ref HEAD` and capture the branch name. If HEAD is detached, use the literal `DETACHED`."
        return <current_branch>

block list_stale_merged(repo_path, current_branch) -> StaleCandidates
    flow:
        "Run `git -C {repo_path} branch --merged main`. Drop `main`, `master`, and {current_branch}. Keep only branches whose last-commit date is older than {stale_age_days} days."
        return <"the filtered list of stale branch names safe to delete">

block ask_user_to_confirm(candidates) -> Confirmation
    effects: asks_user
    flow:
        "Show {candidates} and ask: `Delete these N branches? (yes/no)`. Treat only an unambiguous affirmative as true."
        return <confirmed>

block delete_branches(repo_path, candidates, current_branch)
    effects: runs_commands
    flow:
        "For each branch in {candidates}, re-check it is not `main`, `master`, or {current_branch}. Run `git -C {repo_path} branch -d <branch>` (the lowercase `-d`, never `-D`) and report the result."
        return none
```

Notes:

- `{repo_path}`, `{current_branch}`, `{candidates}`, `{stale_age_days}` are all in-string slot references. The compiler resolves each: parameters become runtime slots in the compiled prompt; bindings get inlined as natural-prose cross-references; `const` values are inlined as their values.
- `const stale_age_days = 60` uses the unified `const` keyword — the compiler infers integer kind from the literal `60`. (No more `int stale_age_days = 60`.)
- Backticks `` `main` ``, `` `master` ``, `` `git -C ... ` `` frame **literal code** — these are not Glyph names.
- `<current_branch>`, `<confirmed>` are **identifier-form** output targets; `<"the filtered list of stale branch names safe to delete">` is the **descriptive form** — quoted prose telling the agent what to synthesize. Use either form for prose-produced values; do **not** write `return "<current_branch>"` (string literal) — that's the anti-pattern this form replaces.
- Return types are **domain types** (`-> BranchName`, `-> StaleCandidates`, `-> Confirmation`) — never primitive names like `-> Text`/`-> List`/`-> Bool`. Domain types are implicitly declared by first use.
- `delete_branches` has no meaningful return, so its header **omits `->` entirely**. The body still returns `none`.
- The thin blocks (`read_current_branch`, `list_stale_merged`, etc.) are the idiom: they're how you give a name (`current_branch`, `candidates`) to a value that comes from prose-guided work. The `flow:` reads as data flow.

---

## Common shapes and when to reach for them

| You want to ... | Reach for ... |
|---|---|
| One-off rule for this skill | inline string: `must avoid "leaving stale TODOs"` |
| A rule reused across several skills | `export const` in a library, then `import` it |
| A multi-step procedure that other skills will call | `export block` in a library |
| A small file-private helper | `block` (no `export`) |
| Dispatching to another agent | `@glyph/std` `subagent` + `send` |
| Branching on a Boolean condition | `if/elif/else` inside `flow:` |
| Branching that needs `<` or arithmetic | bind a `block` call result, then `if <bound_name>:` |
| Per-call-site framing the callee body shouldn't carry | `<call> with "..."` |
| A loop (deferred) | inline-string instruction: `"For each X, do Y."` |
