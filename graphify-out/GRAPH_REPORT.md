# Graph Report - glyph  (2026-05-04)

## Corpus Check
- 52 files · ~741,489 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 1660 nodes · 4621 edges · 42 communities detected
- Extraction: 67% EXTRACTED · 33% INFERRED · 0% AMBIGUOUS · INFERRED: 1545 edges (avg confidence: 0.8)
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
- [[_COMMUNITY_Community 33|Community 33]]
- [[_COMMUNITY_Community 34|Community 34]]
- [[_COMMUNITY_Community 35|Community 35]]
- [[_COMMUNITY_Community 36|Community 36]]
- [[_COMMUNITY_Community 37|Community 37]]
- [[_COMMUNITY_Community 38|Community 38]]
- [[_COMMUNITY_Community 41|Community 41]]
- [[_COMMUNITY_Community 42|Community 42]]
- [[_COMMUNITY_Community 43|Community 43]]
- [[_COMMUNITY_Community 44|Community 44]]

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
- `cross_file_import_resolves_to_dep_file()` --calls--> `check_source_with_resolutions_at_path()`  [INFERRED]
  crates/glyph-lsp/src/lib.rs → .worktrees/lsp/crates/glyph-core/src/lib.rs
- `cross_file_diagnostic_attributable_to_dep_uri()` --calls--> `check_source_with_imports()`  [INFERRED]
  crates/glyph-lsp/src/lib.rs → .worktrees/lsp/crates/glyph-core/src/lib.rs
- `main()` --calls--> `parse()`  [INFERRED]
  crates/glyph-cli/src/main.rs → skills/issue-list-orchestrator/scripts/parse_issues.py
- `compile_source_with_effects()` --calls--> `parse_with_diagnostics_opts()`  [INFERRED]
  .worktrees/lsp/crates/glyph-core/src/lib.rs → /Users/nishantvarshney/genesis/glyph-worktrees/phase-3a-integration/crates/glyph-core/src/parse.rs

## Communities

### Community 0 - "Community 0"
Cohesion: 0.04
Nodes (152): analyze_export_block(), analyze_skill(), analyze_skill_with_usage_tracking(), analyze_with_diagnostics(), analyze_with_diagnostics_receives_enable_effects(), analyze_with_imports(), analyze_with_resolutions(), analyze_with_resolutions_records_block_call_target() (+144 more)

### Community 1 - "Community 1"
Cohesion: 0.03
Nodes (150): serialize_output_contract_shape_for_both_forms(), ast_rewrite(), ast_rewrite_inner(), auto_import_stdlib(), collapse_duplicate_imports(), duplicate_subsection_post_fmt_clears_both_tiers(), emit_merged_descriptions(), emit_merged_effects() (+142 more)

### Community 2 - "Community 2"
Cohesion: 0.02
Nodes (141): Athena tiered research wiki, Glyph Project Index, Promotion path to ./design/, Research Question: human-readable visualizable DSL, Tier: confirmed, Tier: consolidated, Tier: unconfirmed, Trust Tiers (unconfirmed -> confirmed -> consolidated -> design) (+133 more)

### Community 3 - "Community 3"
Cohesion: 0.04
Nodes (137): pipeline_block_two_descriptions_emits_both_parse_and_analyze_diagnostics(), pipeline_export_block_two_descriptions_emits_both_parse_and_analyze_diagnostics(), pipeline_two_constraints_emits_both_parse_and_analyze_diagnostics(), placeholder_string_return_descriptive_is_repairable_on_domain_typed_skill(), placeholder_string_return_is_repairable_on_domain_typed_skill(), DiagBag, duplicate_subsection_pre_fmt_surfaces_both_tiers(), multi_autofix_converges() (+129 more)

### Community 4 - "Community 4"
Cohesion: 0.04
Nodes (99): analyze(), block_call_site_carries_callee_output_contract_object(), emit_is_byte_identical_across_runs_with_output_contract(), find_block_by_name(), find_call(), inline_block_call_site_carries_callee_output_contract_object(), ir_json(), ir_json_after_expand() (+91 more)

### Community 5 - "Community 5"
Cohesion: 0.05
Nodes (106): collect_same_file_resolutions(), record_call_target(), record_context_name_use(), record_cross_file_any_use(), record_cross_file_call(), record_text_use(), walk_flow_for_resolutions(), activate() (+98 more)

### Community 6 - "Community 6"
Cohesion: 0.04
Nodes (91): check_accepts_directory_path(), check_default_format_is_pretty(), check_invalid_exits_one(), check_repairable_exits_two_with_diagnostic_on_stdout(), check_repairable_pretty_renders_to_stderr(), check_valid_exits_zero_and_writes_no_md(), corpus_path(), glyph_bin() (+83 more)

### Community 7 - "Community 7"
Cohesion: 0.06
Nodes (84): directory_mode_cleans_stale_tmp(), failed_compile_leaves_no_tmp_and_no_md(), glyph_bin(), prior_md_survives_failed_rebuild(), procedure_files_stale_tmp_cleaned(), stale_tmp_cleaned_on_rebuild(), assert_has_diagnostic_id(), bare_name_in_flow_fires_text_in_flow_diagnostic() (+76 more)

### Community 8 - "Community 8"
Cohesion: 0.05
Nodes (87): ast_flows_through_for_mixed_repairables(), block_two_constraints_first_wins_second_in_extras(), block_two_contexts_first_wins_second_in_extras(), block_two_descriptions_first_wins_second_in_extras(), block_two_effects_first_wins_second_in_extras(), block_two_flows_first_wins_second_in_extras(), const_alongside_skill_in_same_file(), const_bool_true_literal() (+79 more)

### Community 9 - "Community 9"
Cohesion: 0.04
Nodes (71): brainstorming: anti_patterns block, Awkward: 'offer MUST be its own message' has no primitive, Awkward: conversational style constraints vs control-flow, Example: brainstorming skill Glyph rewrite, Open Question: are skill-to-skill handoffs first-class?, brainstorming: 7 phases (orient, visual companion, intent, approaches, design, write/review, handoff), brainstorming: terminal_skill = writing-plans, brainstorming: trigger block (must_invoke_before creative_work) (+63 more)

### Community 10 - "Community 10"
Cohesion: 0.06
Nodes (47): Skill, arena_with_effects(), avoid_phrasing(), avoid_phrasing_walking_skeleton(), emit(), emit_applies_arm(), emit_branch(), emit_includes_effects_when_enabled() (+39 more)

### Community 11 - "Community 11"
Cohesion: 0.04
Nodes (58): Glyph (Agent Skill DSL), Glyph block concept (reusable sub-component), Glyph compiler pipeline, Glyph constraints: block, Agent Skill DSL Research Index, Agent Skills Standard (SKILL.md), GitHub Agentic Workflows (gh-aw), Competitive Landscape (22 projects) (+50 more)

### Community 12 - "Community 12"
Cohesion: 0.08
Nodes (27): Backend, bare_name_resolves_to_text_decl(), block_call_resolves_same_file(), check_source_with_resolutions(), collect_flow_inline_strings(), covers(), cross_file_diagnostic_attributable_to_dep_uri(), cross_file_import_resolves_to_dep_file() (+19 more)

### Community 13 - "Community 13"
Cohesion: 0.16
Nodes (8): Parser<'a>, first_slot_offset(), is_ident_continue(), is_ident_start(), multiple_slots(), scan_slots(), single_slot(), SlotMatch

### Community 14 - "Community 14"
Cohesion: 0.1
Nodes (29): canonicalize_identifier(), Registry, RegistryEntry, span(), t3_canonicalize_is_idempotent(), t4_register_first_use_returns_canonical_entry_with_span(), t5_repeat_register_preserves_first_use_span(), t6_lookup_hits_across_canonicalized_variants() (+21 more)

### Community 15 - "Community 15"
Cohesion: 0.18
Nodes (30): compile_directory(), compile_directory_with_ir(), corpus_dir(), fix_bug_constraints(), fix_bug_frontmatter(), fix_bug_has_applies_conditional_imported(), fix_bug_has_applies_conditional_same_file(), fix_bug_has_context_section() (+22 more)

### Community 16 - "Community 16"
Cohesion: 0.2
Nodes (20): analyze_error_roundtrip(), byte_span_to_lsp_range(), byte_span_to_range_at_origin(), byte_span_to_range_basic(), diagnostic_to_lsp(), end_position(), file_label_to_url(), missing_required_arg_roundtrip() (+12 more)

### Community 17 - "Community 17"
Cohesion: 0.21
Nodes (21): compile_and_read_ir(), emit_ir_call_carries_callee_context_for_non_inline_call(), emit_ir_conforms_to_schema_full_skill(), emit_ir_context_node_serializes_correctly(), emit_ir_includes_applies_descriptions_on_branch(), emit_ir_includes_applies_descriptions_with_applies_calls(), emit_ir_includes_description_on_block_in_call(), emit_ir_includes_local_refs_on_resolved_call() (+13 more)

### Community 18 - "Community 18"
Cohesion: 0.11
Nodes (17): BlockDecl, ConstDecl, ConstraintMarker, ConstraintMarkerKind, ConstValue, ContextEntry, Decl, DuplicateSubsection (+9 more)

### Community 19 - "Community 19"
Cohesion: 0.12
Nodes (19): Constraints as first-class IR nodes, Glyph IR: region-structured SSA-typed op tree with constraint overlay, LLVM (SSA, CFG, DCE), MLIR (regions-of-regions, dialects), Regions + SSA sweet spot, Swift SIL (region-structured SSA), Behavior trees decorator-attachment pattern, Gherkin-style temporal scoping for constraints (before/after) (+11 more)

### Community 20 - "Community 20"
Cohesion: 0.3
Nodes (13): emit_ir_descriptive_output_contract_shape(), export_block_accepts_output_target_identifier_form(), glyph_bin(), inline_block_output_contract_survives_emit_ir(), ir_json_path(), md_path(), ndjson_contains_id(), output_target_compile_and_emit_ir_shape() (+5 more)

### Community 21 - "Community 21"
Cohesion: 0.37
Nodes (12): ac1_cross_file_resolution(), ac2_circular_import_path(), ac3_import_private(), ac4_import_skill(), ac5_duplicate_import_exit_2(), ac5_unused_import_exit_2(), assert_contains_diagnostic_id(), fixture() (+4 more)

### Community 22 - "Community 22"
Cohesion: 0.35
Nodes (10): descriptive_output_target_in_block_compiles_and_prose_carries_description(), descriptive_output_target_in_export_block_compiles(), glyph_bin(), md_path(), ndjson_contains_id(), placeholder_string_return_repairs_to_descriptive_form(), run_check(), run_compile() (+2 more)

### Community 23 - "Community 23"
Cohesion: 0.49
Nodes (9): applies_no_parens_corpus_fires_diagnostic(), applies_on_non_block_corpus_fires_diagnostic(), applies_with_args_corpus_fires_diagnostic(), branching_corpus_compiles_with_lettered_substeps(), corpus_path(), glyph_bin(), run_check_json(), run_compile() (+1 more)

### Community 24 - "Community 24"
Cohesion: 0.2
Nodes (10): Airflow tab-based multi-view, Compiler Explorer / Godbolt (source-to-output mapping), DSPy compiled prompts visibility, Source attribution / line mapping in compiled view, Stately/XState bidirectional sync, Structurizr DSL (model-first multi-view), Three views pattern for Glyph (code, graph, compiled), n8n Vue Flow canvas with mapping layer (+2 more)

### Community 25 - "Community 25"
Cohesion: 0.25
Nodes (8): DSPy (programmatic prompt optimization), Glyph's unique differentiators (external DSL, agent instructions), Handlebars templating, Jinja2 prompt templating, Template systems lack semantics, types, constraints, Microsoft Agent Framework (SK+AutoGen convergence), AutoGen conversable agent conversations, Semantic Kernel plugins (annotated code)

### Community 26 - "Community 26"
Cohesion: 0.33
Nodes (6): LangGraph framework (stateful graph state machine), LangGraph limitations vs Glyph (not a language, no compilation), LangGraph reducer-driven state schema, DSPy typed signatures as parameter contracts, Named variable SSA-like data flow, LangGraph Studio (closest analogue IDE)

### Community 27 - "Community 27"
Cohesion: 0.4
Nodes (5): Closed OpKind catalog (LLM safety), 5-pass pipeline: Parse, Analyze, Transform, Expand, Validate, Eight IR invariants every pass preserves, PlanCompiler (registry-constrained generation), SatLM (LLM + SMT decoupled verification)

### Community 28 - "Community 28"
Cohesion: 0.5
Nodes (4): Stay an external DSL with canonical formatter, if/for_each/predicates as first-class AST nodes (not strings), No host-language escape hatch, Starlark hermeticity discipline

### Community 29 - "Community 29"
Cohesion: 0.5
Nodes (4): Constraints local to skill/block (anti-Drools), Inform 7 (NL-shaped programming reference), Constraints as named predicate references, not rule bodies, Nix-overlay-inspired skill extension syntax

### Community 30 - "Community 30"
Cohesion: 0.67
Nodes (3): Keyword arguments for 3+ argument calls, Reject retry/timeout as language primitives, Keep primitive count small (~8)

### Community 31 - "Community 31"
Cohesion: 1.0
Nodes (1): Spanned<T>

### Community 33 - "Community 33"
Cohesion: 1.0
Nodes (2): Finding: no existing project combines custom DSL + skill abstractions + compiler + constraints + NL output, Log 2026-04-20: competitive landscape

### Community 34 - "Community 34"
Cohesion: 1.0
Nodes (2): Finding: preliminary stack — code-first + Mermaid/D2 + React Flow + Dagre/ELK + Compiler Explorer pattern, Log 2026-04-20: visualization approaches survey

### Community 35 - "Community 35"
Cohesion: 1.0
Nodes (2): Cytoscape.js, vis-network

### Community 36 - "Community 36"
Cohesion: 1.0
Nodes (2): Temporal struct-based params with ctx object, Temporal stateful function IR

### Community 37 - "Community 37"
Cohesion: 1.0
Nodes (2): Dagster Pydantic config with defaults, Implicit system-provided context

### Community 38 - "Community 38"
Cohesion: 1.0
Nodes (2): PDL (IBM YAML prompt declaration), Plang (string-first NL affinity)

### Community 41 - "Community 41"
Cohesion: 1.0
Nodes (1): Log 2026-04-20: existing agent/LLM systems survey

### Community 42 - "Community 42"
Cohesion: 1.0
Nodes (1): Log 2026-04-20: lessons-from-existing-languages + IR design proposal

### Community 43 - "Community 43"
Cohesion: 1.0
Nodes (1): Log 2026-04-21: reorganised under Athena layout

### Community 44 - "Community 44"
Cohesion: 1.0
Nodes (1): Consolidated tier registry (currently empty)

## Knowledge Gaps
- **269 isolated node(s):** `Document`, `InitOptions`, `SlotMatch`, `IrNode`, `IrSkill` (+264 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **Thin community `Community 31`** (2 nodes): `Spanned<T>`, `.new()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 33`** (2 nodes): `Finding: no existing project combines custom DSL + skill abstractions + compiler + constraints + NL output`, `Log 2026-04-20: competitive landscape`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 34`** (2 nodes): `Finding: preliminary stack — code-first + Mermaid/D2 + React Flow + Dagre/ELK + Compiler Explorer pattern`, `Log 2026-04-20: visualization approaches survey`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 35`** (2 nodes): `Cytoscape.js`, `vis-network`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 36`** (2 nodes): `Temporal struct-based params with ctx object`, `Temporal stateful function IR`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 37`** (2 nodes): `Dagster Pydantic config with defaults`, `Implicit system-provided context`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 38`** (2 nodes): `PDL (IBM YAML prompt declaration)`, `Plang (string-first NL affinity)`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 41`** (1 nodes): `Log 2026-04-20: existing agent/LLM systems survey`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 42`** (1 nodes): `Log 2026-04-20: lessons-from-existing-languages + IR design proposal`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 43`** (1 nodes): `Log 2026-04-21: reorganised under Athena layout`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 44`** (1 nodes): `Consolidated tier registry (currently empty)`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `fmt_source()` connect `Community 1` to `Community 0`, `Community 3`, `Community 6`?**
  _High betweenness centrality (0.045) - this node is a cross-community bridge._
- **Why does `scan_slots()` connect `Community 13` to `Community 0`, `Community 1`, `Community 5`, `Community 7`, `Community 12`?**
  _High betweenness centrality (0.020) - this node is a cross-community bridge._
- **Why does `compile_directory()` connect `Community 15` to `Community 0`, `Community 7`?**
  _High betweenness centrality (0.019) - this node is a cross-community bridge._
- **Are the 75 inferred relationships involving `parse()` (e.g. with `compile_source_with_effects()` and `emit_library_procedures()`) actually correct?**
  _`parse()` has 75 INFERRED edges - model-reasoned connections that need verification._
- **Are the 5 inferred relationships involving `fmt_source()` (e.g. with `.new()` and `parse_with_diagnostics_opts()`) actually correct?**
  _`fmt_source()` has 5 INFERRED edges - model-reasoned connections that need verification._
- **Are the 8 inferred relationships involving `check_source()` (e.g. with `duplicate_subsection_pre_fmt_surfaces_both_tiers()` and `duplicate_subsection_post_fmt_clears_both_tiers()`) actually correct?**
  _`check_source()` has 8 INFERRED edges - model-reasoned connections that need verification._
- **Are the 3 inferred relationships involving `validate_output()` (e.g. with `.get()` and `validate_output_does_not_flag_output_contract_field()`) actually correct?**
  _`validate_output()` has 3 INFERRED edges - model-reasoned connections that need verification._