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

/// Decide whether `(` after `buf` is a call paren (`name.applies(`) or a
/// grouping paren (`not (...)`, ` (...)`). Conservative: any keyword is NOT a
/// receiver; any non-empty buffer ending in identifier-character IS.
fn is_call_receiver(buf: &str) -> bool {
    if matches!(buf, "not" | "and" | "or") {
        return false;
    }
    buf.chars()
        .last()
        .map_or(false, |c| c.is_ascii_alphanumeric() || c == '_')
}

/// Split a condition string into tokens, treating `"..."` as a single token
/// so quoted literals with internal spaces are not fragmented.
///
/// Grouping parens `(` / `)` are split as separate tokens, but call parens
/// (e.g. `name.applies()`, `has_tests(ctx)`) stay attached to their receiver
/// so that `classify_token` can match the `.applies()` suffix. The
/// `is_call_receiver` helper distinguishes the two cases.
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
            '(' if !buf.is_empty() && is_call_receiver(&buf) => {
                // Call paren: receiver in buf — accumulate `(...)` (with depth
                // tracking for nested args) into the same token.
                buf.push('(');
                iter.next();
                let mut depth = 1;
                while let Some(&inner) = iter.peek() {
                    iter.next();
                    buf.push(inner);
                    if inner == '(' {
                        depth += 1;
                    } else if inner == ')' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                }
                out.push(std::mem::take(&mut buf));
            }
            '(' | ')' => {
                if !buf.is_empty() {
                    out.push(std::mem::take(&mut buf));
                }
                out.push(c.to_string());
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

    #[test]
    fn tokenize_grouping_parens_split() {
        assert_eq!(
            tokenize_condition("(big or small)"),
            vec!["(", "big", "or", "small", ")"]
        );
    }

    #[test]
    fn tokenize_call_parens_kept() {
        assert_eq!(tokenize_condition("name.applies()"), vec!["name.applies()"]);
        assert_eq!(tokenize_condition("has_tests(ctx)"), vec!["has_tests(ctx)"]);
    }

    #[test]
    fn tokenize_keyword_paren_excluded() {
        // `not` is a keyword — `(` after it is grouping, not a call.
        assert_eq!(tokenize_condition("not(a)"), vec!["not", "(", "a", ")"]);
    }

    #[test]
    fn tokenize_grouping_with_call_inside() {
        assert_eq!(
            tokenize_condition("(a.applies() or b.applies())"),
            vec!["(", "a.applies()", "or", "b.applies()", ")"]
        );
    }

    #[test]
    fn tokenize_quoted_with_internal_spaces_unchanged() {
        assert_eq!(
            tokenize_condition("\"the user opted in\""),
            vec!["\"the user opted in\""]
        );
    }

    #[test]
    fn tokenize_eq_operator_split_by_whitespace() {
        assert_eq!(
            tokenize_condition("\"high\" == risk"),
            vec!["\"high\"", "==", "risk"]
        );
    }

    #[test]
    fn tokenize_empty_input() {
        assert!(tokenize_condition("").is_empty());
        assert!(tokenize_condition("   ").is_empty());
    }

    #[test]
    fn tokenize_adversarial_unspaced_documents_limitation() {
        // Spec line 466: parser/IR sources guarantee `or`/`and`/`not` are
        // space-separated. With paren-aware tokenization the call-paren
        // accumulator now terminates exactly at the matching `)`, so
        // `name.applies()or` splits into `name.applies()` + `or` (an
        // improvement over the pre-paren behavior). This test pins the new
        // behavior so the surface contract stays explicit.
        assert_eq!(
            tokenize_condition("name.applies()or other"),
            vec!["name.applies()", "or", "other"]
        );
    }
}
