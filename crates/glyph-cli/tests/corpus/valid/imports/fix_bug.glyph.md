import "./prefs.glyph.md" { preserve_existing_patterns }
import "./repo_tools.glyph.md" { inspect_repo }

skill fix_bug(scope = ".")
    description: "Fix a bug in the codebase."
    require preserve_existing_patterns
    flow:
        ctx = inspect_repo(scope)
        return ctx
