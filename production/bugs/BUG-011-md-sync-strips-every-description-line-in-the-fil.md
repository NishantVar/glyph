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

## Independent Agent Finding

### Verdict

Reproduced. The `.md` sync path strips every line whose leading whitespace is followed by `description:`, including worked-example Glyph code inside markdown bodies.

### Reproduction/Refutation

I did not run `scripts/sync_commands_no_desc.sh` against the real repository outputs because this task's write scope is limited to this report. Instead, I copied the script and `decompile.md` into `tmp/bug011-repro.nFzs5q/` and ran the copied script there.

Commands run, summarized:

- Graphify `query_graph` for `sync_commands_no_desc.sh` / `description` / `commands_no_desc`: returned only generic frontmatter nodes, so exact verification used targeted file inspection.
- `git status --short`: showed pre-existing unrelated changes in `.claude/settings.json`, BUG-001 through BUG-006 reports, and untracked `.claude/agents/probe-*` files.
- `nl -ba scripts/sync_commands_no_desc.sh | sed -n '1,110p'`: confirmed the `.md` branch has an unguarded `/^[[:space:]]*description:/ { next }` rule at line 57.
- `rg -n "^[[:space:]]*description:" .agents/commands/glyph/decompile.md .agents/commands/glyph/teach.md .agents/commands_no_desc/glyph/decompile.md .agents/commands_no_desc/glyph/teach.md`: found `description:` lines in the source markdown files, including worked examples, and no matches in the generated checked-in files.
- `nl -ba .agents/commands/glyph/decompile.md | sed -n '584,594p'` and `nl -ba .agents/commands_no_desc/glyph/decompile.md | sed -n '586,596p'`: source line 590 contains the worked-example description; generated output jumps directly from `skill update_docs(scope = ".")` to `require accuracy`.
- Scratch reproduction: copied `scripts/sync_commands_no_desc.sh` and `.agents/commands/glyph/decompile.md` under `tmp/bug011-repro.nFzs5q/`, then ran `bash tmp/bug011-repro.nFzs5q/scripts/sync_commands_no_desc.sh`. The copied script reported a successful sync.
- `awk 'FNR==1{if (NR>1) print prev, count; prev=FILENAME; count=0} /^[[:space:]]*description:/{count++} END{print prev, count}' tmp/bug011-repro.nFzs5q/.agents/commands/glyph/decompile.md tmp/bug011-repro.nFzs5q/.agents/commands_no_desc/glyph/decompile.md`: source count was `10`; generated count was `0`.

### Evidence

The copied source file had ten matching `description:` lines, while the copied generated output had zero. In the reproduced generated file, the worked example around `skill update_docs(scope = ".")` is missing `description: "Update repository documentation to match current code."` exactly as reported.

### Resolution Input

The recommended resolution is appropriate: track YAML frontmatter state in the `.md` awk branch and skip `description:` only inside the first frontmatter block. The same audit should cover the `.glyph` branch because it uses the same unscoped skip rule.
