# Glyph Capabilities

Post-MVP design note for capability-based composition in Glyph.

## Decision

A capability is a reusable contract fragment, not an executable agent entrypoint.

Core distinction:

```text
skill = externally activatable agent contract
capability = reusable internal power / contract bundle
block = callable procedure
children = topology declared by a skill
```

The important rule:

```text
Capabilities may provide named blocks that have flows.
Capabilities must not have an unnamed top-level flow that implicitly merges into a skill.
```

## Why

If a capability has its own top-level `flow:`, the compiler has to answer ambiguous ordering questions:

- Does the capability flow run before the skill flow?
- Does it run inside the skill flow?
- Does it run every time the skill needs that capability?
- What happens when two used capabilities both define top-level flows?
- How are conflicts, ordering, and return values resolved?

Those questions turn capability composition into inheritance or hidden runtime dispatch. That conflicts with Glyph's preference for explicit compiled instructions and analyzable data flow.

## Capability Shape

A capability can define how named operations work. A skill decides when those operations happen.

Candidate future shape:

```glyph
capability CanDelegate
    permissions:
        allow spawns_agent

    constraints:
        must avoid spawning_unbounded_agents
        require clear_child_briefs

    provides:
        block send_message(agent, message)
            flow:
                "Run `scripts/send_agent_message.sh` with the target agent id and message."
                "Capture the command output."
                "If the command fails, report the failure and do not pretend the message was sent."

        block collect_report(agent) -> AgentReport
            flow:
                "Run `scripts/collect_agent_report.sh` for the target agent."
                return <agent_report>
```

The capability does not decide when to send messages. It only defines the reusable operation and the contract around that operation.

The using skill owns the main workflow:

```glyph
skill design_lead(request) -> DesignDirection
    uses:
        CanDelegate

    children:
        ux: ux_designer

    flow:
        ux_agent = spawn_child(ux)
        send_message(ux_agent, "Review the UX flow.")
        report = collect_report(ux_agent)
        return synthesize_direction(report)
```

## Contract Boundary

`uses CanDelegate` should make the capability's provided operations available to the skill and bring its contract along:

- provided blocks become callable by the skill;
- constraints apply to the relevant capability operations or to the skill if declared as global capability constraints;
- permissions/autonomy requirements are unioned into the skill contract;
- effects inferred from provided operation calls must be permitted by the composed contract;
- name conflicts must be explicit, following Glyph's no-shadowing posture.

Current Glyph can approximate this with `export block` libraries. A future `capability` is a contract-aware library: it exports procedures plus the obligations and permissions attached to using them.

## Policy Without Main Flow

Capabilities may eventually need policy for recurring situations, such as child-agent failure or conflicting reports. This should not be a top-level flow. It should be modeled as named procedures, rubrics, or checkpoints that the skill explicitly references.

Preferred direction:

```glyph
capability CanDelegate
    provides:
        block handle_child_blocked(child, blocker)
            flow:
                "Summarize the blocker."
                "Ask the parent agent or user for direction before continuing."
```

Then the skill decides where this applies:

```glyph
flow:
    report = collect_report(ux_agent)
    if child_blocked(report):
        handle_child_blocked(ux_agent, report.blocker)
```

Rule of thumb:

```text
Capabilities define how operations and policies work.
Skills decide when operations and policies run.
```

## Deferred

- Top-level capability flow spines.
- Abstract block holes supplied by capabilities.
- `extends` / inheritance syntax.
- Structured message protocols.
- `wait` / rendezvous primitives for subagent results.
- Dynamic child pools and scheduling policies.

These may become useful later, but they should not be part of the first capability increment.
