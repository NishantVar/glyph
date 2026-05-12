# Glyph Evolution Todo

Priority list for evolving Glyph from a flow-centered skill DSL into a
contract-centered agent language.

## 1. Redefine `skill` As An Activatable Capability

Core model:

```text
skill = public routable capability
block = callable helper/procedure
export block = reusable imported procedure
```

Glyph should feel like a set of agent capabilities plus reusable procedures, not
one program with one global flow.

## 2. Add `when:` Activation Metadata

A skill should state when an agent should choose it.

```glyph
skill fix_bug(scope = ".")
    when: "The user reports a bug, regression, failing test, or unexpected behavior."
```

This makes routing first-class instead of hiding it inside `description:`.

## 3. Add `goal:`

Separate activation from success.

```glyph
goal: "Find and fix the root cause with the smallest safe change."
```

`when:` says when to use the skill. `goal:` says what successful execution means.

## 4. Demote `flow:` Conceptually

Keep `flow:`, but treat it as workflow strategy, not the center of the skill.

The skill contract should be defined by:

```text
when + goal + constraints + permissions + rubrics + output + verify
```

Flow is the suggested path through the work.

## 5. Introduce Output / Artifact Contracts

Make return types meaningful by defining the artifact.

```glyph
output:
    type ChangeSummary
    must_include changed_files, rationale, validation, remaining_risks
```

Agents need to know what they are producing, not just the nominal return label.

## 6. Upgrade Types Into Semantic Contracts

Extend type declarations to describe artifact expectations.

```glyph
type Diagnosis
    purpose: "Explain the root cause."
    must_include symptom, cause, affected_files, evidence, confidence
```

Prioritize this before adding more expression syntax.

## 7. Add First-Class Rubrics

Replace fuzzy boolean logic with judgment policies.

```glyph
rubric high_risk_change
    true_when:
        - touches auth
        - changes persisted data
        - alters public API
        - lacks tests
    then:
        ask for confirmation
```

Agents need decision criteria more than richer boolean expressions.

## 8. Turn Effects Into Permissions / Autonomy Boundaries

Move from compiler-style `effects:` toward agent autonomy contracts.

```glyph
permissions:
    allow reads_files
    allow writes_files
    ask before destructive_commands
    forbid publishing_secrets
```

The core question: what can the agent do without asking?

## 9. Add `checkpoints:` And Escalation Rules

Agents need explicit pause, ask, stop, and escalate points.

```glyph
checkpoints:
    ask_user if "the fix requires deleting files"
    escalate if "the task scope changes substantially"
    stop if "validation cannot be performed with available tools"
```

## 10. Make Authority / Override Semantics Explicit

Constraints should eventually encode whether they are absolute, user-overridable,
or project-level.

```glyph
must avoid destructive_changes
    unless: explicit_user_confirmation
```

This matters because agents merge system, developer, project, skill, user, and
inferred-preference instructions.

## 11. Recenter The IR Around Contracts

The IR should model the skill contract, not mainly a list of flow statements.

Target concepts:

```text
SkillContract
Activation
Goal
Context
Constraints
Permissions
Rubrics
Workflow
OutputContract
Verification
```

This is where the language actually becomes contract-centered.

## 12. Emit A Bundle, Not Only Markdown

After the source model stabilizes, compile to multiple artifacts:

```text
instructions.md
router.json
permissions.json
artifact_contract.md
verification_rubric.md
eval_scenarios.yaml
```

Do this after the source semantics settle, not before.

## 13. Add Agent-Language Quality Diagnostics

Add diagnostics for contract quality:

- skill has no `when`
- skill has no `goal`
- return type has no output contract
- risky permission is ungated
- fuzzy condition has no rubric
- constraints conflict
- output cannot be evaluated
- flow step has unclear actor, action, or artifact

## 14. Improve Syntax Ergonomics Last

Do not prioritize loops, arithmetic, primitive type annotations, complex
expressions, or Python-like power. Those pull Glyph toward programming-language
design.

Short implementation priority:

```text
1. skill activation: when + goal
2. output contracts
3. semantic type contracts
4. permissions/autonomy
5. rubrics
6. checkpoints/escalation
7. contract-centered IR
8. bundle output + evals
```

The key move: make Glyph contract-centered, not flow-centered.
