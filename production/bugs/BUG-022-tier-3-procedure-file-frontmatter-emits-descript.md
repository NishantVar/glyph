# BUG-022: Tier-3 procedure-file frontmatter emits `description:` unquoted/unescaped, producing invalid YAML; diverges from the skill emitter which single-quotes

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/emit/mod.rs:115`
**Found by:** emit-scaffold | **Audit date:** unknown-date

## Description

`emit_procedure` writes the export block's raw author description straight into YAML
frontmatter with no quoting or escaping:

```rust
out.push_str(&format!("description: {}\n", description));
```

The description is author-controlled prose (lib.rs:2359 `let desc =
eb.node.description.as_deref().unwrap_or("")`, passed verbatim at lib.rs:2475). Any
YAML-special content breaks the frontmatter.

The most common trigger is a colon-space, e.g. an export block with
`description: "Diagnose: find the root cause"` emits
`description: Diagnose: find the root cause`, which is invalid YAML (a plain scalar value may
not contain an unescaped `: `). Other triggers: a leading `-`, `#`, `[`, `{`, `'`, `"`, `>`,
`|`, or `*`, or an embedded newline (descriptions can contain `\n` per fmt.rs multi-line
description merge and triple-quoted strings).

The skill emitter handles the same field correctly by single-quoting and escaping
(scaffold.rs:772 `format!("description: '{}'\n", skill.description.replace('\'', "''"))`),
so the identical author description yields valid YAML for a skill but a broken/misparsed
procedure `.md` file.

No analyze pass forbids `:` (or other specials) in descriptions, so this is reachable on a
green build via `glyph compile` of any library with an `export block` whose description
contains a colon.

## Trigger / Reproduction

Create an `export block` with `description: "Diagnose: find the root cause"` (or any
description containing a colon-space). Run `glyph compile`. The emitted procedure `.md` file
will contain `description: Diagnose: find the root cause` in the YAML frontmatter, which any
standards-compliant YAML parser rejects with a mapping-values error.

## Evidence

```rust
// emit/mod.rs line 115 — no quoting:
out.push_str(&format!("description: {}\n", description));

// vs scaffold.rs lines 772-773 — correct single-quoting:
out.push_str(&format!("description: '{}'\n",
    skill.description.replace('\'', "''")));
```

## Recommended Resolution

Use the same single-quoted-with-escaping YAML encoding as the skill path:

```rust
out.push_str(&format!("description: '{}'\n", description.replace('\'', "''")));
```

Ideally also normalize embedded newlines to spaces (or use a YAML block scalar) for both
paths, and extract a shared `yaml_description_line` helper used by both scaffold.rs and
`emit_procedure` so the two emitters cannot drift again.

## Verification Notes

Code inspection and live reproduction both confirm the bug. `emit/mod.rs:115` writes
`description: {}\n` with no quoting. The tokenizer stores descriptions as decoded strings
(real `: ` preserved), and there is no validation that bars YAML-special characters. The
skill emitter at `scaffold.rs:772-773` single-quotes and escapes the same field. A live
`cargo run -p glyph-cli -- compile` with a description containing `Diagnose: find the root
cause` produced `description: Diagnose: find the root cause` in the YAML frontmatter, which
Python's `yaml.safe_load` rejects with "mapping values are not allowed here". The divergence
between the two emitter paths is the root cause; the fix is straightforward.

## Independent Agent Finding

**Verdict:** Reproduced. The production report is still valid on the current checkout.

**Reproduction/Refutation:** I created a scratch library fixture at `tmp/bug022/repro.glyph`
with an `export block diagnose_issue()` whose description was exactly
`"Diagnose: find the root cause"` and whose flow text exceeded the Tier-3 procedure-file
threshold. Running `cargo run -q -p glyph-cli -- compile tmp/bug022/repro.glyph --format=json`
exited `0` and emitted `tmp/bug022/repro/diagnose-issue.md`. Its frontmatter contained the
raw, unquoted line:

```yaml
description: Diagnose: find the root cause
```

Parsing the isolated frontmatter with Python/PyYAML failed with:

```text
ScannerError: mapping values are not allowed here
  in "<unicode string>", line 4, column 22:
    description: Diagnose: find the root cause
                         ^
```

This reproduces the reported behavior rather than refuting it.

**Evidence:** Graphify was queried first for BUG-022/frontmatter/procedure context; it pointed
to the compiled-output frontmatter docs and compiler context. Targeted structural/source checks
then confirmed the current emitter split: `crates/glyph-core/src/emit/mod.rs:115` still has
`format!("description: {}\n", description)`, while `crates/glyph-core/src/emit/scaffold.rs:771-774`
still uses `format!("description: '{}'\n", skill.description.replace('\'', "''"))`. The live
compile proved that `eb.node.description` reaches the procedure emitter unchanged and produces
invalid YAML for a colon-space description.

**Resolution Input:** Keep the existing recommended resolution: encode procedure descriptions
with the same single-quoted YAML scalar escaping used by the skill scaffold path, and preferably
extract a shared helper so skill and procedure frontmatter cannot drift. Consider deciding how
both emitters should handle embedded newlines at the same time.
