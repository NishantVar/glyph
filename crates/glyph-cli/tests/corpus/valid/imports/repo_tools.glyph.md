export block inspect_repo(scope = ".")
    description: "Inspect the repository structure and codebase."
    flow:
        "Examine the repository at {scope} and build context."
        return context
