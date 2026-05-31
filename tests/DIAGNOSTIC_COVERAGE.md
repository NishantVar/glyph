# Diagnostic Coverage Inventory

Maps every compiler-scope diagnostic ID to its triggering test.

Compiler-scope means the Parse, Analyze, Imports, Validate, Build, and
Validate-output (structural) phases. Repair-pass notifications and failures
(`G::repair::*`) and the agent-scope Expand failure `G::expand::llm-unavailable`
are part of the diagnostic contract in [`../docs/reference/diagnostics.md`](../docs/reference/diagnostics.md)
but are out of scope for this compiler-coverage inventory.

Test locations are `cargo nextest` paths (`<binary>::<module-path>::<test>`).
A row whose Test Location is `—` is a **coverage gap**: the ID is emitted by the
compiler but no test triggers it. Gaps are listed again under §Summary.

## Parse diagnostics (34)

| Diagnostic ID | Test Location |
|---|---|
| `G::parse::tab-indent` | glyph-core::tests::check_source_flags_tab_indent_as_repairable |
| `G::parse::mixed-indent` | glyph-core::tests::parse_mixed_indent_diagnostic |
| `G::parse::nested-flow` | glyph-core::tests::parse_nested_flow_diagnostic |
| `G::parse::gated-section` | glyph-cli::diagnostics_invalid::gated_section_effects_emits_gated_section |
| `G::parse::none-with-effects` | glyph-core::tests::effects_none_with_other_effects_rejected |
| `G::parse::multiple-with` | glyph-core::tests::multiple_with_fires_diagnostic |
| `G::parse::with-on-bare-name` | glyph-core::tests::with_on_bare_name_fires_diagnostic |
| `G::parse::operator-in-expression` | glyph-core::tests::parse_operator_in_expression_diagnostic |
| `G::parse::param-slot-in-non-instruction-string` | glyph-cli::parameters::slot_in_description_emits_repairable_parse_diagnostic |
| `G::parse::return-not-terminal` | glyph-core::tests::return_not_terminal_fires_diagnostic |
| `G::parse::return-in-branch` | glyph-core::tests::return_in_branch_fires_diagnostic |
| `G::parse::multiple-returns` | glyph-core::tests::multiple_returns_fires_diagnostic |
| `G::parse::duplicate-subsection` | glyph-core::tests::parse_duplicate_subsection_diagnostic |
| `G::parse::empty-file` | glyph-core::tests::check_source_returns_empty_bag_on_empty_file_repairs_skipped |
| `G::parse::empty-flow` | glyph-cli::diagnostics_invalid::empty_flow_exits_one_with_empty_flow_diagnostic |
| `G::parse::multiple-skills` | glyph-core::tests::parse_multiple_skills_diagnostic |
| `G::parse::applies-no-parens` | glyph-core::tests::applies_no_parens_fires_diagnostic |
| `G::parse::applies-with-args` | glyph-core::tests::applies_with_args_fires_diagnostic |
| `G::parse::none-as-return-type` | glyph-core::parse::none_return_tests::parse_rejects_arrow_lowercase_none |
| `G::parse::malformed-output-target` | glyph-core::parse::output_target_return_tests::malformed_output_target_surfaces_structured_diagnostic |
| `G::parse::output-target-outside-return` | glyph-core::parse::output_target_return_tests::output_target_outside_terminal_return_surfaces_structured_diagnostic |
| `G::parse::bad-indent` | — |
| `G::parse::unterminated-string` | glyph-cli::integration_issue_86::descriptive_form_unterminated_emits_unterminated_string |
| `G::parse::unexpected-char` | glyph-cli::integration_issue_86::unexpected_char_in_flow_emits_diagnostic |
| `G::parse::generated-decl-out-of-order` | glyph-cli::generated_decl_enforcement::generated_decl_followed_by_non_generated_decl_fires_out_of_order |
| `G::parse::generated-block-body-shape` | glyph-cli::generated_decl_enforcement::generated_block_with_multi_statement_flow_body_fires_body_shape |
| `G::parse::leading-zero-numeric` | — |
| `G::parse::marker-missing-operand` | glyph-cli::diagnostics_invalid::freeform_marker_missing_operand_emits_marker_missing_operand |
| `G::parse::flow-statement-in-freeform` | glyph-cli::diagnostics_invalid::freeform_flow_statement_emits_flow_statement_in_freeform |
| `G::parse::effect-keyword-outside-effects-section` | — |
| `G::parse::unknown-marker-word` | glyph-cli::diagnostics_invalid::freeform_unknown_marker_emits_unknown_marker_word |
| `G::parse::applies-outside-condition` | glyph-core::tests::applies_outside_branch_condition_is_parse_error |
| `G::parse::assign-rhs-not-call` | glyph-core::parse::flow_assign_tests::flow_assign_rhs_not_call_recovers_to_barename |
| `G::parse::unexpected` | glyph-cli::diagnostics_invalid::case_insensitive_constraints_emits_marker_required_error |

## Analyze diagnostics (46)

| Diagnostic ID | Test Location |
|---|---|
| `G::analyze::undefined-name` | glyph-cli::constraints_context::undefined_constraint_name_fires_undefined_name_diagnostic |
| `G::analyze::undefined-call` | glyph-core::tests::undefined_call_fires_diagnostic |
| `G::analyze::name-collision` | glyph-core::tests::name_collision_fires_for_duplicate_export_names |
| `G::analyze::type-case-violation` | glyph-cli::diagnostics_invalid::type_snake_case_emits_type_case_violation |
| `G::analyze::value-case-violation` | glyph-cli::diagnostics_invalid::const_pascal_case_emits_value_case_violation |
| `G::analyze::inconsistent-type-spelling` | glyph-cli::diagnostics_invalid::inconsistent_implicit_type_emits_warning |
| `G::analyze::import-private` | glyph-core::tests::import_private_name_fails |
| `G::analyze::import-skill` | glyph-core::tests::import_skill_fails |
| `G::analyze::circular-import` | glyph-core::tests::circular_import_detected_with_path |
| `G::analyze::missing-file` | glyph-core::tests::missing_import_file_detected |
| `G::analyze::duplicate-import` | glyph-core::tests::duplicate_import_is_repairable |
| `G::analyze::unused-import` | glyph-core::tests::unused_import_is_repairable |
| `G::analyze::ambiguous-role` | glyph-cli::constraints_context::bare_text_name_at_body_level_fires_ambiguous_role |
| `G::analyze::effects-under-declared` | glyph-core::tests::effects_under_declared_produces_error |
| `G::analyze::effects-over-declared` | glyph-core::tests::effects_over_declared_produces_warning_exit_zero |
| `G::analyze::missing-effects` | glyph-core::tests::effects_missing_declaration_is_repairable |
| `G::analyze::nominal-mismatch` | glyph-core::analyze::tests::nominal_mismatch_fires |
| `G::analyze::generic-type-name` | glyph-core::tests::return_type_string_on_skill_fires_warning |
| `G::analyze::lossy-coercion` | glyph-core::analyze::tests::lossy_coercion_fires |
| `G::analyze::missing-return` | glyph-core::tests::export_block_requires_return |
| `G::analyze::export-missing-return-type` | glyph-core::tests::export_block_meaningful_return_without_arrow_fires |
| `G::analyze::typed-decl-missing-return` | glyph-cli::typed_decl_missing_return::typed_skill_flow_no_return_fires |
| `G::analyze::return-of-no-value-call` | glyph-core::analyze::flow_assign_tests::return_of_no_value_call_fires_for_void_local_callee |
| `G::analyze::output-target-shadows-binding` | glyph-core::analyze::tests::output_target_name_must_not_shadow_visible_binding |
| `G::analyze::placeholder-string-return` | glyph-core::analyze::tests::placeholder_string_return_is_repairable_on_domain_typed_skill |
| `G::analyze::closure-violation` | glyph-core::tests::ac3_closure_violation_on_private_free_variable |
| `G::analyze::stdlib-missing-import` | glyph-core::tests::stdlib_missing_import_fires_for_subagent |
| `G::analyze::unknown-param-slot` | glyph-cli::parameters::unknown_param_slot_emits_analyze_diagnostic |
| `G::analyze::nested-branch` | glyph-core::tests::nested_branch_fires_diagnostic |
| `G::analyze::empty-skill-body` | glyph-core::tests::analyze_empty_skill_body_diagnostic |
| `G::analyze::no-exports-in-library` | glyph-core::tests::ac4_library_with_zero_exports_fires_no_exports_in_library |
| `G::analyze::missing-required-arg` | glyph-cli::parameters::missing_required_arg_at_call_site_emits_analyze_diagnostic |
| `G::analyze::missing-description` | glyph-cli::constraints_context::missing_description_fires_repairable_diagnostic |
| `G::analyze::text-in-flow` | glyph-cli::constraints_context::bare_name_in_flow_fires_text_in_flow_diagnostic |
| `G::analyze::applies-on-non-block` | glyph-core::tests::applies_on_non_block_fires_error |
| `G::analyze::applies-on-undescribed-block` | glyph-core::tests::applies_on_undescribed_block_fires_repairable |
| `G::analyze::condition-non-boolean-non-predicate` | glyph-core::analyze::tests::int_const_in_condition_position_fires_non_boolean_non_predicate |
| `G::analyze::unmerged-duplicate-subsection` | glyph-core::analyze::unmerged_duplicate_subsection_tests::skill_with_unmerged_extras_emits_error_diagnostic |
| `G::analyze::duplicate-type-decl` | glyph-core::analyze::tests::duplicate_type_decl_emits_diagnostic |
| `G::analyze::duplicate-section` | glyph-core::analyze::tests::duplicate_freeform_section_fires_diag |
| `G::analyze::cardinality-violation` | glyph-core::analyze::tests::empty_goal_section_fires_cardinality_violation |
| `G::analyze::flow-assign-in-block-unsupported` | glyph-core::analyze::flow_assign_tests::flow_assign_in_block_diag |
| `G::analyze::redeclared-flow-binding` | glyph-core::analyze::flow_assign_tests::flow_assign_redecl_param_emits_diag |
| `G::analyze::assignment-rhs-has-no-value` | glyph-core::analyze::flow_assign_tests::flow_assign_no_value_diag |
| `G::analyze::use-before-bind` | glyph-core::analyze::flow_assign_tests::flow_assign_use_before_bind_specialized |
| `G::analyze::call-arg-type-mismatch` | glyph-core::analyze::flow_assign_tests::flow_assign_call_arg_type_mismatch_emits_diag |

## Imports diagnostics (1)

| Diagnostic ID | Test Location |
|---|---|
| `G::imports::unknown-stdlib-module` | glyph-core::tests::stdlib_unknown_module_fires |

## Validate diagnostics (6)

| Diagnostic ID | Test Location |
|---|---|
| `G::validate::duplicate-node-id` | glyph-core::validate::tests::validate_duplicate_node_id |
| `G::validate::unresolved-callee` | glyph-core::validate::tests::validate_unresolved_callee |
| `G::validate::malformed-branch` | glyph-core::validate::tests::validate_malformed_branch_empty_then_body |
| `G::validate::recursive-call` | glyph-core::validate::tests::validate_recursive_call |
| `G::validate::empty-step` | glyph-core::validate::tests::validate_empty_step |
| `G::validate::no-root-skill` | — |

## Build diagnostics (3)

`G::build::compile-error` is an internal generic fallback (`compile pipeline
failed: …`), not a stable public-contract ID. It is intentionally absent from
`docs/reference/diagnostics.md` and is listed here only for completeness.

| Diagnostic ID | Test Location |
|---|---|
| `G::build::skipped-due-to-failed-import` | glyph-core::tests::ac3_failure_skips_dependent_with_warning |
| `G::build::import-outside-out-dir` | glyph-cli::output_flag::out_dir_outside_root_warning_in_json |
| `G::build::compile-error` | — |

## Validate-output / Expand diagnostics (29)

`G::expand::missing-instructions` and `G::expand::extra-h3` are **retired** — no
longer emitted, kept as reserved IDs for forward-compatibility. Their tests
assert non-emission.

| Diagnostic ID | Test Location |
|---|---|
| `G::expand::extra-h2` | glyph-core::validate_output::tests::extra_h2 |
| `G::expand::missing-instructions` *(retired)* | glyph-core::validate_output::tests::missing_instructions_not_emitted_on_flat_shape |
| `G::expand::extra-h3` *(retired)* | glyph-core::validate_output::tests::extra_h3_accepts_valid_h3s |
| `G::expand::step-count-mismatch` | glyph-core::validate_output::tests::step_count_mismatch |
| `G::expand::substep-count-mismatch` | glyph-core::validate_output::tests::substep_count_mismatch |
| `G::expand::constraint-count-mismatch` | glyph-core::validate_output::tests::constraint_count_mismatch |
| `G::expand::context-count-mismatch` | glyph-core::validate_output::tests::context_count_mismatch |
| `G::expand::step-order-mismatch` | glyph-core::validate_output::tests::step_order_mismatch |
| `G::expand::invented-param-ref` | glyph-core::validate_output::tests::invented_param_ref |
| `G::expand::dropped-param-ref` | glyph-core::validate_output::tests::dropped_param_ref |
| `G::expand::unresolved-local-ref` | glyph-core::validate_output::tests::unresolved_local_ref |
| `G::expand::output-target-leak` | glyph-core::validate_output::tests::output_target_leak_is_rejected |
| `G::expand::modifier-leaked` | glyph-core::validate_output::tests::modifier_leaked |
| `G::expand::llm-required-for-call` | glyph-core::tests::with_modifier_not_applied_in_compiled_output |
| `G::expand::llm-required-for-param-description` | glyph-cli::emit_ir::param_description_undescribed_hard_fails_under_stub_filler |
| `G::expand::params-section-mismatch` | glyph-core::validate_output::tests::params_section_mismatch |
| `G::expand::params-section-missing` | glyph-core::validate_output::tests::params_section_missing |
| `G::expand::params-section-spurious` | glyph-core::validate_output::tests::params_section_spurious |
| `G::expand::frontmatter-returned` | glyph-core::validate_output::tests::frontmatter_returned |
| `G::expand::malformed-markdown` | glyph-core::validate_output::tests::malformed_markdown |
| `G::expand::procedure-count-mismatch` | glyph-core::validate_output::tests::procedure_count_mismatch |
| `G::expand::procedure-name-mismatch` | glyph-core::validate_output::tests::procedure_name_mismatch |
| `G::expand::procedure-step-count-mismatch` | glyph-core::validate_output::tests::procedure_step_count_mismatch |
| `G::expand::procedure-ref-missing` | glyph-core::validate_output::tests::procedure_ref_missing |
| `G::expand::procedure-ref-dangling` | glyph-core::validate_output::tests::procedure_ref_dangling |
| `G::expand::procedure-duplicate` | glyph-core::validate_output::tests::procedure_duplicate |
| `G::expand::procedure-order` | glyph-core::validate_output::tests::procedure_order |
| `G::expand::description-shape-missing` | glyph-core::validate_output::tests::description_driven_branch_rejects_raw_applies_condition |
| `G::expand::predicate-prose-missing` | glyph-core::validate_output::tests::check_resolved_predicates_rejects_const_form_with_missing_prose |

## Summary

- **Total compiler-scope diagnostic IDs:** 119
- **IDs with a triggering test:** 114
- **IDs with no triggering test (coverage gaps):** 5
- **Coverage:** 114 / 119 ≈ 95.8%

### Coverage gaps

These IDs are emitted by the compiler but have no test that triggers them.
They are pre-existing gaps, not regressions; closing them is tracked separately.

- `G::parse::bad-indent` — no test asserts this ID.
- `G::parse::leading-zero-numeric` — no test asserts this ID. Leading-zero
  rejection is exercised at the tokenizer level
  (`glyph-core::tokenize::tests::tokenize_rejects_leading_zero_integer`), but
  no test covers the `G::parse::leading-zero-numeric` diagnostic itself.
- `G::parse::effect-keyword-outside-effects-section` — no test asserts this ID.
- `G::validate::no-root-skill` — no test asserts this ID.
- `G::build::compile-error` — no test asserts this ID. This is the internal
  generic fallback diagnostic, not a stable public-contract ID.
