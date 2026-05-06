//! Locked four-form `(strength × polarity)` constraint renderer.
//!
//! See `obsidian/plans/expand-emitter-design-2026-05-04.md` §Locked Templates
//! and `design/compiled-output.md` §Constraint Rendering.

use crate::ir::{Polarity, Strength};

pub const HARD_REQUIRE: &str = "You must {text}.";
pub const HARD_AVOID: &str = "You must never {text}.";
pub const SOFT_REQUIRE: &str = "{Text}.";
pub const SOFT_AVOID: &str = "Avoid {text}.";

pub fn render(strength: Strength, polarity: Polarity, text: &str) -> String {
    // Mixed-corpus tolerance for soft `avoid` only: when the author has
    // supplied a fully-formed prohibition (`"Avoid ..."` or `"Do not ..."`),
    // emit it verbatim instead of wrapping it in another `Avoid {text}.`
    // template, which would produce double-prohibition outputs like
    // `Avoid avoid leaving...` or `Avoid do not make changes...`.
    //
    // Hard avoid intentionally falls through to the locked
    // `You must never {text}.` template — silently dropping the hard
    // strength wording would be worse than the cosmetic ugliness of a
    // doubled prohibition there. Tracked in `design/todo_bugs.md` §Emitter
    // (issue #141): the Phase 5 lint will enforce a canonical const-text
    // shape and let this branch be removed.
    if matches!(strength, Strength::Soft)
        && matches!(polarity, Polarity::Avoid)
        && is_already_prohibition(text)
    {
        let trimmed = text.trim().trim_end_matches('.');
        return format!("{}.", trimmed);
    }
    let normalized = normalize(text);
    match (strength, polarity) {
        (Strength::Hard, Polarity::Require) => HARD_REQUIRE.replace("{text}", &normalized),
        (Strength::Hard, Polarity::Avoid) => HARD_AVOID.replace("{text}", &normalized),
        (Strength::Soft, Polarity::Require) => {
            SOFT_REQUIRE.replace("{Text}", &capitalize_first(&normalized))
        }
        (Strength::Soft, Polarity::Avoid) => SOFT_AVOID.replace("{text}", &normalized),
    }
}

/// True when `text` is already a fully-formed prohibition (case-insensitively
/// starts with `"Avoid "` or `"Do not "`).
fn is_already_prohibition(text: &str) -> bool {
    let lower = text.trim().to_lowercase();
    lower.starts_with("avoid ") || lower.starts_with("do not ")
}

fn normalize(text: &str) -> String {
    let trimmed = text.trim().trim_end_matches('.');
    let mut chars = trimmed.chars();
    match chars.next() {
        Some(c) if c.is_uppercase() => {
            let mut out = String::new();
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            out.push_str(chars.as_str());
            out
        }
        _ => trimmed.to_string(),
    }
}

fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(c) => {
            let mut out = String::new();
            for uc in c.to_uppercase() {
                out.push(uc);
            }
            out.push_str(chars.as_str());
            out
        }
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_require() {
        assert_eq!(
            render(Strength::Hard, Polarity::Require, "stay focused"),
            "You must stay focused."
        );
    }

    #[test]
    fn hard_avoid() {
        assert_eq!(
            render(Strength::Hard, Polarity::Avoid, "skip the tests"),
            "You must never skip the tests."
        );
    }

    #[test]
    fn soft_require_capitalizes() {
        assert_eq!(
            render(Strength::Soft, Polarity::Require, "tests pass"),
            "Tests pass."
        );
    }

    #[test]
    fn soft_avoid() {
        assert_eq!(
            render(Strength::Soft, Polarity::Avoid, "leaving stale references"),
            "Avoid leaving stale references."
        );
    }

    #[test]
    fn normalizes_capital_and_period() {
        assert_eq!(
            render(Strength::Soft, Polarity::Avoid, "Leaving stale references."),
            "Avoid leaving stale references."
        );
    }

    #[test]
    fn soft_require_strips_period_then_capitalizes() {
        assert_eq!(
            render(Strength::Soft, Polarity::Require, "tests pass."),
            "Tests pass."
        );
    }

    #[test]
    fn soft_avoid_passes_through_already_prefixed_text() {
        // Authored prohibitions emit verbatim — never doubled like
        // "Avoid avoid leaving..." or "Avoid do not make changes...".
        assert_eq!(
            render(
                Strength::Soft,
                Polarity::Avoid,
                "Avoid leaving references to removed or renamed symbols."
            ),
            "Avoid leaving references to removed or renamed symbols."
        );
        assert_eq!(
            render(
                Strength::Soft,
                Polarity::Avoid,
                "Do not make changes unrelated to the task."
            ),
            "Do not make changes unrelated to the task."
        );
    }

    #[test]
    fn hard_avoid_does_not_pass_through_prefixed_text() {
        // The pass-through is intentionally limited to soft avoid; hard avoid
        // must preserve the "You must never ..." strength wording even when the
        // authored const text is already prohibition-prefixed. The Phase 5
        // const-shape lint (issue #141) will reject such authoring upstream of
        // Emit. We assert the strength-marker prefix rather than the exact
        // (cosmetically-doubled) wording, which is documented in
        // `design/todo_bugs.md` §Emitter.
        let out = render(
            Strength::Hard,
            Polarity::Avoid,
            "Do not make changes outside the requested scope.",
        );
        assert!(
            out.starts_with("You must never "),
            "expected hard-strength wording to be preserved; got {out:?}"
        );
    }

    #[test]
    fn avoid_pass_through_is_case_insensitive() {
        assert_eq!(
            render(Strength::Soft, Polarity::Avoid, "avoid foo."),
            "avoid foo."
        );
        assert_eq!(
            render(Strength::Soft, Polarity::Avoid, "DO NOT bar."),
            "DO NOT bar."
        );
    }

    #[test]
    fn avoid_pass_through_normalizes_trailing_period() {
        assert_eq!(
            render(Strength::Soft, Polarity::Avoid, "Avoid X"),
            "Avoid X."
        );
    }
}
