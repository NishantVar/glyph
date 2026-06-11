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

## Independent Agent Finding

Verdict: Reproduced. The production bug report is valid and remains a documentation-only inconsistency.

Reproduction/Refutation: I checked the cited guide example and compared it against the authoritative compiled-output contract, emitter references, fixture output, and a fresh scratch compile. `GLYPH_LANGUAGE_GUIDE.md:1183-1185` still shows `- **scope**: description (default: ".")` and `- **target**: description (required)`. `docs/reference/compiled-output.md` §`## Parameters` specifies `Default: <literal>.` and `Required.` tails, and `rg` over `crates/glyph-core/src/emit` found both `emit/templates.rs:56-57` and `emit/scaffold.rs:1122-1123` constructing exactly those strings. A scratch source compiled with `cargo run -q -p glyph-cli -- compile tmp/bug054-parameter-format/repro.glyph --output tmp/bug054-parameter-format/repro.md` emitted:

```markdown
- **scope**: description. Default: ".".
- **target**: description. Required.
```

Evidence: Commands run included `sed -n '1176,1190p' GLYPH_LANGUAGE_GUIDE.md`, `sed -n '60,82p' docs/reference/compiled-output.md`, `rg -n "Default:|Required\\.|default:|required" crates/glyph-core/src/emit -g '*.rs'`, `rg -n "Default:|Required\\.|default:|required" crates/glyph-cli/tests crates/glyph-core/tests -g '*.expected.md' -g '*.md'`, and the scratch compile command above. The relevant fixture output also includes `crates/glyph-cli/tests/corpus/valid/with_params.md:8-9` with `Default: ".".` and `Required.`. The scratch directory was removed after the repro.

Resolution Input: Preserve the existing suggested resolution. Update only the guide's §14 illustrative `## Parameters` example so it uses `Default: <literal>.` and `Required.` instead of the parenthesized lowercase `(default: ...)` / `(required)` form.
