//! Locked deterministic-emitter templates. Single-grep changes only.

use crate::ir::{OutputTargetForm, TypeRegistry};

pub const EXTERNAL_FILE_TEMPLATE: &str = "Load and follow the procedure in `{path}`.";

pub fn kebab_case(snake: &str) -> String {
    snake.replace('_', "-")
}

pub fn external_file_step(path: &str) -> String {
    EXTERNAL_FILE_TEMPLATE.replace("{path}", path)
}

/// Collapse runs of any whitespace (incl. embedded `\n`/`\t` decoded by the
/// tokenizer) to single spaces. Used by every §8.4 template that splices a
/// descriptive text into a single-line sentence.
fn normalize_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Produce the §8.4 "appended sentence" for a return contract, or `None` when
/// the row is "no appended sentence" (`-> Foo` absent and no descriptive output
/// target). The eight rows from the spec table:
///
/// 1. `return <"X">`                                → `Produce: X.`
/// 2. `return <name>` + `-> Foo` + `type Foo = <D>` → `` Produce `name` (`Foo`): D. ``
/// 3. `return <name>` + `-> Foo`, no `type Foo`     → `` Produce `name` (`Foo`). ``
/// 4. `return <name>`, no `-> Foo`                  → `` Produce `name`. ``
/// 5. `return expr` + `-> Foo` + `type Foo = <D>`   → `` Return a `Foo`: D. ``
/// 6. `return expr` + `-> Foo`, no `type Foo`       → `` Return a `Foo`. ``
/// 7. Return-only body                              → same as the corresponding row above
/// 8. No `-> Foo` and no descriptive target         → `None`
///
/// Caller responsibilities:
/// - For rows 1-4: pass `output_form = Some(...)`.
/// - For rows 5-6: pass `output_form = None` (caller's flow ends in plain
///   `return expr` without an output-target form). The caller is responsible
///   for not invoking this helper at all when the only "return" is an
///   identifier path (e.g., `return some_name`) and that path needs a
///   different rendering strategy.
pub fn compute_return_sentence(
    return_type_text: Option<&str>,
    output_form: Option<&OutputTargetForm>,
    type_registry: &TypeRegistry,
) -> Option<String> {
    match output_form {
        Some(OutputTargetForm::Description(text)) => {
            Some(format!("Produce: {}.", normalize_ws(text)))
        }
        Some(OutputTargetForm::Identifier(name)) => Some(match return_type_text {
            Some(t) => match type_registry.descriptions.get(t) {
                Some(d) => format!("Produce `{}` (`{}`): {}.", name, t, normalize_ws(d)),
                None => format!("Produce `{}` (`{}`).", name, t),
            },
            None => format!("Produce `{}`.", name),
        }),
        None => match return_type_text {
            Some(t) => Some(match type_registry.descriptions.get(t) {
                Some(d) => format!("Return a `{}`: {}.", t, normalize_ws(d)),
                None => format!("Return a `{}`.", t),
            }),
            None => None,
        },
    }
}

/// Append the §8.4 sentence to a Step body. `body` is the rendered last-step
/// prose without a trailing newline. Callers join the sentence onto the body
/// after a single space, stripping any trailing period from the body so the
/// transition reads naturally.
pub fn append_return_sentence(body: &str, sentence: &str) -> String {
    let trimmed = body.trim_end().trim_end_matches('.').trim_end();
    if trimmed.is_empty() {
        sentence.to_string()
    } else {
        format!("{trimmed}. {sentence}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{OutputTargetForm, TypeRegistry};

    fn registry_with(name: &str, desc: &str) -> TypeRegistry {
        let mut r = TypeRegistry::default();
        r.descriptions.insert(name.into(), desc.into());
        r
    }

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
    fn row_1_descriptive_target_produces_x() {
        let form = OutputTargetForm::Description("a structured diagnosis".into());
        let s = compute_return_sentence(None, Some(&form), &TypeRegistry::default());
        assert_eq!(s.as_deref(), Some("Produce: a structured diagnosis."));
    }

    #[test]
    fn row_2_named_with_type_decl() {
        let form = OutputTargetForm::Identifier("diagnosis".into());
        let reg = registry_with("Diagnosis", "root cause and severity");
        let s = compute_return_sentence(Some("Diagnosis"), Some(&form), &reg);
        assert_eq!(
            s.as_deref(),
            Some("Produce `diagnosis` (`Diagnosis`): root cause and severity.")
        );
    }

    #[test]
    fn row_3_named_with_type_no_decl() {
        let form = OutputTargetForm::Identifier("diagnosis".into());
        let s = compute_return_sentence(Some("Diagnosis"), Some(&form), &TypeRegistry::default());
        assert_eq!(s.as_deref(), Some("Produce `diagnosis` (`Diagnosis`)."));
    }

    #[test]
    fn row_4_named_no_type() {
        let form = OutputTargetForm::Identifier("diagnosis".into());
        let s = compute_return_sentence(None, Some(&form), &TypeRegistry::default());
        assert_eq!(s.as_deref(), Some("Produce `diagnosis`."));
    }

    #[test]
    fn row_5_expr_with_type_decl() {
        let reg = registry_with("Diagnosis", "root cause and severity");
        let s = compute_return_sentence(Some("Diagnosis"), None, &reg);
        assert_eq!(
            s.as_deref(),
            Some("Return a `Diagnosis`: root cause and severity.")
        );
    }

    #[test]
    fn row_6_expr_with_type_no_decl() {
        let s = compute_return_sentence(Some("Diagnosis"), None, &TypeRegistry::default());
        assert_eq!(s.as_deref(), Some("Return a `Diagnosis`."));
    }

    #[test]
    fn row_8_no_type_no_target() {
        let s = compute_return_sentence(None, None, &TypeRegistry::default());
        assert!(s.is_none());
    }

    #[test]
    fn description_normalizes_whitespace() {
        let form = OutputTargetForm::Description("a  multi\nline\tdescription".into());
        let s = compute_return_sentence(None, Some(&form), &TypeRegistry::default()).unwrap();
        assert_eq!(s, "Produce: a multi line description.");
    }

    #[test]
    fn type_level_description_normalizes_whitespace() {
        let reg = registry_with("Foo", "first  line\nsecond\tline");
        let s = compute_return_sentence(Some("Foo"), None, &reg).unwrap();
        assert_eq!(s, "Return a `Foo`: first line second line.");
    }

    #[test]
    fn append_return_sentence_strips_trailing_period() {
        assert_eq!(
            append_return_sentence("Inspect the scope.", "Produce `diagnosis`."),
            "Inspect the scope. Produce `diagnosis`."
        );
        assert_eq!(
            append_return_sentence("Inspect the scope", "Produce `diagnosis`."),
            "Inspect the scope. Produce `diagnosis`."
        );
    }

    #[test]
    fn append_return_sentence_handles_empty_body() {
        assert_eq!(
            append_return_sentence("", "Produce `diagnosis`."),
            "Produce `diagnosis`."
        );
        assert_eq!(
            append_return_sentence("   ", "Produce `diagnosis`."),
            "Produce `diagnosis`."
        );
    }
}
