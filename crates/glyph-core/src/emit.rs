//! Phase 7 (Emit) — deterministic Markdown projection.
//!
//! Walking-skeleton scope per `design/mvp-acceptance.md` §1: parameterless skill,
//! inline strings as Steps, constraint markers as bulleted Constraints. The output
//! shape is fixed by `design/compiled-output.md`.

use crate::ir::{IrArena, IrNode, Polarity};

pub fn emit(arena: &IrArena) -> String {
    let root_id = arena
        .root_skill()
        .expect("validate guarantees a root skill before emit");
    let skill = match arena.get(root_id) {
        IrNode::Skill(s) => s,
        _ => unreachable!("root skill ID must point to a Skill node"),
    };

    let mut out = String::new();

    // ----- Frontmatter -----
    out.push_str("---\n");
    out.push_str(&format!("name: {}\n", skill.name));
    out.push_str(&format!("description: {}\n", skill.description));
    if !skill.effects.is_empty() {
        out.push_str(&format!("effects: [{}]\n", skill.effects.join(", ")));
    }
    out.push_str("---\n\n");

    // ----- ## Parameters (conditional) -----
    // Per `design/compiled-output.md` §`## Parameters`, the section is emitted
    // only when the skill declares one or more parameters. Each entry renders
    // as a bulleted item with either `(default: <value>)` or `(required)`. The
    // walking-skeleton emitter does not generate descriptions yet (Step 2 LLM
    // work in a later slice), so we omit the description fragment.
    if !skill.params.is_empty() {
        out.push_str("## Parameters\n\n");
        for p in &skill.params {
            match &p.default {
                Some(v) => {
                    out.push_str(&format!("- **{}** (default: {})\n", p.name, v));
                }
                None => {
                    out.push_str(&format!("- **{}** (required)\n", p.name));
                }
            }
        }
        out.push('\n');
    }

    // ----- ## Instructions -----
    out.push_str("## Instructions\n\n");

    // ### Steps (numbered list).
    if !skill.steps.is_empty() {
        out.push_str("### Steps\n\n");
        for (idx, step_id) in skill.steps.iter().enumerate() {
            let text = match arena.get(*step_id) {
                IrNode::InlineInstruction(i) => i.text.clone(),
                _ => panic!("Step node was not an InlineInstruction"),
            };
            out.push_str(&format!("{}. {}\n", idx + 1, text));
        }
        out.push('\n');
    }

    // ### Constraints (bulleted list).
    if !skill.constraints.is_empty() {
        out.push_str("### Constraints\n\n");
        for c_id in &skill.constraints {
            let c = match arena.get(*c_id) {
                IrNode::Constraint(c) => c,
                _ => panic!("Constraint node was not a Constraint"),
            };
            let line = render_constraint(&c.text, c.polarity);
            out.push_str(&format!("- {}\n", line));
        }
        out.push('\n');
    }

    // Trim trailing blank line for byte-stable output.
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

/// Deterministic constraint phrasing per `design/compiled-output.md` §Constraint Rendering.
///
/// The walking skeleton does not invoke an LLM, so this is a small, table-driven
/// projection of polarity onto the resolved text. `require`-polarity text is emitted
/// verbatim. `avoid`-polarity text is reshaped into "Do not <base-form> ...".
fn render_constraint(text: &str, polarity: Polarity) -> String {
    match polarity {
        Polarity::Require => text.to_string(),
        Polarity::Avoid => avoid_phrasing(text),
    }
}

/// Convert the `avoid`-polarity resolved text into a "Do not ..." prohibition.
///
/// Rule: split off the first whitespace-delimited word, gerund-strip it (drop trailing
/// `ing`, optionally re-adding `e` for verbs whose base ends in a single consonant), then
/// emit `Do not <base> <rest>`. If the first word is not a recognisable gerund the text
/// is returned unchanged with a leading "Do not ".
///
/// Walking-skeleton coverage is scoped to the corpus in `mvp-acceptance.md` §1 — namely
/// "Leaving references to removed or renamed symbols." The general rule is intentionally
/// minimal; broader natural-language reshaping is Step 2 (LLM) work in later slices.
fn avoid_phrasing(text: &str) -> String {
    let mut parts = text.splitn(2, char::is_whitespace);
    let first = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("");
    let base = gerund_to_base(first);
    if rest.is_empty() {
        format!("Do not {}", base)
    } else {
        format!("Do not {} {}", base, rest)
    }
}

/// Convert a gerund (e.g., "Leaving") to its base verb form (e.g., "leave").
fn gerund_to_base(word: &str) -> String {
    let lower = word.to_lowercase();
    if let Some(stem) = lower.strip_suffix("ing") {
        // Heuristic: if the stem ends in a single consonant after a single vowel, append "e"
        // (consume → consum + e). For "Leaving" → "Leav" → "Leave".
        let s_bytes = stem.as_bytes();
        let needs_e = match s_bytes.len() {
            0 => false,
            n => {
                let last = s_bytes[n - 1];
                let prev = if n >= 2 { Some(s_bytes[n - 2]) } else { None };
                let is_vowel = |b: u8| matches!(b, b'a' | b'e' | b'i' | b'o' | b'u');
                let is_consonant = |b: u8| b.is_ascii_alphabetic() && !is_vowel(b);
                is_consonant(last) && prev.map(is_vowel).unwrap_or(false)
            }
        };
        if needs_e {
            format!("{}e", stem)
        } else {
            stem.to_string()
        }
    } else {
        // Not a gerund — return lowercased original.
        lower
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gerund_to_base_known() {
        assert_eq!(gerund_to_base("Leaving"), "leave");
        assert_eq!(gerund_to_base("Making"), "make");
        assert_eq!(gerund_to_base("Adding"), "add");
    }

    #[test]
    fn avoid_phrasing_walking_skeleton() {
        let s = avoid_phrasing("Leaving references to removed or renamed symbols.");
        assert_eq!(s, "Do not leave references to removed or renamed symbols.");
    }
}
