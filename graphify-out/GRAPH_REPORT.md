# Graph Report - glyph  (2026-05-03)

## Corpus Check
- 34 files · ~623,683 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 994 nodes · 2448 edges · 37 communities detected
- Extraction: 68% EXTRACTED · 32% INFERRED · 0% AMBIGUOUS · INFERRED: 778 edges (avg confidence: 0.8)
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
- [[_COMMUNITY_Community 34|Community 34]]
- [[_COMMUNITY_Community 35|Community 35]]
- [[_COMMUNITY_Community 36|Community 36]]
- [[_COMMUNITY_Community 37|Community 37]]

## God Nodes (most connected - your core abstractions)
1. `validate_output()` - 51 edges
2. `check_source()` - 36 edges
3. `compile_directory()` - 28 edges
4. `setup_tempdir()` - 27 edges
5. `Glyph Foundations reference card` - 27 edges
6. `parse()` - 26 edges
7. `Parser<'a>` - 21 edges
8. `Glyph Language Surface (syntax)` - 19 edges
9. `compile_source_with_effects()` - 18 edges
10. `compile_directory_with_options()` - 18 edges

## Surprising Connections (you probably didn't know these)
- `5-pass hybrid compiler (Parse, Analyze, Transform, Expand[LLM], Validate)` --semantically_similar_to--> `Source-to-IR 9-step pipeline (Parse, Diagnose, Repair, Re-parse, Resolve, Infer, Normalize, Type, Validate)`  [INFERRED] [semantically similar]
  README.md → design/language-surface.md
- `compile_source_with_effects()` --calls--> `parse()`  [INFERRED]
  crates/glyph-core/src/lib.rs → skills/issue-list-orchestrator/scripts/parse_issues.py
- `compile_source_with_resolved_imports()` --calls--> `parse()`  [INFERRED]
  crates/glyph-core/src/lib.rs → skills/issue-list-orchestrator/scripts/parse_issues.py
- `main()` --calls--> `parse()`  [INFERRED]
  crates/glyph-cli/src/main.rs → skills/issue-list-orchestrator/scripts/parse_issues.py
- `Glyph Language Surface (syntax)` --implements--> `tmp/fix_bug.glyph.md source example`  [INFERRED]
  design/language-surface.md → tmp/fix_bug.glyph.md

## Communities

### Community 0 - "Community 0"
Cohesion: 0.04
Nodes (130): analyze(), ac1_directory_compile_processes_every_file(), ac1_export_text_only_library_check_source_clean(), ac1_export_text_only_library_compiles_exit_zero(), ac2_repo_tools_library_compiles_with_large_export_block(), ac2_topological_order_libraries_before_consumers(), ac3_closure_violation_on_private_free_variable(), ac3_failure_skips_dependent_with_warning() (+122 more)

### Community 1 - "Community 1"
Cohesion: 0.02
Nodes (128): Athena tiered research wiki, Glyph Project Index, Promotion path to ./design/, Research Question: human-readable visualizable DSL, Tier: confirmed, Tier: consolidated, Tier: unconfirmed, Trust Tiers (unconfirmed -> confirmed -> consolidated -> design) (+120 more)

### Community 2 - "Community 2"
Cohesion: 0.06
Nodes (74): analyze_export_block(), analyze_skill(), analyze_skill_with_usage_tracking(), analyze_with_diagnostics(), analyze_with_diagnostics_receives_enable_effects(), analyze_with_imports(), check_applies_in_condition(), check_branch_body_names() (+66 more)

### Community 3 - "Community 3"
Cohesion: 0.05
Nodes (94): directory_mode_cleans_stale_tmp(), failed_compile_leaves_no_tmp_and_no_md(), glyph_bin(), prior_md_survives_failed_rebuild(), procedure_files_stale_tmp_cleaned(), stale_tmp_cleaned_on_rebuild(), applies_no_parens_corpus_fires_diagnostic(), applies_on_non_block_corpus_fires_diagnostic() (+86 more)

### Community 4 - "Community 4"
Cohesion: 0.05
Nodes (92): check_applies_descriptions(), check_applies_descriptions_in_flow(), check_branch_substeps(), check_constraint_count(), check_content_shape(), check_context_count(), check_frontmatter(), check_malformed_markdown() (+84 more)

### Community 5 - "Community 5"
Cohesion: 0.04
Nodes (71): brainstorming: anti_patterns block, Awkward: 'offer MUST be its own message' has no primitive, Awkward: conversational style constraints vs control-flow, Example: brainstorming skill Glyph rewrite, Open Question: are skill-to-skill handoffs first-class?, brainstorming: 7 phases (orient, visual companion, intent, approaches, design, write/review, handoff), brainstorming: terminal_skill = writing-plans, brainstorming: trigger block (must_invoke_before creative_work) (+63 more)

### Community 6 - "Community 6"
Cohesion: 0.04
Nodes (58): Glyph (Agent Skill DSL), Glyph block concept (reusable sub-component), Glyph compiler pipeline, Glyph constraints: block, Agent Skill DSL Research Index, Agent Skills Standard (SKILL.md), GitHub Agentic Workflows (gh-aw), Competitive Landscape (22 projects) (+50 more)

### Community 7 - "Community 7"
Cohesion: 0.09
Nodes (32): Skill, IrArena, IrBlock, IrBranch, IrCall, IrConstraint, IrContext, IrElifBranch (+24 more)

### Community 8 - "Community 8"
Cohesion: 0.18
Nodes (30): compile_directory(), compile_directory_with_ir(), corpus_dir(), fix_bug_constraints(), fix_bug_frontmatter(), fix_bug_has_applies_conditional_imported(), fix_bug_has_applies_conditional_same_file(), fix_bug_has_context_section() (+22 more)

### Community 9 - "Community 9"
Cohesion: 0.21
Nodes (21): compile_and_read_ir(), emit_ir_call_carries_callee_context_null_when_inline(), emit_ir_conforms_to_schema_full_skill(), emit_ir_context_node_serializes_correctly(), emit_ir_includes_applies_descriptions_on_branch(), emit_ir_includes_applies_descriptions_with_applies_calls(), emit_ir_includes_description_on_block_in_call(), emit_ir_includes_local_refs_on_resolved_call() (+13 more)

### Community 10 - "Community 10"
Cohesion: 0.26
Nodes (18): arena_with_effects(), find_block_by_name(), node_id_str(), polarity_str(), projection_mode_str(), role_str(), serialize_branch(), serialize_call() (+10 more)

### Community 11 - "Community 11"
Cohesion: 0.12
Nodes (19): Constraints as first-class IR nodes, Glyph IR: region-structured SSA-typed op tree with constraint overlay, LLVM (SSA, CFG, DCE), MLIR (regions-of-regions, dialects), Regions + SSA sweet spot, Swift SIL (region-structured SSA), Behavior trees decorator-attachment pattern, Gherkin-style temporal scoping for constraints (before/after) (+11 more)

### Community 12 - "Community 12"
Cohesion: 0.2
Nodes (16): arena_with_effects(), avoid_phrasing(), avoid_phrasing_walking_skeleton(), emit_applies_arm(), emit_branch(), emit_includes_effects_when_enabled(), emit_lettered_substeps(), emit_procedure() (+8 more)

### Community 13 - "Community 13"
Cohesion: 0.12
Nodes (15): BlockDecl, ConstraintMarker, ConstraintMarkerKind, ContextEntry, Decl, ElifBranch, ExportBlockDecl, FlowStmt (+7 more)

### Community 14 - "Community 14"
Cohesion: 0.33
Nodes (15): fmt_check_exits_0_when_already_formatted(), fmt_check_exits_1_when_changes_needed(), fmt_corpus_path(), fmt_hoists_body_and_flow_context_into_context_section(), fmt_hoists_body_constraints_into_constraints_section(), fmt_is_idempotent(), fmt_preserves_branch_scoped_markers(), fmt_reorders_sections_to_canonical_layout() (+7 more)

### Community 15 - "Community 15"
Cohesion: 0.39
Nodes (11): ac1_cross_file_resolution(), ac2_circular_import_path(), ac3_import_private(), ac4_import_skill(), ac5_duplicate_import_exit_2(), ac5_unused_import_exit_2(), assert_contains_diagnostic_id(), fixture() (+3 more)

### Community 16 - "Community 16"
Cohesion: 0.27
Nodes (7): first_slot_offset(), is_ident_continue(), is_ident_start(), multiple_slots(), scan_slots(), single_slot(), SlotMatch

### Community 17 - "Community 17"
Cohesion: 0.2
Nodes (10): Airflow tab-based multi-view, Compiler Explorer / Godbolt (source-to-output mapping), DSPy compiled prompts visibility, Source attribution / line mapping in compiled view, Stately/XState bidirectional sync, Structurizr DSL (model-first multi-view), Three views pattern for Glyph (code, graph, compiled), n8n Vue Flow canvas with mapping layer (+2 more)

### Community 18 - "Community 18"
Cohesion: 0.22
Nodes (9): Constraint role with strength x polarity, Context role, InputContract role, OutputContract role, Rationale: input-first role taxonomy, one Constraint role, effects stay separate, Closed role set: InputContract, Step, Constraint, Context, OutputContract, Source marker table: require/avoid/prefer/must/must avoid/prefer avoid, Step role (+1 more)

### Community 19 - "Community 19"
Cohesion: 0.29
Nodes (4): line_index_basic(), LineIndex, Span, Spanned

### Community 20 - "Community 20"
Cohesion: 0.25
Nodes (8): DSPy (programmatic prompt optimization), Glyph's unique differentiators (external DSL, agent instructions), Handlebars templating, Jinja2 prompt templating, Template systems lack semantics, types, constraints, Microsoft Agent Framework (SK+AutoGen convergence), AutoGen conversable agent conversations, Semantic Kernel plugins (annotated code)

### Community 21 - "Community 21"
Cohesion: 0.33
Nodes (6): LangGraph framework (stateful graph state machine), LangGraph limitations vs Glyph (not a language, no compilation), LangGraph reducer-driven state schema, DSPy typed signatures as parameter contracts, Named variable SSA-like data flow, LangGraph Studio (closest analogue IDE)

### Community 22 - "Community 22"
Cohesion: 0.4
Nodes (5): Closed OpKind catalog (LLM safety), 5-pass pipeline: Parse, Analyze, Transform, Expand, Validate, Eight IR invariants every pass preserves, PlanCompiler (registry-constrained generation), SatLM (LLM + SMT decoupled verification)

### Community 23 - "Community 23"
Cohesion: 0.5
Nodes (4): Stay an external DSL with canonical formatter, if/for_each/predicates as first-class AST nodes (not strings), No host-language escape hatch, Starlark hermeticity discipline

### Community 24 - "Community 24"
Cohesion: 0.5
Nodes (4): Constraints local to skill/block (anti-Drools), Inform 7 (NL-shaped programming reference), Constraints as named predicate references, not rule bodies, Nix-overlay-inspired skill extension syntax

### Community 25 - "Community 25"
Cohesion: 0.67
Nodes (3): Keyword arguments for 3+ argument calls, Reject retry/timeout as language primitives, Keep primitive count small (~8)

### Community 26 - "Community 26"
Cohesion: 1.0
Nodes (1): Spanned<T>

### Community 27 - "Community 27"
Cohesion: 1.0
Nodes (2): Finding: no existing project combines custom DSL + skill abstractions + compiler + constraints + NL output, Log 2026-04-20: competitive landscape

### Community 28 - "Community 28"
Cohesion: 1.0
Nodes (2): Finding: preliminary stack — code-first + Mermaid/D2 + React Flow + Dagre/ELK + Compiler Explorer pattern, Log 2026-04-20: visualization approaches survey

### Community 29 - "Community 29"
Cohesion: 1.0
Nodes (2): Cytoscape.js, vis-network

### Community 30 - "Community 30"
Cohesion: 1.0
Nodes (2): Dagster Pydantic config with defaults, Implicit system-provided context

### Community 31 - "Community 31"
Cohesion: 1.0
Nodes (2): PDL (IBM YAML prompt declaration), Plang (string-first NL affinity)

### Community 32 - "Community 32"
Cohesion: 1.0
Nodes (2): Temporal struct-based params with ctx object, Temporal stateful function IR

### Community 34 - "Community 34"
Cohesion: 1.0
Nodes (1): Log 2026-04-20: existing agent/LLM systems survey

### Community 35 - "Community 35"
Cohesion: 1.0
Nodes (1): Log 2026-04-20: lessons-from-existing-languages + IR design proposal

### Community 36 - "Community 36"
Cohesion: 1.0
Nodes (1): Log 2026-04-21: reorganised under Athena layout

### Community 37 - "Community 37"
Cohesion: 1.0
Nodes (1): Consolidated tier registry (currently empty)

## Knowledge Gaps
- **251 isolated node(s):** `SlotMatch`, `IrNode`, `IrSkill`, `IrParam`, `IrInlineInstruction` (+246 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **Thin community `Community 26`** (2 nodes): `Spanned<T>`, `.new()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 27`** (2 nodes): `Finding: no existing project combines custom DSL + skill abstractions + compiler + constraints + NL output`, `Log 2026-04-20: competitive landscape`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 28`** (2 nodes): `Finding: preliminary stack — code-first + Mermaid/D2 + React Flow + Dagre/ELK + Compiler Explorer pattern`, `Log 2026-04-20: visualization approaches survey`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 29`** (2 nodes): `Cytoscape.js`, `vis-network`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 30`** (2 nodes): `Dagster Pydantic config with defaults`, `Implicit system-provided context`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 31`** (2 nodes): `PDL (IBM YAML prompt declaration)`, `Plang (string-first NL affinity)`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 32`** (2 nodes): `Temporal struct-based params with ctx object`, `Temporal stateful function IR`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 34`** (1 nodes): `Log 2026-04-20: existing agent/LLM systems survey`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 35`** (1 nodes): `Log 2026-04-20: lessons-from-existing-languages + IR design proposal`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 36`** (1 nodes): `Log 2026-04-21: reorganised under Athena layout`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Community 37`** (1 nodes): `Consolidated tier registry (currently empty)`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `compile_directory()` connect `Community 8` to `Community 2`, `Community 3`?**
  _High betweenness centrality (0.031) - this node is a cross-community bridge._
- **Why does `validate_output()` connect `Community 4` to `Community 2`?**
  _High betweenness centrality (0.027) - this node is a cross-community bridge._
- **Why does `Skill` connect `Community 7` to `Community 2`, `Community 10`, `Community 12`, `Community 13`?**
  _High betweenness centrality (0.016) - this node is a cross-community bridge._
- **Are the 2 inferred relationships involving `validate_output()` (e.g. with `.get()` and `run_validate_output()`) actually correct?**
  _`validate_output()` has 2 INFERRED edges - model-reasoned connections that need verification._
- **Are the 2 inferred relationships involving `compile_directory()` (e.g. with `.expect()` and `.new()`) actually correct?**
  _`compile_directory()` has 2 INFERRED edges - model-reasoned connections that need verification._
- **What connects `SlotMatch`, `IrNode`, `IrSkill` to the rest of the system?**
  _251 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Community 0` be split into smaller, more focused modules?**
  _Cohesion score 0.04 - nodes in this community are weakly interconnected._