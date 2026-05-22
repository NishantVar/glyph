//! Locked deterministic-emitter templates. Single-grep changes only.

use crate::ir::{OutputTargetForm, TypeRegistry};

pub const EXTERNAL_FILE_TEMPLATE: &str = "Load and follow the procedure in `{path}`.";

pub fn kebab_case(snake: &str) -> String {
    snake.replace('_', "-")
}

pub fn external_file_step(path: &str) -> String {
    EXTERNAL_FILE_TEMPLATE.replace("{path}", path)
}

/// Pick the effective description for a parameter:
/// 1. Per-param `<"…">` (highest precedence).
/// 2. Type-level `type Foo = <"…">` lookup via the registry, when the param
///    has a `name: Foo` annotation.
/// 3. None.
///
/// Shared by the skill `## Parameters` emitter (`scaffold.rs`) and the Tier 3
/// procedure-file emitter (`emit_procedure`) so the two paths cannot drift.
pub fn effective_param_description(
    per_param: Option<&str>,
    type_annotation: Option<&str>,
    type_registry: &TypeRegistry,
) -> Option<String> {
    if let Some(d) = per_param {
        return Some(d.to_string());
    }
    type_annotation.and_then(|t| type_registry.get(t).cloned())
}

/// Render one `## Parameters` bullet given the four authored fields, returning
/// the rendered text **including** the trailing newline. Mirrors the three
/// shapes used by the skill emitter (`scaffold.rs`):
///
/// - multi-line: description has a `\n` or exceeds 120 chars.
/// - single-line with description: `- **name** (Type): desc. Default: X.`
/// - no description: `- **name** (Type). Default: X.` / `Required.`
///
/// The `(Type)` suffix is omitted when `type_annotation` is `None`. The
/// description is rendered verbatim (caller has already chosen
/// per-param vs type-level via `effective_param_description`).
pub fn render_param_bullet(
    name: &str,
    type_annotation: Option<&str>,
    description: Option<&str>,
    default: Option<&str>,
) -> String {
    let type_suffix = match type_annotation {
        Some(t) => format!(" ({})", t),
        None => String::new(),
    };
    let meta_tail = match default {
        Some(v) => format!("Default: {}.", v),
        None => "Required.".to_string(),
    };
    match description {
        Some(desc_text) if desc_text.contains('\n') || desc_text.len() > 120 => {
            let mut out = format!("- **{}**{}:\n", name, type_suffix);
            for line in desc_text.lines() {
                out.push_str(&format!("  {}\n", line));
            }
            out.push_str(&format!("  {}\n", meta_tail));
            out
        }
        Some(desc_text) => {
            let trimmed = desc_text.trim_end_matches('.').trim_end();
            format!(
                "- **{}**{}: {}. {}\n",
                name, type_suffix, trimmed, meta_tail
            )
        }
        None => format!("- **{}**{}. {}\n", name, type_suffix, meta_tail),
    }
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
            Some(t) => match type_registry.get(t) {
                Some(d) => format!("Produce `{}` (`{}`): {}.", name, t, normalize_ws(d)),
                None => format!("Produce `{}` (`{}`).", name, t),
            },
            None => format!("Produce `{}`.", name),
        }),
        None => return_type_text.map(|t| match type_registry.get(t) {
            Some(d) => format!("Return a `{}`: {}.", t, normalize_ws(d)),
            None => format!("Return a `{}`.", t),
        }),
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
        r.insert(name, desc.into());
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

    /// Codex finding #3: TypeRegistry keys are §D6 canonical (ASCII-lower +
    /// strip underscores), so `type RepoContext = …` registered under
    /// `RepoContext` is reachable via `repo_context`, `REPOCONTEXT`,
    /// `repo__context`, etc. The look-up site (`compute_return_sentence` and
    /// `effective_param_description`) must apply the same canonicalization.
    #[test]
    fn type_registry_lookup_is_d6_canonical() {
        let reg = registry_with("RepoContext", "context about this repo");
        // Sanity: exact spelling resolves.
        let exact = compute_return_sentence(Some("RepoContext"), None, &reg);
        assert_eq!(
            exact.as_deref(),
            Some("Return a `RepoContext`: context about this repo.")
        );
        // Snake-case variant: canonical form matches the registry key.
        let snake = compute_return_sentence(Some("repo_context"), None, &reg);
        assert_eq!(
            snake.as_deref(),
            Some("Return a `repo_context`: context about this repo.")
        );
        // Per-param annotation lookup uses the same path.
        let bullet = effective_param_description(None, Some("repo_context"), &reg);
        assert_eq!(bullet.as_deref(), Some("context about this repo"));
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
