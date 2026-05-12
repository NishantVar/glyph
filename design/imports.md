# Imports

How cross-file names bind at compile time: path resolution, importable declarations, collision handling, effect propagation, and auto-fix behaviors. Import syntax is defined in `language-surface.md`; this document owns the semantics.

MVP Tier 3. Builds on import syntax from `language-surface.md` (Tier 1) and qualified callee resolution from `data-flow.md` (Tier 2).

## 1. Path Resolution

Import paths are relative only in the MVP (foundations: MVP imports are local-path). No absolute paths, no bare module names. **The base directory for resolution is always the importing file's directory — never the process current working directory (CWD).** Both `"./sibling.glyph"` and `"../shared/lib.glyph"` resolve from there. Parent-directory traversal (`../`) is allowed.

Concretely, `glyph compile foo.glyph` and `cd subdir && glyph compile ../foo.glyph` resolve `foo.glyph`'s imports identically: in both invocations, an `import "./bar.glyph"` inside `foo.glyph` looks up `bar.glyph` in the same directory as `foo.glyph`. CWD is irrelevant to import resolution. This makes builds reproducible across different shell working directories and matches how other tools (e.g., Rust's `mod` declarations, Node's `require("./...")`) treat relative imports.

### `@glyph/` Reserved Virtual Namespace

Paths beginning with `@glyph/` are not filesystem paths. The `@glyph/` prefix is reserved for compiler-shipped modules and bypasses filesystem resolution entirely — the compiler resolves these in-memory. The MVP recognises exactly one such module: `@glyph/std` (see `stdlib.md` §Distribution and Resolution). Any other `@glyph/*` path fires `G::imports::unknown-stdlib-module` (error). A real on-disk file or directory named `@glyph` is never consulted, even if one exists in the importing file's directory tree.

### Extension Auto-Resolution

The compiler tries the path as written first. If the path does not end in `.glyph` and no file exists at the literal path, the compiler appends `.glyph` and tries again. If neither resolves, the compiler emits a compile error listing the paths tried.

### Missing File

A resolved path that does not point to an existing `.glyph` file is a compile error. The diagnostic includes the fully resolved absolute path.

## 2. What Is Importable

Only explicitly exported declarations are importable (foundations: only exports are importable).

| Declaration | Importable? | Access via |
|---|---|---|
| `export block` | Yes | Selective import or `M.name` call |
| `export const` | Yes | Selective import or `M.name` reference |
| `export type` | Yes | Selective import only (`{ Name }`); whole-module qualified type refs deferred |
| `block` | No -- private | Compile error if named in a selective import |
| `const` | No -- private | Compile error if named in a selective import |
| `type` (non-exported) | No -- private | Compile error if named in a selective import |
| `generated const` | No -- private | Compile error if named in a selective import |
| `skill` | Special | Accessible only via `M.skill_name` on whole-module imports |

### Skill Accessibility

The `skill` entrypoint of an imported module is accessible through whole-module imports so one skill can reference another module's compiled behavior. It cannot be selectively imported -- `import "path" { some_skill }` is a compile error. Skills are compiled units, not reusable building blocks.

### Private Declarations

Attempting to selectively import a private `block` or `const` is a compile error. The diagnostic names the declaration and notes that it is not exported.

### Library Files As Import Targets

Library files (zero `skill` declarations) are the primary import targets. They export reusable blocks and constants consumed by skill files. Importing from a library file follows all the same rules as importing from a skill file — selective imports, whole-module imports, collision handling, and effect propagation all apply identically. The only difference: a library file has no `skill` entrypoint, so `M.skill_name` is not available on whole-module imports of a library. Attempting to access a skill entrypoint on a library's whole-module alias is a compile error.

Library files must have at least one `export` declaration (`G::analyze::no-exports-in-library`). See `language-surface.md` §File-Level Rules for the full library compilation and emission model.

`export type` decls count as library exports, parallel to `export const` and `export block` (see `types.md`, Explicit `type` Declarations section). A file containing only `export type` decls satisfies the library-export rule and compiles cleanly with no `## Parameters` or `### Steps` body — type decls are compile-time only and emit no Markdown.

### Selective-Only Imports for Types

Types are imported **selectively only** in MVP:

```glyph
import "./types.glyph" { RepoContext, Diagnosis }
```

Whole-module qualified type references — e.g., `types.RepoContext` after `import "./types.glyph" as types` — are deferred. Type slots in MVP accept bare identifiers only; qualified type refs would require new TypeRef grammar and canonical-identity rules. Authors who want to expose types to consumers must use selective import.

A whole-module import of a file that contains `export type` decls remains valid for any `export block` or `export const` it also defines; the type names are simply not reachable through the alias.

## 3. Name Collision Rules

Imported names participate in the per-namespace no-shadowing rule defined in `values-and-names.md`. That rule is the single authoritative source for collision semantics across all name sources (locals, parameters, const declarations, imports). Key import-specific points:

**Selective imports.** Each imported name (or its `as` alias) enters one of the importing file's two namespaces (type or value) based on the imported declaration's kind. If it collides with any other visible name in the **same namespace** after case normalization, the compiler emits a collision error. The fix is to alias on the import side or rename the local declaration. Cross-namespace canonical-equal pairs (e.g., importing `type Mode` while a local `block mode_name()` exists) do not collide.

**Whole-module imports.** `import "path" as M` reserves `M` as a single identifier in the **value namespace**. Members are accessed only via qualified names (`M.name`) and do not enter either namespace as bare identifiers. Two whole-module imports (`as M` and `as N`) may expose identically-named exports without collision because `M.foo` and `N.foo` are distinct.

`M` itself is not callable -- `M()` is invalid. It is a namespace, not a value.

### `ResolvedImportKind`

Internally, every resolved selective import alias carries a `ResolvedImportKind` tag with two variants — `Type` and `Value` — that determines which namespace it enters:

- A selective alias inherits its kind from the imported declaration: `dep_exports.types` → `Type`; `dep_exports.blocks` and `dep_exports.texts` → `Value`. So `import "./types.glyph" { Foo }` adds `Foo` to the type namespace, and `import "./lib.glyph" { run_check }` adds `run_check` to the value namespace.
- **Whole-module aliases** (filesystem `import "path" as M` and stdlib `import "@glyph/std" as std`) are always `Value` — the alias names a module handle in the value namespace, even when the module also exports `type` decls.
- **Whole-module qualified type references** (e.g., `param: M.Foo` after `import "./types.glyph" as M`) remain out of MVP scope; type slots accept bare identifiers only. See `types.md` Deferred section.

Two-namespace collision detection runs over the joined set of local declarations and selectively-imported aliases, partitioned by `ResolvedImportKind` and local-decl kind, so the import pipeline and the local sweep share a single mechanism.

## 4. Re-Export Policy

**No re-export in MVP.** If file A imports `export block validate` from file B, file A cannot make `validate` importable to file C. File C must import directly from file B.

An `export block` in file A may call an imported block from file B -- that is normal composition. What is disallowed is re-exporting B's declaration as if A defined it.

Rationale: re-export creates transitive dependency chains that complicate cycle detection and provenance tracking; the MVP closure model requires that an `export block`'s behavior is determined by its own file's declarations.

## 5. Cycle Handling

Circular imports are a compile error. The compiler builds a dependency DAG during import resolution. A back-edge produces a diagnostic naming the full cycle:

```
circular import: A.glyph -> B.glyph -> A.glyph
```

No lazy-loading or forward-declaration workaround in MVP. If a cycle exists, the author refactors shared content into a third file.

### Transitive Imports

A library may import another library, which may import another, and so on. There is **no depth limit** on the import chain. The DAG closure that the compiler builds during multi-file resolution (`pipeline.md` §Multi-File Compilation Order) naturally walks every reachable file, so a chain like `consumer.glyph` → `lib_a.glyph` → `lib_b.glyph` → `lib_c.glyph` resolves identically to a single direct import: each file is parsed once and topologically ordered before its consumers.

Cycle detection (`G::analyze::circular-import`) is the only depth-related constraint. Any acyclic chain — regardless of length — is permitted. Re-export of a transitive name is still forbidden by §4: even when `lib_a` imports a name from `lib_b`, the consumer cannot reach that name through `lib_a` and must import directly from `lib_b`.

## 6. Duplicate Imports

### Same File Imported Twice

Two `import` statements referencing the same resolved path are auto-fixed by merging into one statement. If one is whole-module and the other selective, the compiler merges into the whole-module form. This is a source-to-source fix.

### Same Name From Two Different Files

If two selective imports from different files introduce the same normalized name, the no-shadowing rule applies -- compile error. The author must alias one of them.

## 7. Unused Import Auto-Removal

The compiler auto-removes unused imports from the `.glyph` source file (see also `compiled-output.md`).

**Selective imports:** Each imported name is tracked individually. If `a` is used but `b` is not, the compiler removes `b` from the `{ a, b }` list. If all names are unused, the entire statement is removed.

**Whole-module imports:** If no qualified reference `M.x` appears anywhere in the file, the entire `import "..." as M` statement is removed.

This is a source-to-source fix, not a silent omission from compiled output. It runs in Phase 3a (deterministic source rewrites) of the compiler pipeline — see `pipeline.md`.

## 8. Effect Propagation

Imported `export block` declarations carry their full inferred effect set in the IR (`ir-and-semantics.md`).

When a caller invokes an imported block, the callee's effect set is unioned into the caller's inferred effects:

- If `export block validate` declares `effects: reads_files, runs_commands`, and skill `fix_bug` calls it, then `fix_bug`'s inferred effects include at minimum `{reads_files, runs_commands}`.
- If the caller explicitly declares `effects:`, the declared set must be a superset of the inferred set -- otherwise compile error.
- The compiled output for the caller includes the full unioned effect set.

No import-specific effect syntax is needed. The existing `effects:` clause and inference machinery handle propagation.

**Projection tier does not affect propagation.** Whether an imported block is projected as inline (Tier 1), same-file procedure (Tier 2), or external file (Tier 3) is a Phase 6 layout decision that does not change the callee's effect contribution resolved in Phases 2/5. Tier 3 selection may require the caller to additionally declare `reads_files` — see `ir-and-semantics.md` §Projection Tier And Effect Propagation for the full mechanism.

## 9. Closure Enforcement Timing

Closure is checked at the exporter's compile time (full rules in `data-flow.md`). The importer trusts the export contract. At import time, the compiler checks only:

- The referenced name exists and is exported.
- Types match at call boundaries (nominal matching per `types.md`).
- Effects propagate correctly via union.

The importer does not re-check internal closure of the imported block. This keeps compilation modular -- analyzing a file does not require re-analyzing every transitive dependency's internals.

## Cross-References

- **Declaration headers** (`language-surface.md`): import syntax grammar.
- **Values and names** (`values-and-names.md`): universal no-shadowing rule and case normalization.
- **Calls and args** (`data-flow.md`): qualified callee resolution (`M.name`).
- **Effects** (`ir-and-semantics.md`): effect inference and union propagation.
- **Data flow** (`data-flow.md`): exported block closure requirements.
- **Compiled output** (`compiled-output.md`): import inlining and unused-import auto-removal.

## Deferred

- Package-style, registry-backed, or versioned imports (foundations: MVP imports are local-path).
- Re-export / barrel-file syntax.
- Cycle-breaking mechanisms (interface-only imports, forward declarations, lazy resolution).
- Selective import glob or wildcard patterns.
- Deep qualified access (`a.b.c`) for nested module structures.
- Whole-module qualified type references (`alias.TypeName` after `import "./types.glyph" as alias`). See `types.md` Deferred section.
