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
/// Backslash escapes (`\"`, `\\`, `\n`, `\t`, and unknown `\X`) are honored
/// inside `"..."` and preserved verbatim in the returned token text, mirroring
/// the canonical string-literal escape policy in `tokenize.rs`.
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
                    if inner == '\\' {
                        // Backslash escape — consume the next char verbatim so an
                        // escaped quote (`\"`) or escaped backslash (`\\`) does not
                        // terminate the literal. Token text preserves the surface form
                        // (the raw source slice including the escape sequence); the
                        // recognized-escapes set mirrors `tokenize.rs`
                        // (`scan_triple_string` and the inline `"..."` scanner):
                        // `\"`, `\\`, `\n`, `\t`. Unknown escapes (`\X`) and a trailing
                        // `\` at end of input are preserved as literal source bytes —
                        // we do not decode here because consumers compare against the
                        // surface token text. A trailing `\` with no following char
                        // falls through to the outer loop terminating naturally
                        // (matching the canonical scanner's `p + 1 < content_end`
                        // guard).
                        if let Some(&esc) = iter.peek() {
                            iter.next();
                            lit.push(esc);
                        }
                        continue;
                    }
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

/// Lightweight typed metadata about a flow-local binding, attached to
/// `ConditionContext` for branch-condition classification. Mirrors a subset
/// of `analyze::FlowLocalType` — the classifier itself only needs the
/// agent-shape flag today, but the field is reserved so future kind-aware
/// classification (e.g. distinguishing agent-bindings from value-bindings
/// when used bare) can land without re-plumbing the context.
///
/// Spec `.flow-assign-spec.md` §6.3 (Codex Round 2 High 4): the existing
/// `ConditionContext.bindings` field is untyped (`HashSet<&str>`). The spec
/// asks the implementer to extend it OR add a parallel typed map so
/// flow-local types reach the matcher. We add the typed map here; the
/// untyped `bindings` set is preserved for backward compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConditionFlowLocal {
    /// True iff the producer call returns an agent-shape value (e.g.
    /// `subagent(...)`). See spec §9.1 for the agent-shape rule.
    pub is_agent: bool,
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
    /// Populated for skill-flow contexts via `for_branch_with_consts`; empty
    /// for block-flow contexts (block flow rejects bindings at analyze time).
    pub bindings: HashSet<&'a str>,
    /// Typed flow-local metadata, parallel to `bindings`. See
    /// [`ConditionFlowLocal`] doc for the rationale.
    pub flow_local_bindings: HashMap<&'a str, ConditionFlowLocal>,
}

/// Walk same-file decls and imported-const data into a single
/// `consts: name → TypeTag` table. Extracted from `for_decl` so callers that
/// already hold a borrow on `&mut file.decls` can pre-compute the table once
/// (via [`collect_consts_for_file`]) and feed it to
/// [`ConditionContext::for_decl_with_consts`] without re-borrowing the file.
pub fn collect_consts_for_file<'a>(
    file: &'a crate::ast::SourceFile,
    imported_texts: &'a BTreeMap<String, String>,
    imported_const_types: &'a BTreeMap<String, crate::kind_infer::TypeTag>,
) -> HashMap<&'a str, crate::kind_infer::TypeTag> {
    let mut consts: HashMap<&'a str, crate::kind_infer::TypeTag> = HashMap::new();

    // Same-file consts.
    for decl in &file.decls {
        if let crate::ast::Decl::Const(spanned) = decl {
            let name: &'a str = &spanned.node.name;
            let value = &spanned.node.value;
            let lit = match value {
                crate::ast::ConstValue::String(s) => crate::kind_infer::Literal::String(s.clone()),
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

    consts
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
        let consts = collect_consts_for_file(file, imported_texts, imported_const_types);
        Self::for_decl_with_consts(consts, enclosing_params, enclosing_flow)
    }

    /// Variant of [`for_decl`] used by callers that already hold the consts
    /// table. Necessary on the `analyze` mutable-iteration path: the outer
    /// `for decl in &mut file.decls` mutably borrows `file.decls`, so we
    /// cannot re-call `for_decl` (which immutably borrows the file) inside
    /// the loop. Caller pre-computes consts via
    /// [`collect_consts_for_file`] before entering the mutable loop.
    pub fn for_decl_with_consts(
        consts: HashMap<&'a str, crate::kind_infer::TypeTag>,
        enclosing_params: &'a [crate::ast::Param],
        enclosing_flow: &'a [crate::ast::FlowStmt],
    ) -> Self {
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

        // Bindings: LHS of `name = call(...)` flow statements. The default
        // `for_decl` constructor pre-bakes once per decl with no live walk
        // state, so it cannot accumulate per-branch bindings — callers that
        // need flow-local awareness use `for_branch_with_consts`.
        let bindings: HashSet<&'a str> = HashSet::new();
        for _stmt in enclosing_flow {
            // The walk is preserved so a future binding-form variant lights up
            // automatically; today it inserts nothing.
        }

        Self {
            consts,
            params_with_string_default,
            bindings,
            flow_local_bindings: HashMap::new(),
        }
    }

    /// Per-branch constructor for skill-flow walks (spec `.flow-assign-spec.md`
    /// §6.3 / Codex Round 2 High 4). The caller has just walked some prefix of
    /// the enclosing skill flow, accumulated a live `FlowScope`, and is about
    /// to classify a branch condition. The `flow_local_bindings` snapshot here
    /// reflects the bindings visible at that branch site (outer bindings only;
    /// arm-local bindings have not yet been introduced — they live in the
    /// child scope).
    ///
    /// `for_branch_with_consts` reuses the per-decl `consts` /
    /// `params_with_string_default` snapshots; only the bindings differ
    /// per-branch.
    pub fn for_branch_with_consts(
        consts: HashMap<&'a str, crate::kind_infer::TypeTag>,
        params_with_string_default: HashSet<&'a str>,
        flow_local_bindings: HashMap<&'a str, ConditionFlowLocal>,
    ) -> Self {
        let bindings: HashSet<&'a str> = flow_local_bindings.keys().copied().collect();
        Self {
            consts,
            params_with_string_default,
            bindings,
            flow_local_bindings,
        }
    }
}

/// Classify a single tokenized term against the surrounding context.
/// Mirrors the dispatch in `analyze.rs::classify_token` (Task 6 rewires the
/// analyze caller to consult this version).
fn classify_token(tok: &str, ctx: &ConditionContext) -> ConditionTokenKind {
    if matches!(tok, "and" | "or" | "not" | "==" | "!=" | "(" | ")") {
        return ConditionTokenKind::Operator;
    }
    if tok.starts_with('"') {
        return ConditionTokenKind::PredicateLiteral;
    }
    if tok.contains(".applies()") {
        // Syntactic classification: any `NAME.applies()` form is PredicateApplies.
        // Semantic validation (receiver must be a known block) is done by
        // `check_applies_in_condition`, not here.
        return ConditionTokenKind::PredicateApplies;
    }
    // Numeric literal: integer or float token. `f64::from_str` accepts every
    // well-formed integer literal too.
    if tok.parse::<f64>().is_ok() {
        return ConditionTokenKind::Numeric;
    }
    if let Some(tag) = ctx.consts.get(tok) {
        return match tag {
            crate::kind_infer::TypeTag::String => ConditionTokenKind::PredicateConst,
            crate::kind_infer::TypeTag::Bool => ConditionTokenKind::Boolean,
            crate::kind_infer::TypeTag::Int | crate::kind_infer::TypeTag::Float => {
                ConditionTokenKind::Numeric
            }
            _ => ConditionTokenKind::Boolean,
        };
    }
    if ctx.params_with_string_default.contains(tok) {
        return ConditionTokenKind::PredicateConst;
    }
    if ctx.bindings.contains(tok) {
        return ConditionTokenKind::Boolean;
    }
    ConditionTokenKind::Boolean
}

/// Walk left from `end` (the immediate left neighbour of `==`). If `end` is
/// `)`, walk back to the matching `(`. Else return `end`. Unbalanced parens
/// fall back to the immediate neighbour; malformed conditions are caught by
/// other analyze rules.
pub(crate) fn match_paren_left(raw: &[String], end: usize) -> usize {
    if raw[end] != ")" {
        return end;
    }
    let mut depth = 1;
    let mut i = end;
    while i > 0 {
        i -= 1;
        match raw[i].as_str() {
            ")" => depth += 1,
            "(" => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    end
}

pub(crate) fn match_paren_right(raw: &[String], start: usize) -> usize {
    if raw[start] != "(" {
        return start;
    }
    let mut depth = 1;
    let mut i = start;
    while i + 1 < raw.len() {
        i += 1;
        match raw[i].as_str() {
            "(" => depth += 1,
            ")" => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    start
}

/// The single classifier. Replaces the three ad-hoc tokenize-and-classify
/// sites in analyze.rs, expand.rs, and emit/stub_fill.rs.
pub fn classify_condition(condition: &str, ctx: &ConditionContext) -> ConditionClassification {
    // Canonical trailing-`:` strip. Closes existing TODOs in expand.rs:187
    // and emit/branch.rs:23-25.
    let trimmed = condition.trim().trim_end_matches(':').trim();
    let raw = tokenize_condition(trimmed);

    let kinds: Vec<ConditionTokenKind> = raw.iter().map(|t| classify_token(t, ctx)).collect();

    let mut is_operand = vec![false; raw.len()];
    for (i, tok) in raw.iter().enumerate() {
        if tok == "==" || tok == "!=" {
            if i > 0 {
                let lhs_end = i - 1;
                let lhs_start = match_paren_left(&raw, lhs_end);
                for j in lhs_start..=lhs_end {
                    is_operand[j] = true;
                }
            }
            if i + 1 < raw.len() {
                let rhs_start = i + 1;
                let rhs_end = match_paren_right(&raw, rhs_start);
                for j in rhs_start..=rhs_end {
                    is_operand[j] = true;
                }
            }
        }
    }

    let mut tokens = Vec::with_capacity(raw.len());
    let mut summary = ConditionClassification::default();
    for (i, (text, kind)) in raw.into_iter().zip(kinds.iter().copied()).enumerate() {
        let operand = is_operand[i];
        if !operand {
            match kind {
                ConditionTokenKind::Boolean => summary.has_boolean_token = true,
                ConditionTokenKind::Numeric => summary.has_numeric_bare_condition = true,
                ConditionTokenKind::PredicateApplies
                | ConditionTokenKind::PredicateConst
                | ConditionTokenKind::PredicateLiteral => summary.has_predicate_token = true,
                ConditionTokenKind::Operator => {
                    if matches!(text.as_str(), "and" | "not") {
                        summary.has_compositional_operator = true;
                    } else if text == "==" || text == "!=" {
                        summary.has_boolean_token = true;
                        summary.has_comparison_operator = true;
                    }
                }
            }
        }
        tokens.push(ClassifiedConditionToken {
            text,
            kind,
            is_comparison_operand: operand,
        });
    }
    summary.tokens = tokens;
    summary
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
    fn pure_predicate_rejects_comparison_operator() {
        let c = ConditionClassification {
            tokens: vec![],
            has_boolean_token: false,
            has_predicate_token: true,
            has_compositional_operator: false,
            has_comparison_operator: true,
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

    use crate::ast::{ConstDecl, ConstValue, Decl, SourceFile};
    use crate::span::{Span, Spanned};

    fn const_decl(name: &str, body: &str) -> Decl {
        Decl::Const(Spanned::new(
            ConstDecl {
                name: name.to_string(),
                value: ConstValue::String(body.to_string()),
                exported: false,
                generated: false,
            },
            Span::new(0, 0, 0),
        ))
    }

    fn ctx_inputs(
        pairs: &[(&'static str, &'static str)],
    ) -> (
        SourceFile,
        BTreeMap<String, String>,
        BTreeMap<String, crate::kind_infer::TypeTag>,
    ) {
        let decls = pairs
            .iter()
            .map(|(name, body)| const_decl(name, body))
            .collect();
        let file = SourceFile { decls };
        (file, BTreeMap::new(), BTreeMap::new())
    }

    #[test]
    fn classify_pure_predicates_pass() {
        let (file, imports, types) = ctx_inputs(&[("complex_change", "the change is complex")]);
        let ctx = ConditionContext::for_decl(&file, &imports, &types, &[], &[]);

        let cases = [
            "a.applies()",
            "complex_change",
            "\"the user opted in\"",
            "a.applies() or b.applies()",
            "a.applies() or \"literal\"",
        ];
        for cond in cases {
            let c = classify_condition(cond, &ctx);
            assert!(c.is_pure_predicate(), "pure predicate failed for: {cond}");
        }
    }

    #[test]
    fn classify_or_does_not_disqualify_pure() {
        let (file, imports, types) = ctx_inputs(&[("big", "is big"), ("small", "is small")]);
        let ctx = ConditionContext::for_decl(&file, &imports, &types, &[], &[]);
        assert!(classify_condition("big or small", &ctx).is_pure_predicate());
    }

    #[test]
    fn classify_and_disqualifies_pure() {
        let (file, imports, types) = ctx_inputs(&[("big", "is big"), ("small", "is small")]);
        let ctx = ConditionContext::for_decl(&file, &imports, &types, &[], &[]);
        assert!(!classify_condition("big and small", &ctx).is_pure_predicate());
    }

    #[test]
    fn classify_eq_disqualifies_pure_and_marks_operands() {
        let (file, imports, types) = ctx_inputs(&[]);
        let ctx = ConditionContext::for_decl(&file, &imports, &types, &[], &[]);
        let c = classify_condition("risk == \"high\"", &ctx);
        assert!(!c.is_pure_predicate());
        assert!(c.has_comparison_operator);

        let texts: Vec<(&str, bool)> = c
            .tokens
            .iter()
            .map(|t| (t.text.as_str(), t.is_comparison_operand))
            .collect();
        assert!(texts.contains(&("risk", true)));
        assert!(texts.contains(&("\"high\"", true)));
    }

    #[test]
    fn classify_eq_with_paren_operand_marks_full_group() {
        let (file, imports, types) = ctx_inputs(&[]);
        let ctx = ConditionContext::for_decl(&file, &imports, &types, &[], &[]);
        let c = classify_condition("risk == (\"high\")", &ctx);
        for t in &c.tokens {
            if matches!(t.text.as_str(), "(" | "\"high\"" | ")") {
                assert!(t.is_comparison_operand, "{} should be an operand", t.text);
            }
        }
        assert!(!c.has_predicate_token);
    }

    #[test]
    fn classify_eq_with_paren_on_left_marks_full_group() {
        let (file, imports, types) = ctx_inputs(&[]);
        let ctx = ConditionContext::for_decl(&file, &imports, &types, &[], &[]);
        let c = classify_condition("(\"high\") == risk", &ctx);
        for t in &c.tokens {
            if matches!(t.text.as_str(), "(" | "\"high\"" | ")") {
                assert!(t.is_comparison_operand, "{} should be an operand", t.text);
            }
        }
        assert!(!c.has_predicate_token);
    }

    #[test]
    fn classify_eq_unbalanced_falls_back_to_immediate_neighbour() {
        let (file, imports, types) = ctx_inputs(&[]);
        let ctx = ConditionContext::for_decl(&file, &imports, &types, &[], &[]);
        let c = classify_condition("risk == ) malformed", &ctx);
        let close_paren = c.tokens.iter().find(|t| t.text == ")").unwrap();
        assert!(close_paren.is_comparison_operand);
    }

    #[test]
    fn classify_eq_with_nested_parens_marks_full_group() {
        let (file, imports, types) = ctx_inputs(&[]);
        let ctx = ConditionContext::for_decl(&file, &imports, &types, &[], &[]);
        let c = classify_condition("risk == ((\"high\"))", &ctx);
        // All parens AND the inner string must be operands.
        for t in &c.tokens {
            if matches!(t.text.as_str(), "(" | ")" | "\"high\"") {
                assert!(t.is_comparison_operand, "{} should be an operand", t.text);
            }
        }
        // Inner string is an operand, so it must NOT count toward has_predicate_token.
        assert!(!c.has_predicate_token);
    }

    #[test]
    fn classify_multiple_eq_marks_all_operands() {
        let (file, imports, types) = ctx_inputs(&[]);
        let ctx = ConditionContext::for_decl(&file, &imports, &types, &[], &[]);
        let c = classify_condition("a == 1 and b == 2", &ctx);

        // Both `==` are present; both pairs of operands must be marked.
        let operand_texts: Vec<&str> = c
            .tokens
            .iter()
            .filter(|t| t.is_comparison_operand)
            .map(|t| t.text.as_str())
            .collect();
        assert!(
            operand_texts.contains(&"a"),
            "a should be operand: {:?}",
            operand_texts
        );
        assert!(
            operand_texts.contains(&"1"),
            "1 should be operand: {:?}",
            operand_texts
        );
        assert!(
            operand_texts.contains(&"b"),
            "b should be operand: {:?}",
            operand_texts
        );
        assert!(
            operand_texts.contains(&"2"),
            "2 should be operand: {:?}",
            operand_texts
        );

        // The `and` is a compositional operator → not pure.
        assert!(!c.is_pure_predicate());
        assert!(c.has_compositional_operator);
        assert!(c.has_comparison_operator);
    }

    #[test]
    fn classify_param_with_string_default_is_predicate_const() {
        let (file, imports, types) = ctx_inputs(&[]);
        let mut ctx = ConditionContext::for_decl(&file, &imports, &types, &[], &[]);
        ctx.params_with_string_default.insert("risk");
        let c = classify_condition("risk", &ctx);
        assert_eq!(c.tokens.len(), 1);
        assert_eq!(c.tokens[0].kind, ConditionTokenKind::PredicateConst);
        assert!(c.is_pure_predicate());
    }

    #[test]
    fn classify_numeric_bare_condition_flag() {
        let (file, imports, types) = ctx_inputs(&[]);
        let ctx = ConditionContext::for_decl(&file, &imports, &types, &[], &[]);
        assert!(classify_condition("5", &ctx).has_numeric_bare_condition);
        assert!(!classify_condition("max_attempts == 3", &ctx).has_numeric_bare_condition);
        assert!(classify_condition("x == 3 and 5", &ctx).has_numeric_bare_condition);
    }

    #[test]
    fn classify_string_const_is_predicate_const() {
        let (file, imports, types) = ctx_inputs(&[("complex", "is complex")]);
        let ctx = ConditionContext::for_decl(&file, &imports, &types, &[], &[]);
        let c = classify_condition("complex", &ctx);
        assert_eq!(c.tokens.len(), 1);
        assert_eq!(c.tokens[0].kind, ConditionTokenKind::PredicateConst);
        assert!(c.is_pure_predicate());
    }

    #[test]
    fn classify_imported_string_const_is_predicate_const() {
        let mut imports: BTreeMap<String, String> = BTreeMap::new();
        imports.insert(
            "imported_big".to_string(),
            "imported description".to_string(),
        );
        let mut types: BTreeMap<String, crate::kind_infer::TypeTag> = BTreeMap::new();
        types.insert(
            "imported_big".to_string(),
            crate::kind_infer::TypeTag::String,
        );
        let file = SourceFile { decls: vec![] };
        let ctx = ConditionContext::for_decl(&file, &imports, &types, &[], &[]);
        let c = classify_condition("imported_big", &ctx);
        assert_eq!(c.tokens[0].kind, ConditionTokenKind::PredicateConst);
    }

    /// Task 6 — `for_decl_with_consts` and `for_decl` must produce equivalent
    /// classification results when fed the same inputs. The two constructors
    /// are semantic peers; only the borrow-shape differs.
    #[test]
    fn for_decl_and_for_decl_with_consts_produce_equivalent_context() {
        let (file, mut imports, mut types) =
            ctx_inputs(&[("complex_change", "the change is complex")]);
        imports.insert(
            "imported_big".to_string(),
            "imported description".to_string(),
        );
        types.insert(
            "imported_big".to_string(),
            crate::kind_infer::TypeTag::String,
        );

        let consts = collect_consts_for_file(&file, &imports, &types);
        let ctx_a = ConditionContext::for_decl(&file, &imports, &types, &[], &[]);
        let ctx_b = ConditionContext::for_decl_with_consts(consts, &[], &[]);

        for cond in [
            "complex_change",
            "imported_big",
            "complex_change or imported_big",
        ] {
            let a = classify_condition(cond, &ctx_a);
            let b = classify_condition(cond, &ctx_b);
            let kinds_a: Vec<_> = a.tokens.iter().map(|t| t.kind).collect();
            let kinds_b: Vec<_> = b.tokens.iter().map(|t| t.kind).collect();
            assert_eq!(kinds_a, kinds_b, "kinds differ for `{cond}`");
            assert_eq!(
                a.has_predicate_token, b.has_predicate_token,
                "predicate flag differs for `{cond}`"
            );
            assert_eq!(
                a.has_boolean_token, b.has_boolean_token,
                "boolean flag differs for `{cond}`"
            );
        }
    }

    // ─── B15: condition tokenizer handles backslash escapes in strings ───
    //
    // Escape policy mirrors `tokenize.rs` (`scan_triple_string` and the
    // inline `"..."` scanner): `\"` and `\\` (and `\n`, `\t`) are
    // recognized; unknown escapes (`\X`) and a trailing `\` at end of
    // input are preserved as literal source bytes. `tokenize_condition`
    // keeps the surface-form token (the verbatim source slice, quotes
    // and escapes included), so the produced token equals the source
    // string and therefore covers the full source range (span
    // preservation by construction).

    #[test]
    fn tokenize_escaped_quote_is_single_literal() {
        // Source: `"a\"b"` — 6 chars: `"`, `a`, `\`, `"`, `b`, `"`.
        // Pre-fix this split at the `\"` and produced two tokens.
        let src = r#""a\"b""#;
        let toks = tokenize_condition(src);
        assert_eq!(toks.len(), 1, "expected single token, got {:?}", toks);
        assert_eq!(
            toks[0], src,
            "token text must preserve the verbatim source slice"
        );
        // Span preservation: token text length equals source byte length,
        // so the token's source range is `[0, src.len())`.
        assert_eq!(toks[0].len(), src.len());
    }

    #[test]
    fn tokenize_escaped_backslash_is_single_literal() {
        // Source: `"a\\b"` — escaped backslash inside the literal.
        let src = r#""a\\b""#;
        let toks = tokenize_condition(src);
        assert_eq!(toks.len(), 1, "expected single token, got {:?}", toks);
        assert_eq!(toks[0], src);
        assert_eq!(toks[0].len(), src.len());
    }

    #[test]
    fn tokenize_escaped_quote_does_not_bleed_into_following_operator() {
        // Adversarial: `\"` inside a literal followed by `==` and another
        // token. The literal must close at the *real* closing quote, not
        // at the escaped one — otherwise the `==` is swallowed.
        let src = r#""a\"b" == risk"#;
        assert_eq!(tokenize_condition(src), vec![r#""a\"b""#, "==", "risk"]);
    }

    #[test]
    fn tokenize_plain_string_unchanged_regression() {
        // Regression: a plain (no-escape) string still tokenizes as one
        // token whose text equals the source slice.
        let src = r#""hello world""#;
        assert_eq!(tokenize_condition(src), vec![src]);
    }

    #[test]
    fn tokenize_trailing_backslash_at_end_of_input_preserved() {
        // `tokenize_condition` returns `Vec<String>` and has no error
        // path. A trailing `\` with no following char (and no closing
        // `"`) is preserved as a literal `\` — the resulting token is
        // the verbatim source (`"a\`), matching the canonical
        // tokenize.rs guard (`p + 1 < content_end` falls through to the
        // literal-byte branch when `\` is the last byte).
        let src = "\"a\\";
        let toks = tokenize_condition(src);
        assert_eq!(toks.len(), 1, "expected single token, got {:?}", toks);
        assert_eq!(toks[0], src);
    }
}
