# BUG-081: sync_commands_no_desc.sh catch-all copies stray .ir.json artifacts into commands_no_desc, defeating the release parity check

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `scripts/sync_commands_no_desc.sh:29-64`
**Found by:** gap:git-ignore-attributes-interaction | **Audit date:** unknown-date

## Description

The sync loop `for file in "$SRC_DIR"/*` (line 29) iterates EVERY file in `.agents/commands/glyph/`, and the `*)` catch-all (lines 61-63) copies any non-.glyph/non-.md file verbatim into `.agents/commands_no_desc/glyph/`. Because `glyph compile` leaves `.ir.json` sidecars in `.agents/commands/glyph/` (confirmed on disk: icompile.ir.json, compile.ir.json, audit.ir.json, decompile.ir.json, teach.ir.json), the sync silently copies those IR files into commands_no_desc. Combined with the `*.ir.json` gitignore, this creates a blind spot in the release pre-flight: `.agents/skills/release/SKILL.md` step 1 runs this sync then checks `git status --porcelain .agents/commands_no_desc/` to detect drift, but git status never reports ignored files, so a divergence in any copied .ir.json (or any other stray ignored artifact) is invisible to the parity check.

Concrete trigger: an author with stale `.agents/commands/glyph/compile.ir.json` runs the release pre-flight; the sync copies the stale IR into commands_no_desc, but the parity check reports "in sync" and clears the release, even though the no_desc tree now contains a stale/uncommitted IR copy that ships to symlink-mode installs.

## Trigger / Reproduction

Run `scripts/sync_commands_no_desc.sh` when `.agents/commands/glyph/` contains any `.ir.json` sidecar files (left by a prior `glyph compile` invocation). Observe that `.agents/commands_no_desc/glyph/` receives matching `.ir.json` copies. Then run `git status --porcelain .agents/commands_no_desc/` and observe no output — the copied IR files are silently gitignored and the parity check passes despite the stale artifact.

## Evidence

```sh
for file in "$SRC_DIR"/*; do
    [ -f "$file" ] || continue
    filename=$(basename "$file")
    dest_file="$DEST_DIR/$filename"
    rel_src=".agents/commands/glyph/$filename"
    ext="${filename##*.}"

    case "$ext" in
        glyph)
            # ... strips description lines, adds banner
            ;;
        md)
            # ... strips description lines, adds banner
            ;;
        *)
            # Unknown extension: copy verbatim (no banner, no strip).
            cp "$file" "$dest_file"
            ;;
    esac
done
```

## Recommended Resolution

Restrict the loop to the intended source types — e.g. iterate `"$SRC_DIR"/*.glyph` and `"$SRC_DIR"/*.md` only, or add a guard skipping `*.ir.json` (and other generated sidecars) before the case statement. This keeps commands_no_desc a faithful, fully-tracked mirror of the two source file kinds and removes the gitignored noise that the release parity check cannot observe.

## Verification Notes

All three conditions of the bug chain are confirmed on disk. First, `.agents/commands/glyph/` contains `.ir.json` sidecars (audit, compile, decompile, icompile, teach) that are gitignored via the `*.ir.json` pattern in `.gitignore`. Second, the sync script's `*)` catch-all at lines 61-63 copies any unrecognized extension verbatim, producing matching `.ir.json` files in `.agents/commands_no_desc/glyph/`. Third, `git check-ignore` confirms those destination IR files are also matched by `*.ir.json`, so `git status --porcelain .agents/commands_no_desc/` cannot report them. The parity check blind spot is real. Severity is low because `.ir.json` files are build artifacts that agents don't consume at runtime; the concrete harm is limited to stale IR copies silently shipping in symlink-mode installs, with no impact on compiled skill content.
