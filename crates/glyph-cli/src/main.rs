//! Glyph CLI binary — walking-skeleton entrypoint per `design/mvp-acceptance.md` §1.
//!
//! Usage:
//!   glyph compile <path-to.glyph.md>
//!
//! Exits 0 on success, 1 on hard error, 3 on invocation error per
//! `design/build-foundation.md` §A6.

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(name = "glyph", about = "Glyph compiler", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Compile a `.glyph.md` source file into Markdown next to the source.
    Compile {
        /// Path to the source file.
        path: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Compile { path } => match glyph_core::compile_file(&path) {
            Ok(_) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("glyph: compile failed: {:?}", e);
                ExitCode::from(1)
            }
        },
    }
}
