# Repair — Open Questions And Deferred Work

Backlog for the Repair pass. Tracked here rather than as GitHub issues per the design-folder pruning convention.

## Deferred Features

### Cross-file repair

MVP: Repair only edits the current file. If a diagnostic requires changes to another file (e.g., an imported block is not exported, or a stdlib-non-resident import is missing), Repair emits a non-repairable diagnostic.

Post-MVP work:

- Cross-file editing of imported `.glyph` files when diagnostics require it.
- Auto-import discovery for non-stdlib libraries (adding `import` lines to files the author did not reference).

Source: [[design/repair]] §6 (repair/error boundary), [[docs/architecture/repair]] §3.2, §7.

### Constraint canonical-form rewrite

Future Repair extension: rewrite non-canonical-form `avoid:` / `require:` / `must:` / `must avoid:` text into the canonical form expected by the locked four-form template ([[docs/reference/compiled-output]] §Constraint Rendering, [[GLYPH_LANGUAGE_GUIDE]] §7.2). Canonical form: lowercase first word, no trailing period, noun-phrase or imperative-clause shape.

Out of scope for the current emitter work.

### Type description coherence check

Future Repair extension. When a compilation unit contains both a `type Foo = <"...">` declaration and any downstream usage of `Foo` (typed parameter, typed default, or typed return), the type-level description is the source of truth. Per-slot overrides are expected to *specialize* that anchor, not *contradict* it.

Diagnostic IDs to register:

| ID | Trigger | LLM judges |
|---|---|---|
| `G::repair::type-description-conflict` | `type Foo = <"X">` exists AND a param has `: Foo = <"Y">` | Does `Y` specialize `X`, or does it contradict it? |
| `G::repair::default-violates-type-description` | `type Foo = <"X">` exists AND a param has `: Foo = literal` | Does the literal value satisfy `X`? E.g., `type RiskLevel = <"one of: low, medium, high">` + `risk: RiskLevel = "extreme"` is flagged. |
| `G::repair::return-description-conflict` | `type Foo = <"X">` exists AND a `-> Foo` block has `return <"Y">` | Does `Y` describe a value consistent with `X`? Same specialization-vs-contradiction posture. |

**Tier.** All three are `repairable` warnings. The LLM proposes a fix — rewrite the override to be a clean specialization, change the default, or rewrite the type-level description if the type drifted — and the author can accept or hand-edit. Once accepted, the check is idempotent: the same pairing is not re-flagged on the next compile.

**Scoped to anchored types.** The check only runs when a `type Foo` declaration exists in the same compilation unit. Without an anchor, multiple per-slot descriptions of the same nominal type are independent — there is no source of truth to compare against, and Repair leaves them alone. This keeps the check from firing on legitimate ad-hoc per-param refinements.

**Author escape hatch.** A line comment `// glyph-allow: type-description-conflict` placed on the param's line suppresses the check for that slot. The same suppression token form (`// glyph-allow: <short-id>`) covers the default and return variants — `// glyph-allow: default-violates-type-description` and `// glyph-allow: return-description-conflict` respectively. Use sparingly, for genuine intentional divergences where the override deliberately departs from the anchor's description.

**Why warning, not error.** Picking which side of a tension wins is a semantic judgment the author should make. The coherence check surfaces the conflict and proposes a rewrite, but does not silently drop or rewrite either side without confirmation.

## Open Questions

- **Diagnostic taxonomy.** The diagnostic shape and classification tiers are defined in [[docs/reference/diagnostics]]. The full catalog of individual diagnostics will be built out as the compiler is implemented.
- **Security and trust.** Prevent Repair from adding imports, effects, exports, or generated const values that broaden behavior beyond the author's apparent intent.
- **Generation limits.** Whether the compiler should limit the number of `generated const` declarations per file.
- **Migration hashing.** Whether `generated const` should carry a compiler-generated hash for migration detection when language rules change. (See [[0015-repair-idempotence-as-name-resolution]] for the current name-resolution-only stance.)
- **Tooling.** IDE highlighting, gutter markers, or quick-fix actions for promoting `generated const` to `const`.

## Placement Rule Enforcement

All generated declarations must appear after all non-generated top-level declarations in the source file. Compiler enforcement of this ordering rule is deferred. Planned analyze-pass diagnostic working name: `G::analyze::generated-placement`. Until that issue lands, the rule is a documented contract that Repair and authors honor manually.

Source: [[design/repair]] §4, [[language-surface]] §3.6 and §3.7.
