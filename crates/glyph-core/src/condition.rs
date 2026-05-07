//! Condition classification — decoded representation of `if` / `elif`
//! condition strings, owned by Analyze and consumed by Lower.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

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

/// One classified token from a condition string. Carries the original text
/// (so emit can render verbatim) plus its kind and whether it sits inside an
/// `==` operand expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedConditionToken {
    pub text: String,
    pub kind: ConditionTokenKind,
    /// True when this token is part of an `==` operand expression. Operands
    /// keep their underlying `kind`; summary flags ignore them; emit renders
    /// them verbatim (preserving quotes for string operands).
    pub is_comparison_operand: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConditionClassification {
    pub tokens: Vec<ClassifiedConditionToken>,
    /// All summary flags below count only tokens with
    /// `is_comparison_operand == false`.
    pub has_boolean_token: bool,
    pub has_predicate_token: bool,
    /// `and` and `not` only. `or` is allowed in pure-predicate framing per
    /// `design/data-flow.md` §Composition Rules and intentionally does NOT
    /// set this flag.
    pub has_compositional_operator: bool,
    /// `==` presence; also sets `has_boolean_token` so pure-predicate fails
    /// closed.
    pub has_comparison_operator: bool,
    /// Numeric token in non-operand position (i.e., bare numeric in
    /// condition).
    pub has_numeric_bare_condition: bool,
}

impl ConditionClassification {
    /// Mirrors `BranchPredicateShape::is_pure_predicate` (crate::ir).
    pub fn is_pure_predicate(&self) -> bool {
        self.has_predicate_token
            && !self.has_boolean_token
            && !self.has_compositional_operator
            && !self.has_comparison_operator
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

/// Context for classifying a single condition. Owned by Analyze; built once
/// per enclosing decl (skill or block) and reused for every branch inside.
pub struct ConditionContext<'a> {
    /// Same-file consts ∪ imported consts, mapped to inferred TypeTag.
    /// Used to disambiguate a bare identifier as `PredicateConst` (String-typed)
    /// vs `Boolean` / `Numeric`.
    pub consts: HashMap<&'a str, crate::kind_infer::TypeTag>,
    /// Param names whose default value is a string literal or String-typed
    /// const ref. Treated as PredicateConst when used bare in a condition.
    pub params_with_string_default: HashSet<&'a str>,
    /// Local bindings produced by `name = call(...)`. MVP: classified as
    /// Boolean. Kind-tracking deferred (see Out of Scope §1 in design spec).
    /// Currently always empty: the AST has no binding-form `FlowStmt` variant
    /// yet. Reserved for the kind-tracking work that will land later.
    pub bindings: HashSet<&'a str>,
}

impl<'a> ConditionContext<'a> {
    /// Build a context for an enclosing decl. Caller passes:
    /// - `file`: same-file decls (used to find same-file consts).
    /// - `imported_texts`: imported const name → rendered body. Already
    ///   prepared at the analyze caller as `resolved_imports.text_values`.
    /// - `imported_const_types`: complementary TypeTag map.
    /// - `enclosing_params`: header params of the enclosing skill/block.
    /// - `enclosing_flow`: flow body. Walked here to extract binding LHS into
    ///   `bindings` (currently no-op; see field doc). Caller passes `&[]` for
    ///   export blocks.
    pub fn for_decl(
        file: &'a crate::ast::SourceFile,
        imported_texts: &'a BTreeMap<String, String>,
        imported_const_types: &'a BTreeMap<String, crate::kind_infer::TypeTag>,
        enclosing_params: &'a [crate::ast::Param],
        enclosing_flow: &'a [crate::ast::FlowStmt],
    ) -> Self {
        let mut consts: HashMap<&'a str, crate::kind_infer::TypeTag> = HashMap::new();

        // Same-file consts.
        for decl in &file.decls {
            if let crate::ast::Decl::Const(spanned) = decl {
                let name: &'a str = &spanned.node.name;
                let value = &spanned.node.value;
                let lit = match value {
                    crate::ast::ConstValue::String(s) => {
                        crate::kind_infer::Literal::String(s.clone())
                    }
                    crate::ast::ConstValue::Int(s) | crate::ast::ConstValue::Float(s) => {
                        crate::kind_infer::Literal::Number(s.clone())
                    }
                    crate::ast::ConstValue::Bool(s) => crate::kind_infer::Literal::Bool(s.clone()),
                };
                consts.insert(name, crate::kind_infer::infer_primitive(&lit));
            }
        }

        // Imported consts: ensure every imported name has a TypeTag entry.
        // Prefer the explicit imported_const_types when present; fall back to
        // String when only the rendered body is available (existing behavior).
        for name in imported_texts.keys() {
            let tag = imported_const_types
                .get(name)
                .cloned()
                .unwrap_or(crate::kind_infer::TypeTag::String);
            let key: &'a str = name.as_str();
            consts.entry(key).or_insert(tag);
        }

        // Params with string default. `Param.default` is a pre-rendered
        // String including the surrounding quotes for string literals
        // (see ast.rs:215-221). Detect a string default by leading `"`,
        // skipping name-references which carry a bare ident.
        let mut params_with_string_default: HashSet<&'a str> = HashSet::new();
        for param in enclosing_params {
            let name: &'a str = &param.name;
            if param.default_is_name_ref {
                continue; // name-ref defaults: kind tracked elsewhere.
            }
            if let Some(default) = &param.default {
                if default.starts_with('"') {
                    params_with_string_default.insert(name);
                }
            }
        }

        // Bindings: LHS of `name = call(...)` flow statements. The AST has no
        // binding variant yet, so this loop currently inserts nothing. The
        // walk is retained so future bindings light up automatically once a
        // binding-form variant is added.
        let bindings: HashSet<&'a str> = HashSet::new();
        for _stmt in enclosing_flow {
            // No binding-form variant in current AST — see field doc.
        }

        Self {
            consts,
            params_with_string_default,
            bindings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_predicate_requires_predicate_token_and_no_compositional_operator() {
        let c = ConditionClassification {
            tokens: vec![ClassifiedConditionToken {
                text: "complex_change".to_string(),
                kind: ConditionTokenKind::PredicateConst,
                is_comparison_operand: false,
            }],
            has_boolean_token: false,
            has_predicate_token: true,
            has_compositional_operator: false,
            has_comparison_operator: false,
            has_numeric_bare_condition: false,
        };
        assert!(c.is_pure_predicate());
    }

    #[test]
    fn pure_predicate_rejects_boolean_token() {
        let c = ConditionClassification {
            tokens: vec![],
            has_boolean_token: true,
            has_predicate_token: true,
            has_compositional_operator: false,
            has_comparison_operator: false,
            has_numeric_bare_condition: false,
        };
        assert!(!c.is_pure_predicate());
    }

    #[test]
    fn pure_predicate_rejects_compositional_operator() {
        let c = ConditionClassification {
            tokens: vec![],
            has_boolean_token: false,
            has_predicate_token: true,
            has_compositional_operator: true,
            has_comparison_operator: false,
            has_numeric_bare_condition: false,
        };
        assert!(!c.is_pure_predicate());
    }

    #[test]
    fn classified_token_carries_text_kind_operand_flag() {
        let t = ClassifiedConditionToken {
            text: "risk".to_string(),
            kind: ConditionTokenKind::Boolean,
            is_comparison_operand: true,
        };
        assert_eq!(t.text, "risk");
        assert_eq!(t.kind, ConditionTokenKind::Boolean);
        assert!(t.is_comparison_operand);
    }

    #[test]
    fn extended_classification_has_comparison_and_numeric_bare_flags() {
        let c = ConditionClassification {
            tokens: vec![],
            has_boolean_token: false,
            has_predicate_token: false,
            has_compositional_operator: false,
            has_comparison_operator: true,
            has_numeric_bare_condition: false,
        };
        // == sets has_comparison_operator AND fails has_predicate purity.
        assert!(c.has_comparison_operator);
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
