# BUG-011: md sync strips every `description:` line in the file, corrupting worked-example code blocks

**Severity:** high | **Confidence:** high | **Status:** confirmed
**Location:** `scripts/sync_commands_no_desc.sh:49-59`
**Found by:** scripts | **Audit date:** unknown-date

## Description

The `.md` branch of the awk program deletes ALL lines matching `/^[[:space:]]*description:/` across the whole file, not just the single frontmatter `description:` key. The script's own comment (lines 8-13) claims it only handles the frontmatter banner/description, but the strip rule is unscoped. Several command sources (`.agents/commands/glyph/decompile.md` and `teach.md`) contain `description:` lines inside indented code-fence worked examples — for instance `decompile.md` line 590 `description: "Update repository documentation to match current code."` inside a `skill ...` Glyph example. These get silently deleted from the generated `commands_no_desc/glyph/*.md`. This is already visible in the checked-in generated output: `.agents/commands_no_desc/glyph/decompile.md` now reads `skill update_docs(scope = ".")` immediately followed by `require accuracy` — the `description:` line is gone, producing a structurally broken Glyph example that is fed to agents at runtime.

## Trigger / Reproduction

Run `scripts/sync_commands_no_desc.sh`. Inspect the generated `.agents/commands_no_desc/glyph/decompile.md` — every `description:` line inside indented code-fence blocks (lines 499, 590, 606, 631, 645, 652, 662, 680 in the source) will be absent, leaving structurally incomplete Glyph skill examples.

## Evidence

```awk
        md)
            awk -v src="$rel_src" '
            NR==1 {
                print
                print "# AUTO-GENERATED FILE -- DO NOT EDIT"
                print "# Source: " src
                print "# Regenerate: scripts/sync_commands_no_desc.sh"
                next
            }
            /^[[:space:]]*description:/ { next }
            { print }
            ' "$file" > "$dest_file"
            ;;
```

## Recommended Resolution

Scope the strip to the frontmatter only. Track frontmatter state in awk: count `---` delimiter lines and only apply the `description:`-skip rule while inside the first frontmatter block (between the 1st and 2nd `^---$`). For example, set a flag `in_fm` that turns on after printing line 1/banner and turns off at the second `---`, and gate the `description:` skip on `in_fm`. The same scoping fix should be applied to the `.glyph` branch (line 44) if that branch also processes files that may contain `description:` inside code examples.

## Verification Notes

The awk `.md` branch applies `/^[[:space:]]*description:/ { next }` to every line in the file with no guard restricting it to the YAML frontmatter. The source file `.agents/commands/glyph/decompile.md` contains `description:` lines at lines 590, 606, 631, 645, 652, 662, 680 inside indented code-block worked examples. Comparing the source against the checked-in generated output at `.agents/commands_no_desc/glyph/decompile.md` confirms the corruption: source line 590 (`description: "Update repository documentation to match current code."`) is absent from the generated file. The script comment explicitly claims it only handles the frontmatter banner/description, so this is an unintentional scope violation with live impact on agents consuming these command files.
