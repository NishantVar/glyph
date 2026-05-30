---
name: audit
description: 'Runs semantic checks (currently constraint-conflict scanning) against each declaration''s resolved constraint set and surfaces any contradictions or tensions to the author.'
---

## Parameters

- **source_path**. Required.

## Steps

1. Follow the scan-constraint-conflicts procedure below.

### Procedure: scan-constraint-conflicts

1. Enumerate every `.glyph` source under {source_path}: a single file when {source_path} is a `.glyph` file, or every `*.glyph` recursively under {source_path} when it is a directory. For each source, derive `<stem>` by stripping the trailing `.glyph` from the basename and look for the IR sidecar at `<dir>/<stem>.ir.json`.
2. If any source is missing its IR sidecar, stop and tell the user to run `/glyph:compile {source_path}` first — `/glyph:audit` reads the resolved IR rather than re-parsing the source, so it requires a prior successful compile.
3. Across every IR sidecar, enumerate every declaration whose `constraints:` set has 2 or more entries. Skip declarations with 0 or 1 constraints without an LLM call.
4. For each such declaration, load `.agents/skills/glyph/semantic_validation.md` and follow its procedure, passing the declaration's resolved constraint set as input.
5. Aggregate the returned JSON conflict reports across every audited declaration. Surface a single human-readable report grouped by source file and declaration: each `contradiction` is reported as a fatal finding, each `tension` is reported as a warning. The contract is that the author edits the source and recompiles — `/glyph:audit` never rewrites the source or the compiled `.md`.
6. Exit non-zero if any `contradiction` was reported; exit zero otherwise (tensions are warnings, not failures).

