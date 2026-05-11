# Diagnostic Coverage Inventory

Single source of truth mapping every compiler-scope diagnostic ID to at least one triggering test.

## Parse diagnostics (19)

| Diagnostic ID | Test Location |
|---|---|
| `G::parse::tab-indent` | glyph-core::tests::check_source_flags_tab_indent_as_repairable |
| `G::parse::mixed-indent` | glyph-core::tests::parse_mixed_indent_diagnostic |
| `G::parse::nested-flow` | glyph-core::tests::parse_nested_flow_diagnostic |
| `G::parse::none-with-effects` | glyph-core::tests::effects_none_with_other_effects_rejected |
| `G::parse::multiple-with` | glyph-core::tests::multiple_with_fires_diagnostic |
| `G::parse::with-on-bare-name` | glyph-core::tests::with_on_bare_name_fires_diagnostic |
| `G::parse::operator-in-expression` | glyph-core::tests::parse_operator_in_expression_diagnostic |
| `G::parse::param-slot-in-non-instruction-string` | glyph-core::tests::slot_in_description_emits_repairable_parse_diagnostic |
| `G::parse::return-not-terminal` | glyph-core::tests::return_not_terminal_fires_diagnostic |
| `G::parse::return-in-branch` | glyph-core::tests::return_in_branch_fires_diagnostic |
| `G::parse::multiple-returns` | glyph-core::tests::multiple_returns_fires_diagnostic |
| `G::parse::duplicate-subsection` | glyph-core::tests::parse_duplicate_subsection_diagnostic |
| `G::parse::empty-file` | glyph-core::tests::check_source_returns_empty_bag_on_empty_file_repairs_skipped |
| `G::parse::empty-flow` | glyph-cli::diagnostics_invalid::empty_flow_exits_one_with_empty_flow_diagnostic |
| `G::parse::multiple-skills` | glyph-core::tests::parse_multiple_skills_diagnostic |
| `G::parse::applies-no-parens` | glyph-core::tests::applies_no_parens_fires_diagnostic |
| `G::parse::applies-with-args` | glyph-core::tests::applies_with_args_fires_diagnostic |
| `G::parse::malformed-output-target` | glyph-core::parse::output_target_return_tests::malformed_output_target_surfaces_structured_diagnostic |
| `G::parse::output-target-outside-return` | glyph-core::parse::output_target_return_tests::output_target_outside_terminal_return_surfaces_structured_diagnostic |

## Analyze diagnostics (30)

| Diagnostic ID | Test Location |
|---|---|
| `G::analyze::undefined-name` | glyph-cli::diagnostics_invalid::undefined_constraint_name_fires_undefined_name_diagnostic |
| `G::analyze::undefined-call` | glyph-core::tests::undefined_call_fires_diagnostic |
| `G::analyze::name-collision` | glyph-core::tests::name_collision_fires_for_duplicate_export_names |
| `G::analyze::import-private` | glyph-core::tests::import_private_name_fails |
| `G::analyze::import-skill` | glyph-core::tests::import_skill_fails |
| `G::analyze::circular-import` | glyph-core::tests::circular_import_detected_with_path |
| `G::analyze::missing-file` | glyph-core::tests::missing_import_file_detected |
| `G::analyze::duplicate-import` | glyph-core::tests::duplicate_import_is_repairable |
| `G::analyze::unused-import` | glyph-core::tests::unused_import_is_repairable |
| `G::analyze::ambiguous-role` | glyph-core::tests::bare_text_name_at_body_level_fires_ambiguous_role |
| `G::analyze::effects-under-declared` | glyph-core::tests::effects_under_declared_produces_error |
| `G::analyze::effects-over-declared` | glyph-core::tests::effects_over_declared_produces_warning_exit_zero |
| `G::analyze::missing-effects` | glyph-core::tests::effects_missing_declaration_is_repairable |
| `G::analyze::nominal-mismatch` | glyph-core::analyze::tests::nominal_mismatch_fires |
| `G::analyze::lossy-coercion` | glyph-core::analyze::tests::lossy_coercion_fires |
| `G::analyze::missing-return` | glyph-core::tests::export_block_requires_return |
| `G::analyze::typed-decl-missing-return` | crates/glyph-cli/tests/typed_decl_missing_return.rs |
| `G::analyze::output-target-shadows-binding` | glyph-core::analyze::tests::output_target_name_must_not_shadow_visible_binding |
| `G::analyze::placeholder-string-return` | glyph-core::analyze::tests::placeholder_string_return_is_repairable_on_domain_typed_skill |
| `G::analyze::closure-violation` | glyph-core::tests::ac3_closure_violation_on_private_free_variable |
| `G::analyze::stdlib-missing-import` | glyph-core::tests::stdlib_missing_import_fires_for_subagent |
| `G::analyze::unknown-param-slot` | glyph-cli::diagnostics_invalid::unknown_param_slot_emits_analyze_diagnostic |
| `G::analyze::nested-branch` | glyph-core::tests::nested_branch_fires_diagnostic |
| `G::analyze::empty-skill-body` | glyph-core::tests::analyze_empty_skill_body_diagnostic |
| `G::analyze::no-exports-in-library` | glyph-core::tests::ac4_library_with_zero_exports_fires_no_exports_in_library |
| `G::analyze::missing-required-arg` | glyph-cli::parameters::missing_required_arg_at_call_site_emits_analyze_diagnostic / export_block_missing_required_arg_at_call_site_emits_analyze_diagnostic / glyph-cli::imports::imported_export_block_missing_required_arg_exit_1 |
| `G::analyze::missing-description` | glyph-core::tests::missing_description_fires_repairable_diagnostic |
| `G::analyze::text-in-flow` | glyph-core::tests::bare_name_in_flow_fires_text_in_flow_diagnostic |
| `G::analyze::applies-on-non-block` | glyph-core::tests::applies_on_non_block_fires_error |
| `G::analyze::applies-on-undescribed-block` | glyph-core::tests::applies_on_undescribed_block_fires_repairable |

## Imports diagnostics (1)

| Diagnostic ID | Test Location |
|---|---|
| `G::imports::unknown-stdlib-module` | glyph-core::tests::stdlib_unknown_module_fires |

## Validate diagnostics (5)

| Diagnostic ID | Test Location |
|---|---|
| `G::validate::duplicate-node-id` | glyph-core::validate::tests::validate_duplicate_node_id |
| `G::validate::unresolved-callee` | glyph-core::validate::tests::validate_unresolved_callee |
| `G::validate::malformed-branch` | glyph-core::validate::tests::validate_malformed_branch_empty_then_body |
| `G::validate::recursive-call` | glyph-core::validate::tests::validate_recursive_call |
| `G::validate::empty-step` | glyph-core::validate::tests::validate_empty_step |

## Build diagnostics (1)

| Diagnostic ID | Test Location |
|---|---|
| `G::build::skipped-due-to-failed-import` | glyph-core::tests::ac3_failure_skips_dependent_with_warning |

## Validate-output / Expand diagnostics (27)

| Diagnostic ID | Test Location |
|---|---|
| `G::expand::extra-h2` | glyph-core::validate_output::tests::extra_h2 |
| `G::expand::missing-instructions` | glyph-core::validate_output::tests::missing_instructions |
| `G::expand::extra-h3` | glyph-core::validate_output::tests::extra_h3 |
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

## Summary

- **Total diagnostic IDs:** 83
- **Total with triggering tests:** 83
- **Coverage:** 100%
