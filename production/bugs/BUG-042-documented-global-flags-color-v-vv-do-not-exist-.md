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
