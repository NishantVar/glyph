//! `{name}` slot scanner shared by Parse and Analyze.
//!
//! Glyph string literals may contain `{IDENTIFIER}` slots that resolve to
//! parameters or local bindings. Per `design/values-and-names.md` §No
//! Interpolation and `design/build-foundation.md` §A2 (Tokenizer), slots:
//!
//! - Are recognised by the tokenizer when scanning string content.
//! - Use the strict `{IDENTIFIER}` grammar — no whitespace, no expressions,
//!   no nested braces.
//! - Are legal only in **instruction-bearing** string positions (Step /
//!   Constraint / Context prose, generated block bodies, inline `flow:`
//!   instruction strings). Any `{name}` in a non-instruction string —
//!   `description:` body, parameter default value, etc. — fires
//!   `G::parse::param-slot-in-non-instruction-string` (`design/diagnostics.md`).
//!
//! The walking-skeleton tokenizer keeps string content as a single
//! `StringLit(String)` token (cooked text). Slot recognition is therefore
//! implemented as a post-pass over the literal content. Two helpers:
//!
//! - [`scan_slots`] — return every `{name}` occurrence in a string with the
//!   byte offset *within the literal content* (not the source). Used by
//!   Analyze for `G::analyze::unknown-param-slot`.
//! - [`first_slot_offset`] — the byte offset of the first `{name}` slot, or
//!   `None`. Used by Parse for the non-instruction-string check.
//!
//! Identifiers follow `values-and-names.md`: `[A-Za-z_][A-Za-z0-9_]*`.

/// One `{name}` slot found inside a string literal's cooked content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotMatch {
    /// Byte offset of the opening `{` *within the literal content* (0-indexed).
    pub start_in_content: usize,
    /// Byte offset just past the closing `}`.
    pub end_in_content: usize,
    /// Identifier between the braces.
    pub name: String,
}

/// Scan a literal-string content for `{IDENTIFIER}` slots.
pub fn scan_slots(content: &str) -> Vec<SlotMatch> {
    let bytes = content.as_bytes();
    let mut out: Vec<SlotMatch> = Vec::new();
    let mut i: usize = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b != b'{' {
            i += 1;
            continue;
        }
        let open = i;
        let id_start = i + 1;
        // First identifier byte must be alpha or `_`.
        if id_start >= bytes.len() || !is_ident_start(bytes[id_start]) {
            i += 1;
            continue;
        }
        let mut j = id_start;
        while j < bytes.len() && is_ident_continue(bytes[j]) {
            j += 1;
        }
        if j < bytes.len() && bytes[j] == b'}' {
            let name = std::str::from_utf8(&bytes[id_start..j])
                .expect("ASCII identifier")
                .to_string();
            out.push(SlotMatch {
                start_in_content: open,
                end_in_content: j + 1,
                name,
            });
            i = j + 1;
        } else {
            i = open + 1;
        }
    }
    out
}

/// Convenience: byte offset of the first slot, or `None`.
pub fn first_slot_offset(content: &str) -> Option<usize> {
    scan_slots(content).first().map(|s| s.start_in_content)
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}
fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_no_slots() {
        assert!(scan_slots("").is_empty());
        assert!(scan_slots("plain text").is_empty());
    }

    #[test]
    fn single_slot() {
        let r = scan_slots("hello {scope} world");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].name, "scope");
        assert_eq!(&"hello {scope} world"[r[0].start_in_content..r[0].end_in_content], "{scope}");
    }

    #[test]
    fn multiple_slots() {
        let r = scan_slots("{a} and {b_2}");
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].name, "a");
        assert_eq!(r[1].name, "b_2");
    }

    #[test]
    fn malformed_slot_ignored() {
        // `{}` is not a valid slot (empty identifier); `{1bad}` starts with a digit;
        // `{open` has no closing brace.
        assert!(scan_slots("{}").is_empty());
        assert!(scan_slots("{1bad}").is_empty());
        assert!(scan_slots("{open").is_empty());
    }

    #[test]
    fn first_slot_offset_helper() {
        assert_eq!(first_slot_offset("plain"), None);
        assert_eq!(first_slot_offset("a {x} b"), Some(2));
    }
}
