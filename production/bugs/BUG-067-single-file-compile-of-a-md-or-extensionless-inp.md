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

## Independent Agent Finding

**Verdict:** Reproduced for `.md` single-file input. Partially narrowed for extensionless input: an extensionless source is accepted and compiles successfully, but in the tested case it did not overwrite the extensionless input itself; it wrote a sibling `<input>.md` file instead.

**Reproduction/Refutation:** Using `crates/glyph-cli/tests/fixtures/predicate_inline_literal.glyph` as a valid source fixture, I copied it under `tmp/bug-067-repro/` as both `predicate_inline_literal_copy.md` and `extensionless_source`. Running `cargo run -p glyph-cli -- compile tmp/bug-067-repro/predicate_inline_literal_copy.md` exited 0 and changed the `.md` input from Glyph source into compiled Markdown in place. Running `cargo run -p glyph-cli -- compile tmp/bug-067-repro/extensionless_source` also exited 0, but left `extensionless_source` unchanged and created `extensionless_source.md` containing the compiled Markdown.

**Evidence:** Before compiling, `predicate_inline_literal_copy.md` had SHA-256 `75c93e57b7f217e180a88490e604e1eb1e3bf14f9db2c595394dbb6f967e5c96` and size 274 bytes. After the compile command exited 0, the same path had SHA-256 `e460597a9b0c36b4071dca2cc34c7c0fa4ac06056ab06a61effb7cfc4da08eec` and size 253 bytes, with content beginning with compiled Markdown frontmatter (`---`, `name: predicate_inline_literal`). For the extensionless run, `extensionless_source` remained at SHA-256 `75c93e57b7f217e180a88490e604e1eb1e3bf14f9db2c595394dbb6f967e5c96` and size 274 bytes, while `extensionless_source.md` was created with SHA-256 `e460597a9b0c36b4071dca2cc34c7c0fa4ac06056ab06a61effb7cfc4da08eec` and size 253 bytes.

**Resolution Input:** Preserve the existing suggested resolution. A single-file `.glyph` extension check prevents both the reproduced `.md` self-overwrite and the acceptance of extensionless sources. A canonicalized input/output equality guard is still useful as a direct data-loss backstop, but by itself would not reject extensionless inputs that generate neighboring `.md` files.
