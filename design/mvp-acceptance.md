# MVP Acceptance Criteria

This document defines the walking skeleton, test corpus structure, multi-file acceptance project, and exit criteria for the Glyph compiler MVP.

**Architectural premise:** The compiler (`glyph-core` + `glyph-cli`) is fully deterministic. It implements Phases 1 (Parse), 2 (Analyze), 4 (Lower), 5 (Validate), 6-Step 1 (deterministic Expand), and 7 (Emit). Phase 3 (Repair) and Phase 6-Step 2 (LLM prose reshaping) are handled externally by an agent skill that reads the compiler's diagnostic/IR output, acts on it, and re-invokes the compiler. See `build-foundation.md` for the crate structure and CLI contract.

**Exit codes:** `0` = success (all phases complete, `.md` emitted), `1` = hard errors (compilation cannot proceed), `2` = repairable diagnostics only (pipeline stops after Phase 2, agent can repair and re-invoke), `3` = invocation error (bad flags, missing path, permission denied, IO failure).

**Agent interaction model:**
1. Agent invokes `glyph compile foo.glyph.md --format json --emit-ir`.
2. If exit code 2: agent reads JSON diagnostics from stdout, repairs source, re-invokes.
3. If exit code 0: compiler ran Phases 1→2→4→5→6-Step1→7, wrote `foo.md` (mechanical expansion) + `foo.ir.json` (full typed IR with `with` modifiers, projection tiers, etc.).
4. Agent optionally post-processes `foo.md` using `foo.ir.json` for LLM-quality prose (Step 2 reshaping, `with` modifier application). This is outside the compiler's scope.


## 1. Walking Skeleton (v0.0.1)

The walking skeleton is the first thing that compiles end-to-end via `glyph compile`. It forces every deterministic compiler phase to exist — even if most are trivial pass-throughs.

**Design constraints:**
- Parameterless (no `## Parameters` in output)
- All names explicitly defined (zero `repairable` diagnostics → pipeline does not stop at Phase 2)
- All flow items are inline strings (no `Call` nodes → Expand Step 1 is trivial, no `with` modifiers)
- Explicit `effects:` and `description:` (no inference gaps)
- No imports (single-file)

### Source: `update_docs.glyph.md`

```glyph
skill update_docs()
    description: "Update repository documentation to match current code."
    require accuracy
    avoid stale_references

    effects: reads_files, writes_files

    flow:
        "Scan the repository for files with documentation."
        "Compare each document against the current code for accuracy."
        "Update any sections that are outdated or incorrect."
        "Verify all cross-references and links are still valid."

const accuracy = "Ensure all documentation accurately reflects the current code."
const stale_references = "Leaving references to removed or renamed symbols."
```

### Expected output: `update_docs.md`

```md
---
name: update_docs
description: Update repository documentation to match current code.
effects: [reads_files, writes_files]
---

## Instructions

### Steps

1. Scan the repository for files with documentation.
2. Compare each document against the current code for accuracy.
3. Update any sections that are outdated or incorrect.
4. Verify all cross-references and links are still valid.

### Constraints

- Ensure all documentation accurately reflects the current code.
- Do not leave references to removed or renamed symbols.
```

### Phase-by-phase walkthrough

| Phase | What happens |
|-------|-------------|
| 1 Parse | Parses skill header, `const` declarations, constraint markers, `flow:` with 4 inline strings. No imports → trivial DAG. |
| 2 Analyze | `accuracy` and `stale_references` resolve to same-file `const` bindings. `require`/`avoid` markers set constraint role+polarity. Effects match (declared ⊇ inferred). Zero diagnostics → pipeline continues. |
| 4 Lower | Inline strings become `InlineInstruction` nodes with `role: Step`. Constraint markers + resolved text become `Constraint` nodes with strength/polarity. Node IDs assigned. |
| 5 Validate | All checks pass: node IDs unique, no unresolved callees, no cycles, no empty steps. |
| 6 Step 1 | `const` refs on constraints already resolved to strings. Inline strings pass through. No `Call` nodes → no projection tier decisions. |
| 7 Emit | Assembles frontmatter (name, description, effects), `## Instructions` with `### Steps` (4 items) and `### Constraints` (2 items). No `## Parameters`, no `### Context`. Writes `update_docs.md`. |

### CLI test

```bash
glyph compile tests/corpus/valid/update_docs.glyph.md --format json
# Exit code: 0
# Writes: update_docs.md (matches golden snapshot)
# Stdout JSON: { "diagnostics": [], "emitted": ["update_docs.md"] }
```

**v0.0.1 passes when:** `glyph compile update_docs.glyph.md` exits 0 and produces byte-identical `update_docs.md` on every run.


## 2. Test Corpus

Structure: `tests/corpus/` with four subdirectories. Snapshot tests via `insta` in `glyph-cli/tests/`.

```
tests/corpus/
├── valid/              # Compiles cleanly (exit 0); golden .md snapshot
├── repairable/         # Stops at Phase 2 (exit 2); golden diagnostic JSON snapshot
├── invalid/            # Hard-fails (exit 1); golden diagnostic JSON snapshot
└── multi-file/         # The 5-skill acceptance project (§3)
```

### 2.1 `valid/` — Clean compilation (exit code 0)

Each file compiles end-to-end without diagnostics (or with only warnings). Snapshot = compiled `.md` output.

The compiler produces **mechanical expansion** — resolved body text from Step 1, without LLM prose reshaping. `with` modifiers are recorded in the IR (`.ir.json`) but not applied to the `.md`. Call bodies are expanded using their resolved text. This is structurally correct output; the agent's optional Step 2 post-processing refines prose quality.

| File | What it tests |
|------|---------------|
| `update_docs.glyph.md` | Walking skeleton. Parameterless, inline strings only, explicit const defs, no calls. |
| `fix_bug.glyph.md` | Flagship. Parameters with defaults, `block` defs, `const` defs, `with` modifier (stored in IR, not applied in `.md`), `return`, constraint markers at body level. Exercises call expansion, projection tier assignment, return folding. |
| `constraint_only.glyph.md` | Skill with `constraints:` section but no `flow:`. Tests `### Steps` omission in output. |
| `branching.glyph.md` | Skill with `if`/`elif`/`else` in flow. Tests conditional projection (lettered sub-steps per arm). Note: `==` in `if` conditions is branch-condition syntax, not a value-level operator — does not trigger `G::parse::operator-in-expression`. |
| `effects_over_declared.glyph.md` | Skill that declares more effects than inferred. Compiles successfully; emits `G::analyze::effects-over-declared` warning (stderr). |
| `explicit_blocks.glyph.md` | Skill with 4+ statement private block. Tests Tier 2 same-file procedure projection. |
| `library_text_only.glyph.md` | Library file with only `export const` constants. Tests zero `.md` emission, no error. |
| `library_with_blocks.glyph.md` | Library file with `export block` declarations. Tests library compilation path and procedure emission rules. |

### 2.2 `repairable/` — Stops at Phase 2 (exit code 2)

Each file has `repairable` diagnostics. The compiler stops after Phase 2 and emits diagnostics as JSON on stdout. It does **not** repair the source — that's the agent's job. Snapshot = diagnostic JSON output.

| File | Diagnostics exercised |
|------|----------------------|
| `novice_fix_bug.glyph.md` | `G::analyze::undefined-name`, `G::analyze::undefined-call` |
| `novice_simple.glyph.md` | `G::analyze::undefined-name`, `G::analyze::ambiguous-role` |
| `indent_tabs.glyph.md` | `G::parse::tab-indent` |
| `mixed_indent.glyph.md` | `G::parse::mixed-indent` |
| `nested_branch.glyph.md` | `G::analyze::nested-branch` |
| `missing_effects.glyph.md` | `G::analyze::missing-effects` |
| `duplicate_import.glyph.md` | `G::analyze::duplicate-import` |
| `unused_import.glyph.md` | `G::analyze::unused-import` |
| `duplicate_subsection.glyph.md` | `G::parse::duplicate-subsection` |
| `operator_expr.glyph.md` | `G::parse::operator-in-expression` |
| `param_slot_default.glyph.md` | `G::parse::param-slot-in-non-instruction-string` |
| `stdlib_missing_import.glyph.md` | `G::analyze::stdlib-missing-import` |
| `missing_return.glyph.md` | `G::analyze::missing-return` |
| `missing_description.glyph.md` | `G::analyze::missing-description` |

### 2.3 `invalid/` — Hard-fail (exit code 1)

Each file must fail with specific diagnostic IDs. Snapshot = diagnostic JSON output.

When a file contains both `error` and `repairable` diagnostics, exit code is `1` (errors win over repairables).

| File | Expected diagnostic |
|------|-------------------|
| `empty.glyph.md` | `G::parse::empty-file` |
| `empty_flow.glyph.md` | `G::parse::empty-flow` |
| `two_skills.glyph.md` | `G::parse::multiple-skills` |
| `nested_flow.glyph.md` | `G::parse::nested-flow` |
| `none_with_effects.glyph.md` | `G::parse::none-with-effects` |
| `chained_with.glyph.md` | `G::parse::multiple-with` |
| `with_bare_name.glyph.md` | `G::parse::with-on-bare-name` |
| `return_mid_flow.glyph.md` | `G::parse::return-not-terminal` |
| `return_in_branch.glyph.md` | `G::parse::return-in-branch` |
| `two_returns.glyph.md` | `G::parse::multiple-returns` |
| `name_collision.glyph.md` | `G::analyze::name-collision` |
| `import_private.glyph.md` | `G::analyze::import-private` |
| `import_skill.glyph.md` | `G::analyze::import-skill` |
| `circular_import_a.glyph.md` + `_b.glyph.md` | `G::analyze::circular-import` |
| `missing_file.glyph.md` | `G::analyze::missing-file` |
| `effects_under.glyph.md` | `G::analyze::effects-under-declared` |
| `empty_skill.glyph.md` | `G::analyze::empty-skill-body` |
| `missing_required_arg.glyph.md` / `export_block_missing_required_arg.glyph.md` / `imports/missing_required_arg_imported.glyph.md` | `G::analyze::missing-required-arg` (call sites that omit a positional argument for a parameter without a default — fires for private `block`, same-file `export block`, and imported `export block` callees; PRD #103 / Issues #104, #105) |
| `bad_param_slot.glyph.md` | `G::analyze::unknown-param-slot` |
| `closure_leak.glyph.md` | `G::analyze::closure-violation` |
| `library_no_exports.glyph.md` | `G::analyze::no-exports-in-library` |
| `import_unknown_stdlib.glyph.md` | `G::imports::unknown-stdlib-module` |


## 3. Multi-File Acceptance Project

A 5-skill project in `tests/corpus/multi-file/` that exercises imports, DAG resolution, library files, branching, and standalone compilation. All files are fully valid (no repair needed) and compile end-to-end with exit code 0.

### Import DAG

```
prefs.glyph.md ◄──── fix_bug.glyph.md
                         │
repo_tools.glyph.md ◄───┤
         ▲               │
         └────── review_pr.glyph.md

update_docs.glyph.md  (standalone, no imports)
```

Topological compile order: `prefs` → `repo_tools` → {`fix_bug`, `review_pr`, `update_docs`} (last three are independent).

### 3.1 `prefs.glyph.md` — Preferences library

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

**Tests:** Library with only `export const`. Zero `.md` emission. Names importable by consumers.

### 3.2 `repo_tools.glyph.md` — Reusable procedures library

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

**Tests:** Library with `export block` declarations. Each block has explicit effects, parameters with defaults, return type, and multi-statement flow. Tests Tier 2/3 projection decisions at consumer call sites.

### 3.3 `fix_bug.glyph.md` — Flagship skill

```glyph
import "./prefs.glyph.md" { preserve_existing_patterns }
import "./repo_tools.glyph.md" { inspect_repo }

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

**Tests:** Selective import from two libraries. Cross-file name resolution. `with` modifier on imported call (stored in IR; mechanical `.md` uses resolved body text without modifier application). Mix of imported const, local const, local blocks. Tier 1 inline (small blocks) + Tier 2 same-file procedure (if any block exceeds threshold). Return folding. Constraint rendering with imported `preserve_existing_patterns`.

### 3.4 `review_pr.glyph.md` — Skill with branching

```glyph
import "./repo_tools.glyph.md" { inspect_repo, run_tests }

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

**Tests:** Branching (`if`/`else`) with conditional projection. Multiple imports from same library. Parameters with defaults. Two constraint markers. Imported `export block` called in branch body. Tests lettered sub-step rendering.

### 3.5 `update_docs.glyph.md` — Standalone skill

Same as the walking skeleton (§1). Tests standalone compilation with no imports alongside import-heavy siblings.


## 4. Diagnostic Coverage

### 4.1 Compiler-scope diagnostics (MVP-required)

Every diagnostic below is emitted by the deterministic compiler (Phases 1, 2, 4, 5, 7) and must have at least one triggering test.

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

**Analyze phase (27):**

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
| `G::analyze::nominal-mismatch` | error |
| `G::analyze::lossy-coercion` | error |
| `G::analyze::missing-return` | repairable |
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

**Validate-output phase (26):**

Phase 6b structural validation, implemented in `glyph validate-output`. These diagnostics check that Step 2's Markdown output faithfully projects the input IR. All are classification `error`. Full catalog in `expand.md` §4.2, cross-referenced in `agent-skill.md` §`glyph validate-output`.

| ID | Classification |
|----|---------------|
| `G::expand::extra-h2` | error |
| `G::expand::missing-instructions` | error |
| `G::expand::extra-h3` | error |
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
| `G::expand::step-too-long` | error |
| `G::expand::constraint-multi-sentence` | error |
| `G::expand::frontmatter-returned` | error |
| `G::expand::malformed-markdown` | error |
| `G::expand::procedure-count-mismatch` | error |
| `G::expand::procedure-name-mismatch` | error |
| `G::expand::procedure-step-count-mismatch` | error |
| `G::expand::procedure-ref-missing` | error |
| `G::expand::procedure-ref-dangling` | error |
| `G::expand::procedure-duplicate` | error |
| `G::expand::procedure-order` | error |

**Total: 82 compiler-scope diagnostic IDs** (19 Parse + 29 Analyze + 1 Imports + 5 Validate + 1 Build + 27 Validate-output).

### 4.2 Agent-scope diagnostics (not in compiler)

These diagnostics are the responsibility of the external agent skill that drives Repair (Phase 3) and Expand Step 2 (Phase 6). They are part of the Glyph spec but not implemented in the compiler binary.

**Repair notifications (5):** `G::repair::generated-const`, `G::repair::generated-block`, `G::repair::branch-extracted`, `G::repair::inferred-effects`, `G::repair::constraint-tension`

**Repair execution failures (5):** `G::repair::llm-unavailable`, `G::repair::output-invalid`, `G::repair::no-convergence`, `G::repair::constraint-contradiction`, `G::repair::constraint-scan-malformed`

**Expand Step 2 execution (1):** `G::expand::llm-unavailable`

**Total: 11 agent-scope diagnostic IDs.** These will be tested when the agent skill is implemented. Phase 6b structural validation (25 `G::expand::*` diagnostics) moved to compiler-scope under `glyph validate-output` — see §4.1.

### 4.3 Post-MVP diagnostics

These require features not fully exercised in the MVP:

| ID | Classification | Reason deferred |
|----|---------------|-----------------|
| `G::analyze::nominal-mismatch` | error | Requires richer type-checking scenarios beyond MVP skill complexity |
| `G::analyze::lossy-coercion` | error | Requires numeric type boundary testing (float→int) |

### 4.4 Testing strategy for Validate diagnostics

Validate phase (Phase 5) diagnostics fire on malformed IR that passes Phase 2 but violates post-Lower invariants. These are rare in normal compilation (Phase 2 catches most problems). Testing strategy:

1. **Unit tests with hand-crafted invalid IR.** Construct IR nodes that violate specific invariants (duplicate node IDs, unresolved callees, cyclic call graphs, empty steps), feed directly to the Validate phase.
2. **One unit test per Validate diagnostic ID.**


## 5. MVP Exit Criteria

The MVP is complete when all five bars are met:

### Bar 1: All deterministic phases implemented

Phases 1 (Parse), 2 (Analyze), 4 (Lower), 5 (Validate), 6-Step 1 (deterministic Expand), and 7 (Emit) are implemented and exercised by the test corpus. The compiler produces mechanical `.md` output and structured `.ir.json` from valid source.

### Bar 2: Deterministic output

Every file in `tests/corpus/valid/` and `tests/corpus/multi-file/` produces byte-identical `.md` output, byte-identical `.ir.json` output (when `--emit-ir` is set), and byte-identical diagnostic JSON output (under `--format=json`) on every run. The compiler is fully deterministic — no LLM, no randomness — but byte-stability across the JSON outputs is **not** trivial: it depends on the JSON-determinism invariant in `build-foundation.md` §JSON Determinism (`BTreeMap` for any map-shaped JSON, plus diagnostic arrays sorted by `(file, span.start.byte, id)`). This bar guards against accidental non-determinism from hash maps, file ordering, and unsorted diagnostic emission.

### Bar 3: Multi-file project compiles

The 5-skill project in `tests/corpus/multi-file/` (§3) compiles successfully:
- `prefs.glyph.md` compiles (exit 0, zero `.md` emission)
- `repo_tools.glyph.md` compiles (exit 0, emits procedure files if blocks exceed Tier 1 threshold)
- `fix_bug.glyph.md` compiles (exit 0, imports from prefs + repo_tools resolve)
- `review_pr.glyph.md` compiles (exit 0, imports from repo_tools, branching works)
- `update_docs.glyph.md` compiles (exit 0, standalone)
- DAG order respected: libraries compile before consumers
- Cross-file name resolution works

### Bar 4: `--strict` mode

`--strict` treats `repairable` diagnostics as hard errors (exit code 1 instead of 2). Succeeds on every file in `tests/corpus/valid/` and `tests/corpus/multi-file/`. Fails on every file in `tests/corpus/repairable/`.

### Bar 5: Diagnostic coverage

Every compiler-scope diagnostic ID (§4.1, 75 total) has at least one triggering test:
- Parse and Analyze diagnostics: triggered by corpus files in `invalid/` (exit 1) and `repairable/` (exit 2)
- Validate diagnostics: triggered by unit tests with hand-crafted invalid IR (§4.4)
- Validate-output diagnostics (25 Phase 6b): triggered by unit tests feeding crafted `.ir.json` + `.md` pairs to `glyph validate-output`
- Build diagnostic: triggered by multi-file test with a deliberately failed dependency
- Warnings (`effects-over-declared`): triggered by `valid/` corpus file that compiles successfully with warning on stderr


## 6. Snapshot Testing with `insta`

All snapshot tests use the `insta` crate for Rust, located in `glyph-cli/tests/`.

**Test organization:**

```rust
#[test]
fn valid_update_docs() {
    let result = glyph_compile("tests/corpus/valid/update_docs.glyph.md");
    assert_eq!(result.exit_code, 0);
    insta::assert_snapshot!(result.emitted_md("update_docs.md"));
}

#[test]
fn repairable_novice_fix_bug() {
    let result = glyph_compile("tests/corpus/repairable/novice_fix_bug.glyph.md");
    assert_eq!(result.exit_code, 2);
    insta::assert_snapshot!(result.diagnostics_json());
    // Verify specific diagnostic IDs present
    assert!(result.has_diagnostic("G::analyze::undefined-name"));
    assert!(result.has_diagnostic("G::analyze::undefined-call"));
}

#[test]
fn invalid_empty_file() {
    let result = glyph_compile("tests/corpus/invalid/empty.glyph.md");
    assert_eq!(result.exit_code, 1);
    insta::assert_snapshot!(result.diagnostics_json());
    assert!(result.has_diagnostic("G::parse::empty-file"));
}

#[test]
fn strict_rejects_repairable() {
    let result = glyph_compile_strict("tests/corpus/repairable/novice_fix_bug.glyph.md");
    assert_eq!(result.exit_code, 1); // --strict promotes exit 2 → exit 1
}
```

**Snapshot review workflow:**
1. Run tests — `insta` creates `.snap.new` files for new/changed snapshots
2. Review with `cargo insta review` — accept or reject each change
3. Commit accepted `.snap` files alongside source changes

**What to snapshot:**
- `valid/`: compiled `.md` output + exit code 0
- `repairable/`: diagnostic JSON output + exit code 2
- `invalid/`: diagnostic JSON output + exit code 1
- `multi-file/`: compiled `.md` for each skill file + build summary (DAG order, emission list) + exit code 0
