//! Phase 2 (Analyze) — walking-skeleton trivial pass-through.
//!
//! In the full compiler this resolves names, infers roles, validates effects, etc.
//! For slice 1 the input is already self-contained: every name in the skeleton's
//! source resolves to a same-file `text` declaration, and constraint markers carry
//! their own role/polarity. So we return the AST unchanged.

use crate::ast::SourceFile;

pub fn analyze(file: SourceFile) -> SourceFile {
    file
}
