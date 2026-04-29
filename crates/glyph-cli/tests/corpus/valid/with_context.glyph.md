skill fix_bug(scope = ".")
    description: "Debug and fix a bug in the codebase."
    context:
        project_layout
        "The bug is assumed to be reproducible locally."
    flow:
        "Inspect the failure in the codebase."
        "Identify the root cause."
        "Apply a minimal fix."

text project_layout = "This codebase uses a monorepo layout with per-crate Cargo.toml files."
