# BUG-067: Single-file compile of a `.md` (or extensionless) input can overwrite the input file

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `crates/glyph-core/src/lib.rs:2192-2199`
**Found by:** x-io | **Audit date:** unknown-date

## Description

`compiled_output_path` strips a `.glyph` or `.md` suffix from the input file name and re-appends `.md`. For a normal `.glyph` input this is correct. But single-file compile (`run_compile` in `main.rs`) performs no extension check on the input path. If a user runs `glyph compile notes.md`, `compiled_output_path("notes.md")` strips `.md` to stem `notes` then produces output `notes.md` — identical to the input path. `atomic_write` then writes a temp and renames it over `notes.md`, silently destroying the original source content with the compiled output. Directory mode is unaffected because `collect_glyph_sources` only gathers `*.glyph`. This is a data-loss footgun reachable via an unusual but valid single-file invocation.

## Trigger / Reproduction

1. Copy a valid `.glyph` file as `notes.md`.
2. Run `glyph compile notes.md` (no `--output` flag).
3. On success, `notes.md` is silently overwritten with the compiled Markdown output; the original source is lost.

Live reproduction confirmed: copying a valid `.glyph` corpus file as `predicate_inline_literal_copy.md` and running `cargo run -p glyph-cli -- compile /tmp/predicate_inline_literal_copy.md` produced exit 0 and silently overwrote the source.

## Evidence

```rust
let stem = file_name
    .strip_suffix(".glyph")
    .unwrap_or(file_name.strip_suffix(".md").unwrap_or(file_name));
parent.join(format!("{}.md", stem))
// For input "notes.md": stem = "notes", output = "notes.md" == input
```

## Recommended Resolution

In single-file `run_compile`, reject inputs that do not end in `.glyph` with a clear error message, or detect when the computed output path equals the canonicalized input path and refuse to write, emitting an error. The simplest fix is the extension check:

```rust
if !input.extension().map_or(false, |e| e == "glyph") {
    return Err(GlyphError::invalid_input("source file must have a .glyph extension"));
}
```

## Verification Notes

The code path is confirmed end-to-end. `compiled_output_path` at lib.rs:2192-2199 strips `.md` suffix and re-appends `.md`, yielding an output path identical to the input. `run_compile` in `main.rs` performs no extension check and accepts any `PathBuf`. `atomic_write` fires when `CompileOutcome::Compiled` is returned. Live reproduction produced exit 0 with source overwrite. Severity is low (not medium/critical) because it requires storing valid Glyph source in a `.md`-named file, which is non-standard usage explicitly discouraged by docs (`glyph compile <path-to.glyph>`), but data loss does occur on success when triggered.
