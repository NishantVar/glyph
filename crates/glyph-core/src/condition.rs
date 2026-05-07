//! Condition classification — decoded representation of `if` / `elif`
//! condition strings, owned by Analyze and consumed by Lower.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionTokenKind {
    Boolean,
    Numeric,
    PredicateApplies,
    PredicateConst,
    PredicateLiteral,
    Operator,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConditionClassification {
    pub tokens: Vec<ConditionTokenKind>,
    pub has_boolean_token: bool,
    pub has_predicate_token: bool,
    pub has_compositional_operator: bool,
    pub has_numeric_token: bool,
}

impl ConditionClassification {
    // Mirrors `BranchPredicateShape::is_pure_predicate` (crate::ir).
    pub fn is_pure_predicate(&self) -> bool {
        self.has_predicate_token && !self.has_boolean_token && !self.has_compositional_operator
    }
}

/// Split a condition string into tokens, treating `"..."` as a single token
/// so quoted literals with internal spaces are not fragmented.
///
/// Note: '(' and ')' are NOT split as separate tokens.
/// `my_block.applies()` must remain a single token so that `classify_token`
/// can match the `.applies()` suffix.  Standalone `(` / `)` only appear as
/// operator tokens when they are separated from other tokens by whitespace
/// (the whitespace arm handles the split, and `classify_token` maps them to
/// Operator).
///
/// // TODO: escaped quotes are not supported (`"a\"b"` truncates at the backslash-quote).
pub fn tokenize_condition(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut iter = s.chars().peekable();
    let mut buf = String::new();
    while let Some(&c) = iter.peek() {
        match c {
            '"' => {
                if !buf.is_empty() {
                    out.push(std::mem::take(&mut buf));
                }
                let mut lit = String::from('"');
                iter.next();
                while let Some(&inner) = iter.peek() {
                    iter.next();
                    lit.push(inner);
                    if inner == '"' {
                        break;
                    }
                }
                out.push(lit);
            }
            ' ' | '\t' => {
                if !buf.is_empty() {
                    out.push(std::mem::take(&mut buf));
                }
                iter.next();
            }
            _ => {
                buf.push(c);
                iter.next();
            }
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_predicate_requires_predicate_token_and_no_compositional_operator() {
        let c = ConditionClassification {
            tokens: vec![ConditionTokenKind::PredicateConst],
            has_boolean_token: false,
            has_predicate_token: true,
            has_compositional_operator: false,
            has_numeric_token: false,
        };
        assert!(c.is_pure_predicate());
    }

    #[test]
    fn pure_predicate_rejects_boolean_token() {
        let c = ConditionClassification {
            tokens: vec![
                ConditionTokenKind::PredicateConst,
                ConditionTokenKind::Operator,
                ConditionTokenKind::Boolean,
            ],
            has_boolean_token: true,
            has_predicate_token: true,
            has_compositional_operator: true,
            has_numeric_token: false,
        };
        assert!(!c.is_pure_predicate());
    }

    #[test]
    fn pure_predicate_rejects_compositional_operator() {
        let c = ConditionClassification {
            tokens: vec![
                ConditionTokenKind::PredicateConst,
                ConditionTokenKind::Operator,
                ConditionTokenKind::PredicateConst,
            ],
            has_boolean_token: false,
            has_predicate_token: true,
            has_compositional_operator: true,
            has_numeric_token: false,
        };
        assert!(!c.is_pure_predicate());
    }
}
