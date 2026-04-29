//! glyph-core: deterministic compiler phases for the Glyph language.
//!
//! Walking-skeleton scope (slice 1): minimum viable Phase 1 / 2 / 4 / 5 / 6-Step1 / 7
//! that produces a byte-identical golden snapshot for `update_docs.glyph.md` per
//! `design/mvp-acceptance.md` §1.

pub mod analyze;
pub mod ast;
pub mod emit;
pub mod expand;
pub mod ir;
pub mod lower;
pub mod parse;
pub mod span;
pub mod tokenize;
pub mod validate;

use std::path::Path;

#[derive(Debug)]
pub enum CompileError {
    Read { path: String, source: std::io::Error },
    Parse(parse::ParseError),
    Lower(lower::LowerError),
    Validate(validate::ValidateError),
    Write { path: String, source: std::io::Error },
}

/// Run all walking-skeleton phases and return the compiled Markdown.
///
/// Phases: 1 (Parse) → 2 (Analyze) → 4 (Lower) → 5 (Validate) → 6-Step1 (Expand) → 7 (Emit).
pub fn compile_source(source: &str, file_id: u32) -> Result<String, CompileError> {
    let (file, _line_index) = parse::parse(source, file_id).map_err(CompileError::Parse)?;
    let file = analyze::analyze(file);
    let arena = lower::lower(&file).map_err(CompileError::Lower)?;
    validate::validate(&arena).map_err(CompileError::Validate)?;
    let arena = expand::expand_step1(arena);
    Ok(emit::emit(&arena))
}

/// End-to-end file-driven compile: read `<name>.glyph.md`, write `<name>.md` next to it.
pub fn compile_file(path: &Path) -> Result<std::path::PathBuf, CompileError> {
    let source = std::fs::read_to_string(path).map_err(|e| CompileError::Read {
        path: path.display().to_string(),
        source: e,
    })?;
    let compiled = compile_source(&source, 0)?;
    let out_path = compiled_output_path(path);
    std::fs::write(&out_path, &compiled).map_err(|e| CompileError::Write {
        path: out_path.display().to_string(),
        source: e,
    })?;
    Ok(out_path)
}

/// Map `foo.glyph.md` → `foo.md` next to the source file.
fn compiled_output_path(input: &Path) -> std::path::PathBuf {
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let file_name = input.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let stem = file_name.strip_suffix(".glyph.md").unwrap_or(
        file_name
            .strip_suffix(".md")
            .unwrap_or(file_name),
    );
    parent.join(format!("{}.md", stem))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_path_strips_glyph_md() {
        let p = compiled_output_path(Path::new("tests/corpus/valid/update_docs.glyph.md"));
        assert_eq!(p, Path::new("tests/corpus/valid/update_docs.md"));
    }
}
