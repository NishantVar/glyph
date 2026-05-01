import "./repo_tools.glyph.md" { inspect_repo, run_tests }

skill review_pr(scope = ".", risk = "medium")
    description: "Review a pull request for correctness, style, and safety."
    require thorough_review
    require check_tests

    flow:
        inspect_repo(scope) with "focus on changed files in the PR"
        if risk == "high"
            context security_note
            run_tests(scope)
            "Verify no security-sensitive code paths are affected."
        else
            "Spot-check test coverage for changed code."
        "Summarize findings with actionable feedback."
        return "Produce a structured review with approval status and comments."

text thorough_review = "Review every changed file, not just the ones that look interesting."
text check_tests = "Verify that tests exist for changed behavior and that they pass."
text security_note = "This is a high-risk change that may affect security-sensitive code paths."
