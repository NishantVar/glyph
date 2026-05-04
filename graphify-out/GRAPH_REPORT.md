# Graph Report - phase-3a-integration  (2026-05-04)

## Corpus Check
- 51 files · ~254,360 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 1639 nodes · 4538 edges · 44 communities detected
- Extraction: 67% EXTRACTED · 33% INFERRED · 0% AMBIGUOUS · INFERRED: 1518 edges (avg confidence: 0.8)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- [[_COMMUNITY_Community 0|Community 0]]
- [[_COMMUNITY_Community 1|Community 1]]
- [[_COMMUNITY_Community 2|Community 2]]
- [[_COMMUNITY_Community 3|Community 3]]
- [[_COMMUNITY_Community 4|Community 4]]
- [[_COMMUNITY_Community 5|Community 5]]
- [[_COMMUNITY_Community 6|Community 6]]
- [[_COMMUNITY_Community 7|Community 7]]
- [[_COMMUNITY_Community 8|Community 8]]
- [[_COMMUNITY_Community 9|Community 9]]
- [[_COMMUNITY_Community 10|Community 10]]
- [[_COMMUNITY_Community 11|Community 11]]
- [[_COMMUNITY_Community 12|Community 12]]
- [[_COMMUNITY_Community 13|Community 13]]
- [[_COMMUNITY_Community 14|Community 14]]
- [[_COMMUNITY_Community 15|Community 15]]
- [[_COMMUNITY_Community 16|Community 16]]
- [[_COMMUNITY_Community 17|Community 17]]
- [[_COMMUNITY_Community 18|Community 18]]
- [[_COMMUNITY_Community 19|Community 19]]
- [[_COMMUNITY_Community 20|Community 20]]
- [[_COMMUNITY_Community 21|Community 21]]
- [[_COMMUNITY_Community 22|Community 22]]
- [[_COMMUNITY_Community 23|Community 23]]
- [[_COMMUNITY_Community 24|Community 24]]
- [[_COMMUNITY_Community 25|Community 25]]
- [[_COMMUNITY_Community 26|Community 26]]
- [[_COMMUNITY_Community 27|Community 27]]
- [[_COMMUNITY_Community 28|Community 28]]
- [[_COMMUNITY_Community 29|Community 29]]
- [[_COMMUNITY_Community 30|Community 30]]
- [[_COMMUNITY_Community 31|Community 31]]
- [[_COMMUNITY_Community 32|Community 32]]
- [[_COMMUNITY_Community 33|Community 33]]
- [[_COMMUNITY_Community 35|Community 35]]
- [[_COMMUNITY_Community 36|Community 36]]
- [[_COMMUNITY_Community 37|Community 37]]
- [[_COMMUNITY_Community 38|Community 38]]
- [[_COMMUNITY_Community 39|Community 39]]
- [[_COMMUNITY_Community 40|Community 40]]
- [[_COMMUNITY_Community 42|Community 42]]
- [[_COMMUNITY_Community 43|Community 43]]
- [[_COMMUNITY_Community 44|Community 44]]
- [[_COMMUNITY_Community 45|Community 45]]

## God Nodes (most connected - your core abstractions)
1. `parse()` - 79 edges
2. `fmt_source()` - 65 edges
3. `check_source()` - 58 edges
4. `validate_output()` - 56 edges
5. `analyze_with_diagnostics()` - 55 edges
6. `parse_with_diagnostics_opts()` - 33 edges
7. `analyze_with_imports()` - 30 edges
8. `Parser<'a>` - 28 edges
9. `compile_directory()` - 28 edges
10. `parse_output_target()` - 27 edges

## Surprising Connections (you probably didn't know these)
- `5-pass hybrid compiler (Parse, Analyze, Transform, Expand[LLM], Validate)` --semantically_similar_to--> `Source-to-IR 9-step pipeline (Parse, Diagnose, Repair, Re-parse, Resolve, Infer, Normalize, Type, Validate)`  [INFERRED] [semantically similar]
  README.md → design/language-surface.md
- `NodeId` --calls--> `lower_output_contract_for_flow()`  [INFERRED]
  /Users/nishantvarshney/genesis/glyph/crates/glyph-core/src/ir.rs → crates/glyph-core/src/lower.rs
- `Description` --calls--> `lower_with_imports()`  [INFERRED]
  /Users/nishantvarshney/genesis/glyph/crates/glyph-core/src/output_target.rs → crates/glyph-core/src/lower.rs
- `Identifier` --calls--> `lower_with_imports()`  [INFERRED]
  /Users/nishantvarshney/genesis/glyph/crates/glyph-core/src/output_target.rs → crates/glyph-core/src/lower.rs
- `compile_source_with_effects()` --calls--> `parse_with_diagnostics_opts()`  [INFERRED]
  /Users/nishantvarshney/genesis/glyph/crates/glyph-core/src/lib.rs → crates/glyph-core/src/parse.rs

## Communities

### Community 0 - "Community 0"
Cohesion: 0.03
Nodes (149): serialize_output_contract_shape_for_both_forms(), ast_rewrite(), ast_rewrite_inner(), auto_import_stdlib(), collapse_duplicate_imports(), duplicate_subsection_post_fmt_clears_both_tiers(), emit_merged_descriptions(), emit_merged_effects() (+141 more)

### Community 1 - "Community 1"
Cohesion: 0.03
Nodes (148): pipeline_block_two_descriptions_emits_both_parse_and_analyze_diagnostics(), pipeline_export_block_two_descriptions_emits_both_parse_and_analyze_diagnostics(), pipeline_two_constraints_emits_both_parse_and_analyze_diagnostics(), placeholder_string_return_descriptive_is_repairable_on_domain_typed_skill(), placeholder_string_return_is_repairable_on_domain_typed_skill(), duplicate_subsection_pre_fmt_surfaces_both_tiers(), ac1_directory_compile_processes_every_file(), ac1_export_text_only_library_check_source_clean() (+140 more)

### Community 2 - "Community 2"
Cohesion: 0.02
Nodes (141): Athena tiered research wiki, Glyph Project Index, Promotion path to ./design/, Research Question: human-readable visualizable DSL, Tier: confirmed, Tier: consolidated, Tier: unconfirmed, Trust Tiers (unconfirmed -> confirmed -> consolidated -> design) (+133 more)

### Community 3 - "Community 3"
Cohesion: 0.05
Nodes (133): analyze_with_diagnostics(), analyze_with_diagnostics_receives_enable_effects(), analyze_with_resolutions(), analyze_with_resolutions_records_block_call_target(), analyze_with_resolutions_records_text_constraint(), analyze_with_resolutions_stdlib_call_marked_stdlib(), analyze_with_resolutions_unresolved_call_no_resolution(), check_ids() (+125 more)

### Community 4 - "Community 4"
Cohesion: 0.03
Nodes (98): analyze(), Skill, arena_with_effects(), serialize_ir_json(), serialize_param(), append_sentence(), block_output_contract_folds_before_inline_expansion(), collapse_whitespace_for_prose() (+90 more)

### Community 5 - "Community 5"
Cohesion: 0.04
Nodes (100): placeholder_description(), placeholder_identifier(), applies_no_parens_corpus_fires_diagnostic(), applies_on_non_block_corpus_fires_diagnostic(), applies_with_args_corpus_fires_diagnostic(), branching_corpus_compiles_with_lettered_substeps(), corpus_path(), glyph_bin() (+92 more)

### Community 6 - "Community 6"
Cohesion: 0.05
Nodes (98): activate(), check_applies_descriptions(), check_applies_descriptions_in_flow(), check_branch_substeps(), check_constraint_count(), check_content_shape(), check_context_count(), check_frontmatter() (+90 more)

### Community 7 - "Community 7"
Cohesion: 0.06
Nodes (59): analyze_export_block(), analyze_skill(), analyze_skill_with_usage_tracking(), analyze_with_imports(), check_applies_in_condition(), check_block_return_calls(), check_branch_body_names(), check_context_entry_name() (+51 more)

### Community 8 - "Community 8"
Cohesion: 0.05
Nodes (86): block_two_constraints_first_wins_second_in_extras(), block_two_contexts_first_wins_second_in_extras(), block_two_descriptions_first_wins_second_in_extras(), block_two_effects_first_wins_second_in_extras(), block_two_flows_first_wins_second_in_extras(), const_alongside_skill_in_same_file(), const_bool_true_literal(), const_bool_uppercase_preserved_in_ast() (+78 more)

### Community 9 - "Community 9"
Cohesion: 0.04
Nodes (71): brainstorming: anti_patterns block, Awkward: 'offer MUST be its own message' has no primitive, Awkward: conversational style constraints vs control-flow, Example: brainstorming skill Glyph rewrite, Open Question: are skill-to-skill handoffs first-class?, brainstorming: 7 phases (orient, visual companion, intent, approaches, design, write/review, handoff), brainstorming: terminal_skill = writing-plans, brainstorming: trigger block (must_invoke_before creative_work) (+63 more)

### Community 10 - "Community 10"
Cohesion: 0.04
Nodes (58): Glyph (Agent Skill DSL), Glyph block concept (reusable sub-component), Glyph compiler pipeline, Glyph constraints: block, Agent Skill DSL Research Index, Agent Skills Standard (SKILL.md), GitHub Agentic Workflows (gh-aw), Competitive Landscape (22 projects) (+50 more)

### Community 11 - "Community 11"
Cohesion: 0.08
Nodes (26): Backend, bare_name_resolves_to_text_decl(), block_call_resolves_same_file(), check_source_with_resolutions(), collect_flow_inline_strings(), covers(), cross_file_diagnostic_attributable_to_dep_uri(), cursor_outside_slot_returns_none() (+18 more)

### Community 12 - "Community 12"
Cohesion: 0.09
Nodes (30): is_builtin_type_name(), is_domain_return_type(), canonicalize_identifier(), Registry, RegistryEntry, span(), t3_canonicalize_is_idempotent(), t4_register_first_use_returns_canonical_entry_with_span() (+22 more)

### Community 13 - "Community 13"
Cohesion: 0.12
Nodes (30): block_call_site_carries_callee_output_contract_object(), find_block_by_name(), find_call(), inline_block_call_site_carries_callee_output_contract_object(), ir_json(), ir_json_after_expand(), node_id_str(), opt_typetag_to_json() (+22 more)

### Community 14 - "Community 14"
Cohesion: 0.18
Nodes (30): compile_directory(), compile_directory_with_ir(), corpus_dir(), fix_bug_constraints(), fix_bug_frontmatter(), fix_bug_has_applies_conditional_imported(), fix_bug_has_applies_conditional_same_file(), fix_bug_has_context_section() (+22 more)

### Community 15 - "Community 15"
Cohesion: 0.21
Nodes (21): compile_and_read_ir(), emit_ir_call_carries_callee_context_for_non_inline_call(), emit_ir_conforms_to_schema_full_skill(), emit_ir_context_node_serializes_correctly(), emit_ir_includes_applies_descriptions_on_branch(), emit_ir_includes_applies_descriptions_with_applies_calls(), emit_ir_includes_description_on_block_in_call(), emit_ir_includes_local_refs_on_resolved_call() (+13 more)

### Community 16 - "Community 16"
Cohesion: 0.2
Nodes (19): analyze_error_roundtrip(), byte_span_to_lsp_range(), byte_span_to_range_at_origin(), byte_span_to_range_basic(), diagnostic_to_lsp(), end_position(), file_label_to_url(), multi_character_single_line_span() (+11 more)

### Community 17 - "Community 17"
Cohesion: 0.1
Nodes (18): BlockDecl, ConstDecl, ConstraintMarker, ConstraintMarkerKind, ConstValue, ContextEntry, Decl, DuplicateSubsection (+10 more)

### Community 18 - "Community 18"
Cohesion: 0.12
Nodes (19): Constraints as first-class IR nodes, Glyph IR: region-structured SSA-typed op tree with constraint overlay, LLVM (SSA, CFG, DCE), MLIR (regions-of-regions, dialects), Regions + SSA sweet spot, Swift SIL (region-structured SSA), Behavior trees decorator-attachment pattern, Gherkin-style temporal scoping for constraints (before/after) (+11 more)

### Community 19 - "Community 19"
Cohesion: 0.21
Nodes (16): avoid_phrasing(), avoid_phrasing_walking_skeleton(), emit(), emit_applies_arm(), emit_branch(), emit_includes_effects_when_enabled(), emit_lettered_substeps(), emit_procedure() (+8 more)

### Community 20 - "Community 20"
Cohesion: 0.31
Nodes (17): assert_has_diagnostic_id(), bare_name_in_flow_fires_text_in_flow_diagnostic(), bare_text_name_at_body_level_fires_ambiguous_role(), body_level_avoid_hoists_to_constraints_section(), body_level_context_hoists_to_context_section(), constraint_only_compiles_with_constraints_no_steps(), context_section_emits_before_steps(), fixture() (+9 more)

### Community 21 - "Community 21"
Cohesion: 0.3
Nodes (13): emit_ir_descriptive_output_contract_shape(), export_block_accepts_output_target_identifier_form(), glyph_bin(), inline_block_output_contract_survives_emit_ir(), ir_json_path(), md_path(), ndjson_contains_id(), output_target_compile_and_emit_ir_shape() (+5 more)

### Community 22 - "Community 22"
Cohesion: 0.39
Nodes (11): ac1_cross_file_resolution(), ac2_circular_import_path(), ac3_import_private(), ac4_import_skill(), ac5_duplicate_import_exit_2(), ac5_unused_import_exit_2(), assert_contains_diagnostic_id(), fixture() (+3 more)

### Community 23 - "Community 23"
Cohesion: 0.35
Nodes (10): descriptive_output_target_in_block_compiles_and_prose_carries_description(), descriptive_output_target_in_export_block_compiles(), glyph_bin(), md_path(), ndjson_contains_id(), placeholder_string_return_repairs_to_descriptive_form(), run_check(), run_compile() (+2 more)

### Community 24 - "Community 24"
Cohesion: 0.49
Nodes (9): assert_has_diagnostic_id(), export_block_missing_default_emits_analyze_diagnostic(), fixture(), glyph_bin(), run_check(), run_compile(), skill_with_params_compiles_and_emits_parameters_section(), slot_in_description_emits_repairable_parse_diagnostic() (+1 more)

### Community 25 - "Community 25"
Cohesion: 0.2
Nodes (10): Airflow tab-based multi-view, Compiler Explorer / Godbolt (source-to-output mapping), DSPy compiled prompts visibility, Source attribution / line mapping in compiled view, Stately/XState bidirectional sync, Structurizr DSL (model-first multi-view), Three views pattern for Glyph (code, graph, compiled), n8n Vue Flow canvas with mapping layer (+2 more)

### Community 26 - "Community 26"
Cohesion: 0.42
Nodes (8): compile_fixture(), const_bool_inlines_into_compiled_md(), const_bool_uppercase_normalizes_to_lowercase(), const_float_inlines_into_compiled_md(), const_int_inlines_into_compiled_md(), const_string_inlines_into_compiled_md(), corpus_source(), glyph_bin()

### Community 27 - "Community 27"
Cohesion: 0.25
Nodes (8): DSPy (programmatic prompt optimization), Glyph's unique differentiators (external DSL, agent instructions), Handlebars templating, Jinja2 prompt templating, Template systems lack semantics, types, constraints, Microsoft Agent Framework (SK+AutoGen convergence), AutoGen conversable agent conversations, Semantic Kernel plugins (annotated code)

### Community 28 - "Community 28"
Cohesion: 0.33
Nodes (6): LangGraph framework (stateful graph state machine), LangGraph limitations vs Glyph (not a language, no compilation), LangGraph reducer-driven state schema, DSPy typed signatures as parameter contracts, Named variable SSA-like data flow, LangGraph Studio (closest analogue IDE)

### Community 29 - "Community 29"
Cohesion: 0.4
Nodes (5): Closed OpKind catalog (LLM safety), 5-pass pipeline: Parse, Analyze, Transform, Expand, Validate, Eight IR invariants every pass preserves, PlanCompiler (registry-constrained generation), SatLM (LLM + SMT decoupled verification)

### Community 30 - "Community 30"
Cohesion: 0.5
Nodes (4): Stay an external DSL with canonical formatter, if/for_each/predicates as first-class AST nodes (not strings), No host-language escape hatch, Starlark hermeticity discipline

### Community 31 - "Community 31"
Cohesion: 0.5
Nodes (4): Constraints local to skill/block (anti-Drools), Inform 7 (NL-shaped programming reference), Constraints as named predicate references, not rule bodies, Nix-overlay-inspired skill extension syntax

### Community 32 - "Community 32"
Cohesion: 0.67
Nodes (3): Keyword arguments for 3+ argument calls, Reject retry/timeout as language primitives, Keep primitive count small (~8)

### Community 33 - "Community 33"
Cohesion: 1.0
Nodes (1): Spanned<T>

### Community 35 - "Community 35"
Cohesion: 1.0
Nodes (2): Finding: no existing project combines custom DSL + skill abstractions + compiler + constraints + NL output, Log 2026-04-20: competitive landscape

### Community 36 - "Community 36"
Cohesion: 1.0
Nodes (2): Finding: preliminary stack — code-first + Mermaid/D2 + React Flow + Dagre/ELK + Compiler Explorer pattern, Log 2026-04-20: visualization approaches survey

### Community 37 - "Community 37"
Cohesion: 1.0
Nodes (2): Cytoscape.js, vis-network

### Community 38 - "Community 38"
Cohesion: 1.0
Nodes (2): Temporal struct-based params with ctx object, Temporal stateful function IR

### Community 39 - "Community 39"
Cohesion: 1.0
Nodes (2): Dagster Pydantic config with defaults, Implicit system-provided context

### Community 40 - "Community 40"
Cohesion: 1.0
Nodes (2): PDL (IBM YAML prompt declaration), Plang (string-first NL affinity)

### Community 42 - "Community 42"
Cohesion: 1.0
Nodes (1): Log 2026-04-20: existing agent/LLM systems survey

### Community 43 - "Community 43"
Cohesion: 1.0
Nodes (1): Log 2026-04-20: lessons-from-existing-languages + IR design proposal

### Community 44 - "Community 44"
Cohesion: 1.0
Nodes (1): Log 2026-04-21: reorganised under Athena layout

### Community 45 - "Community 45"
Cohesion: 1.0
Nodes (1): Consolidated tier registry (currently empty)

## Knowledge Gaps
- **269 isolated node(s):** `Document`, `InitOptions`, `SlotMatch`, `IrNode`, `IrSkill` (+264 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **Thin community `Community 33`** (2 nodes): `Spanned<T>`, `.new()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 35`** (2 nodes): `Finding: no existing project combines custom DSL + skill abstractions + compiler + constraints + NL output`, `Log 2026-04-20: competitive landscape`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 36`** (2 nodes): `Finding: preliminary stack — code-first + Mermaid/D2 + React Flow + Dagre/ELK + Compiler Explorer pattern`, `Log 2026-04-20: visualization approaches survey`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 37`** (2 nodes): `Cytoscape.js`, `vis-network`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 38`** (2 nodes): `Temporal struct-based params with ctx object`, `Temporal stateful function IR`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 39`** (2 nodes): `Dagster Pydantic config with defaults`, `Implicit system-provided context`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 40`** (2 nodes): `PDL (IBM YAML prompt declaration)`, `Plang (string-first NL affinity)`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 42`** (1 nodes): `Log 2026-04-20: existing agent/LLM systems survey`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 43`** (1 nodes): `Log 2026-04-20: lessons-from-existing-languages + IR design proposal`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 44`** (1 nodes): `Log 2026-04-21: reorganised under Athena layout`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 45`** (1 nodes): `Consolidated tier registry (currently empty)`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `fmt_source()` connect `Community 0` to `Community 3`, `Community 5`, `Community 7`?**
  _High betweenness centrality (0.041) - this node is a cross-community bridge._
- **Why does `compile_directory()` connect `Community 14` to `Community 3`?**
  _High betweenness centrality (0.026) - this node is a cross-community bridge._
- **Why does `Skill` connect `Community 4` to `Community 17`, `Community 7`?**
  _High betweenness centrality (0.024) - this node is a cross-community bridge._
- **Are the 75 inferred relationships involving `parse()` (e.g. with `compile_source_with_effects()` and `emit_library_procedures()`) actually correct?**
  _`parse()` has 75 INFERRED edges - model-reasoned connections that need verification._
- **Are the 5 inferred relationships involving `fmt_source()` (e.g. with `.new()` and `parse_with_diagnostics_opts()`) actually correct?**
  _`fmt_source()` has 5 INFERRED edges - model-reasoned connections that need verification._
- **Are the 8 inferred relationships involving `check_source()` (e.g. with `duplicate_subsection_pre_fmt_surfaces_both_tiers()` and `duplicate_subsection_post_fmt_clears_both_tiers()`) actually correct?**
  _`check_source()` has 8 INFERRED edges - model-reasoned connections that need verification._
- **Are the 3 inferred relationships involving `validate_output()` (e.g. with `.get()` and `validate_output_does_not_flag_output_contract_field()`) actually correct?**
  _`validate_output()` has 3 INFERRED edges - model-reasoned connections that need verification._