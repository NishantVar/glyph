<!-- Codex review Finding (high): a block whose flow contains a branch must NOT be flattened to Tier 1. Pre-fix, resolve_block_body_text only kept inline strings, the branch was silently dropped, and Tier 1 emit produced an empty/incomplete step. The fix forces Tier 2 projection so the structured branch survives the procedure emit. -->
---
name: main
description: Test branch survives small block.
---

## Instructions

### Steps

1. Follow the plan-big-change procedure below.
2. Move on.

### Procedure: plan-big-change

1. Decide whether the change is big applies and, if so:
   a. Stop and confirm with user.

