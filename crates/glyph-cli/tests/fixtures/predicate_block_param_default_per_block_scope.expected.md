<!-- Codex review Finding (medium): expand.rs flat-merged every block's string-default params into one file-level lookup, so duplicate names across blocks collided. With per-block scoping, each branch resolves predicate consts against (skill consts ∪ owning-block params) only. -->
---
name: main
description: Two blocks share param name `flag`; each branch must resolve to its own block's default.
---

## Instructions

### Steps

1. Follow the first procedure below.
2. Follow the second procedure below.

### Procedure: first

1. Decide whether the first block flag is set applies and, if so:
   a. Stop and confirm with user.

### Procedure: second

1. Decide whether the second block flag is set applies and, if so:
   a. Pause and ask the user.

