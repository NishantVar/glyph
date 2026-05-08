<!-- Codex review Finding (medium): block params with string defaults are classified as PredicateConst by Analyze (condition.rs:304) but Expand only merged the root skill's string-default params into consts_for_lookup, so a branch inside a block referencing the block's own param rendered the bare name instead of the resolved prose. -->
---
name: main
description: Test block param string default resolves as predicate.
---

## Instructions

### Steps

1. Follow the helper procedure below.
2. Done.

### Procedure: helper

1. Decide whether the change needs review applies and, if so:
   a. Stop and confirm with user.

