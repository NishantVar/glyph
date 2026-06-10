# BUG-039: invented-param-ref false positive: `## Parameters` section is not excluded from curly-ref scan despite the comment

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/validate_output.rs:919-934`
**Found by:** validate-1 | **Audit date:** unknown-date

## Description

`check_param_refs` documents (line 919) that it finds `{name}` references "in the md body (excluding ## Parameters section)", but `find_curly_refs` (lines 986-1008) scans the entire markdown and only skips fenced/indented code regions — it never excludes the `## Parameters` H2.

`render_param_bullet` (`emit/templates.rs`) emits the author-supplied parameter description verbatim. If any parameter's description contains a curly-brace token that is not itself a declared param or `local_ref` name (e.g. a description like `"falls back to the {placeholder} value"`), `find_curly_refs` picks it up from the Parameters section and `check_param_refs` emits a spurious `G::expand::invented-param-ref` for `placeholder`, failing validation of valid compiled output. The behavior contradicts the function's own documented contract.

## Trigger / Reproduction

A skill with a parameter whose description contains a `{word}` token:

```
export skill fetch(target)
    description: "fetch the resource"
    params:
        target: "falls back to the {placeholder} value"
    flow:
        "retrieve {target}"
```

Running `glyph validate-output` emits:
```
error[G::expand::invented-param-ref]: `{placeholder}` reference does not match any declared parameter
```

Despite `{placeholder}` appearing only in the parameter description (not in a flow step), the validation fails.

## Evidence

```rust
// Find all {name} references in the md body (excluding ## Parameters section)
let md_refs = find_curly_refs(md);  // find_curly_refs does NOT exclude ## Parameters
```

The comment documents the intended contract; the implementation violates it. `find_curly_refs` has no logic to track or skip a `## Parameters` H2 section.

## Recommended Resolution

Make `find_curly_refs` skip lines within the `## Parameters` H2 section: track an `in_parameters` flag toggled on `## Parameters` and reset on the next `## ` or `### ` heading. This matches the documented contract. Alternatively, pass the parsed structure and scan only non-Parameters body sections.

## Verification Notes

The code at `validate_output.rs` line 919-920 has a clear mismatch between the comment ("excluding ## Parameters section") and the implementation: `find_curly_refs(md)` is called on the entire markdown string. The `find_curly_refs` function (lines 986-1009) only skips fenced code blocks and 4-space-indented lines, with no logic to track or skip a `## Parameters` H2 section. The `emit_parameters_section` in `scaffold.rs` emits parameter descriptions verbatim via `push_literal` without escaping curly braces. Reproducing with `cargo run -p glyph-cli -- validate-output` on a crafted IR JSON whose `target` param has description `"falls back to the {placeholder} value"` confirms the spurious `G::expand::invented-param-ref` error. No existing test covers `{ident}` tokens in `## Parameters` bullet text.
