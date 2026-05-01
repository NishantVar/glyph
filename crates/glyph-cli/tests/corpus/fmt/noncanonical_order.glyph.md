skill fix_bug()
    flow:
        "Find the bug."
        "Fix it."
    constraints:
        require accuracy
    context:
        project_layout
    description: "Fix a bug in the codebase."

text accuracy = "Be accurate."
text project_layout = "Monorepo layout."
