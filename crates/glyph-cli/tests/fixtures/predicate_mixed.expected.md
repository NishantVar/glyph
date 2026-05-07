<!-- deterministic fallback; LLM-shaped output deferred to Step 2 -->
---
name: predicate_mixed
description: Demonstrate mixed condition: predicate const AND-NOT inline literal.
---

## Instructions

### Steps

1. If the requested change requires regenerating multi-line prose that repair or prose-reshape originally authored, beyond a localised wording or value swap and not is dry run:
   a. Stop and recommend running `/glyph:compile` instead — incremental edit cannot regenerate prose.

