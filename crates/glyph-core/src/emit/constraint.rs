//! Bold colon-marker constraint renderer.
//!
//! The polarity label (`Must` / `Must avoid` / `Require` / `Avoid`) is
//! grammatically isolated from the const body, so an author may write the
//! body in any natural shape — declarative, gerund, noun phrase — without
//! the emitter trying to graft a verb onto it.

use crate::ir::{Polarity, Strength};

pub fn render(strength: Strength, polarity: Polarity, text: &str) -> String {
    let label = match (strength, polarity) {
        (Strength::Hard, Polarity::Require) => "Must",
        (Strength::Hard, Polarity::Avoid) => "Must avoid",
        (Strength::Soft, Polarity::Require) => "Require",
        (Strength::Soft, Polarity::Avoid) => "Avoid",
    };
    let body = text.trim();
    if ends_with_sentence_punctuation(body) {
        format!("**{label}:** {body}")
    } else {
        format!("**{label}:** {body}.")
    }
}

fn ends_with_sentence_punctuation(text: &str) -> bool {
    matches!(text.chars().last(), Some('.') | Some('!') | Some('?'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_require() {
        assert_eq!(
            render(Strength::Hard, Polarity::Require, "Stay focused."),
            "**Must:** Stay focused."
        );
    }

    #[test]
    fn hard_avoid() {
        assert_eq!(
            render(Strength::Hard, Polarity::Avoid, "Skipping the tests."),
            "**Must avoid:** Skipping the tests."
        );
    }

    #[test]
    fn soft_require() {
        assert_eq!(
            render(Strength::Soft, Polarity::Require, "Tests pass."),
            "**Require:** Tests pass."
        );
    }

    #[test]
    fn soft_avoid() {
        assert_eq!(
            render(Strength::Soft, Polarity::Avoid, "Leaving stale references."),
            "**Avoid:** Leaving stale references."
        );
    }

    #[test]
    fn preserves_author_capitalization() {
        assert_eq!(
            render(Strength::Soft, Polarity::Avoid, "leaving stale references."),
            "**Avoid:** leaving stale references."
        );
    }

    #[test]
    fn appends_period_when_missing() {
        assert_eq!(
            render(Strength::Soft, Polarity::Avoid, "Leaving stale references"),
            "**Avoid:** Leaving stale references."
        );
    }

    #[test]
    fn preserves_question_mark() {
        assert_eq!(
            render(Strength::Soft, Polarity::Require, "Did you check the tests?"),
            "**Require:** Did you check the tests?"
        );
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(
            render(Strength::Hard, Polarity::Require, "  Stay focused.  "),
            "**Must:** Stay focused."
        );
    }

    #[test]
    fn declarative_body_under_avoid_reads_cleanly() {
        // The whole point of the colon-marker shape: a declarative body
        // ("Routing is by …") under `avoid` polarity emits without trying
        // to graft a verb onto the front.
        assert_eq!(
            render(
                Strength::Soft,
                Polarity::Avoid,
                "Routing is by the per-surface manifest's `name` field, not the cmux tab title."
            ),
            "**Avoid:** Routing is by the per-surface manifest's `name` field, not the cmux tab title."
        );
    }
}
