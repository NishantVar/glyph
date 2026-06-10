# BUG-054: Parameter bullet example in guide uses wrong format vs compiled-output contract and emitter

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `GLYPH_LANGUAGE_GUIDE.md:1184-1185`
**Found by:** x-contract | **Audit date:** unknown-date

## Description

`GLYPH_LANGUAGE_GUIDE.md` §14 shows compiled `## Parameters` bullets as `- **scope**: description (default: ".")` and `- **target**: description (required)` — parenthesized, lowercase. The authoritative `compiled-output.md` §Parameters (lines 65-79) and the emitter both use a capitalized, period-terminated tail: `Default: <literal>.` and `Required.` (emit/templates.rs:54-57, emit/scaffold.rs:1120-1123). So the guide's illustrative output does not match what the compiler emits. Authors anticipating `(default: ".")` / `(required)` will instead see `Default: .` / `Required.`

## Trigger / Reproduction

A skill author reads §14, writes a parameter with a default value, compiles the skill, and observes `- **scope**: description. Default: ".".` instead of the `(default: ".")` format shown in the guide. The discrepancy causes confusion about whether the compiler is working correctly.

## Evidence

```markdown
<!-- GLYPH_LANGUAGE_GUIDE.md lines 1183-1185 -->
## Parameters
- **scope**: description (default: ".")
- **target**: description (required)
```

```rust
// emit/templates.rs:56-57 (actual emitter):
// Some(v) => format!("Default: {}.", v),
// None    => "Required.".to_string()
//
// compiled-output.md §Parameters (lines 65-69):
// - **name** (Type): description. Default: "value".
// - **name** (Type): description. Required.
```

## Recommended Resolution

Update the guide's §14 example to use `Default: <literal>.` and `Required.` to match `compiled-output.md` and the emitter. For example:

```markdown
## Parameters
- **scope**: description. Default: ".".
- **target**: description. Required.
```

## Verification Notes

The discrepancy is confirmed real: the guide uses parenthesized lowercase `(default: ".")` / `(required)`, while the emitter (templates.rs and scaffold.rs) and compiled-output.md both use capitalized, period-terminated `Default: X.` / `Required.` Multiple `.expected.md` fixtures confirm the emitter format (e.g. `predicate_param_default.expected.md` line 9: `Default: "the change needs review".`). This is a documentation-only inconsistency with no impact on compiler behavior.
