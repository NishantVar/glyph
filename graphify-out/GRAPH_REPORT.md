# Graph Report - .  (2026-05-03)

## Corpus Check
- 1000 files · ~0 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 994 nodes · 2448 edges · 43 communities detected
- Extraction: 68% EXTRACTED · 32% INFERRED · 0% AMBIGUOUS · INFERRED: 778 edges (avg confidence: 0.8)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- [[_COMMUNITY_Code Cluster 0|Code Cluster 0]]
- [[_COMMUNITY_Code Cluster 1|Code Cluster 1]]
- [[_COMMUNITY_Code Cluster 2|Code Cluster 2]]
- [[_COMMUNITY_Code Cluster 3|Code Cluster 3]]
- [[_COMMUNITY_Code Cluster 4|Code Cluster 4]]
- [[_COMMUNITY_Code Cluster 5|Code Cluster 5]]
- [[_COMMUNITY_Code Cluster 6|Code Cluster 6]]
- [[_COMMUNITY_Code Cluster 7|Code Cluster 7]]
- [[_COMMUNITY_Code Cluster 8|Code Cluster 8]]
- [[_COMMUNITY_Code Cluster 9|Code Cluster 9]]
- [[_COMMUNITY_Code Cluster 10|Code Cluster 10]]
- [[_COMMUNITY_Code Cluster 11|Code Cluster 11]]
- [[_COMMUNITY_Code Cluster 12|Code Cluster 12]]
- [[_COMMUNITY_Code Cluster 13|Code Cluster 13]]
- [[_COMMUNITY_Code Cluster 14|Code Cluster 14]]
- [[_COMMUNITY_Code Cluster 15|Code Cluster 15]]
- [[_COMMUNITY_Code Cluster 16|Code Cluster 16]]
- [[_COMMUNITY_Code Cluster 17|Code Cluster 17]]
- [[_COMMUNITY_Code Cluster 18|Code Cluster 18]]
- [[_COMMUNITY_Code Cluster 19|Code Cluster 19]]
- [[_COMMUNITY_Code Cluster 20|Code Cluster 20]]
- [[_COMMUNITY_Code Cluster 21|Code Cluster 21]]
- [[_COMMUNITY_Code Cluster 22|Code Cluster 22]]
- [[_COMMUNITY_Code Cluster 23|Code Cluster 23]]
- [[_COMMUNITY_Code Cluster 24|Code Cluster 24]]
- [[_COMMUNITY_Code Cluster 25|Code Cluster 25]]
- [[_COMMUNITY_Code Cluster 26|Code Cluster 26]]
- [[_COMMUNITY_Code Cluster 27|Code Cluster 27]]
- [[_COMMUNITY_Code Cluster 28|Code Cluster 28]]
- [[_COMMUNITY_Code Cluster 29|Code Cluster 29]]
- [[_COMMUNITY_Code Cluster 30|Code Cluster 30]]
- [[_COMMUNITY_Code Cluster 31|Code Cluster 31]]
- [[_COMMUNITY_Code Cluster 32|Code Cluster 32]]
- [[_COMMUNITY_Code Cluster 33|Code Cluster 33]]
- [[_COMMUNITY_Code Cluster 34|Code Cluster 34]]
- [[_COMMUNITY_Code Cluster 35|Code Cluster 35]]
- [[_COMMUNITY_Code Cluster 36|Code Cluster 36]]
- [[_COMMUNITY_Code Cluster 37|Code Cluster 37]]
- [[_COMMUNITY_Code Cluster 38|Code Cluster 38]]
- [[_COMMUNITY_Code Cluster 39|Code Cluster 39]]
- [[_COMMUNITY_Code Cluster 40|Code Cluster 40]]
- [[_COMMUNITY_Code Cluster 41|Code Cluster 41]]
- [[_COMMUNITY_Code Cluster 42|Code Cluster 42]]

## God Nodes (most connected - your core abstractions)
1. `validate_output()` - 51 edges
2. `check_source()` - 36 edges
3. `compile_directory()` - 28 edges
4. `Glyph Foundations reference card` - 27 edges
5. `setup_tempdir()` - 27 edges
6. `parse()` - 26 edges
7. `Parser<'a>` - 21 edges
8. `Glyph Language Surface (syntax)` - 19 edges
9. `compile_source_with_effects()` - 18 edges
10. `compile_directory_with_options()` - 18 edges

## Surprising Connections (you probably didn't know these)
- `5-pass hybrid compiler (Parse, Analyze, Transform, Expand[LLM], Validate)` --semantically_similar_to--> `Source-to-IR 9-step pipeline (Parse, Diagnose, Repair, Re-parse, Resolve, Infer, Normalize, Type, Validate)`  [INFERRED] [semantically similar]
  README.md → design/language-surface.md
- `Glyph Language Surface (syntax)` --implements--> `tmp/fix_bug.glyph.md source example`  [INFERRED]
  design/language-surface.md → tmp/fix_bug.glyph.md
- `Compiled output Markdown shape` --implements--> `tmp/fix_bug.md compiled output example`  [INFERRED]
  design/compiled-output.md → tmp/fix_bug.md
- `Glyph Project Index` --references--> `Research Question: human-readable visualizable DSL`  [EXTRACTED]
  AGENTS.md → research/agent-skill-dsl/AGENTS.md
- `Glyph Project Index` --references--> `Tier: unconfirmed`  [EXTRACTED]
  AGENTS.md → research/agent-skill-dsl/AGENTS.md

## Communities

### Community 0 - "Code Cluster 0"
Cohesion: 0.04
Nodes (131): analyze(), ac1_directory_compile_processes_every_file(), ac1_export_text_only_library_check_source_clean(), ac1_export_text_only_library_compiles_exit_zero(), ac2_repo_tools_library_compiles_with_large_export_block(), ac2_topological_order_libraries_before_consumers(), ac3_closure_violation_on_private_free_variable(), ac3_failure_skips_dependent_with_warning() (+123 more)

### Community 1 - "Code Cluster 1"
Cohesion: 0.02
Nodes (128): Athena tiered research wiki, Glyph Project Index, Promotion path to ./design/, Research Question: human-readable visualizable DSL, Tier: confirmed, Tier: consolidated, Tier: unconfirmed, Trust Tiers (unconfirmed -> confirmed -> consolidated -> design) (+120 more)

### Community 2 - "Code Cluster 2"
Cohesion: 0.06
Nodes (93): ast_rewrite(), fmt_source(), FmtResult, is_constraint_marker(), is_context_marker(), preparse_rewrite(), rewrite_decl_body(), Section (+85 more)

### Community 3 - "Code Cluster 3"
Cohesion: 0.07
Nodes (61): analyze_export_block(), analyze_skill(), analyze_skill_with_usage_tracking(), analyze_with_diagnostics(), analyze_with_diagnostics_receives_enable_effects(), analyze_with_imports(), check_applies_in_condition(), check_branch_body_names() (+53 more)

### Community 4 - "Code Cluster 4"
Cohesion: 0.04
Nodes (71): brainstorming: anti_patterns block, Awkward: 'offer MUST be its own message' has no primitive, Awkward: conversational style constraints vs control-flow, Example: brainstorming skill Glyph rewrite, Open Question: are skill-to-skill handoffs first-class?, brainstorming: 7 phases (orient, visual companion, intent, approaches, design, write/review, handoff), brainstorming: terminal_skill = writing-plans, brainstorming: trigger block (must_invoke_before creative_work) (+63 more)

### Community 5 - "Code Cluster 5"
Cohesion: 0.04
Nodes (58): Glyph (Agent Skill DSL), Glyph block concept (reusable sub-component), Glyph compiler pipeline, Glyph constraints: block, Agent Skill DSL Research Index, Agent Skills Standard (SKILL.md), GitHub Agentic Workflows (gh-aw), Competitive Landscape (22 projects) (+50 more)

### Community 6 - "Code Cluster 6"
Cohesion: 0.08
Nodes (51): assert_contains_diagnostic_id(), empty_file_exits_one_with_empty_file_diagnostic(), empty_flow_does_not_emit_md_file(), empty_flow_exits_one_with_empty_flow_diagnostic(), fixture(), glyph_bin(), json_format_produces_ndjson_on_stdout(), json_output_is_byte_identical_across_runs() (+43 more)

### Community 7 - "Code Cluster 7"
Cohesion: 0.07
Nodes (38): Skill, DiagBag, arena_with_effects(), count_words(), expand_step1(), expand_step1_with_imported_descriptions(), IrArena, IrBlock (+30 more)

### Community 8 - "Code Cluster 8"
Cohesion: 0.18
Nodes (30): compile_directory(), compile_directory_with_ir(), corpus_dir(), fix_bug_constraints(), fix_bug_frontmatter(), fix_bug_has_applies_conditional_imported(), fix_bug_has_applies_conditional_same_file(), fix_bug_has_context_section() (+22 more)

### Community 9 - "Code Cluster 9"
Cohesion: 0.21
Nodes (21): compile_and_read_ir(), emit_ir_call_carries_callee_context_null_when_inline(), emit_ir_conforms_to_schema_full_skill(), emit_ir_context_node_serializes_correctly(), emit_ir_includes_applies_descriptions_on_branch(), emit_ir_includes_applies_descriptions_with_applies_calls(), emit_ir_includes_description_on_block_in_call(), emit_ir_includes_local_refs_on_resolved_call() (+13 more)

### Community 10 - "Code Cluster 10"
Cohesion: 0.12
Nodes (19): Constraints as first-class IR nodes, Glyph IR: region-structured SSA-typed op tree with constraint overlay, LLVM (SSA, CFG, DCE), MLIR (regions-of-regions, dialects), Regions + SSA sweet spot, Swift SIL (region-structured SSA), Behavior trees decorator-attachment pattern, Gherkin-style temporal scoping for constraints (before/after) (+11 more)

### Community 11 - "Code Cluster 11"
Cohesion: 0.26
Nodes (18): arena_with_effects(), find_block_by_name(), node_id_str(), polarity_str(), projection_mode_str(), role_str(), serialize_branch(), serialize_call() (+10 more)

### Community 12 - "Code Cluster 12"
Cohesion: 0.31
Nodes (17): assert_has_diagnostic_id(), bare_name_in_flow_fires_text_in_flow_diagnostic(), bare_text_name_at_body_level_fires_ambiguous_role(), body_level_avoid_hoists_to_constraints_section(), body_level_context_hoists_to_context_section(), constraint_only_compiles_with_constraints_no_steps(), context_section_emits_before_steps(), fixture() (+9 more)

### Community 13 - "Code Cluster 13"
Cohesion: 0.21
Nodes (16): avoid_phrasing(), avoid_phrasing_walking_skeleton(), emit(), emit_applies_arm(), emit_branch(), emit_includes_effects_when_enabled(), emit_lettered_substeps(), emit_procedure() (+8 more)

### Community 14 - "Code Cluster 14"
Cohesion: 0.12
Nodes (15): BlockDecl, ConstraintMarker, ConstraintMarkerKind, ContextEntry, Decl, ElifBranch, ExportBlockDecl, FlowStmt (+7 more)

### Community 15 - "Code Cluster 15"
Cohesion: 0.39
Nodes (11): ac1_cross_file_resolution(), ac2_circular_import_path(), ac3_import_private(), ac4_import_skill(), ac5_duplicate_import_exit_2(), ac5_unused_import_exit_2(), assert_contains_diagnostic_id(), fixture() (+3 more)

### Community 16 - "Code Cluster 16"
Cohesion: 0.27
Nodes (7): first_slot_offset(), is_ident_continue(), is_ident_start(), multiple_slots(), scan_slots(), single_slot(), SlotMatch

### Community 17 - "Code Cluster 17"
Cohesion: 0.42
Nodes (10): check_accepts_directory_path(), check_default_format_is_pretty(), check_invalid_exits_one(), check_repairable_exits_two_with_diagnostic_on_stdout(), check_repairable_pretty_renders_to_stderr(), check_valid_exits_zero_and_writes_no_md(), corpus_path(), glyph_bin() (+2 more)

### Community 18 - "Code Cluster 18"
Cohesion: 0.2
Nodes (10): Airflow tab-based multi-view, Compiler Explorer / Godbolt (source-to-output mapping), DSPy compiled prompts visibility, Source attribution / line mapping in compiled view, Stately/XState bidirectional sync, Structurizr DSL (model-first multi-view), Three views pattern for Glyph (code, graph, compiled), n8n Vue Flow canvas with mapping layer (+2 more)

### Community 19 - "Code Cluster 19"
Cohesion: 0.49
Nodes (9): applies_no_parens_corpus_fires_diagnostic(), applies_on_non_block_corpus_fires_diagnostic(), applies_with_args_corpus_fires_diagnostic(), branching_corpus_compiles_with_lettered_substeps(), corpus_path(), glyph_bin(), run_check_json(), run_compile() (+1 more)

### Community 20 - "Code Cluster 20"
Cohesion: 0.49
Nodes (9): assert_has_diagnostic_id(), export_block_missing_default_emits_analyze_diagnostic(), fixture(), glyph_bin(), run_check(), run_compile(), skill_with_params_compiles_and_emits_parameters_section(), slot_in_description_emits_repairable_parse_diagnostic() (+1 more)

### Community 21 - "Code Cluster 21"
Cohesion: 0.22
Nodes (9): Constraint role with strength x polarity, Context role, InputContract role, OutputContract role, Rationale: input-first role taxonomy, one Constraint role, effects stay separate, Closed role set: InputContract, Step, Constraint, Context, OutputContract, Source marker table: require/avoid/prefer/must/must avoid/prefer avoid, Step role (+1 more)

### Community 22 - "Code Cluster 22"
Cohesion: 0.42
Nodes (8): clean_pass_exits_zero(), compiler_emitted_output_passes_validation(), glyph_bin(), minimal_md(), missing_file_exits_three(), run_validate_output(), violations_exit_one_json(), violations_exit_one_pretty()

### Community 23 - "Code Cluster 23"
Cohesion: 0.25
Nodes (8): DSPy (programmatic prompt optimization), Glyph's unique differentiators (external DSL, agent instructions), Handlebars templating, Jinja2 prompt templating, Template systems lack semantics, types, constraints, Microsoft Agent Framework (SK+AutoGen convergence), AutoGen conversable agent conversations, Semantic Kernel plugins (annotated code)

### Community 24 - "Code Cluster 24"
Cohesion: 0.29
Nodes (4): line_index_basic(), LineIndex, Span, Spanned

### Community 25 - "Code Cluster 25"
Cohesion: 0.52
Nodes (6): ac1_directory_compile_all_files(), ac2_topological_order(), ac3_failure_skips_dependent(), ac4_stale_md_untouched_with_note(), ac5_exit_1_partial_output(), glyph_bin()

### Community 26 - "Code Cluster 26"
Cohesion: 0.33
Nodes (6): LangGraph framework (stateful graph state machine), LangGraph limitations vs Glyph (not a language, no compilation), LangGraph reducer-driven state schema, DSPy typed signatures as parameter contracts, Named variable SSA-like data flow, LangGraph Studio (closest analogue IDE)

### Community 27 - "Code Cluster 27"
Cohesion: 0.4
Nodes (5): Closed OpKind catalog (LLM safety), 5-pass pipeline: Parse, Analyze, Transform, Expand, Validate, Eight IR invariants every pass preserves, PlanCompiler (registry-constrained generation), SatLM (LLM + SMT decoupled verification)

### Community 28 - "Code Cluster 28"
Cohesion: 0.7
Nodes (4): ac1_export_text_only_library_cli(), ac3_closure_violation_cli(), ac4_no_exports_in_library_cli(), glyph_bin()

### Community 29 - "Code Cluster 29"
Cohesion: 0.5
Nodes (4): Stay an external DSL with canonical formatter, if/for_each/predicates as first-class AST nodes (not strings), No host-language escape hatch, Starlark hermeticity discipline

### Community 30 - "Code Cluster 30"
Cohesion: 0.5
Nodes (4): Constraints local to skill/block (anti-Drools), Inform 7 (NL-shaped programming reference), Constraints as named predicate references, not rule bodies, Nix-overlay-inspired skill extension syntax

### Community 31 - "Code Cluster 31"
Cohesion: 0.67
Nodes (3): Keyword arguments for 3+ argument calls, Reject retry/timeout as language primitives, Keep primitive count small (~8)

### Community 32 - "Code Cluster 32"
Cohesion: 1.0
Nodes (2): Finding: no existing project combines custom DSL + skill abstractions + compiler + constraints + NL output, Log 2026-04-20: competitive landscape

### Community 33 - "Code Cluster 33"
Cohesion: 1.0
Nodes (2): Finding: preliminary stack — code-first + Mermaid/D2 + React Flow + Dagre/ELK + Compiler Explorer pattern, Log 2026-04-20: visualization approaches survey

### Community 34 - "Code Cluster 34"
Cohesion: 1.0
Nodes (2): Cytoscape.js, vis-network

### Community 35 - "Code Cluster 35"
Cohesion: 1.0
Nodes (2): Dagster Pydantic config with defaults, Implicit system-provided context

### Community 36 - "Code Cluster 36"
Cohesion: 1.0
Nodes (2): PDL (IBM YAML prompt declaration), Plang (string-first NL affinity)

### Community 37 - "Code Cluster 37"
Cohesion: 1.0
Nodes (2): Temporal struct-based params with ctx object, Temporal stateful function IR

### Community 38 - "Code Cluster 38"
Cohesion: 1.0
Nodes (1): Spanned<T>

### Community 39 - "Code Cluster 39"
Cohesion: 1.0
Nodes (1): Log 2026-04-20: existing agent/LLM systems survey

### Community 40 - "Code Cluster 40"
Cohesion: 1.0
Nodes (1): Log 2026-04-20: lessons-from-existing-languages + IR design proposal

### Community 41 - "Code Cluster 41"
Cohesion: 1.0
Nodes (1): Log 2026-04-21: reorganised under Athena layout

### Community 42 - "Code Cluster 42"
Cohesion: 1.0
Nodes (1): Consolidated tier registry (currently empty)

## Knowledge Gaps
- **251 isolated node(s):** `Trust Tiers (unconfirmed -> confirmed -> consolidated -> design)`, `Key Properties (human-readable, skill-oriented, separate authoring/execution, visualizable, small syntax, hybrid compilation, modular, reliability-first)`, `Safety Sandwich pattern: deterministic passes bound the LLM expansion`, `Differentiation from DSPy, LangGraph, Jinja, LMQL, CrewAI`, `Novice Learnability By Inspection` (+246 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **Thin community `Code Cluster 32`** (2 nodes): `Finding: no existing project combines custom DSL + skill abstractions + compiler + constraints + NL output`, `Log 2026-04-20: competitive landscape`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Code Cluster 33`** (2 nodes): `Finding: preliminary stack — code-first + Mermaid/D2 + React Flow + Dagre/ELK + Compiler Explorer pattern`, `Log 2026-04-20: visualization approaches survey`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Code Cluster 34`** (2 nodes): `Cytoscape.js`, `vis-network`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Code Cluster 35`** (2 nodes): `Dagster Pydantic config with defaults`, `Implicit system-provided context`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Code Cluster 36`** (2 nodes): `PDL (IBM YAML prompt declaration)`, `Plang (string-first NL affinity)`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Code Cluster 37`** (2 nodes): `Temporal struct-based params with ctx object`, `Temporal stateful function IR`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Code Cluster 38`** (2 nodes): `Spanned<T>`, `.new()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Code Cluster 39`** (1 nodes): `Log 2026-04-20: existing agent/LLM systems survey`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Code Cluster 40`** (1 nodes): `Log 2026-04-20: lessons-from-existing-languages + IR design proposal`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Code Cluster 41`** (1 nodes): `Log 2026-04-21: reorganised under Athena layout`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Code Cluster 42`** (1 nodes): `Consolidated tier registry (currently empty)`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `compile_directory()` connect `Code Cluster 8` to `Code Cluster 0`, `Code Cluster 3`?**
  _High betweenness centrality (0.031) - this node is a cross-community bridge._
- **Why does `validate_output()` connect `Code Cluster 2` to `Code Cluster 6`?**
  _High betweenness centrality (0.027) - this node is a cross-community bridge._
- **Why does `Skill` connect `Code Cluster 7` to `Code Cluster 11`, `Code Cluster 3`, `Code Cluster 14`?**
  _High betweenness centrality (0.016) - this node is a cross-community bridge._
- **Are the 2 inferred relationships involving `validate_output()` (e.g. with `.get()` and `run_validate_output()`) actually correct?**
  _`validate_output()` has 2 INFERRED edges - model-reasoned connections that need verification._
- **Are the 2 inferred relationships involving `compile_directory()` (e.g. with `.expect()` and `.new()`) actually correct?**
  _`compile_directory()` has 2 INFERRED edges - model-reasoned connections that need verification._
- **What connects `Trust Tiers (unconfirmed -> confirmed -> consolidated -> design)`, `Key Properties (human-readable, skill-oriented, separate authoring/execution, visualizable, small syntax, hybrid compilation, modular, reliability-first)`, `Safety Sandwich pattern: deterministic passes bound the LLM expansion` to the rest of the system?**
  _251 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Code Cluster 0` be split into smaller, more focused modules?**
  _Cohesion score 0.04 - nodes in this community are weakly interconnected._