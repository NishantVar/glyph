//! Standalone `glyph-lsp` binary.
//!
//! Speaks JSON-RPC framed per the LSP spec on stdin/stdout. The same logic is
//! also exposed as a `glyph lsp` subcommand on the `glyph` CLI binary; this
//! standalone exists so users who prefer `cargo install --path crates/glyph-lsp`
//! get a focused binary they can point editors at directly.

fn main() -> std::io::Result<()> {
    glyph_lsp::run_stdio()
}
