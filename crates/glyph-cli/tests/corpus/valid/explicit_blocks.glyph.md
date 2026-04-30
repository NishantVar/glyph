block review_code()
    flow:
        "Scan for style violations and anti-patterns."
        "Check for security vulnerabilities."
        "Check for performance issues in hot paths."
        "Compile a list of findings with severity ratings."

block small_helper()
    flow:
        "Do a quick check."

skill code_review()
    description: "Review code for issues."
    flow:
        "Gather the relevant files."
        review_code()
        "Summarize findings."
        small_helper()
