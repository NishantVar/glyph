# BUG-042: Documented global flags --color, -v, -vv do not exist in the CLI

**Severity:** medium | **Confidence:** high | **Status:** confirmed
**Location:** `docs/reference/cli.md:63-68`
**Found by:** x-contract | **Audit date:** unknown-date

## Description

`docs/reference/cli.md` §Global flags documents `-v` (info logging), `-vv` (debug logging), and `--color <when>` with `always|never|auto` plus `NO_COLOR`/`CLICOLOR` env support as flags available on all subcommands. The clap `Cli` struct in `crates/glyph-cli/src/main.rs` declares only one global arg, `enable_effects`; there is no verbosity flag and no color flag. Color is hard-coded to `ColorChoice::Auto` with no env handling.

Because clap rejects unknown args, invoking `glyph -v compile foo.glyph` or `glyph --color never check foo.glyph` errors out as an unrecognized argument instead of behaving as documented. Likewise `NO_COLOR`/`CLICOLOR` are ignored.

## Trigger / Reproduction

```
glyph -v compile foo.glyph
# → error: unexpected argument '-v' found

glyph --color never check foo.glyph
# → error: unexpected argument '--color' found
```

Any invocation using the documented global flags fails immediately.

## Evidence

```md
<!-- docs/reference/cli.md lines 63-68 -->
| `-v`            |       | Set log level to info (phase boundaries, file processing) |
| `-vv`           |       | Set log level to debug (IR diffs, detailed phase output) |
| `--color <when>` |      | Terminal color mode: `always`, `never`, `auto` (default: `auto`). Also respects `NO_COLOR` and `CLICOLOR` environment variables. |
| `--enable-effects` |    | Enable the effects subsystem ... |

Default log level is warn (errors and warnings only). `-v` adds info (phase start/end, files processed). `-vv` adds debug (IR snapshots, diagnostic details).
```

Actual CLI (only global arg):

```rust
// crates/glyph-cli/src/main.rs — Cli struct
#[arg(long, global = true)]
enable_effects: bool,   // the only global flag; no -v/-vv/--color

// color is hard-coded:
let writer = StandardStream::stderr(ColorChoice::Auto);
```

## Recommended Resolution

Either implement the documented `--color`/`-v`/`-vv` flags (with `NO_COLOR`/`CLICOLOR` env handling) in the `Cli` struct, or remove them from `docs/reference/cli.md` §Global flags and §Diagnostic Output so the documented surface matches the binary.

## Verification Notes

Running `cargo run -p glyph-cli -- --help` shows only `--enable-effects`, `-h`, and `-V` as global options. Running `glyph -v compile ...` produces `error: unexpected argument '-v' found` and `glyph --color never compile ...` produces `error: unexpected argument '--color' found`. The divergence is a real doc-vs-implementation contract mismatch that causes user-visible errors on plausible inputs documented in the reference.

## Independent Agent Finding

**Verdict:** Reproduced.

**Reproduction/Refutation:** The current CLI rejects the documented global `-v`, `-vv`, and `--color <when>` flags before subcommand execution. `cargo run -q -p glyph-cli -- --help` lists only `--enable-effects`, `-h/--help`, and `-V/--version` as top-level options.

**Evidence:**
- Graphify query for `glyph-cli global flags clap Cli struct color verbosity enable_effects docs reference cli global flags` identified the relevant contract/implementation boundary: `docs/reference/cli.md` `Global flags (all subcommands)` and `crates/glyph-cli/src/main.rs` `Cli`.
- A targeted `rg` search for the documented global flags in `docs/reference/cli.md` confirms the docs still list `-v`, `-vv`, `--color <when>`, and `NO_COLOR`/`CLICOLOR` support at `docs/reference/cli.md:63-65`, plus a default-log-level note at line 68.
- `ast-grep` for `struct Cli { $$$ }` confirms `crates/glyph-cli/src/main.rs:27-35` contains only `#[arg(long, global = true)] enable_effects: bool` before the subcommand; there is no verbosity or color field.
- `ast-grep` for `StandardStream::stderr($ARG)` confirms stderr rendering still uses `StandardStream::stderr(ColorChoice::Auto)` at `crates/glyph-cli/src/main.rs:740`.
- `cargo run -q -p glyph-cli -- --help` exits 0 and shows `Options:` containing `--enable-effects`, `-h/--help`, and `-V/--version`; it does not show `-v`, `-vv`, or `--color`.
- `cargo run -q -p glyph-cli -- -v compile foo.glyph` exits 2 with `error: unexpected argument '-v' found`.
- `cargo run -q -p glyph-cli -- -vv check foo.glyph` exits 2 with `error: unexpected argument '-v' found` (clap reports the first unsupported short `v`).
- `cargo run -q -p glyph-cli -- --color never check foo.glyph` exits 2 with `error: unexpected argument '--color' found`.

**Resolution Input:** Preserve the existing recommended resolution: either implement the documented `--color`/`-v`/`-vv` flags, including `NO_COLOR`/`CLICOLOR` handling, or remove those flags from `docs/reference/cli.md` so the stable CLI contract matches the binary.
