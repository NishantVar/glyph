# Glyph Import Resolution Semantics

This document defines how cross-file names bind at compile time: path resolution, importable declarations, name collision rules, effect propagation, closure enforcement, and auto-fix behaviors.

## Status

Builds on the import syntax fixed in `declaration-headers.md` and the qualified callee resolution in `calls-and-args.md`. This document owns the semantics; the syntax is not reopened here.

## 1. Path Resolution

Import paths are relative only in the MVP (boundary 11, `boundaries.md:37-38`). No absolute paths, no bare module names.

The base directory for resolution is the importing file's directory. Both `"./sibling.glyph.md"` and `"../shared/lib.glyph.md"` resolve from there.

Parent-directory traversal (`../`) is allowed. Teams commonly share libraries in parent directories.

### Extension Auto-Resolution

The compiler tries the path as written first. If the path does not end in `.glyph.md` and no file exists at the literal path, the compiler appends `.glyph.md` and tries again. If neither resolves to an existing file, the compiler emits a compile error listing the paths tried.

This means `import "./repo_tools" as repo_tools` works when `./repo_tools.glyph.md` exists.

### Missing File

A resolved path that does not point to an existing `.glyph.md` file is a compile error. The diagnostic includes the fully resolved absolute path so the author can see exactly what the compiler looked for.

## 2. What Is Importable

Only explicitly exported declarations are importable (boundary 10, `boundaries.md:34-35`).

| Declaration | Importable? | Access via |
|---|---|---|
| `export block` | Yes | Selective import or `M.name` call |
| `export text` | Yes | Selective import or `M.name` reference |
| `block` | No — private | Compile error if named in a selective import |
| `text` | No — private | Compile error if named in a selective import |
| `skill` | Special | Accessible only via `M.skill_name` on whole-module imports |

### Skill Accessibility

The `skill` entrypoint of an imported module is accessible through whole-module imports (`authoring-surface.md:252`) so one skill can reference another module's compiled behavior. It cannot be selectively imported — `import "path" { some_skill }` is a compile error for the skill declaration. Skills are compiled units, not reusable building blocks. `export block` is the sole reusable-block import mechanism.

### Private Declarations

Attempting to selectively import a private `block` or private `text` is a compile error. The diagnostic should name the declaration and note that it is not exported.

## 3. Selective Import Collision Rules

Each selectively imported name (or its `as` alias) enters the importing file's flat namespace. The no-shadowing rule from `values-and-literals.md:139-149` applies uniformly:

- If an imported name collides with a same-file `text`, parameter, or local binding, the compiler emits a collision error.
- The fix is always to alias on the import side (`a as my_a`) or rename the local declaration.
- `import "path" { b as c }` — only `c` enters scope. `b` is not directly visible as a bare name in the importing file.
- Case normalization applies: importing `MakePlan` collides with a local `make_plan` because they normalize to the same identifier (`values-and-literals.md:99-103`).

## 4. Whole-Module Import Collision Rules

`import "path" as M` reserves `M` as a single identifier in the importing file's namespace. It collides with any local declaration whose normalized name matches `M`.

Members are accessed only via qualified names (`M.name`). They do not enter the flat namespace. Two whole-module imports `as M` and `as N` may expose identically-named exports without collision because `M.foo` and `N.foo` are distinct qualified names.

`M` itself is not callable — `M()` is invalid. It is a namespace, not a value.

## 5. No-Shadowing Rule

The no-shadowing rule from `values-and-literals.md:139-149` is the universal collision rule. It extends uniformly across all name sources:

- Same-file `text` and `export text` declarations.
- Parameters of the enclosing `skill` or `block`.
- Local bindings inside `flow:`.
- Selectively imported names (or their aliases).
- Whole-module import aliases.

If the same normalized name is reachable from two or more of these sources, the compiler rejects the program. No precedence ordering, no silent wins. The author renames or aliases one of the conflicting declarations.

## 6. Re-Export Policy

**No re-export in MVP.** If file A imports `export block validate` from file B, file A cannot make `validate` importable to file C. File C must import directly from file B.

An `export block` in file A may call an imported `export block` from file B — that is normal composition. What is disallowed is re-exporting B's declaration as if A defined it.

Justification:

- Re-export creates transitive dependency chains that complicate cycle detection and provenance tracking.
- The MVP closure model requires that an `export block`'s behavior is determined by its own file's declarations (`data-flow-and-calls.md:153-166`). Re-export obscures the source of truth.

## 7. Cycle Handling

Circular imports are a compile error. The compiler builds a dependency DAG during import resolution. A back-edge produces a diagnostic naming the full cycle:

```
circular import: A.glyph.md → B.glyph.md → A.glyph.md
```

No lazy-loading or forward-declaration workaround in MVP. If a cycle exists, the author refactors the shared content into a third file that both files import.

**Deferred:** Post-MVP cycle-breaking mechanisms (e.g., interface-only imports, forward declarations, or lazy resolution) if real authoring patterns require them.

## 8. Duplicate Imports

### Same File Imported Twice

Two `import` statements referencing the same resolved path are auto-fixed by merging into one statement. If one is whole-module and the other selective, the compiler merges them into the whole-module form (which already exposes all exported members). This is a source-to-source fix — the compiler modifies the `.glyph.md` file, similar to unused-import removal.

### Same Name From Two Different Files

If two selective imports from different files introduce the same normalized name into scope, the no-shadowing rule applies — compile error. The author must alias one of them.

## 9. Unused Import Auto-Removal

Per `compiled-output.md:167-169`, the compiler auto-removes unused imports from the `.glyph.md` source file.

**Selective imports:** Each imported name is tracked individually. If `a` is used but `b` is not, the compiler removes `b` from the `{ a, b }` list. If all names in a selective import are unused, the entire `import` statement is removed.

**Whole-module imports:** If no qualified reference `M.x` appears anywhere in the file, the entire `import "..." as M` statement is removed.

This is a source-to-source fix, not a silent omission from compiled output. The exact pipeline stage (pre-compilation lint, repair pass, or dedicated import-pruning pass) is an open question per `compiled-output.md:169`.

## 10. Effect Propagation

Imported `export block` declarations carry their full inferred effect set in the IR (`effects.md:120`).

When a caller invokes an imported `export block`, the callee's effect set is unioned into the caller's inferred effects (`effects.md:76-83`):

- If `export block validate` declares `effects: reads_files, runs_commands`, and skill `fix_bug` calls `validate(...)`, then `fix_bug`'s inferred effects include at minimum `{reads_files, runs_commands}`.
- If the caller explicitly declares `effects:`, the declared set must be a superset of the inferred set — otherwise compile error (`effects.md:82`).
- The compiled output for the caller includes the full unioned effect set.

No import-specific effect syntax is needed. The existing `effects:` clause and inference machinery handle propagation.

## 11. Closure Enforcement Timing

Closure is checked at the exporter's compile time. When `export block validate` is compiled in its source file, the compiler verifies:

- No references to undeclared external names.
- No dependency on caller context.
- All effects declared (or inferable from the call graph).

The importer trusts the export contract. At import time, the compiler checks only:

- The referenced name exists and is exported.
- Types match at call boundaries (nominal matching per `types.md`).
- Effects propagate correctly via union.

The importer does not re-check internal closure of the imported block. This keeps compilation modular — analyzing a file does not require re-analyzing every transitive dependency's internals.

## End-To-End Example

Three files demonstrating selective and whole-module imports, effect propagation, and collision avoidance:

**`shared/safety.glyph.md`:**

```glyph
export text unrelated_edits = """
Do not modify code outside the specified scope.
"""

export block validate_changes(files) -> ValidationResult
    effects: reads_files, runs_commands

    flow:
        run_tests(files)
        run_linter(files)
        return validation_result()
```

**`shared/repo.glyph.md`:**

```glyph
export block inspect_repo(scope) -> RepoContext
    effects: reads_files, reads_env

    flow:
        scan_files(scope)
        read_git_state()
        return repo_context()
```

**`fix_bug.glyph.md`:**

```glyph
import "./shared/safety" { unrelated_edits, validate_changes }
import "./shared/repo" as repo

skill fix_bug(scope)
    avoid unrelated_edits                  // selective import, bare name

    effects: reads_files, reads_env, writes_files, runs_commands

    flow:
        ctx = repo.inspect_repo(scope)     // whole-module qualified call
        root_cause = diagnose(ctx)
        apply_fix(root_cause)
        result = validate_changes(ctx)     // selective import, bare call
        return summarize(result)
```

Resolution summary:

- `unrelated_edits` resolves to `export text` from `safety.glyph.md` via selective import.
- `validate_changes` resolves to `export block` from `safety.glyph.md` via selective import.
- `repo.inspect_repo` resolves to `export block` from `repo.glyph.md` via whole-module qualified access.
- Extension auto-resolution: `"./shared/safety"` resolves to `./shared/safety.glyph.md`.
- Effect union: `fix_bug` must declare at least `{reads_files, reads_env, runs_commands}` from its two imported callees, plus `writes_files` from `apply_fix`.

## Interaction With Other Design Areas

- **Declaration headers** (`declaration-headers.md`): Import syntax is fixed there. This document does not reopen syntax.
- **Values and literals** (`values-and-literals.md`): The no-shadowing rule and case normalization are defined there. This document extends them to imported names.
- **Calls and args** (`calls-and-args.md`): Qualified callee resolution (`M.name`) is defined there. This document confirms that `M` must be a whole-module import alias.
- **Effects** (`effects.md`): Effect inference and union propagation are defined there. This document confirms they apply across import boundaries.
- **Compiled output** (`compiled-output.md`): Import inlining and unused-import auto-removal are defined there. This document adds duplicate-import merging as a companion auto-fix.
- **Data flow and calls** (`data-flow-and-calls.md`): Exported block closure requirements are defined there. This document specifies that closure is checked at export time, not at each import site.

## Deferred

- Package-style, registry-backed, or versioned imports (boundary 11).
- Re-export / barrel-file syntax.
- Cycle-breaking mechanisms (interface-only imports, forward declarations, lazy resolution).
- Whether selective imports support glob or wildcard patterns (`declaration-headers.md:249`).
- Deep qualified access (`a.b.c`) for nested module structures.
