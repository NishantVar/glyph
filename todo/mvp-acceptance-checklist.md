# MVP Acceptance Checklist

Implementation checklist for the Glyph compiler MVP. Public-facing
acceptance contract lives in [[mvp-acceptance]]; the
walking-skeleton illustration lives in
[[mvp-walking-skeleton]]. This file is the working
list of "things still to do" before the MVP is shippable.

## Test corpus layout

```
tests/corpus/
├── valid/              # Compiles cleanly (exit 0); golden .md snapshot
├── repairable/         # Stops at Phase 2 (exit 2); golden diagnostic JSON
├── invalid/            # Hard-fails (exit 1); golden diagnostic JSON
└── multi-file/         # The 5-skill acceptance project
```

Snapshot tests via `insta`, located in `glyph-cli/tests/`.

### `valid/`

| File | What it tests |
|------|---------------|
| `update_docs.glyph` | Walking skeleton. Parameterless, inline strings only, explicit const defs, no calls. |
| `fix_bug.glyph` | Flagship. Parameters with defaults, `block` defs, `const` defs, `with` modifier (stored in IR, not applied in `.md`), `return`, constraint markers at body level. Exercises call expansion, projection tier assignment, return folding. |
| `constraint_only.glyph` | Skill with `constraints:` section but no `flow:`. Tests `## Steps` omission in output. |
| `branching.glyph` | Skill with `if`/`elif`/`else` in flow. Tests conditional projection (lettered sub-steps per arm). `==` in `if` conditions is branch-condition syntax, not a value-level operator. |
| `effects_over_declared.glyph` | Skill that declares more effects than inferred. Compiles successfully; emits `G::analyze::effects-over-declared` warning (stderr). |
| `explicit_blocks.glyph` | Skill with 4+ statement private block. Tests Tier 2 same-file procedure projection. |
| `library_text_only.glyph` | Library file with only `export const` constants. Tests zero `.md` emission, no error. |
| `library_with_blocks.glyph` | Library file with `export block` declarations. Tests library compilation path and procedure emission rules. |
| `predicate_const_single_arm.glyph` | Single-arm `if` guarded by a string-kinded `const` predicate. Tests `predicate_const` token kind, `resolved_predicates` map population, and pure-predicate branch projection with one arm. |
| `predicate_const_multi_arm.glyph` | Multi-arm `if`/`elif`/`else` using two string-const predicates. Tests that all arms contribute to `resolved_predicates` and that the pure-predicate "decide which applies" projection fires. |
| `predicate_inline_literal.glyph` | `if` condition is a quoted string literal. Tests `predicate_literal` token kind, that no `resolved_predicates` map entry is written for literals, and that the literal prose appears verbatim in the branch projection header. |
| `predicate_mixed.glyph` | `if` arm mixes a string-const predicate with `and not is_dry_run`. Tests `predicate_const` + `boolean` token kinds in one condition, mixed-condition branch projection, and that `resolved_predicates` is populated only for the predicate token. |

### `repairable/` (exit code 2)

| File | Diagnostics exercised |
|------|----------------------|
| `novice_fix_bug.glyph` | `G::analyze::undefined-name`, `G::analyze::undefined-call` |
| `novice_simple.glyph` | `G::analyze::undefined-name`, `G::analyze::ambiguous-role` |
| `indent_tabs.glyph` | `G::parse::tab-indent` |
| `mixed_indent.glyph` | `G::parse::mixed-indent` |
| `nested_branch.glyph` | `G::analyze::nested-branch` |
| `missing_effects.glyph` | `G::analyze::missing-effects` |
| `duplicate_import.glyph` | `G::analyze::duplicate-import` |
| `unused_import.glyph` | `G::analyze::unused-import` |
| `duplicate_subsection.glyph` | `G::parse::duplicate-subsection` |
| `operator_expr.glyph` | `G::parse::operator-in-expression` |
| `param_slot_default.glyph` | `G::parse::param-slot-in-non-instruction-string` |
| `stdlib_missing_import.glyph` | `G::analyze::stdlib-missing-import` |
| `missing_return.glyph` | `G::analyze::missing-return` |
| `missing_description.glyph` | `G::analyze::missing-description` |

### `invalid/` (exit code 1)

| File | Expected diagnostic |
|------|-------------------|
| `empty.glyph` | `G::parse::empty-file` |
| `empty_flow.glyph` | `G::parse::empty-flow` |
| `two_skills.glyph` | `G::parse::multiple-skills` |
| `nested_flow.glyph` | `G::parse::nested-flow` |
| `none_with_effects.glyph` | `G::parse::none-with-effects` |
| `chained_with.glyph` | `G::parse::multiple-with` |
| `with_bare_name.glyph` | `G::parse::with-on-bare-name` |
| `return_mid_flow.glyph` | `G::parse::return-not-terminal` |
| `return_in_branch.glyph` | `G::parse::return-in-branch` |
| `two_returns.glyph` | `G::parse::multiple-returns` |
| `name_collision.glyph` | `G::analyze::name-collision` |
| `import_private.glyph` | `G::analyze::import-private` |
| `import_skill.glyph` | `G::analyze::import-skill` |
| `circular_import_a.glyph` + `_b.glyph` | `G::analyze::circular-import` |
| `missing_file.glyph` | `G::analyze::missing-file` |
| `effects_under.glyph` | `G::analyze::effects-under-declared` |
| `empty_skill.glyph` | `G::analyze::empty-skill-body` |
| `missing_required_arg.glyph` / `export_block_missing_required_arg.glyph` / `imports/missing_required_arg_imported.glyph` | `G::analyze::missing-required-arg` |
| `bad_param_slot.glyph` | `G::analyze::unknown-param-slot` |
| `closure_leak.glyph` | `G::analyze::closure-violation` |
| `library_no_exports.glyph` | `G::analyze::no-exports-in-library` |
| `import_unknown_stdlib.glyph` | `G::imports::unknown-stdlib-module` |

## Multi-file acceptance project

A 5-skill project in `tests/corpus/multi-file/` that exercises imports,
DAG resolution, library files, branching, and standalone compilation.
All files are fully valid (no repair needed) and compile end-to-end
with exit code 0.

Import DAG:

```
prefs.glyph ◄──── fix_bug.glyph
                         │
repo_tools.glyph ◄───┤
         ▲               │
         └────── review_pr.glyph

update_docs.glyph  (standalone, no imports)
```

Topological order: `prefs` → `repo_tools` → {`fix_bug`, `review_pr`, `update_docs`}.

### `prefs.glyph` — Preferences library

```glyph
// Team-wide coding preferences.

export const preserve_existing_patterns = """
Prefer the repository's existing patterns, helper APIs, naming, and file
organization before introducing a new abstraction or style.
"""

export const safety_first = """
Never execute destructive operations without explicit confirmation.
"""

export const minimal_changes = """
Make the smallest change that solves the problem.
"""
```

Tests: Library with only `export const`. Zero `.md` emission. Names
importable by consumers.

### `repo_tools.glyph` — Reusable procedures library

```glyph
export block inspect_repo(scope = ".") -> Report
    effects: reads_files

    flow:
        "Read the project structure in {scope}."
        "Identify relevant source files and their relationships."
        "Note any configuration files, test suites, and documentation."
        return "Produce a summary report of the repository layout and key files."

export block run_tests(scope = ".") -> TestResult
    effects: reads_files, runs_commands

    flow:
        "Identify the test framework used in {scope}."
        "Run the existing test suite."
        "Collect pass/fail results and any error output."
        return "Produce a structured test result with pass count, fail count, and failure details."
```

Tests: Library with `export block` declarations. Each block has
explicit effects, parameters with defaults, return type, and
multi-statement flow. Tests Tier 2/3 projection decisions at consumer
call sites.

### `fix_bug.glyph` — Flagship skill

```glyph
import "./prefs.glyph" { preserve_existing_patterns }
import "./repo_tools.glyph" { inspect_repo }

skill fix_bug(scope = ".")
    description: "Debug and fix a bug in the codebase with minimal, targeted changes."
    require preserve_existing_patterns
    avoid unrelated_edits

    effects: reads_files, writes_files, runs_commands

    flow:
        inspect_repo(scope) with "focus on the area where the bug was reported"
        identify_root_cause()
        "Don't propose a fix until you've confirmed the root cause."
        patch_minimally()
        validate_fix()
        return summarize_changes()

const unrelated_edits = "Making changes outside the requested scope or fixing unrelated issues."

block identify_root_cause()
    flow:
        "Trace the reported symptoms to their origin."
        "Confirm the root cause with evidence from logs, tests, or code inspection."

block patch_minimally()
    flow:
        "Apply the smallest change that fixes the root cause."
        "Preserve existing patterns and avoid unnecessary refactoring."

block validate_fix()
    flow:
        "Verify the fix resolves the original issue."
        "Run related tests to check for regressions."

block summarize_changes()
    flow:
        "List what was changed and why."
```

Tests: Selective import from two libraries. Cross-file name
resolution. `with` modifier on imported call (stored in IR;
mechanical `.md` uses resolved body text without modifier
application). Mix of imported const, local const, local blocks.
Tier 1 inline (small blocks) + Tier 2 same-file procedure (if any
block exceeds threshold). Return folding. Constraint rendering with
imported `preserve_existing_patterns`.

### `review_pr.glyph` — Skill with branching

```glyph
import "./repo_tools.glyph" { inspect_repo, run_tests }

skill review_pr(scope = ".", risk = "medium")
    description: "Review a pull request for correctness, style, and safety."
    require thorough_review
    require check_tests

    effects: reads_files, runs_commands

    flow:
        inspect_repo(scope) with "focus on changed files in the PR"
        if risk == "high":
            run_tests(scope)
            "Verify no security-sensitive code paths are affected."
        else:
            "Spot-check test coverage for changed code."
        "Summarize findings with actionable feedback."
        return "Produce a structured review with approval status and comments."

const thorough_review = "Review every changed file, not just the ones that look interesting."
const check_tests = "Verify that tests exist for changed behavior and that they pass."
```

Tests: Branching (`if`/`else`) with conditional projection. Multiple
imports from same library. Parameters with defaults. Two constraint
markers. Imported `export block` called in branch body. Tests
lettered sub-step rendering.

### `update_docs.glyph` — Standalone skill

Same as the walking skeleton ([[mvp-walking-skeleton]]).
Tests standalone compilation with no imports alongside import-heavy
siblings.

## Diagnostic-ID coverage matrix

Every diagnostic below must have at least one triggering test.

### Compiler-scope diagnostics

**Parse phase (17):**

| ID | Classification |
|----|---------------|
| `G::parse::tab-indent` | repairable |
| `G::parse::mixed-indent` | repairable |
| `G::parse::nested-flow` | error |
| `G::parse::none-with-effects` | error |
| `G::parse::multiple-with` | error |
| `G::parse::with-on-bare-name` | error |
| `G::parse::operator-in-expression` | repairable |
| `G::parse::param-slot-in-non-instruction-string` | repairable |
| `G::parse::return-not-terminal` | error |
| `G::parse::return-in-branch` | error |
| `G::parse::multiple-returns` | error |
| `G::parse::duplicate-subsection` | repairable |
| `G::parse::empty-file` | error |
| `G::parse::empty-flow` | error |
| `G::parse::multiple-skills` | error |
| `G::parse::applies-no-parens` | error |
| `G::parse::applies-with-args` | error |

**Analyze phase (26):**

| ID | Classification |
|----|---------------|
| `G::analyze::undefined-name` | repairable |
| `G::analyze::undefined-call` | repairable |
| `G::analyze::name-collision` | error |
| `G::analyze::import-private` | error |
| `G::analyze::import-skill` | error |
| `G::analyze::circular-import` | error |
| `G::analyze::missing-file` | error |
| `G::analyze::duplicate-import` | repairable |
| `G::analyze::unused-import` | repairable |
| `G::analyze::ambiguous-role` | repairable |
| `G::analyze::effects-under-declared` | error |
| `G::analyze::effects-over-declared` | warning |
| `G::analyze::missing-effects` | repairable |
| `G::analyze::missing-return` | repairable |
| `G::analyze::typed-decl-missing-return` | error |
| `G::analyze::closure-violation` | error |
| `G::analyze::stdlib-missing-import` | repairable |
| `G::analyze::unknown-param-slot` | error |
| `G::analyze::nested-branch` | repairable |
| `G::analyze::empty-skill-body` | error |
| `G::analyze::no-exports-in-library` | error |
| `G::analyze::missing-required-arg` | error |
| `G::analyze::missing-description` | repairable |
| `G::analyze::const-in-flow` | repairable |
| `G::analyze::applies-on-non-block` | error |
| `G::analyze::applies-on-undescribed-block` | repairable |

**Imports phase (1):**

| ID | Classification |
|----|---------------|
| `G::imports::unknown-stdlib-module` | error |

**Validate phase (5):**

| ID | Classification |
|----|---------------|
| `G::validate::duplicate-node-id` | error |
| `G::validate::unresolved-callee` | error |
| `G::validate::malformed-branch` | error |
| `G::validate::recursive-call` | error |
| `G::validate::empty-step` | error |

**Build phase (1):**

| ID | Classification |
|----|---------------|
| `G::build::skipped-due-to-failed-import` | warning |

**Validate-output phase (22):** Phase 6b structural validation, implemented in `glyph validate-output`. These diagnostics check that Step 2's Markdown output faithfully projects the input IR. All are classification `error`.

| ID | Classification |
|----|---------------|
| `G::expand::extra-h2` | error |
| `G::expand::step-count-mismatch` | error |
| `G::expand::substep-count-mismatch` | error |
| `G::expand::constraint-count-mismatch` | error |
| `G::expand::context-count-mismatch` | error |
| `G::expand::step-order-mismatch` | error |
| `G::expand::invented-param-ref` | error |
| `G::expand::dropped-param-ref` | error |
| `G::expand::unresolved-local-ref` | error |
| `G::expand::modifier-leaked` | error |
| `G::expand::params-section-mismatch` | error |
| `G::expand::params-section-missing` | error |
| `G::expand::params-section-spurious` | error |
| `G::expand::frontmatter-returned` | error |
| `G::expand::malformed-markdown` | error |
| `G::expand::procedure-count-mismatch` | error |
| `G::expand::procedure-name-mismatch` | error |
| `G::expand::procedure-step-count-mismatch` | error |
| `G::expand::procedure-ref-missing` | error |
| `G::expand::procedure-ref-dangling` | error |
| `G::expand::procedure-duplicate` | error |
| `G::expand::procedure-order` | error |

**Total: 72 compiler-scope diagnostic IDs** (17 Parse + 26 Analyze + 1 Imports + 5 Validate + 1 Build + 22 Validate-output).

### Agent-scope diagnostics (not in compiler)

These diagnostics are the responsibility of the external agent skill
that drives Repair (Phase 3) and Expand Step 2 (Phase 6). They are
part of the Glyph spec but not implemented in the compiler binary.

- **Repair notifications (5):** `G::repair::generated-const`, `G::repair::generated-block`, `G::repair::branch-extracted`, `G::repair::inferred-effects`, `G::repair::constraint-tension`
- **Repair execution failures (5):** `G::repair::llm-unavailable`, `G::repair::output-invalid`, `G::repair::no-convergence`, `G::repair::constraint-contradiction`, `G::repair::constraint-scan-malformed`
- **Expand Step 2 execution (1):** `G::expand::llm-unavailable`

**Total: 11 agent-scope diagnostic IDs.**

### Post-MVP diagnostics

| ID | Classification | Reason deferred |
|----|---------------|-----------------|
| `G::analyze::nominal-mismatch` | error | Requires richer type-checking scenarios beyond MVP skill complexity |
| `G::analyze::lossy-coercion` | error | Requires numeric type boundary testing (float→int) |

### Testing strategy for Validate diagnostics

Validate phase (Phase 5) diagnostics fire on malformed IR that passes
Phase 2 but violates post-Lower invariants. These are rare in normal
compilation. Testing strategy:

1. **Unit tests with hand-crafted invalid IR.** Construct IR nodes that violate specific invariants (duplicate node IDs, unresolved callees, cyclic call graphs, empty steps), feed directly to the Validate phase.
2. **One unit test per Validate diagnostic ID.**

## Exit-criteria checklist (MVP done when all five bars are met)

### Bar 1: All deterministic phases implemented

- [ ] Phase 1 (Parse) implemented and exercised by the test corpus.
- [ ] Phase 2 (Analyze) implemented and exercised.
- [ ] Phase 4 (Lower) implemented and exercised.
- [ ] Phase 5 (Validate) implemented and exercised.
- [ ] Phase 6-Step 1 (deterministic Expand) implemented and exercised.
- [ ] Phase 7 (Emit) implemented and exercised.

The compiler produces mechanical `.md` output and structured `.ir.json`
from valid source.

### Bar 2: Deterministic output

- [ ] Every file in `tests/corpus/valid/` and `tests/corpus/multi-file/`
      produces byte-identical `.md` output across runs.
- [ ] Same files produce byte-identical `.ir.json` when `--emit-ir` is set.
- [ ] Same files produce byte-identical diagnostic JSON under
      `--format=json`.
- [ ] No `HashMap` anywhere on a `Serialize` path reachable from CLI output
      (enforced per ADR 0006).
- [ ] Diagnostic arrays sorted by `(file, span.start.byte, id)` (ADR 0006).

### Bar 3: Multi-file project compiles

- [ ] `prefs.glyph` compiles (exit 0, zero `.md` emission).
- [ ] `repo_tools.glyph` compiles (exit 0, emits procedure files if blocks exceed Tier 1 threshold).
- [ ] `fix_bug.glyph` compiles (exit 0, imports from prefs + repo_tools resolve).
- [ ] `review_pr.glyph` compiles (exit 0, imports from repo_tools, branching works).
- [ ] `update_docs.glyph` compiles (exit 0, standalone).
- [ ] DAG order respected: libraries compile before consumers.
- [ ] Cross-file name resolution works.

### Bar 4: `--strict` mode

- [ ] `--strict` treats `repairable` diagnostics as hard errors (exit code 1 instead of 2).
- [ ] Succeeds on every file in `tests/corpus/valid/` and `tests/corpus/multi-file/`.
- [ ] Fails on every file in `tests/corpus/repairable/`.

### Bar 5: Diagnostic coverage

- [ ] Every compiler-scope diagnostic ID (72 total) has at least one triggering test.
- [ ] Parse and Analyze diagnostics triggered by corpus files in `invalid/` and `repairable/`.
- [ ] Validate diagnostics triggered by unit tests with hand-crafted invalid IR.
- [ ] Validate-output diagnostics (22 Phase 6b) triggered by unit tests feeding crafted `.ir.json` + `.md` pairs to `glyph validate-output`.
- [ ] Build diagnostic triggered by multi-file test with a deliberately failed dependency.
- [ ] Warnings (`effects-over-declared`) triggered by `valid/` corpus file that compiles successfully with warning on stderr.

## Snapshot testing with `insta`

All snapshot tests use the `insta` crate for Rust, located in `glyph-cli/tests/`.

### Test organization

```rust
#[test]
fn valid_update_docs() {
    let result = glyph_compile("tests/corpus/valid/update_docs.glyph");
    assert_eq!(result.exit_code, 0);
    insta::assert_snapshot!(result.emitted_md("update_docs.md"));
}

#[test]
fn repairable_novice_fix_bug() {
    let result = glyph_compile("tests/corpus/repairable/novice_fix_bug.glyph");
    assert_eq!(result.exit_code, 2);
    insta::assert_snapshot!(result.diagnostics_json());
    assert!(result.has_diagnostic("G::analyze::undefined-name"));
    assert!(result.has_diagnostic("G::analyze::undefined-call"));
}

#[test]
fn invalid_empty_file() {
    let result = glyph_compile("tests/corpus/invalid/empty.glyph");
    assert_eq!(result.exit_code, 1);
    insta::assert_snapshot!(result.diagnostics_json());
    assert!(result.has_diagnostic("G::parse::empty-file"));
}

#[test]
fn strict_rejects_repairable() {
    let result = glyph_compile_strict("tests/corpus/repairable/novice_fix_bug.glyph");
    assert_eq!(result.exit_code, 1);
}
```

### Snapshot review workflow

1. Run tests — `insta` creates `.snap.new` files for new/changed snapshots.
2. Review with `cargo insta review` — accept or reject each change.
3. Commit accepted `.snap` files alongside source changes.

### What to snapshot

- `valid/`: compiled `.md` output + exit code 0.
- `repairable/`: diagnostic JSON output + exit code 2.
- `invalid/`: diagnostic JSON output + exit code 1.
- `multi-file/`: compiled `.md` for each skill file + build summary (DAG order, emission list) + exit code 0.
