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

/// Prepend `"the "` to a `return <"…">` description that reads as a bare
/// noun phrase, so the locked Description-fold wrappers
/// (`", and return X as your result."` and `"Return X as your result."`)
/// produce grammatical prose. Descriptions whose first word is already an
/// article/demonstrative/possessive/quantifier, a numeric literal, or a
/// clause-introducing word (wh-words like `what`/`whether`/`how`) are
/// passed through unchanged — those forms are grammatical without a
/// determiner, and prepending `"the "` would corrupt them
/// (e.g. `"return the what to synthesize"`).
pub fn ensure_determiner(description: &str) -> String {
    /// Lowercased first words that already make the description fit the
    /// locked wrapper without a leading article. Articles/demonstratives/
    /// possessives/quantifiers/numerals already determine the noun phrase;
    /// wh-words and `if` introduce a clause, which `return X` accepts as
    /// an object without an article.
    const LEADING_NO_PREPEND: &[&str] = &[
        // articles
        "the", "a", "an", // demonstratives
        "this", "that", "these", "those", // possessives
        "my", "your", "our", "their", "his", "her", "its", "whose",
        // quantifiers / determiners
        "no", "every", "each", "some", "any", "all", "both", "several", "many", "much", "more",
        "most", "less", "fewer", // numerals
        "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
        // wh-words and other clause leaders that take `return X` directly
        "what", "whether", "why", "how", "who", "whom", "which", "where", "when", "if",
    ];
    let first = description.split_whitespace().next().unwrap_or("");
    if first.is_empty() {
        return description.to_string();
    }
    let first_lc = first.to_ascii_lowercase();
    let starts_with_digit = first.chars().next().is_some_and(|c| c.is_ascii_digit());
    if starts_with_digit || LEADING_NO_PREPEND.iter().any(|d| *d == first_lc) {
        description.to_string()
    } else {
        format!("the {description}")
    }
}

/// Append the `Identifier` return-fold suffix to a final-Step body, stripping
/// any trailing period from the body first so the suffix begins with `, `.
pub fn append_identifier_suffix(body: &str) -> String {
    let trimmed = body.trim_end().trim_end_matches('.');
    format!("{trimmed}{IDENTIFIER_RETURN_SUFFIX}")
}

/// Append the `Description` return-fold suffix with the description text
/// substituted in. The description is run through [`ensure_determiner`] so
/// the locked wrapper reads as a grammatical sentence.
pub fn append_description_suffix(body: &str, description: &str) -> String {
    let trimmed = body.trim_end().trim_end_matches('.');
    let phrase = ensure_determiner(description);
    format!("{trimmed}{DESCRIPTION_RETURN_SUFFIX_PREFIX}{phrase}{DESCRIPTION_RETURN_SUFFIX_TAIL}")
}

/// When there is no prior step body to suffix-onto (e.g., a return-only
/// skill), emit a standalone "Return ... as your result." sentence.
pub fn standalone_return_identifier(name: &str) -> String {
    let humanized = name.replace('_', " ");
    format!("Return {humanized} as your result.")
}

pub fn standalone_return_description(description: &str) -> String {
    let phrase = ensure_determiner(description);
    format!("Return {phrase} as your result.")
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
            "Return the root cause and affected files as your result."
        );
    }

    #[test]
    fn ensure_determiner_prepends_the_for_bare_noun_phrase() {
        assert_eq!(
            ensure_determiner("path to the produced .glyph file"),
            "the path to the produced .glyph file"
        );
        assert_eq!(
            ensure_determiner("root cause and affected files"),
            "the root cause and affected files"
        );
    }

    #[test]
    fn ensure_determiner_leaves_existing_determiner_alone() {
        assert_eq!(ensure_determiner("the test summary"), "the test summary");
        assert_eq!(ensure_determiner("a list of files"), "a list of files");
        assert_eq!(ensure_determiner("an inventory"), "an inventory");
        assert_eq!(ensure_determiner("your final answer"), "your final answer");
        assert_eq!(ensure_determiner("two paths joined"), "two paths joined");
    }

    /// Regression: descriptive returns that introduce a clause (wh-words,
    /// `if`) must pass through untouched. `return X` accepts a clause as its
    /// object in English; prepending `"the"` would corrupt them.
    #[test]
    fn ensure_determiner_leaves_wh_clauses_alone() {
        assert_eq!(
            ensure_determiner("what to synthesize"),
            "what to synthesize"
        );
        assert_eq!(
            ensure_determiner("whether the user confirmed"),
            "whether the user confirmed"
        );
        assert_eq!(ensure_determiner("how to proceed"), "how to proceed");
        assert_eq!(
            ensure_determiner("why the build failed"),
            "why the build failed"
        );
        assert_eq!(
            ensure_determiner("which option was selected"),
            "which option was selected"
        );
        assert_eq!(
            ensure_determiner("if the file was modified"),
            "if the file was modified"
        );
    }

    #[test]
    fn ensure_determiner_handles_leading_digit() {
        assert_eq!(
            ensure_determiner("3 files in priority order"),
            "3 files in priority order"
        );
    }

    #[test]
    fn ensure_determiner_handles_empty_input() {
        assert_eq!(ensure_determiner(""), "");
    }

    #[test]
    fn ensure_determiner_is_case_insensitive_on_first_word() {
        assert_eq!(ensure_determiner("The result"), "The result");
        assert_eq!(ensure_determiner("Your output"), "Your output");
    }

    #[test]
    fn append_description_suffix_inserts_determiner() {
        assert_eq!(
            append_description_suffix("Run the compiler.", "path to the produced .glyph file"),
            "Run the compiler, and return the path to the produced .glyph file as your result."
        );
    }

    #[test]
    fn standalone_return_description_inserts_determiner() {
        assert_eq!(
            standalone_return_description("path to the produced .glyph file"),
            "Return the path to the produced .glyph file as your result."
        );
    }
}
