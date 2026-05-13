# Glyph — User-Facing Surface Implementation TODOs

Work-tracking items extracted from [[user-facing-todo]] during the 2026-05-13 design-folder pruning. These are implementation gaps and known bugs against language constructs that the design already commits to but the MVP toolchain does not yet support.

## Deferred Parser Support

Items where the language guide / design already commits to a syntax but the MVP parser does not yet accept it. These are implementation gaps, not open design questions.

### Typed parameters

[[GLYPH_LANGUAGE_GUIDE]] §"Type annotations" and [[types]] describe parameter type annotations of the form `name: DomainType` (e.g. `skill implement_feature(scope: PathSpec, risk: RiskLevel = "medium")`). The MVP parser's `parse_param_list` (`crates/glyph-core/src/parse.rs`) only accepts `name [= "default"]` — it stops at the `:`, leaving the parser to fail on the next `expect(Rparen)`.

When the failing source also contains a descriptive return (`return <"…">`), the post-parse `<`-scan formerly masked the real cause by emitting `G::parse::output-target-outside-return` against the unconsumed `<` — pointing the author at the wrong line entirely. The scan is now gated on the parser's failure offset: only `<` tokens at-or-before the failure are reported (which preserves the structured diagnostic for stray-`<` cases like `< bar` at statement start, where the `<` itself *is* the failure cause); tokens past the failure are unreached and suppressed. With typed-param parsing still missing, the parameter-list failure surfaces as a generic `Parse(Eof)` instead of a misdirected output-target diagnostic.

Landing typed parameters requires:

- Extending `parse_param_list` to optionally consume `: <Ident>` between the name and any default.
- Adding `type_annot: Option<Spanned<String>>` to the `Param` AST node (and forwarding through `IrParam` if the IR consumers want it; today `analyze.rs::emit_nominal_mismatch` is already a placeholder waiting for typed annotations to land).
- Regression tests covering `name: Type`, `name: Type = "default"`, and the negative `name:` (missing type ident).

Until then, authors must omit type annotations on parameters even though the language guide treats them as part of MVP.

### Calls and unmarked bare names in `context:` / `constraints:`

The `context:` and `constraints:` sub-section parsers accept bare-name references and inline strings, but bail silently on call shapes (`name()`) and — in `constraints:` — on bare names without a polarity marker. The bail returns `Parse(Eof)` with no AST and no diagnostic. After PR #140 narrowed the post-parse `<`-scan, no fallback diagnostic surfaces either, so the failure is effectively invisible.

Land structured diagnostics for these positions:

- `G::parse::call-in-context-section` — `name()` under `context:`. Hint: `context:` accepts bare const names, inline strings, or `context`-prefixed markers; calls are not legal here.
- `G::parse::call-in-constraints-section` — `name()` under `constraints:`. Hint: `constraints:` accepts marker-prefixed bare names (`require <name>`, `avoid <name>`, `must <name>`, `must avoid <name>`) or inline strings; calls are not legal here.
- `G::parse::bare-name-in-constraints-section` — unmarked bare name under `constraints:`. Hint: prefix with a polarity marker (`require` / `avoid` / `must` / `must avoid`).

Each diagnostic should point at the offending span and its hint should suggest the rewrite the author most likely meant. The fix is parser-local; no AST or analyzer changes.

Until then, authors hitting silent compile failures in these sections should check whether they accidentally wrote a call (`name()`) or an unmarked bare name in `constraints:`.

## Known Author-Visible Emitter Gaps

### Branch-as-last-step drops `return <X>` suffix (P1 from issue #118 codex review)

**Symptom for the author:** A skill or block whose `flow:` ends with a structural step (e.g. `if/elif/else`) followed by `return <X>` compiles with no "as your result" prose anywhere in the rendered Markdown — the author's output target is silently dropped.

**Reproduction:**

```glyph
skill main()
    description: "..."
    flow:
        if condition
            "do A"
        else
            "do B"
        return <current_branch>
```

The compiled `.md` shows the `if … then:` block but never folds `current_branch` into a final visible step. The four locked OC templates (`append_identifier_suffix`, `append_description_suffix`, `standalone_return_identifier`, `standalone_return_description`) never fire.

**Root cause:** `crates/glyph-core/src/emit/scaffold.rs` (the `IrNode::Branch` arm of the main scaffold walker) ignores `is_last` and `skill_oc_form`. The `IrNode::InlineInstruction` arm and the `IrNode::Call` Tier-1 arm both honor these and route through the locked templates; the branch arm does not.

**Recommended fix shape:** When the last visible step is `IrNode::Branch` and `skill_oc_form.is_some()`, emit the branch as today, then append one additional standalone return step using `templates::standalone_return_identifier(name)` or `templates::standalone_return_description(desc)`. This mirrors how the empty-resolved-body Tier-1 path already handles the case where there is no body to fold the suffix into. Folding the suffix into the last sub-step of the last branch arm is the alternative but it's awkward — different arms produce different terminal sub-steps and the suffix would need to fold into each independently; the design doc is silent on this, and the standalone-after-branch shape is the least surprising and most consistent with existing Tier-1 callees.

**Audit candidates** (same gap likely recurs):

- Tier-2 same-file procedure path (`### Procedure:` block, `projection_tier == 2`) when a branch is the final step inside the procedure body.
- Tier-3 external-file path (`emit_procedure` in `emit/mod.rs`) — same question.
- Block-level emission when an `IrNode::Block` is materialized as a procedure — does its emitter also gate on `is_last` for branches?

**Test gap:** No fixture currently has `if/else/return <X>` as the closing pattern, which is why this regression has not been caught. A regression test exercising both `return <Identifier>` and `return <"description">` shapes with a terminal branch should accompany the fix.
