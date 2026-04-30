skill review_code()
    description: "Review code for issues."
    context project_conventions
    context "Always check for security vulnerabilities."
    flow:
        "Scan the repository."
        context repo_layout
        "Report findings."

text project_conventions = "Strict linting rules."
text repo_layout = "Monorepo layout."
