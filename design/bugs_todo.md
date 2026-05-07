# Known Bugs (To Fix)

Tracked compiler bugs that are not yet patched. Each entry: severity, where it surfaces, the bad behavior, and the fix shape.

## Tier-3 procedure `.md` write failures are silently swallowed

**Severity:** Medium
**Surfaces in:** `crates/glyph-core/src/lib.rs:1502` (`emit_library_procedures`)

`emit_library_procedures` writes one `.md` per qualifying export block under `<parent>/<lib_stem>/<block-name-kebab>.md`. Both `std::fs::create_dir_all(&subdir).ok()` (line 1523) and `atomic_write(&out_path, &markdown).ok()` (line 1553) discard their `Result`. After the writes the path is unconditionally pushed into `emitted` and propagated into `procedure_paths`, so the consumer file's compiled `.md` references a procedure that may not exist on disk.

Net effect: a Tier-3 write failure (permissions, full disk, pre-existing directory at the target path, etc.) produces a successful build (`exit 0`) with a broken cross-file reference. The recently-added `BuildResult.io_errors` channel exists precisely for this kind of failure — it covers entry `.md` and `.ir.json` writes (`IoFailureKind::Md` / `IoFailureKind::IrJson`), but procedure writes were not threaded through it.

**Fix shape:** thread `&mut Vec<IoFailure>` (or return `Result<Vec<(String, String)>, IoFailure>`) through `emit_library_procedures` and push `IoFailure { kind: IoFailureKind::Md, ... }` on each `create_dir_all` / `atomic_write` failure. Also flip `any_failure = true` in the caller so `BuildResult.exit_code` reflects the failure. Add an integration test that pre-creates the procedure output path as a directory and asserts exit 3 + `cannot write` stderr.
