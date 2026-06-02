# agents/quality/roundtrip

Durable home for semantic round-trip evaluation: drift findings, coverage maps,
chronic-drift / accepted-drift policy, and aggregate QA reports promoted from
per-run archives. **Owned by the QA Engineer (`evaluation-engineer-agent`)** —
distinct from the rest of the `agents/**` scaffold.

Keep this surface separate from strict verification. The strict verifier owns
current-run pass/fail evidence; this home owns qualitative semantic-drift
strategy. Promote a per-run archive here only when it becomes durable coverage or
policy.
