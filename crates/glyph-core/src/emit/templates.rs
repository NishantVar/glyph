//! Locked deterministic-emitter templates. Single-grep changes only.

pub const EXTERNAL_FILE_TEMPLATE: &str = "Load and follow the procedure in `{path}`.";
pub const IDENTIFIER_RETURN_SUFFIX: &str = ", and return that as your result.";
pub const DESCRIPTION_RETURN_SUFFIX_PREFIX: &str = ", and return ";
pub const DESCRIPTION_RETURN_SUFFIX_TAIL: &str = " as your result.";

pub fn kebab_case(snake: &str) -> String {
    snake.replace('_', "-")
}

pub fn external_file_step(path: &str) -> String {
    EXTERNAL_FILE_TEMPLATE.replace("{path}", path)
}

/// Append the `Identifier` return-fold suffix to a final-Step body, stripping
/// any trailing period from the body first so the suffix begins with `, `.
pub fn append_identifier_suffix(body: &str) -> String {
    let trimmed = body.trim_end().trim_end_matches('.');
    format!("{trimmed}{IDENTIFIER_RETURN_SUFFIX}")
}

/// Append the `Description` return-fold suffix with the description text
/// substituted in.
pub fn append_description_suffix(body: &str, description: &str) -> String {
    let trimmed = body.trim_end().trim_end_matches('.');
    format!("{trimmed}{DESCRIPTION_RETURN_SUFFIX_PREFIX}{description}{DESCRIPTION_RETURN_SUFFIX_TAIL}")
}

/// When there is no prior step body to suffix-onto (e.g., a return-only
/// skill), emit a standalone "Return ... as your result." sentence.
pub fn standalone_return_identifier(name: &str) -> String {
    let humanized = name.replace('_', " ");
    format!("Return {humanized} as your result.")
}

pub fn standalone_return_description(description: &str) -> String {
    format!("Return {description} as your result.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kebab_simple() {
        assert_eq!(kebab_case("foo_bar"), "foo-bar");
        assert_eq!(kebab_case("single"), "single");
        assert_eq!(kebab_case(""), "");
    }

    #[test]
    fn external_file_substitutes_path() {
        assert_eq!(
            external_file_step("repo_tools/inspect-repo.md"),
            "Load and follow the procedure in `repo_tools/inspect-repo.md`."
        );
    }

    #[test]
    fn identifier_suffix_strips_trailing_period() {
        assert_eq!(
            append_identifier_suffix("Run cargo test."),
            "Run cargo test, and return that as your result."
        );
        assert_eq!(
            append_identifier_suffix("Run cargo test"),
            "Run cargo test, and return that as your result."
        );
    }

    #[test]
    fn description_suffix_substitutes_text() {
        assert_eq!(
            append_description_suffix("Run cargo test.", "the test summary"),
            "Run cargo test, and return the test summary as your result."
        );
    }

    #[test]
    fn standalone_return_identifier_humanizes_name() {
        assert_eq!(
            standalone_return_identifier("current_branch"),
            "Return current branch as your result."
        );
        assert_eq!(
            standalone_return_identifier("result"),
            "Return result as your result."
        );
    }

    #[test]
    fn standalone_return_description_uses_text() {
        assert_eq!(
            standalone_return_description("root cause and affected files"),
            "Return root cause and affected files as your result."
        );
    }
}
